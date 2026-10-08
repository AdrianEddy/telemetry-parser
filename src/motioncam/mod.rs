// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2026 Adrian <adrian.eddy at gmail>

//! MotionCam Pro RAW video (`.mcraw`), the container read by https://github.com/mirsadm/motioncam-decoder.
//! Only the metadata is read here, the raw frames are not decoded.
//!
//! The container is little endian: an 8 byte header (`MOTION ` + version), then items of `u32 type, u32 size`
//! followed by `size` bytes. The first item is the container metadata (JSON), then every frame is a BUFFER item
//! (the compressed raw image) followed by a METADATA item (the frame's JSON). Gyro, accelerometer, OIS and audio
//! chunks are written along the way. The end of the file holds their indexes, then the frame index data and, in
//! the last 24 bytes, the BUFFER_INDEX item pointing at it.
//!
//! Every timestamp is an Android sensor timestamp in nanoseconds. A frame's timestamp is the start of the exposure
//! of its first row; with a REALTIME timestamp source frames and motion samples share one clock.

use std::io::*;
use std::sync::{ Arc, atomic::AtomicBool };
use byteorder::{ ReadBytesExt, LittleEndian };
use serde_json::Value;

use crate::tags_impl::*;
use crate::*;

const HEADER: &[u8; 7] = b"MOTION ";
const CONTAINER_VERSION: u8 = 3;
const INDEX_MAGIC: u32 = 0x8A905612;
const STREAM_VERSION: u32 = 1; // of the gyro, accelerometer and OIS chunks and their indexes

// Item types
const BUFFER_INDEX:        u32 = 0;
const BUFFER:              u32 = 2;
const METADATA:            u32 = 3;
const AUDIO_INDEX:         u32 = 4;
const AUDIO_DATA:          u32 = 5;
const AUDIO_DATA_METADATA: u32 = 6;
const AUDIO_DATA_F32:      u32 = 7;
const GYRO_INDEX:          u32 = 8;
const GYRO_DATA:           u32 = 9;
const OIS_INDEX:           u32 = 10;
const OIS_DATA:            u32 = 11;
const ACCELEROMETER_INDEX: u32 = 12;
const ACCELEROMETER_DATA:  u32 = 13;

const MOTION_SAMPLE_SIZE: u64 = 24; // i64 timestamp, f32 x, y, z, u32 reserved
const OIS_SAMPLE_SIZE:    u64 = 16; // i64 timestamp, f32 x, y shift

const MAX_JSON_SIZE: u32 = 64 << 20; // the container metadata is up to a few hundred KB, a frame's ~12 KB
const MAX_DURATION_NS: f64 = 1e11; // sanity bound of an exposure or readout time

#[derive(Default)]
pub struct MotionCam {
    pub model: Option<String>,
    sidecar: Option<String>,
    frame_readout_time: Option<f64>,
    accurate_timestamps: bool,
}

impl MotionCam {
    pub fn camera_type(&self) -> String {
        "MotionCam".to_owned()
    }
    pub fn has_accurate_timestamps(&self) -> bool {
        // The motion samples are on the frames' clock, rebased to the first frame
        self.accurate_timestamps
    }
    pub fn possible_extensions() -> Vec<&'static str> {
        vec!["mcraw", "mp4", "mov", "mxf", "dng"]
    }
    pub fn frame_readout_time(&self) -> Option<f64> {
        self.frame_readout_time
    }
    pub fn normalize_imu_orientation(v: String) -> String {
        v
    }

    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], filepath: P, options: &crate::InputOptions) -> Option<Self> {
        if buffer.len() > HEADER.len() && buffer.starts_with(HEADER) {
            return Some(Self::default());
        }
        if options.dont_look_for_sidecar_files { return None; }
        let path = filepath.as_ref().to_str().unwrap_or_default();
        let sidecar = sidecar_candidates(path).into_iter().find(|x| is_mcraw(x))?;
        Some(Self { sidecar: Some(sidecar), ..Default::default() })
    }

    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, size: usize, progress_cb: F, cancel_flag: Arc<AtomicBool>, options: crate::InputOptions) -> Result<Vec<SampleInfo>> {
        if let Some(path) = self.sidecar.clone() {
            let mut file = filesystem::open_file(&path)?;
            let size = file.size as u64;
            return self.parse_container(&mut file.file, size, progress_cb, cancel_flag, &options);
        }
        self.parse_container(stream, size as u64, progress_cb, cancel_flag, &options)
    }

    fn parse_container<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, file_size: u64, progress_cb: F, cancel_flag: Arc<AtomicBool>, options: &crate::InputOptions) -> Result<Vec<SampleInfo>> {
        let probe = options.probe_only;

        stream.seek(SeekFrom::Start(0))?;
        let mut header = [0u8; 8];
        stream.read_exact(&mut header)?;
        if !header.starts_with(HEADER) {
            return Err(invalid("Not a MotionCam container"));
        }
        if header[7] != CONTAINER_VERSION {
            log::warn!("MotionCam container version {}, expected {CONTAINER_VERSION}", header[7]);
        }
        let (kind, len) = read_item(stream)?;
        if kind != METADATA || len > MAX_JSON_SIZE {
            return Err(invalid("Missing MotionCam container metadata"));
        }
        let container: Value = serde_json::from_slice(&read_payload(stream, len, file_size)?)?;
        let buffer_start = stream.stream_position()?;
        self.model = camera_model(&container);

        // The indexes are written when the recording is finalized. Without them (an interrupted recording) the
        // whole file is walked instead
        let mut walk = Walk::default();
        let (mut frames, chunks) = match read_frame_index(stream, file_size, buffer_start) {
            Some((offsets, index_data)) => {
                let last_frame = offsets.iter().map(|x| x.offset).max().unwrap_or(buffer_start);
                walk_items(stream, last_frame, index_data, file_size, false, &mut walk);
                (offsets, walk.indexed)
            }
            None => {
                log::warn!("MotionCam frame index not found, walking the whole file");
                walk_items(stream, buffer_start, file_size, file_size, probe, &mut walk);
                let pick = |indexed: Vec<u64>, seen: Vec<u64>| if indexed.is_empty() { seen } else { indexed };
                let frames = walk.seen.frames.iter().map(|&offset| Offset { offset, timestamp: None }).collect();
                (frames, Chunks {
                    frames:        Vec::new(),
                    audio:         pick(walk.indexed.audio, walk.seen.audio),
                    gyro:          pick(walk.indexed.gyro, walk.seen.gyro),
                    accelerometer: pick(walk.indexed.accelerometer, walk.seen.accelerometer),
                    ois:           pick(walk.indexed.ois, walk.seen.ois),
                })
            }
        };

        // ------------------------------- Frames -------------------------------
        // Each frame's metadata is turned into tags as it's read: most of it is the lens shading map, which is kept
        // only when it changes and not as JSON
        frames.sort_by_key(|x| x.timestamp);
        if probe { frames.truncate(1); }
        let mut loaded: Vec<Frame> = Vec::with_capacity(frames.len());
        let mut previous_shading = None;
        for (i, frame) in frames.iter().enumerate() {
            if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) { break; }
            let md = read_frame_metadata(stream, frame.offset, file_size).unwrap_or_else(|e| {
                log::warn!("MotionCam frame at {}: {e}", frame.offset);
                Value::Null
            });
            // A frame from the index keeps its place even without metadata, so the sample index stays the frame number
            let Some(timestamp) = frame.timestamp.or_else(|| json_i64(md.get("timestamp"))) else {
                log::warn!("MotionCam frame at {} has no timestamp", frame.offset);
                continue;
            };
            let duration = |key: &str| json_f64(md.get(key)).filter(|x| *x > 0.0 && *x < MAX_DURATION_NS);
            let (exposure, readout) = (duration("exposureTime"), duration("rollingShutterSkewNs"));
            let received_ms = json_i64(md.get("recvdTimestampMs"));
            let capture_area = capture_area(&container, &md);
            let tags = frame_tags(timestamp, md, &container, capture_area.as_ref(), &mut previous_shading, options);
            loaded.push(Frame { timestamp, exposure, readout, received_ms, capture_area, tags });
            if i % 32 == 0 {
                progress_cb(i as f64 / frames.len() as f64 * 0.9);
            }
        }
        // Written in time order but for the odd late frame, which the index has in its place
        loaded.sort_by_key(|x| x.timestamp);
        let frame_timestamps: Vec<i64> = loaded.iter().map(|x| x.timestamp).collect();

        // Gyroflow puts a frame at the middle of the exposure of its centre row, so that moment of the first frame
        // is the origin of the motion samples' time. Exposure and readout hardly change, they're taken from the first
        // frame which has them
        let exposure = loaded.iter().find_map(|x| x.exposure).unwrap_or_default();
        let readout = loaded.iter().find_map(|x| x.readout).unwrap_or_default();
        let origin = frame_timestamps.first().map(|ts| ts.saturating_add(((exposure + readout) / 2.0).round() as i64));
        self.frame_readout_time = Some(readout / 1e6).filter(|x| *x > 0.0);
        // With an UNKNOWN timestamp source the frames' clock "can not be compared to timestamps from other
        // subsystems (e.g. accelerometer, gyro etc.)" (Android), so the samples have to be synchronized
        self.accurate_timestamps = origin.is_some() && json_f64(container.get("timestampSource")) != Some(0.0);
        if origin.is_some() && !self.accurate_timestamps {
            log::warn!("MotionCam timestamp source is UNKNOWN, the motion samples need to be synchronized with the video");
        }
        let capture_area = loaded.iter().find_map(|x| x.capture_area);
        let received_ms = loaded.first().and_then(|x| x.received_ms);
        let frame_rate = frame_rate(&frame_timestamps);

        // ------------------------------- Motion -------------------------------
        let limit = |chunks: &[u64]| -> Vec<u64> { chunks.iter().copied().take(if probe { 1 } else { usize::MAX }).collect() };
        let gyro = read_samples(stream, &limit(&chunks.gyro),          GYRO_DATA,          MOTION_SAMPLE_SIZE, file_size);
        let accl = read_samples(stream, &limit(&chunks.accelerometer), ACCELEROMETER_DATA, MOTION_SAMPLE_SIZE, file_size);
        let ois  = read_samples(stream, &limit(&chunks.ois),           OIS_DATA,           OIS_SAMPLE_SIZE,    file_size);
        let channels = json_f64(container.pointer("/extraData/audioChannels")).filter(|x| (1.0..=64.0).contains(x)).unwrap_or(1.0) as u64;
        let audio = if probe { Vec::new() } else { read_audio_chunks(stream, &chunks.audio, channels, file_size) };
        progress_cb(0.95);

        // Without frames, the time starts at the first gyro sample
        let origin = origin.or_else(|| gyro.first().map(|x| x.0)).unwrap_or_default();
        let seconds = |ts: i64| (ts as i128 - origin as i128) as f64 / 1e9;
        let vec3 = |(ts, v): &Sample| TimeVector3 { t: seconds(*ts), x: v[0] as f64, y: v[1] as f64, z: v[2] as f64 };
        let gyro: Vec<TimeVector3<f64>> = gyro.iter().map(vec3).collect();
        let accl: Vec<TimeVector3<f64>> = accl.iter().map(vec3).collect();
        let ois:  Vec<TimeVector3<f64>> = ois.iter().map(vec3).collect();

        // ------------------------------- Samples -------------------------------
        let first_ts = frame_timestamps.first().copied().unwrap_or(origin);
        let nominal_duration_ms = frame_rate.map(|fps| 1000.0 / fps).unwrap_or_default();
        let mut samples = Vec::with_capacity(loaded.len().max(1));
        for (i, frame) in loaded.into_iter().enumerate() {
            let mut map = frame.tags;
            // The moment gyroflow puts the frame at, for its offset from the frame's time in the video
            let capture = frame.timestamp.saturating_add(((frame.exposure.unwrap_or(exposure) + frame.readout.unwrap_or(readout)) / 2.0).round() as i64);
            util::insert_tag(&mut map, tag!(parsed GroupId::Imager, TagId::Custom("CaptureTime".into()), "Capture time", f64, |v| format!("{v:.3} ms"), seconds(capture) * 1000.0, vec![]), options);
            let duration_ms = frame_timestamps.get(i + 1).map(|next| milliseconds(*next, frame.timestamp)).unwrap_or(nominal_duration_ms);
            samples.push(SampleInfo { sample_index: i as u64, timestamp_ms: milliseconds(frame.timestamp, first_ts), duration_ms, tag_map: Some(map), ..Default::default() });
        }
        if samples.is_empty() {
            samples.push(SampleInfo { tag_map: Some(GroupedTagMap::new()), ..Default::default() });
        }

        // The clip-wide tags go to the first frame
        let map = samples[0].tag_map.get_or_insert_with(GroupedTagMap::new);
        if let Some(fps) = frame_rate {
            util::insert_tag(map, tag!(parsed GroupId::Default, TagId::FrameRate, "Frame rate", f64, |v| format!("{v:.4} fps"), fps, vec![]), options);
        }
        if let Some(ms) = received_ms.filter(|x| *x > 0) {
            util::insert_tag(map, tag!(parsed GroupId::Default, TagId::CaptureTimestamp, "Capture timestamp", u64, |&v| chrono::TimeZone::timestamp_opt(&chrono::Utc, v as i64, 0).single().map(|x| x.to_string()).unwrap_or_default(), ms as u64 / 1000, vec![]), options);
        }
        if let Some(f) = json_f64_array(container.get("apertures")).and_then(|x| x.first().copied()).filter(|x| *x > 0.0) {
            util::insert_tag(map, tag!(parsed GroupId::Lens, TagId::IrisFStop, "Aperture", f32, |v| format!("f/{v:.2}"), f as f32, vec![]), options);
        }
        if let Some(size) = json_f64_array(container.get("pixelArraySize")).filter(|x| x.len() >= 2 && x[0] >= 1.0 && x[1] >= 1.0) {
            util::insert_tag(map, tag!(parsed GroupId::Imager, TagId::SensorSizePixels, "Sensor pixel size", u32x2, |v| format!("{v:?}"), (size[0] as u32, size[1] as u32), vec![]), options);
            if let Some(mm) = json_f64_array(container.get("physicalSensorSizeMm")).filter(|x| x.len() >= 2 && x[0] > 0.0 && x[1] > 0.0) {
                let pitch = ((mm[0] / size[0] * 1e6).round() as u32, (mm[1] / size[1] * 1e6).round() as u32);
                util::insert_tag(map, tag!(parsed GroupId::Imager, TagId::PixelPitch, "Pixel pitch", u32x2, |v| format!("{v:?} nm"), pitch, vec![]), options);
            }
        }
        if let Some(area) = capture_area {
            let origin = ((area.active_origin.0 + area.crop.0) as f32, (area.active_origin.1 + area.crop.1) as f32);
            util::insert_tag(map, tag!(parsed GroupId::Imager, TagId::CaptureAreaOrigin, "Capture area origin", f32x2, |v| format!("{v:?}"), origin, vec![]), options);
            util::insert_tag(map, tag!(parsed GroupId::Imager, TagId::CaptureAreaSize,   "Capture area size",   f32x2, |v| format!("{v:?}"), (area.size.0 as f32, area.size.1 as f32), vec![]), options);
        }

        let imu_orientation = imu_orientation(&container);
        if !gyro.is_empty() {
            util::insert_tag(map, tag!(parsed GroupId::Gyroscope, TagId::Data,        "Gyroscope data",  Vec_TimeVector3_f64, |v| format!("{v:?}"), gyro, vec![]), options);
            util::insert_tag(map, tag!(parsed GroupId::Gyroscope, TagId::Unit,        "Gyroscope unit",  String, |v| v.to_string(), "rad/s".into(), vec![]), options);
            util::insert_tag(map, tag!(parsed GroupId::Gyroscope, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.clone(), vec![]), options);
        }
        if !accl.is_empty() {
            // Including gravity, iOS values already converted from g by the app
            util::insert_tag(map, tag!(parsed GroupId::Accelerometer, TagId::Data,        "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{v:?}"), accl, vec![]), options);
            util::insert_tag(map, tag!(parsed GroupId::Accelerometer, TagId::Unit,        "Accelerometer unit", String, |v| v.to_string(), "m/s²".into(), vec![]), options);
            util::insert_tag(map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation",    String, |v| v.to_string(), imu_orientation.clone(), vec![]), options);
        }
        if !ois.is_empty() {
            // Android's lens shift in pixels of the active array (x, y; z unused)
            util::insert_tag(map, tag!(parsed GroupId::LensOSS, TagId::Data, "OIS lens shift", Vec_TimeVector3_f64, |v| format!("{v:?}"), ois, vec![]), options);
            util::insert_tag(map, tag!(parsed GroupId::LensOSS, TagId::Unit, "OIS lens shift unit", String, |v| v.to_string(), "px".into(), vec![]), options);
        }
        if !audio.is_empty() {
            let group = GroupId::Custom("Audio".into());
            // Per chunk with a timestamp: its time on the motion samples' time base and its length in samples per channel
            let chunks: Vec<TimeScalar<i64>> = audio.iter().filter_map(|x| Some(TimeScalar { t: seconds(x.timestamp?), v: x.samples as i64 })).collect();
            if !chunks.is_empty() {
                util::insert_tag(map, tag!(parsed group.clone(), TagId::Data, "Audio chunks", Vec_TimeScalar_i64, |v| format!("{v:?}"), chunks, vec![]), options);
            }
            let total: u64 = audio.iter().map(|x| x.samples).sum();
            util::insert_tag(map, tag!(parsed group.clone(), TagId::Count, "Audio samples per channel", u64, |v| v.to_string(), total, vec![]), options);
            if let Some(rate) = json_f64(container.pointer("/extraData/audioSampleRate")).filter(|x| *x > 0.0) {
                util::insert_tag(map, tag!(parsed group.clone(), TagId::Frequency, "Audio sample rate", u32, |v| format!("{v} Hz"), rate as u32, vec![]), options);
            }
            util::insert_tag(map, tag!(parsed group.clone(), TagId::Custom("Channels".into()), "Audio channels", u32, |v| v.to_string(), channels as u32, vec![]), options);
            // The index's "startTimestampMs" is 0, or (3.0) the first chunk's timestamp in nanoseconds
            if let Some(start) = walk.audio_start.filter(|x| *x != 0 && (seconds(*x)).abs() < 3600.0) {
                util::insert_tag(map, tag!(parsed group, TagId::Custom("StartTime".into()), "Audio start time", f64, |v| format!("{v:.6} s"), seconds(start), vec![]), options);
            }
        }
        util::insert_tag(map, tag!(parsed GroupId::Default, TagId::Metadata, "Container metadata", Json, |v| v.to_string(), container, vec![]), options);

        progress_cb(1.0);
        Ok(samples)
    }
}

/// Where the recorded frame lies on the sensor. MotionCam crops the centre of the raw image (the pre-correction
/// active array), and averages 2x2 blocks when binning
#[derive(Clone, Copy)]
struct CaptureArea {
    active_origin: (f64, f64), // top left of the active array in the pixel array
    crop: (f64, f64),          // top left of the recorded part in the active array
    size: (f64, f64),          // the recorded part, in sensor pixels
    frame: (f64, f64),         // the recorded frame, in its own pixels
}

fn capture_area(container: &Value, frame: &Value) -> Option<CaptureArea> {
    let (w, h) = (json_f64(frame.get("width"))?, json_f64(frame.get("height"))?);
    let raw_w = json_f64(frame.get("originalWidth")).unwrap_or(w);
    let raw_h = json_f64(frame.get("originalHeight")).unwrap_or(h);
    let bin = if frame.get("isBinned").and_then(Value::as_bool).unwrap_or_default() { 2.0 } else { 1.0 };
    let size = (w * bin, h * bin);
    if w < 1.0 || h < 1.0 || size.0 > raw_w || size.1 > raw_h { return None; }
    // Left and top, the same in a [left, top, right, bottom] and an [x, y, width, height] rect
    let active = json_f64_array(container.get("preCorrectionActiveArrayRect")).filter(|x| x.len() == 4).unwrap_or(vec![0.0; 4]);
    Some(CaptureArea {
        active_origin: (active[0], active[1]),
        crop: ((raw_w - size.0) / 2.0, (raw_h - size.1) / 2.0),
        size,
        frame: (w, h),
    })
}

/// A frame read, before the samples are put in time order
struct Frame {
    timestamp: i64,
    exposure: Option<f64>,
    readout: Option<f64>,
    received_ms: Option<i64>,
    capture_area: Option<CaptureArea>,
    tags: GroupedTagMap,
}

/// Tags of one frame, and its whole metadata
fn frame_tags(timestamp: i64, mut md: Value, container: &Value, area: Option<&CaptureArea>, previous_shading: &mut Option<Vec<Vec<f32>>>, options: &crate::InputOptions) -> GroupedTagMap {
    let mut map = GroupedTagMap::new();
    util::insert_tag(&mut map, tag!(parsed GroupId::Default, TagId::Custom("SensorTimestamp".into()), "Sensor timestamp", i64, |v| format!("{v} ns"), timestamp, vec![]), options);
    if md.is_null() { return map; }

    if let Some(ns) = json_f64(md.get("exposureTime")).filter(|x| *x > 0.0) {
        util::insert_tag(&mut map, tag!(parsed GroupId::Imager, TagId::ExposureTime, "Exposure time", f64, |v| format!("{v:.4} ms"), ns / 1e6, vec![]), options);
        let speed = if ns >= 1e9 { ((ns / 1e9).round() as u32, 1) } else { (1, (1e9 / ns).round() as u32) };
        util::insert_tag(&mut map, tag!(parsed GroupId::Exposure, TagId::ShutterSpeed, "Shutter speed", u32x2, |v| format!("{}/{}s", v.0, v.1), speed, vec![]), options);
    }
    if let Some(iso) = json_f64(md.get("iso")).filter(|x| *x > 0.0) {
        util::insert_tag(&mut map, tag!(parsed GroupId::Exposure, TagId::ISOValue, "ISO", u16, |v| v.to_string(), iso.round().min(u16::MAX as f64) as u16, vec![]), options);
    }
    if let Some(ns) = json_f64(md.get("rollingShutterSkewNs")).filter(|x| *x > 0.0) {
        util::insert_tag(&mut map, tag!(parsed GroupId::Imager, TagId::FrameReadoutTime, "Frame readout time", f64, |v| format!("{v:.4} ms"), ns / 1e6, vec![]), options);
    }
    if let Some(ns) = json_f64(md.get("sensorFrameDurationNs")).filter(|x| *x > 0.0) {
        util::insert_tag(&mut map, tag!(parsed GroupId::Imager, TagId::Custom("FrameDuration".into()), "Frame duration", f64, |v| format!("{v:.4} ms"), ns / 1e6, vec![]), options);
    }
    if let Some(mm) = json_f64(md.get("focalLengthMm")).filter(|x| *x > 0.0) {
        util::insert_tag(&mut map, tag!(parsed GroupId::Lens, TagId::FocalLength, "Focal length", f32, |v| format!("{v:.2} mm"), mm as f32, vec![]), options);
    }
    if let Some(diopters) = json_f64(md.get("focusDistance")).filter(|x| *x >= 0.0) {
        // Android reports diopters, 0 being infinity
        let meters = if diopters > 0.0 { (1.0 / diopters) as f32 } else { f32::INFINITY };
        util::insert_tag(&mut map, tag!(parsed GroupId::Lens, TagId::FocusDistance, "Focus distance", f32, |v| format!("{v:.2} m"), meters, vec![]), options);
    }
    if let Some(o) = json_f64(md.get("orientation")) {
        let name = match o as i64 { 0 => "Portrait", 1 => "Reverse portrait", 2 => "Landscape", 3 => "Reverse landscape", _ => "Unknown" };
        util::insert_tag(&mut map, tag!(parsed GroupId::Default, TagId::Custom("ScreenOrientation".into()), "Screen orientation", String, |v| v.to_string(), name.into(), vec![]), options);
    }

    // The camera's own calibration, in pixels of the recorded frame. Kept out of the Lens group: there it would be
    // taken as the geometry of the video, which can be scaled
    if let Some(area) = area {
        let calibration = GroupId::Custom("LensCalibration".into());
        let bin = area.size.0 / area.frame.0;
        let get = |key: &str| md.get(key).or_else(|| container.get(key)).and_then(|x| json_f64_array(Some(x)));
        if let Some(k) = get("lensIntrinsicCalibration").filter(|x| x.len() >= 4 && x[0] > 0.0 && x[1] > 0.0) {
            // Android's is in pixels of the pre-correction active array, which the raw image covers
            let f = ((k[0] / bin) as f32, (k[1] / bin) as f32);
            let c = (((k[2] - area.crop.0) / bin) as f32, ((k[3] - area.crop.1) / bin) as f32);
            util::insert_tag(&mut map, tag!(parsed calibration.clone(), TagId::PixelFocalLength, "Pixel focal length", f32x2, |v| format!("({:.2}, {:.2}) px", v.0, v.1), f, vec![]), options);
            util::insert_tag(&mut map, tag!(parsed calibration.clone(), TagId::PrincipalPoint,   "Principal point",    f32x2, |v| format!("({:.2}, {:.2}) px", v.0, v.1), c, vec![]), options);
            util::insert_tag(&mut map, tag!(parsed calibration.clone(), TagId::PixelWidth,  "Frame width",  u32, |v| format!("{v} px"), area.frame.0 as u32, vec![]), options);
            util::insert_tag(&mut map, tag!(parsed calibration.clone(), TagId::PixelHeight, "Frame height", u32, |v| format!("{v} px"), area.frame.1 as u32, vec![]), options);
        }
        if let Some(k) = get("lensDistortion").filter(|x| x.len() == 5) {
            // Android's LENS_DISTORTION is OpenCV's standard model (corrected -> distorted, normalized by the focal
            // length), with the coefficients in another order: [k1, k2, k3, p1, p2] -> [k1, k2, p1, p2, k3]
            util::insert_tag(&mut map, tag!(parsed calibration.clone(), TagId::DistortionModel,        "Distortion model",        String,  |v| v.to_string(), "opencv_standard".into(), vec![]), options);
            util::insert_tag(&mut map, tag!(parsed calibration,         TagId::DistortionCoefficients, "Distortion coefficients", Vec_f64, |v| format!("{v:?}"), vec![k[0], k[1], k[3], k[4], k[2]], vec![]), options);
        }
    }

    // The shading map is most of a frame's metadata. Kept as numbers instead of JSON, and only when it changes
    let shading = md.get("lensShadingMap").and_then(Value::as_array).and_then(|channels| channels.iter()
        .map(|x| x.as_array()?.iter().map(|v| v.as_f64().map(|v| v as f32)).collect::<Option<Vec<f32>>>())
        .collect::<Option<Vec<Vec<f32>>>>());
    if let Some(shading) = shading {
        if let Some(md) = md.as_object_mut() { md.remove("lensShadingMap"); }
        if previous_shading.as_ref() != Some(&shading) {
            if let (Some(w), Some(h)) = (json_f64(md.get("lensShadingMapWidth")), json_f64(md.get("lensShadingMapHeight"))) {
                util::insert_tag(&mut map, tag!(parsed GroupId::Lens, TagId::Custom("ShadingMapSize".into()), "Lens shading map size", u32x2, |v| format!("{v:?}"), (w as u32, h as u32), vec![]), options);
            }
            util::insert_tag(&mut map, tag!(parsed GroupId::Lens, TagId::Shading, "Lens shading map (per Bayer channel, row major)", Vec_Vec_f32, |v| format!("{v:?}"), shading.clone(), vec![]), options);
            *previous_shading = Some(shading);
        }
    }
    util::insert_tag(&mut map, tag!(parsed GroupId::Custom("FrameMetadata".into()), TagId::Metadata, "Frame metadata", Json, |v| v.to_string(), md, vec![]), options);
    map
}

/// "Manufacturer model" of the phone
fn camera_model(container: &Value) -> Option<String> {
    fn string(x: Option<&Value>) -> Option<&str> {
        x.and_then(Value::as_str).map(str::trim).filter(|x| !x.is_empty())
    }
    let build = container.pointer("/extraData/postProcessSettings/metadata");
    let manufacturer = string(build.and_then(|x| x.get("build.manufacturer")));
    let model = string(build.and_then(|x| x.get("build.model"))).or_else(|| string(container.pointer("/deviceSpecificProfile/deviceModel")))?;
    match manufacturer {
        Some(m) if !model.to_ascii_lowercase().starts_with(&m.to_ascii_lowercase()) => Some(format!("{m} {model}")),
        _ => Some(model.to_owned())
    }
}

/// Frames per second from the frame timestamps: frame periods over time, each interval counting as the number of
/// periods it spans, so dropped frames don't count and the jitter of single intervals doesn't add up
fn frame_rate(timestamps: &[i64]) -> Option<f64> {
    let intervals: Vec<i64> = timestamps.windows(2).map(|x| x[1].saturating_sub(x[0])).filter(|x| *x > 0).collect();
    let mut sorted = intervals.clone();
    sorted.sort_unstable();
    let median = *sorted.get(sorted.len().checked_sub(1)? / 2)? as f64;
    let periods: f64 = intervals.iter().map(|x| (*x as f64 / median).round().max(1.0)).sum();
    let span: f64 = intervals.iter().map(|x| *x as f64).sum();
    Some(periods * 1e9 / span)
}

fn milliseconds(ts: i64, since: i64) -> f64 {
    (ts as i128 - since as i128) as f64 / 1e6
}

/// Axes of the motion samples for gyroflow: x right, y up, z towards the viewer, in the recorded raw image.
///
/// The camera's axes (x right, y down, z forward in the raw image) follow from the sensor orientation, the
/// clockwise rotation that makes the raw image upright on the phone held in portrait. Android's LENS_POSE_ROTATION
/// (x, y, z, w) gives the same rotation from the phone's sensor axes, but it's often a placeholder and the sensor
/// orientation is what apps rely on, so the pose is used only without one. Gyroflow's axes are the camera's with
/// y and z flipped
fn imu_orientation(container: &Value) -> String {
    let axes = |m: [[f64; 3]; 3]| axes_string(&[m[0], m[1].map(|x| -x), m[2].map(|x| -x)]);
    let front = json_f64(container.get("lensFacing")) == Some(0.0);
    let from_sensor = json_f64(container.get("sensorOrientation")).and_then(|angle| axes(sensor_orientation_rotation(angle, front)));
    let from_pose = pose_rotation(container).and_then(axes);
    match (from_sensor, from_pose) {
        (Some(sensor), Some(pose)) if sensor != pose => {
            log::warn!("MotionCam lens pose ({pose}) doesn't agree with the sensor orientation ({sensor}), using the sensor orientation");
            sensor
        }
        (Some(x), _) | (None, Some(x)) => x,
        (None, None) => "yXZ".into() // the usual back camera, at 90°
    }
}

fn pose_rotation(container: &Value) -> Option<[[f64; 3]; 3]> {
    // UNDEFINED: "represented by default values matching its default facing"
    if json_f64(container.get("lensPoseReference")) == Some(2.0) { return None; }
    let q = json_f64_array(container.get("lensPoseRotation")).filter(|x| x.len() == 4)?;
    let norm = q.iter().map(|x| x * x).sum::<f64>().sqrt();
    if !(0.5..1.5).contains(&norm) { return None; } // not set
    let (x, y, z, w) = (q[0] / norm, q[1] / norm, q[2] / norm, q[3] / norm);
    Some([
        [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w),       2.0 * (x * z + y * w)],
        [2.0 * (x * y + z * w),       1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w)],
        [2.0 * (x * z - y * w),       2.0 * (y * z + x * w),       1.0 - 2.0 * (x * x + y * y)],
    ])
}

/// Rotation from the phone's sensor axes to the camera's, from the sensor orientation in degrees. "If the top side of
/// the camera sensor is aligned with the right edge of the screen in natural orientation, the value should be 90.
/// If the top side of a front-facing camera sensor is aligned with the right of the screen, the value should be 270"
fn sensor_orientation_rotation(angle: f64, front: bool) -> [[f64; 3]; 3] {
    let back = |angle: f64| {
        let (s, c) = (angle.to_radians().sin().round(), angle.to_radians().cos().round());
        [[c, -s, 0.0], [-s, -c, 0.0], [0.0, 0.0, -1.0]]
    };
    if front {
        // A back camera with the sensor's top towards the same edge, turned around its y axis to face the user
        let m = back(360.0 - angle);
        [m[0].map(|x| -x), m[1], m[2].map(|x| -x)]
    } else {
        back(angle)
    }
}

/// Signed axis permutation closest to the rotation (rows: output axes, columns: input axes), as an orientation string
fn axes_string(m: &[[f64; 3]; 3]) -> Option<String> {
    let mut used = [false; 3];
    let mut out = String::with_capacity(3);
    for row in m {
        let (axis, value) = row.iter().enumerate().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))?;
        if used[axis] || value.abs() < 0.5 { return None; }
        used[axis] = true;
        let c = ['X', 'Y', 'Z'][axis];
        out.push(if *value < 0.0 { c.to_ascii_lowercase() } else { c });
    }
    Some(out)
}

/// Where the MCRAW of a clip rendered from it can be: next to a video, with the same name. Next to a DNG sequence
/// (gyroflow opens it as `clip_%06d.dng`) as `clip.mcraw`, or next to the folder holding the frames, named after it
fn sidecar_candidates(path: &str) -> Vec<String> {
    let ext = filesystem::get_extension(path);
    let mut names = Vec::new();
    match ext.as_str() {
        "" | "mcraw" => { }
        "dng" => {
            let folder = filesystem::get_folder(path);
            let filename = filesystem::get_filename(path);
            let stem = filename.rsplit_once('.').map(|x| x.0).unwrap_or(&filename);
            if let Some(base) = sequence_base(stem) {
                names.push(join(&folder, base));
            }
            let parent = filesystem::get_folder(&folder);
            let folder_name = filesystem::get_filename(&folder);
            if parent != folder && !folder_name.is_empty() {
                names.push(join(&parent, &folder_name));
            }
        }
        _ => names.push(path[..path.len() - ext.len() - 1].to_owned())
    }
    names.into_iter().filter_map(|x| filesystem::file_with_extension(&format!("{x}.{ext}"), "mcraw")).collect()
}

/// `clip` of `clip_%06d` or `clip_000123`
fn sequence_base(stem: &str) -> Option<&str> {
    let pattern = stem.rfind("%0").filter(|&i| stem.ends_with('d') && stem[i + 2..stem.len() - 1].bytes().all(|x| x.is_ascii_digit()));
    let without_number = match pattern {
        Some(i) => &stem[..i],
        None => stem.trim_end_matches(|x: char| x.is_ascii_digit())
    };
    if without_number.len() == stem.len() { return None; }
    Some(without_number.trim_end_matches(['_', '-', '.'])).filter(|x| !x.is_empty())
}

fn join(folder: &str, name: &str) -> String {
    let separator = if folder.contains('\\') && !folder.contains('/') { '\\' } else { '/' };
    format!("{}{separator}{name}", folder.trim_end_matches(['/', '\\']))
}

fn is_mcraw(path: &str) -> bool {
    let mut header = [0u8; 8];
    filesystem::open_file(path).and_then(|mut x| x.file.read_exact(&mut header)).is_ok() && header.starts_with(HEADER)
}

// ------------------------------- Container -------------------------------

struct Offset {
    offset: u64,
    timestamp: Option<i64>,
}

#[derive(Default)]
struct Chunks {
    frames: Vec<u64>,
    audio: Vec<u64>,
    gyro: Vec<u64>,
    accelerometer: Vec<u64>,
    ois: Vec<u64>,
}

#[derive(Default)]
struct Walk {
    indexed: Chunks,
    seen: Chunks,
    audio_start: Option<i64>,
    unknown_kinds: Vec<u32>,
}

fn invalid(msg: &str) -> Error {
    Error::new(ErrorKind::InvalidData, msg)
}
fn le_i64(x: &[u8]) -> i64 { i64::from_le_bytes(x[..8].try_into().unwrap()) }
fn le_u32(x: &[u8]) -> u32 { u32::from_le_bytes(x[..4].try_into().unwrap()) }
fn le_f32(x: &[u8]) -> f32 { f32::from_le_bytes(x[..4].try_into().unwrap()) }

fn read_item<T: Read>(stream: &mut T) -> Result<(u32, u32)> {
    Ok((stream.read_u32::<LittleEndian>()?, stream.read_u32::<LittleEndian>()?))
}

fn read_payload<T: Read + Seek>(stream: &mut T, len: u32, file_size: u64) -> Result<Vec<u8>> {
    if stream.stream_position()? + len as u64 > file_size {
        return Err(invalid("Item extends past the end of the file"));
    }
    let mut data = vec![0u8; len as usize];
    stream.read_exact(&mut data)?;
    Ok(data)
}

/// Offsets of `count` `i64 offset, i64 timestamp` entries, those inside the file
fn read_offsets<T: Read>(stream: &mut T, count: u64, file_size: u64) -> Option<Vec<u64>> {
    let mut data = vec![0u8; count as usize * 16];
    stream.read_exact(&mut data).ok()?;
    Some(data.chunks_exact(16).map(|x| le_i64(x) as u64).filter(|x| *x < file_size).collect())
}

/// Frame offsets and timestamps, and where they are in the file
fn read_frame_index<T: Read + Seek>(stream: &mut T, file_size: u64, buffer_start: u64) -> Option<(Vec<Offset>, u64)> {
    let tail = file_size.checked_sub(24).filter(|x| *x >= buffer_start)?;
    stream.seek(SeekFrom::Start(tail)).ok()?;
    let (kind, len) = read_item(stream).ok()?;
    let magic = stream.read_u32::<LittleEndian>().ok()?;
    let count = stream.read_i32::<LittleEndian>().ok()?;
    let data = stream.read_i64::<LittleEndian>().ok()?;
    if kind != BUFFER_INDEX || len != 16 || magic != INDEX_MAGIC || count < 0 || data < buffer_start as i64 || data as u64 + count as u64 * 16 > tail {
        return None;
    }
    stream.seek(SeekFrom::Start(data as u64)).ok()?;
    let mut buf = vec![0u8; count as usize * 16];
    stream.read_exact(&mut buf).ok()?;
    let frames: Vec<Offset> = buf.chunks_exact(16).map(|x| Offset { offset: le_i64(x) as u64, timestamp: Some(le_i64(&x[8..])) }).collect();
    if frames.iter().any(|x| x.offset < buffer_start || x.offset >= data as u64) {
        return None;
    }
    Some((frames, data as u64))
}

/// Walks the items in [start, end), collecting the indexes and the positions of the data chunks
fn walk_items<T: Read + Seek>(stream: &mut T, start: u64, end: u64, file_size: u64, probe: bool, walk: &mut Walk) {
    let mut pos = start;
    while pos + 8 <= end {
        if stream.seek(SeekFrom::Start(pos)).is_err() { break; }
        let Ok((kind, len)) = read_item(stream) else { break; };
        let next = pos + 8 + len as u64;
        if next > end { break; }
        match kind {
            BUFFER                         => walk.seen.frames.push(pos),
            AUDIO_DATA | AUDIO_DATA_F32    => walk.seen.audio.push(pos),
            GYRO_DATA                      => walk.seen.gyro.push(pos),
            ACCELEROMETER_DATA             => walk.seen.accelerometer.push(pos),
            OIS_DATA                       => walk.seen.ois.push(pos),
            METADATA | AUDIO_DATA_METADATA => { }
            AUDIO_INDEX => {
                // i64 count, i64 start timestamp, then the entries
                let mut header = [0u8; 16];
                let count = (len >= 16 && stream.read_exact(&mut header).is_ok()).then(|| le_i64(&header)).filter(|x| *x >= 0);
                match count.filter(|x| (*x as u64).checked_mul(16) == Some(len as u64 - 16)).and_then(|x| read_offsets(stream, x as u64, file_size)) {
                    Some(list) => {
                        walk.audio_start = Some(le_i64(&header[8..]));
                        walk.indexed.audio = list;
                    }
                    None => log::warn!("MotionCam: unsupported audio index at {pos}"),
                }
            }
            GYRO_INDEX | ACCELEROMETER_INDEX | OIS_INDEX => {
                // u32 version, u32 count, then the entries
                let mut header = [0u8; 8];
                let count = (len >= 8 && stream.read_exact(&mut header).is_ok() && le_u32(&header) == STREAM_VERSION).then(|| le_u32(&header[4..]) as u64);
                match count.filter(|x| len as u64 == 8 + x * 16).and_then(|x| read_offsets(stream, x, file_size)) {
                    Some(list) => match kind {
                        GYRO_INDEX          => walk.indexed.gyro = list,
                        ACCELEROMETER_INDEX => walk.indexed.accelerometer = list,
                        _                   => walk.indexed.ois = list,
                    },
                    None => log::warn!("MotionCam: unsupported index item {kind} at {pos}"),
                }
            }
            _ => {
                // A newer item, skipped. The walk is bounded by the frame index data, so its size can be trusted
                if !walk.unknown_kinds.contains(&kind) {
                    log::warn!("MotionCam: skipping unknown item {kind} at {pos}");
                    walk.unknown_kinds.push(kind);
                }
            }
        }
        if probe && !walk.seen.frames.is_empty() && !walk.seen.gyro.is_empty() { break; }
        pos = next;
    }
}

fn read_frame_metadata<T: Read + Seek>(stream: &mut T, offset: u64, file_size: u64) -> Result<Value> {
    stream.seek(SeekFrom::Start(offset))?;
    let (kind, len) = read_item(stream)?;
    if kind != BUFFER { return Err(invalid("Not a frame")); }
    stream.seek(SeekFrom::Current(len as i64))?;
    let (kind, len) = read_item(stream)?;
    if kind != METADATA || len > MAX_JSON_SIZE { return Err(invalid("Frame without metadata")); }
    Ok(serde_json::from_slice(&read_payload(stream, len, file_size)?)?)
}

/// Timestamp and x, y, z of a gyro or accelerometer sample; x, y, 0 of an OIS one
type Sample = (i64, [f32; 3]);

/// The samples of the gyro, accelerometer or OIS chunks. Broken chunks are skipped
fn read_samples<T: Read + Seek>(stream: &mut T, chunks: &[u64], kind: u32, sample_size: u64, file_size: u64) -> Vec<Sample> {
    let mut samples = Vec::new();
    for &offset in chunks {
        match read_chunk(stream, offset, kind, sample_size, file_size) {
            Ok(data) => samples.extend(data.chunks_exact(sample_size as usize).map(|x| {
                let z = if sample_size >= 20 { le_f32(&x[16..]) } else { 0.0 };
                (le_i64(x), [le_f32(&x[8..]), le_f32(&x[12..]), z])
            })),
            Err(e) => log::warn!("MotionCam item {kind} at {offset}: {e}"),
        }
    }
    samples.sort_by_key(|x| x.0);
    samples
}

/// A chunk's samples: after `u32 version, u32 count`, `count` x `sample_size` bytes
fn read_chunk<T: Read + Seek>(stream: &mut T, offset: u64, kind: u32, sample_size: u64, file_size: u64) -> Result<Vec<u8>> {
    stream.seek(SeekFrom::Start(offset))?;
    let (item_kind, len) = read_item(stream)?;
    if item_kind != kind { return Err(invalid("Unexpected item")); }
    let (version, count) = (stream.read_u32::<LittleEndian>()?, stream.read_u32::<LittleEndian>()?);
    if version != STREAM_VERSION || len as u64 != 8 + count as u64 * sample_size {
        return Err(invalid("Invalid chunk"));
    }
    read_payload(stream, len - 8, file_size)
}

struct AudioChunk {
    timestamp: Option<i64>,
    samples: u64, // per channel
}

fn read_audio_chunks<T: Read + Seek>(stream: &mut T, chunks: &[u64], channels: u64, file_size: u64) -> Vec<AudioChunk> {
    chunks.iter().filter_map(|&offset| {
        stream.seek(SeekFrom::Start(offset)).ok()?;
        let (kind, len) = read_item(stream).ok()?;
        let bytes_per_sample = match kind { AUDIO_DATA => 2, AUDIO_DATA_F32 => 4, _ => return None };
        let timestamp = audio_timestamp(stream, offset, len, file_size);
        Some(AudioChunk { timestamp, samples: len as u64 / (bytes_per_sample * channels) })
    }).collect()
}

/// The timestamp in the item after the chunk, added later: older files don't have it
fn audio_timestamp<T: Read + Seek>(stream: &mut T, offset: u64, len: u32, file_size: u64) -> Option<i64> {
    if offset.checked_add(16 + len as u64)? > file_size { return None; }
    stream.seek(SeekFrom::Current(len as i64)).ok()?;
    let (kind, len) = read_item(stream).ok()?;
    if kind != AUDIO_DATA_METADATA || len < 8 { return None; }
    stream.read_i64::<LittleEndian>().ok()
}

// ------------------------------- JSON -------------------------------
// Numbers which don't fit a double (the nanosecond timestamps) are written as strings

fn json_f64(v: Option<&Value>) -> Option<f64> {
    match v? {
        Value::Number(x) => x.as_f64(),
        Value::String(x) => x.trim().parse().ok(),
        _ => None
    }.filter(|x: &f64| x.is_finite())
}
fn json_i64(v: Option<&Value>) -> Option<i64> {
    match v? {
        Value::Number(x) => x.as_i64().or_else(|| x.as_f64().filter(|x| x.is_finite()).map(|x| x.round() as i64)),
        Value::String(x) => x.trim().parse().ok(),
        _ => None
    }
}
fn json_f64_array(v: Option<&Value>) -> Option<Vec<f64>> {
    v?.as_array()?.iter().map(|x| json_f64(Some(x))).collect()
}
