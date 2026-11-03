// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2026 Adrian <adrian.eddy at gmail>

//! ARRI's per-frame metadata: the camera's exposure, white balance and lens (focus, focal length, iris) at each frame, and
//! its tilt and roll from level. There are no gyroscope nor accelerometer readings in ARRI's files. It's read from
//! - ARRIRAW frame sequences: a clip is a folder of frames `<clip>.<NNNNNNN>.ari`, or `.arx` as a Codex recorder writes them
//!   with HDE-encoded images, each a file of its own with a 4096-byte little-endian header before its image (SMPTE RDD
//!   30:2014; an `.arx` header has the magic `CDXA` in place of `ARRI`). Only the headers are read, never the images;
//! - ARRI's MXF files, of the ALEXA Mini and Mini LF and of the ALEXA 35 (`mxf.rs`).
//!
//! Each source's values are taken to the same tags (`Frame`, `Clip`).

use std::io::*;
use std::sync::{ Arc, atomic::{ AtomicBool, Ordering::Relaxed } };

use crate::tags_impl::*;
use crate::*;
use crate::gyroflow::cdng;

mod mxf;

const HEADER_SIZE: usize = 4096;

/// The subheaders after the root, by where each starts in the header and its size (RDD 30 §6.1): image data, image content,
/// camera device, lens data, VFX, clip, sound, camera and ARRI-reserved. Each opens with its defined content length, a u32
/// counting itself, and a field is there only where that length covers it: fields are added by lengthening it into the
/// subheader's reserved bytes. The image data subheader is 60 bytes long in RDD 30:2014 and an ALEXA 65's frames, 68 in a
/// Mini LF's, and the long camera model name at 0x2F8 comes after the 408 bytes RDD 30:2014 defines of the camera device's
const SUBHEADERS: [(usize, usize); 9] = [(0x010, 68), (0x054, 268), (0x160, 528), (0x370, 200), (0x438, 184), (0x4F0, 552), (0x718, 220), (0x7F4, 84), (0x848, 1976)];

/// The frame rate the frames are timed by when the file has none, as Sony's MXF frames are
const DEFAULT_FPS: f64 = 25.0;

#[derive(Default)]
pub struct ArriRaw {
    pub model: Option<String>,
    /// The file the clip is read from
    path: String,
    /// Whether it's an MXF file, else a frame of a sequence
    is_mxf: bool,
}

/// A frame's header, its fields read where their subheader defines them (`SUBHEADERS`). A field with all of its bits set is
/// unset (RDD 30 §6.1)
struct Header<'a>(&'a [u8; HEADER_SIZE]);

impl Header<'_> {
    /// The `len` bytes of the field at `at`, when its subheader defines them. A defined length longer than its subheader
    /// isn't one, and defines nothing
    fn bytes(&self, at: usize, len: usize) -> Option<&[u8]> {
        let &(start, size) = SUBHEADERS.iter().find(|&&(start, size)| (start..start + size).contains(&at))?;
        let defined = u32::from_le_bytes(self.0[start..start + 4].try_into().ok()?) as usize;
        (defined <= size && at + len <= start + defined).then(|| &self.0[at..at + len])
    }
    /// A u32 field, also when all of its bits are set (`u32`)
    fn raw_u32(&self, at: usize) -> Option<u32> {
        Some(u32::from_le_bytes(self.bytes(at, 4)?.try_into().ok()?))
    }
    fn u32(&self, at: usize) -> Option<u32> {
        self.raw_u32(at).filter(|&v| v != u32::MAX)
    }
    fn u16(&self, at: usize) -> Option<u16> {
        Some(u16::from_le_bytes(self.bytes(at, 2)?.try_into().ok()?)).filter(|&v| v != u16::MAX)
    }
    fn f32(&self, at: usize) -> Option<f32> {
        self.u32(at).map(f32::from_bits).filter(|v| v.is_finite())
    }
    /// A text field of `len` bytes (NFSTS): its UTF-8 up to a NUL or the field's end, trimmed. The Mini LF pads its model
    /// name with bytes 0xFF after the NUL, and an unset field is all 0xFF, a byte no UTF-8 has: the text ends at one too
    fn string(&self, at: usize, len: usize) -> Option<String> {
        let bytes = self.bytes(at, len)?;
        let text = &bytes[..bytes.iter().position(|&c| c == 0 || c == 0xFF).unwrap_or(len)];
        Some(std::str::from_utf8(text).ok()?.trim().to_owned()).filter(|x| !x.is_empty())
    }
}

/// A frame's values in the units of their tags (`insert`), as far as its source gives them: a frame header (`of_header`),
/// or an MXF file's frame metadata (`mxf.rs`)
#[derive(Default)]
struct Frame {
    /// mm
    focal_length: Option<f32>,
    /// m
    focus_distance: Option<f32>,
    t_stop: Option<f32>,
    /// ms
    exposure_time: Option<f64>,
    /// Degrees
    shutter_angle: Option<f32>,
    exposure_index: Option<u32>,
    /// K
    white_balance: Option<u32>,
    /// The ND filter's optical density
    nd_density: Option<f64>,
    /// The camera's tilt and roll from level in degrees (`tilt_roll`)
    tilt_roll: Option<(f32, f32)>,
    /// What has no tag of its own, for the sample's metadata
    md: serde_json::Map<String, serde_json::Value>,
}

impl Frame {
    /// The camera's settings in a frame header, which hold for its recording: the exposure index, the white balance and
    /// its tint, and the ND filter
    fn settings_of_header(h: &Header) -> Self {
        let mut md = serde_json::Map::new();
        if let Some(v) = h.f32(0x60) { md.insert("white_balance_tint".into(), (v as f64).into()); }
        Self {
            exposure_index: h.u32(0x74),
            white_balance: h.u32(0x5C),
            // Its type and its density in thousandths. A type of 0 is none, but the ALEXA LF, which has internal filters,
            // writes 0 in all its frames here, so 0 doesn't tell
            nd_density: h.u16(0x388).filter(|&kind| kind != 0).and(h.u16(0x38A)).map(|v| v as f64 / 1000.0),
            md,
            ..Default::default()
        }
    }

    /// A frame header's values: the camera's settings (`settings_of_header`) and the frame's own. Besides the tags', the
    /// master timecode and its frame counter, the white balance factors on the red, green and blue photosites, and the
    /// lens encoders' uncalibrated values, an LDS's and the lens motor's of focus, zoom and iris
    fn of_header(h: &Header) -> Self {
        let mut frame = Self::settings_of_header(h);
        if let Some(v) = Self::timecode(h) { frame.md.insert("timecode".into(), v.into()); }
        if let Some(v) = h.u32(0x1AC) { frame.md.insert("frame_counter".into(), v.into()); }
        if let Some(v) = [0x64, 0x68, 0x6C].iter().map(|&at| h.f32(at).map(f64::from)).collect::<Option<Vec<f64>>>() {
            frame.md.insert("white_balance_factors".into(), v.into());
        }
        let mut encoders = serde_json::Map::new();
        for (i, value) in ["focus", "zoom", "iris"].iter().enumerate() {
            for (j, source) in ["lds", "motor"].iter().enumerate() {
                if let Some(v) = h.u16(0x3B8 + i * 4 + j * 2) { encoders.insert(format!("{value}_{source}"), v.into()); }
            }
        }
        if !encoders.is_empty() { frame.md.insert("lens_encoders".into(), encoders.into()); }
        // In µm, 0 when it isn't known, as it isn't with all bits set either
        frame.focal_length = h.u32(0x37C).filter(|&v| v > 0).map(|v| v as f32 / 1000.0);
        frame.focus_distance = Self::focus_distance(h);
        frame.t_stop = h.u32(0x384).map(t_stop);
        // In µs and in thousandths of a degree
        frame.exposure_time = h.u32(0x18C).map(|v| v as f64 / 1000.0);
        frame.shutter_angle = h.u32(0x190).map(|v| v as f32 / 1000.0);
        frame.tilt_roll = Self::inclinometer(h);
        frame
    }

    /// A sequence's frame header's values (`of_header`), but the T-stop of a frame that has it in thousandths
    /// (`iris_in_thousandths`)
    fn of_sequence_header(h: &Header) -> Self {
        let mut frame = Self::of_header(h);
        if iris_in_thousandths(h) {
            frame.t_stop = h.u32(0x384).map(|v| v as f32 / 1000.0);
        }
        frame
    }

    /// The camera's inclinometer reading (RDD 30 §6.1.6), its tilt and roll (`tilt_roll`): i16 tenths of a degree in the
    /// low 16 bits. An ALEXA LF without one writes 0x0FFF (409.5°, out of range) and 0xFFFF, which alone would be a roll
    /// of -0.1°
    fn inclinometer(h: &Header) -> Option<(f32, f32)> {
        let angle = |at| h.u32(at).map(|v| v as u16 as i16 as f32 / 10.0);
        angle(0x45C).zip(angle(0x460)).and_then(|(tilt, roll)| tilt_roll(tilt, roll))
    }

    /// The focus distance in metres: in thousandths of an inch (unit 0) or in millimetres (unit 1, where RDD 30 says microns:
    /// an ALEXA LF clip's frames have 2931 for the 2.93 m of its Codex clip XML). 0 isn't known, and all bits set is infinity
    /// (RDD 30 §6.1.5) where the unit is given: a distance without one is unset, as lens data with all bits set is
    fn focus_distance(h: &Header) -> Option<f32> {
        let metres_per_unit = match h.u32(0x374)? {
            0 => 0.0000254,
            1 => 0.001,
            _ => return None,
        };
        match h.raw_u32(0x378)? {
            0 => None,
            u32::MAX => Some(f32::INFINITY),
            distance => Some((distance as f64 * metres_per_unit) as f32),
        }
    }

    /// The master timecode, HH:MM:SS:FF in BCD, with a `;` before the frames where it drops frames (RDD 30 §6.1.4)
    fn timecode(h: &Header) -> Option<String> {
        let tc = h.u32(0x1A8).filter(|tc| (0..8).all(|i| (tc >> (i * 4)) & 0xF < 10))?;
        let separator = if h.u32(0x1B4) == Some(1) { ';' } else { ':' };
        Some(format!("{:02x}:{:02x}:{:02x}{separator}{:02x}", tc >> 24, (tc >> 16) & 0xFF, (tc >> 8) & 0xFF, tc & 0xFF))
    }

    /// Inserts the frame's tags into `map`, and what has no tag of its own into `md`
    fn insert(self, map: &mut GroupedTagMap, md: &mut serde_json::Map<String, serde_json::Value>, options: &crate::InputOptions) {
        if let Some(v) = self.focal_length {
            util::insert_tag(map, tag!(parsed GroupId::Lens, TagId::FocalLength, "Focal length", f32, |v| format!("{v:.2} mm"), v, Vec::new()), options);
        }
        if let Some(v) = self.focus_distance {
            util::insert_tag(map, tag!(parsed GroupId::Lens, TagId::FocusDistance, "Focus distance", f32, |v| format!("{v:.3} m"), v, Vec::new()), options);
        }
        if let Some(v) = self.t_stop {
            util::insert_tag(map, tag!(parsed GroupId::Lens, TagId::IrisTStop, "Iris T-stop", f32, |v| format!("T{v:.1}"), v, Vec::new()), options);
        }
        if let Some(v) = self.exposure_time {
            util::insert_tag(map, tag!(parsed GroupId::Imager, TagId::ExposureTime, "Exposure time", f64, |v| format!("{v:.3} ms"), v, Vec::new()), options);
        }
        if let Some(v) = self.shutter_angle {
            util::insert_tag(map, tag!(parsed GroupId::Exposure, TagId::ShutterAngle, "Shutter angle", f32, |v| format!("{v:.1}°"), v, Vec::new()), options);
        }
        if let Some(v) = self.exposure_index {
            util::insert_tag(map, tag!(parsed GroupId::Exposure, TagId::ISOValue, "Exposure index", u16, |v| v.to_string(), v.min(u16::MAX as u32) as u16, Vec::new()), options);
        }
        if let Some(v) = self.white_balance {
            util::insert_tag(map, tag!(parsed GroupId::Colors, TagId::WhiteBalance, "White balance", u16, |v| format!("{v}K"), v.min(u16::MAX as u32) as u16, Vec::new()), options);
        }
        // As the fraction of the light the filter passes, 1/N, which Sony's setting is
        if let Some(density) = self.nd_density {
            let n = 10f64.powf(density).round().clamp(1.0, u16::MAX as f64) as u16;
            util::insert_tag(map, tag!(parsed GroupId::Default, TagId::NDFilterSetting, "ND filter", u16, |&v| if v == 1 { "Clear".into() } else { format!("1/{v}") }, n, Vec::new()), options);
        }
        if let Some(v) = self.tilt_roll {
            util::insert_tag(map, tag!(parsed GroupId::CameraOrientation, TagId::TiltRoll, "Camera tilt and roll", f32x2, |v| format!("tilt {:.1}°, roll {:.1}°", v.0, v.1), v, Vec::new()), options);
        }
        md.extend(self.md);
    }
}

/// The T-stop of a linear iris value, in thousandths of a stop: T1 at 1000 and a stop more, the T-number √2 times, each
/// 1000: T1.4 at 2000, T2 at 3000 (RDD 30 §6.1.5, RDD 55's `LinearIris`, "1/1000 of a stop with T1 ≙ 1000" in ARRI's
/// JSON schema of its `lensIris`)
fn t_stop(linear: u32) -> f32 {
    2f64.powf((linear as f64 - 1000.0) / 2000.0) as f32
}

/// Whether a frame header has its T-stop in thousandths (4991 for T4.99) rather than on the linear scale (`t_stop`):
/// a frame that Codex's software converted (`Codex Digital` where the camera writes its recording medium) from an ALEXA
/// Mini's or Mini LF's MXF file, whose frames have their lens values only in their dynamic metadata, the T-stop in
/// thousandths (`Frame::of_dynamic`), which Codex copies. An F004C001 clip's `.arx` frames have 4991 where the frame
/// header the camera wrote in its MXF file has 5638, both T4.99. The frames a Codex recorder wrote of an ALEXA 65's or
/// ALEXA LF's recording, `.ari` and `.arx` (`CDXA`) alike, are on the linear scale (an ALEXA LF's 2837 and 2667, T1.89
/// and T1.78, are its Zeiss MA100 T1.9 and ARRI SP40 T1.8 wide open), and so is a frame header the camera wrote
fn iris_in_thousandths(h: &Header) -> bool {
    matches!(h.string(0x29C, 8).as_deref(), Some("A-MINI" | "A-MINILF")) && h.string(0x2A4, 32).is_some_and(|x| x.starts_with("Codex Digital"))
}

/// The camera's tilt and roll from level in degrees, a reading of its inclinometer: the tilt +90° straight up, the roll
/// positive clockwise as seen from behind the camera (RDD 30 §6.1.6). A reading out of range is none
fn tilt_roll(tilt: f32, roll: f32) -> Option<(f32, f32)> {
    (tilt.abs() <= 90.0 && roll.abs() <= 180.0).then_some((tilt, roll))
}

/// What a clip's metadata says of it: a frame header's (`of_header`), or an ALEXA 35's JSON documents (`mxf.rs`)
#[derive(Default)]
struct Clip {
    model: Option<String>,
    serial: Option<String>,
    lens: Option<String>,
    lens_serial: Option<String>,
    /// The anamorphic desqueeze, 2.0 for a 2x lens
    pixel_aspect_ratio: Option<f32>,
    /// The frame rate it states: the project's, else the sensor's
    fps: Option<f64>,
    /// What has no tag of its own, for the sample's metadata
    md: serde_json::Map<String, serde_json::Value>,
}

impl Clip {
    /// A frame header's: the camera's model name (`ARRI ALEXA Mini LF`) where it has one, else its type (`A-MINILF`)
    fn of_header(h: &Header) -> Self {
        let camera_type = h.string(0x29C, 8);
        let mut md = serde_json::Map::new();
        for (key, v) in [("camera_type", camera_type.clone()), ("camera_firmware", h.string(0x2E0, 24)), ("reel", h.string(0x4F8, 8)), ("clip_name", h.string(0x698, 24))] {
            if let Some(v) = v { md.insert(key.into(), v.into()); }
        }
        let rate = |at| h.u32(at).filter(|&v| v > 0).map(|v| v as f64 / 1000.0);
        let (sensor_fps, project_fps) = (rate(0x1A0), rate(0x1A4));
        for (key, v) in [("sensor_fps", sensor_fps), ("project_fps", project_fps)] {
            if let Some(v) = v { md.insert(key.into(), v.into()); }
        }
        Self {
            model: h.string(0x2F8, 20).or(camera_type),
            serial: h.u32(0x170).map(|v| v.to_string()),
            lens: h.string(0x398, 32),
            lens_serial: h.u32(0x380).map(|v| v.to_string()),
            pixel_aspect_ratio: h.f32(0xC4).filter(|v| *v > 0.0),
            fps: project_fps.or(sensor_fps),
            md,
        }
    }

    /// Inserts the clip's tags into `map`, and what has no tag of its own into `md`
    fn insert(self, map: &mut GroupedTagMap, md: &mut serde_json::Map<String, serde_json::Value>, options: &crate::InputOptions) {
        if let Some(v) = self.serial {
            util::insert_tag(map, tag!(parsed GroupId::Default, TagId::SerialNumber, "Camera serial number", String, |v| v.clone(), v, Vec::new()), options);
        }
        if let Some(v) = self.lens {
            util::insert_tag(map, tag!(parsed GroupId::Lens, TagId::Name, "Lens name", String, |v| v.clone(), v, Vec::new()), options);
        }
        if let Some(v) = self.lens_serial {
            util::insert_tag(map, tag!(parsed GroupId::Lens, TagId::SerialNumber, "Lens serial number", String, |v| v.clone(), v, Vec::new()), options);
        }
        // Gyroflow's lens calibration takes it as the clip's horizontal stretch
        if let Some(v) = self.pixel_aspect_ratio {
            util::insert_tag(map, tag!(parsed GroupId::Imager, TagId::PixelAspectRatio, "Pixel aspect ratio", f32, |v| format!("{v:.3}"), v, Vec::new()), options);
            md.insert("horizontal_stretch".into(), (v as f64).into());
        }
        md.extend(self.md);
    }
}

/// Inserts a sample's metadata, what has no tag of its own, when there's any, as the `Metadata` of `group`
fn insert_metadata(map: &mut GroupedTagMap, group: GroupId, description: &str, md: serde_json::Map<String, serde_json::Value>, options: &crate::InputOptions) {
    if !md.is_empty() {
        util::insert_tag(map, tag!(parsed group, TagId::Metadata, description, Json, |v| serde_json::to_string(v).unwrap(), serde_json::Value::Object(md), Vec::new()), options);
    }
}

/// The frame rate the frames are timed by: the file's, else `DEFAULT_FPS`
fn timing_fps(fps: Option<f64>) -> f64 {
    fps.unwrap_or_else(|| {
        log::warn!("ARRI: the file has no frame rate, its frames are timed at {DEFAULT_FPS} fps");
        DEFAULT_FPS
    })
}

/// The sample of frame `index`, at its time at `fps`. Its metadata is in a group of its own, `FrameMetadata`, as
/// MotionCam's frames' is: Gyroflow merges the `Default` group's of every sample into the clip's, an array appended to
/// the one before it
fn frame_sample(index: u64, frame: Option<Frame>, fps: f64, options: &crate::InputOptions) -> SampleInfo {
    let (mut map, mut md) = (GroupedTagMap::new(), serde_json::Map::new());
    if let Some(frame) = frame { frame.insert(&mut map, &mut md, options); }
    insert_metadata(&mut map, GroupId::Custom("FrameMetadata".into()), "Frame metadata", md, options);
    SampleInfo { sample_index: index, timestamp_ms: index as f64 * 1000.0 / fps, duration_ms: 1000.0 / fps, tag_map: Some(map), ..Default::default() }
}

impl ArriRaw {
    /// One brand whatever the file and its model: Gyroflow takes it as the camera's brand, its lens profiles' "ARRI"
    pub fn camera_type(&self) -> String {
        "ARRI".to_owned()
    }
    pub fn has_accurate_timestamps(&self) -> bool {
        true
    }
    pub fn possible_extensions() -> Vec<&'static str> {
        vec!["ari", "arx", "mxf"]
    }
    pub fn frame_readout_time(&self) -> Option<f64> {
        None
    }
    pub fn normalize_imu_orientation(v: String) -> String {
        v
    }

    /// A frame of a sequence (`is_header`), or one of ARRI's MXF files (`mxf::detect`)
    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], filepath: P, _options: &crate::InputOptions) -> Option<Self> {
        let is_mxf = match (Self::is_header(buffer), mxf::detect(buffer)) {
            (true, _) => false,
            (false, true) => true,
            (false, false) => return None,
        };
        Some(Self { path: filepath.as_ref().to_str().unwrap_or_default().to_owned(), is_mxf, ..Default::default() })
    }

    /// Whether `data` starts with a frame header: the magic `ARRI` (`.ari`) or `CDXA` (`.arx`), then the byte order mark
    /// 0x78563412 as a little-endian u32 (RDD 30 §6.1.1)
    fn is_header(data: &[u8]) -> bool {
        data.len() >= HEADER_SIZE && matches!(&data[..4], b"ARRI" | b"CDXA") && data[4..8] == [0x12, 0x34, 0x56, 0x78]
    }

    /// A sample of the clip's tags, then one per frame. Of a sequence, from the frame picked to the first frame missing after
    /// it, as FFmpeg reads an image sequence from the frame picked (`cdng::sequence`): frame k of them is frame k of the
    /// video, at k over the frame rate. A frame whose header can't be read is a gap, the frames after it keep their place.
    /// A probe reads the frame picked only
    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, size: usize, progress_cb: F, cancel_flag: Arc<AtomicBool>, options: crate::InputOptions) -> Result<Vec<SampleInfo>> {
        if self.is_mxf {
            return self.parse_mxf(stream, size as u64, progress_cb, &cancel_flag, &options);
        }
        let mut header = [0u8; HEADER_SIZE];
        stream.seek(SeekFrom::Start(0))?;
        stream.read_exact(&mut header)?;
        let clip = Clip::of_header(&Header(&header));
        let fps = clip.fps;

        let paths = if options.probe_only { vec![self.path.clone()] } else { cdng::sequence(&self.path, &cdng::sequence_files(&self.path)) };
        let mut samples = Vec::with_capacity(paths.len() + 1);
        samples.push(self.clip_sample(clip, None, fps, &options));
        let fps = timing_fps(fps);
        for (index, path) in paths.iter().enumerate() {
            if cancel_flag.load(Relaxed) { break; }
            progress_cb(index as f64 / paths.len() as f64);
            // The frame picked is the one in `stream`, its header read above
            if index > 0 && let Err(e) = Self::read_header(path, &mut header) {
                log::warn!("ARRI: {path}: {e}, the frame is skipped");
                continue;
            }
            samples.push(frame_sample(index as u64, Some(Frame::of_sequence_header(&Header(&header))), fps, &options));
        }
        Ok(samples)
    }

    fn read_header(path: &str, header: &mut [u8; HEADER_SIZE]) -> Result<()> {
        filesystem::open_file(path)?.file.read_exact(header)?;
        if Self::is_header(header) { Ok(()) } else { Err(Error::new(ErrorKind::InvalidData, "it doesn't start with an ARRIRAW frame header")) }
    }

    /// The sample of the clip's tags, with the camera's settings where the file has them for the whole clip (an MXF
    /// file's frame header, `Frame::settings_of_header`), and the frame rate the frames are timed by where the file has
    /// one. It's the clip's `FrameRate`, so that the frames aren't taken as captured off-speed against the video
    fn clip_sample(&mut self, mut clip: Clip, settings: Option<Frame>, fps: Option<f64>, options: &crate::InputOptions) -> SampleInfo {
        self.model = clip.model.take();
        let (mut map, mut md) = (GroupedTagMap::new(), serde_json::Map::new());
        clip.insert(&mut map, &mut md, options);
        if let Some(settings) = settings { settings.insert(&mut map, &mut md, options); }
        if let Some(fps) = fps {
            util::insert_tag(&mut map, tag!(parsed GroupId::Default, TagId::FrameRate, "Frame rate", f64, |v| format!("{v:.3} fps"), fps, Vec::new()), options);
        }
        insert_metadata(&mut map, GroupId::Default, "Metadata", md, options);
        SampleInfo { tag_map: Some(map), ..Default::default() }
    }
}
