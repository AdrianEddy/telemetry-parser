// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2025 Adrian <adrian.eddy at gmail>

use std::io::*;
use std::sync::{ Arc, atomic::AtomicBool };

use byteorder::{ BigEndian, ReadBytesExt };
use crate::*;
use crate::tags_impl::*;
use memchr::memmem;

#[derive(Default)]
pub struct Nikon {
    pub model: Option<String>,
    pub lens: Option<String>,
    record_framerate: Option<f64>,
    frame_readout_time: Option<f64>,
}
impl Nikon {
    pub fn camera_type(&self) -> String {
        "Nikon".to_owned()
    }
    pub fn has_accurate_timestamps(&self) -> bool {
        true
    }
    pub fn possible_extensions() -> Vec<&'static str> {
        vec!["mp4", "mov", "nev", "r3d"]
    }
    pub fn frame_readout_time(&self) -> Option<f64> {
        self.frame_readout_time
    }
    pub fn normalize_imu_orientation(v: String) -> String {
        v
    }

    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], _filepath: P, _options: &crate::InputOptions) -> Option<Self> {
        if memmem::find(buffer, b"Nikon").is_some() && memmem::find(buffer, b"NCTG").is_some() {
            return Some(Self {
                model: None,
                lens: None,
                record_framerate: None,
                frame_readout_time: None
            });
        }
        None
    }

    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, size: usize, progress_cb: F, cancel_flag: Arc<AtomicBool>, options: crate::InputOptions) -> Result<Vec<SampleInfo>> {
        let mut samples = Vec::new();
        let mut first_map = GroupedTagMap::new();

        while let Ok((typ, _offs, size, header_size)) = util::read_box(stream) {
            if size == 0 || typ == 0 { break; }
            let org_pos = stream.stream_position()?;

            if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) { break; }

            if typ == fourcc("moov") || typ == fourcc("udta") {
                continue; // go inside these boxes
            } else {
                if typ == fourcc("NCDT") {
                    // The clip's metadata is a list of boxes: NCHD (its version), NCTG (the tags), NCTH and NCVW (the
                    // thumbnail and the preview), NCM1, NCM2 and NCDB. The tags are NCTG's data, up to its end
                    let buf = util::read_box_data(stream, size, header_size)?;
                    match util::find_box_data(&buf, b"NCTG") {
                        Some(tags) => if let Err(e) = self.parse_nev_clip_metadata(tags, &mut first_map, &options) {
                            log::warn!("Nikon: the clip metadata can't be read: {e}");
                        },
                        None => log::warn!("Nikon: the clip metadata (NCDT) has no tags (NCTG)"),
                    }
                }

                stream.seek(SeekFrom::Start(org_pos + size - header_size as u64))?;
            }
        }
        stream.seek(SeekFrom::Start(0))?;

        // Of each frame only its header is read (`frame_header`), which holds its metadata
        util::read_track_samples(stream, size, mp4parse::TrackType::Video, true, |mut info: SampleInfo, sample: &mut util::TrackSample<T>, _video_md: Option<&VideoMetadata>| {
            if size > 0 {
                progress_cb(sample.position() as f64 / size as f64);
            }

            if sample.size() > 8 {
                // The clip's metadata goes on its first sample, whichever entry of the track that is. A sample without a
                // frame header (an AVC proxy's) has no metadata of its own
                let mut map = if samples.is_empty() { first_map.clone() } else { GroupedTagMap::new() };
                let header = Self::frame_header(sample)?;
                if let Err(e) = self.parse_nev_frame_metadata(header.unwrap_or_default(), &mut map, &options) {
                    log::warn!("Nikon: the metadata of sample {} can't be read: {e}", info.sample_index);
                }
                if !sample.is_left_out() {
                    info.tag_map = Some(map);
                    samples.push(info);
                }
            }
            Ok(())
        }, cancel_flag)?;

        if samples.is_empty() && !first_map.is_empty() {
            samples.push(SampleInfo {
                tag_map: Some(first_map),
                ..Default::default()
            });
        }

        Ok(samples)
    }

    pub fn parse_nev_clip_metadata(&mut self, data: &[u8], map: &mut GroupedTagMap, options: &crate::InputOptions) -> Result<()> {
        let mut md = serde_json::Map::<String, serde_json::Value>::new();
        let mut cursor = Cursor::new(data);
        let len = data.len() as u64;

        while cursor.position() + 8 <= len {
            // Read tag header
            let tag_id = cursor.read_u32::<BigEndian>()?;
            let type_id = cursor.read_u16::<BigEndian>()?;
            let count = cursor.read_u16::<BigEndian>()? as usize;

            // Calculate value size based on type
            let type_size: usize = match type_id {
                1 | 2 | 6 | 7 => 1,  // BYTE, ASCII, SBYTE, UNDEFINED
                3 | 8 => 2,          // SHORT, SSHORT
                4 | 9 | 11 => 4,     // LONG, SLONG, FLOAT
                5 | 10 | 12 => 8,    // RATIONAL, SRATIONAL, DOUBLE
                _ => 1,
            };
            let value_size = count * type_size;

            if cursor.position() + value_size as u64 > len { break; }

            let mut value_bytes = vec![0u8; value_size];
            cursor.read_exact(&mut value_bytes)?;

            // Helper: read string
            let as_string = || -> String {
                let end = value_bytes.iter().position(|&b| b == 0).unwrap_or(value_bytes.len());
                String::from_utf8_lossy(&value_bytes[..end]).to_string()
            };

            let as_rational = || -> Option<f64> {
                if value_bytes.len() >= 8 {
                    let mut rdr = Cursor::new(&value_bytes);
                    let num = rdr.read_u32::<BigEndian>().ok()?;
                    let den = rdr.read_u32::<BigEndian>().ok()?;
                    if den > 0 { Some(num as f64 / den as f64) } else { None }
                } else { None }
            };

            // Helper: read u16/u32
            let as_u32 = || -> Option<u32> {
                let mut rdr = Cursor::new(&value_bytes);
                match type_id {
                    3 | 8 => rdr.read_u16::<BigEndian>().ok().map(|v| v as u32),
                    4 | 9 => rdr.read_u32::<BigEndian>().ok(),
                    _ => None,
                }
            };
            let as_i32 = || -> Option<i32> {
                let mut rdr = Cursor::new(&value_bytes);
                match type_id {
                    8 => rdr.read_i16::<BigEndian>().ok().map(|v| v as i32),  // SSHORT
                    9 => rdr.read_i32::<BigEndian>().ok(),                    // SLONG
                    6 => rdr.read_i8().ok().map(|v| v as i32),                // SBYTE
                    _ => None,
                }
            };

            let as_f64 = || -> Option<f64> {
                let mut rdr = Cursor::new(&value_bytes);
                match type_id {
                    5 => { // RATIONAL (u32/u32)
                        let num = rdr.read_u32::<BigEndian>().ok()?;
                        let den = rdr.read_u32::<BigEndian>().ok()?;
                        (den != 0).then(|| num as f64 / den as f64)
                    }
                    10 => { // SRATIONAL (i32/i32)
                        let num = rdr.read_i32::<BigEndian>().ok()?;
                        let den = rdr.read_i32::<BigEndian>().ok()?;
                        (den != 0).then(|| num as f64 / den as f64)
                    }
                    11 => rdr.read_f32::<BigEndian>().ok().map(|v| v as f64), // FLOAT
                    12 => rdr.read_f64::<BigEndian>().ok(),                   // DOUBLE
                    1 | 7 => value_bytes.get(0).copied().map(|v| v as f64),    // BYTE/UNDEFINED
                    3 => rdr.read_u16::<BigEndian>().ok().map(|v| v as f64),   // SHORT
                    4 => rdr.read_u32::<BigEndian>().ok().map(|v| v as f64),   // LONG
                    8 => rdr.read_i16::<BigEndian>().ok().map(|v| v as f64),   // SSHORT
                    9 => rdr.read_i32::<BigEndian>().ok().map(|v| v as f64),   // SLONG
                    _ => None,
                }
            };

            // (optional) timecode decode helper: try ASCII, else treat as frame count
            let frames_to_timecode = |frames: u32, fps: f64| -> String {
                let fps_i = fps.round().max(1.0) as u32;
                let ff = frames % fps_i;
                let total_sec = frames / fps_i;
                let ss = total_sec % 60;
                let mm = (total_sec / 60) % 60;
                let hh = (total_sec / 3600) % 100;
                format!("{:02}:{:02}:{:02}:{:02}", hh, mm, ss, ff)
            };

            // Match tags and insert
            match tag_id {
                0x0000_0002 => {
                    let model = as_string();
                    self.model = Some(model.clone());
                    md.insert("camera_model".into(), model.into());
                }
                0x0000_0016 => { // Framerate
                    if let Some(fps) = as_rational() {
                        self.record_framerate = Some(fps);
                        util::insert_tag(map, tag!(parsed GroupId::Default, TagId::FrameRate, "Frame rate", f64, |v| format!("{:.3}", v), fps, vec![]), options);
                        md.insert("framerate".into(), fps.into());
                    }
                }
                0x0000_0017 => { // Record Framerate
                    if let Some(fps) = as_rational() {
                        if self.record_framerate.is_none() {
                            self.record_framerate = Some(fps);
                            util::insert_tag(map, tag!(parsed GroupId::Default, TagId::FrameRate, "Frame rate", f64, |v| format!("{:.3}", v), fps, vec![]), options);
                        }
                        md.insert("record_framerate".into(), fps.into());
                    }
                }
                0x0000_0001 => { md.insert("make".into(), as_string().into()); }
                0x0000_0003 => { md.insert("camera_firmware_version".into(), as_string().into()); }
                0x0000_0011 => { md.insert("local_datetime".into(), as_string().into()); }
                0x0000_0012 => { md.insert("gmt_datetime".into(), as_string().into()); }
                0x0000_0013 => { if let Some(v) = as_u32() { md.insert("record_mode".into(), v.into()); } }
                0x0000_0014 => { if let Some(v) = as_u32() { md.insert("flip_horizontal".into(), v.into()); } }
                0x0000_0015 => { if let Some(v) = as_u32() { md.insert("flip_vertical".into(), v.into()); } }
                0x0000_0019 => { md.insert("timezone".into(), as_string().into()); }
                0x0000_0021 => { if let Some(v) = as_u32() { md.insert("color_space".into(), v.into()); } }
                0x0000_0022 => { if let Some(v) = as_u32() { md.insert("image_width".into(), v.into()); } }
                0x0000_0023 => { if let Some(v) = as_u32() { md.insert("image_height".into(), v.into()); } }
                0x0000_0024 => { if let Some(v) = as_u32() { md.insert("bits_per_component".into(), v.into()); } }
                0x0000_0025 => { if let Some(v) = as_u32() { md.insert("bit_depth".into(), v.into()); } }
                0x0000_0026 => { if let Some(v) = as_u32() { md.insert("audio_channels".into(), v.into()); } }
                0x0000_0027 => { if let Some(v) = as_u32() { md.insert("audio_format".into(), v.into()); } }
                0x0000_0031 => { if let Some(v) = as_u32() { md.insert("channel_mask".into(), v.into()); } }
                0x0000_0032 => { if let Some(v) = as_u32() { md.insert("audio_codec".into(), v.into()); } }
                0x0000_0033 => { if let Some(v) = as_u32() { md.insert("sample_size".into(), v.into()); } }
                0x0000_0034 => { if let Some(v) = as_u32() { md.insert("samplerate".into(), v.into()); } }
                0x0000_1017 => { if let Some(v) = as_u32() { md.insert("white_balance_kelvin".into(), v.into()); } }
                0x0000_101A => { if let Some(v) = as_u32() { md.insert("clip_default_color_version".into(), v.into()); } }

                // Standard EXIF tags (0x01xxxxxx = EXIF IFD prefix)
                0x0100_0112 => { if let Some(v) = as_u32() { md.insert("orientation".into(), v.into()); } }
                0x0110_829A => { // Exposure Time
                    if let Some(val) = as_rational() {
                        util::insert_tag(map, tag!(parsed GroupId::Default, TagId::ExposureTime, "Exposure time", f32, |v| format!("{:.6}", v), val as f32, vec![]), options);
                        md.insert("exposure_time".into(), val.into());
                    }
                }
                0x0110_829D => { // F-Number
                    if let Some(val) = as_rational() {
                        util::insert_tag(map, tag!(parsed GroupId::Lens, TagId::IrisFStop, "Aperture", f32, |v| format!("f/{:.1}", v), val as f32, vec![]), options);
                        md.insert("f_number".into(), val.into());
                    }
                }
                0x0110_8827 | 0x0110_8832 => { // ISO
                    if let Some(val) = as_u32() {
                        util::insert_tag(map, tag!(parsed GroupId::Default, TagId::ISOValue, "ISO", u32, |v| v.to_string(), val, vec![]), options);
                        md.insert("iso".into(), val.into());
                    }
                }
                // EXIF ExposureBiasValue (SRATIONAL) -> your SDK exposure_compensation
                0x0110_9204 => {
                    if let Some(val) = as_f64() {
                        md.insert("exposure_compensation".into(), val.into());
                    }
                }
                0x0110_920A => { // Focal Length
                    if let Some(val) = as_rational() {
                        util::insert_tag(map, tag!(parsed GroupId::Lens, TagId::FocalLength, "Focal length", f32, |v| format!("{:.1} mm", v), val as f32, vec![]), options);
                        md.insert("lens_focal_length".into(), val.into());
                    }
                }
                0x0110_8822 => { if let Some(v) = as_u32() { md.insert("exposure_program".into(), v.into()); } }
                0x0110_9207 => { if let Some(v) = as_u32() { md.insert("metering_mode".into(), v.into()); } }
                0x0110_A431 => { md.insert("camera_pin".into(), as_string().into()); }
                0x0110_A433 => { md.insert("lens_make".into(), as_string().into()); }
                0x0110_A432 => { if let Some(val) = as_rational() { md.insert("lens_info".into(), val.into()); } }
                0x0110_A435 => { md.insert("lens_serial_number".into(), as_string().into()); }
                0x0110_A434 => {
                    let name = as_string();
                    util::insert_tag(map, tag!(parsed GroupId::Lens, TagId::DisplayName, "Lens name", String, |v| v.clone(), name.clone(), vec![]), options);
                    md.insert("lens_name".into(), name.into());
                }

                // Nikon MakerNotes (0x0200xxxx prefix)
                0x0200_0005 => { md.insert("white_balance_setting".into(), as_string().trim().into()); }
                0x0200_0007 => { md.insert("focus_mode".into(), as_string().trim().into()); }
                0x0200_001B => { if let Some(v) = as_u32() { md.insert("nikon_0x1b".into(), v.into()); } }
                0x0200_002A => { if let Some(v) = as_u32() { md.insert("nikon_0x2a".into(), v.into()); } }
                0x0200_003C => { if let Some(v) = as_u32() { md.insert("nikon_0x3c".into(), v.into()); } }
                0x0200_003F => { if let Some(val) = as_rational() { md.insert("exposure_fine_tune".into(), val.into()); } } // Possibly exposure fine tuning
                0x0200_0084 => { if let Some(val) = as_rational() { md.insert("nikon_lens_info".into(), val.into()); } }
                0x0200_00A7 => { if let Some(v) = as_u32() { md.insert("shutter_count".into(), v.into()); } }
                0x0200_00AB => { md.insert("variant_program".into(), as_string().into()); }
                0x0200_00B1 => { if let Some(v) = as_u32() { md.insert("nikon_0xb1".into(), v.into()); } }
                0x0000_0018 => { if let Some(v) = as_u32() { md.insert("audio_format".into(), v.into()); } }
                0x0000_1000 => { // StartEdgeCode
                    if type_id == 2 {
                        md.insert("start_edge_timecode".into(), as_string().into());
                    } else if let (Some(fr), Some(fps)) = (as_u32(), self.record_framerate) {
                        md.insert("start_edge_timecode".into(), frames_to_timecode(fr, fps).into());
                    } else if let Some(fr) = as_u32() {
                        md.insert("start_edge_timecode_frames".into(), fr.into());
                    }
                }
                0x0000_1001 => { // StartTimecode
                    if type_id == 2 {
                        md.insert("start_absolute_timecode".into(), as_string().into());
                    } else if let (Some(fr), Some(fps)) = (as_u32(), self.record_framerate) {
                        md.insert("start_absolute_timecode".into(), frames_to_timecode(fr, fps).into());
                    } else if let Some(fr) = as_u32() {
                        md.insert("start_absolute_timecode_frames".into(), fr.into());
                    }
                }
                0x0000_1071 => { // Model
                    let s = as_string();
                    if !s.is_empty() {
                        self.model = Some(s.clone());
                        md.insert("camera_model".into(), s.into());
                    }
                }
                0x0000_1006 => { let s = as_string(); if !s.is_empty() { md.insert("camera_pin".into(), s.into()); } } // SerialNumber
                0x0000_1023 => { let d = as_string(); if !d.is_empty() { md.insert("local_date".into(), d.into()); } } // DateCreated (YYYYMMDD)
                0x0000_1024 => { let t = as_string(); if !t.is_empty() { md.insert("local_time".into(), t.into()); } } // TimeCreated (HHMMSS)
                0x0000_1025 => { let fw = as_string(); if !fw.is_empty() { md.insert("camera_firmware_version".into(), fw.into()); } }
                0x0000_1036 => { if let Some(v) = as_f64() { md.insert("pixel_aspect_ratio".into(), v.into()); } }
                0x0000_106e => { let s = as_string(); if !s.is_empty() { md.insert("lens_mount".into(), s.into()); } }
                0x0000_1070 => { let s = as_string(); if !s.is_empty() { md.insert("lens_name".into(), s.into()); } }
                0x0000_10a1 => { let s = as_string(); if !s.is_empty() { md.insert("sensor_name".into(), s.into()); } }
                0x0000_200d => { if let Some(v) = as_f64() { md.insert("white_balance_kelvin".into(), v.into()); } }
                0x0000_403b => { if let Some(v) = as_u32() { md.insert("iso".into(), v.into()); } }
                0x0000_406a => { if let Some(v) = as_f64() { md.insert("f_number".into(), v.into()); } }
                0x0000_406b => { if let Some(v) = as_f64() { md.insert("lens_focal_length".into(), v.into()); } }
                _ => {
                    let key = format!("tag_0x{:08x}", tag_id);
                    match type_id {
                        2 => { md.insert(key, as_string().into()); }
                        5 | 10 | 11 | 12 => { if let Some(v) = as_f64() { md.insert(key, v.into()); } }
                        6 | 8 | 9 => { if let Some(v) = as_i32() { md.insert(key, v.into()); } }
                        1 | 3 | 4 | 7 => { if let Some(v) = as_u32() { md.insert(key, v.into()); } }
                        _ => {}
                    }
                }
            }
        }

        // Insert all metadata as JSON
        if !md.is_empty() {
            util::insert_tag(map, tag!(parsed GroupId::Default, TagId::Metadata, "Metadata", Json, |v| serde_json::to_string(v).unwrap(), serde_json::Value::Object(md), vec![]), options);
        }

        Ok(())
    }

    /// The frame header (`NRFH`) of an R3D NE or N-RAW frame, its atoms; `None` for a sample that isn't such a frame, or
    /// when a read leaves the sample out (`TrackSample::read`). A frame is an `NRAW` atom of its format's version
    /// (`NRFM`), the header, an `NRTH` atom (whose `NRTI` states the image's size) and the image: the header, which holds
    /// the frame's metadata, follows the version, 892 bytes 20 bytes in (under 1 KiB of the ZR's frames of up to 7.4 MiB),
    /// and is the only part read. The atoms are found by their sizes, each within its parent
    fn frame_header<'a, T: Read + Seek>(sample: &'a mut util::TrackSample<T>) -> Result<Option<&'a [u8]>> {
        // The size and type of the atom `at` bytes into the frame
        fn atom<T: Read + Seek>(sample: &mut util::TrackSample<T>, at: u64) -> Result<Option<(u64, [u8; 4])>> {
            Ok(sample.read(at, 8)?.map(|h| (u64::from(u32::from_be_bytes([h[0], h[1], h[2], h[3]])), [h[4], h[5], h[6], h[7]])))
        }
        let end = match atom(sample, 0)? {
            Some((size, kind)) if &kind == b"NRAW" && size >= 8 => size.min(sample.size()),
            _ => return Ok(None),
        };
        let mut at = 8;
        while at + 8 <= end {
            let Some((size, kind)) = atom(sample, at)? else { return Ok(None) };
            if size < 8 || at + size > end {
                return Ok(None);
            }
            if &kind == b"NRFH" {
                return sample.read(at + 8, size - 8);
            }
            at += size;
        }
        Ok(None)
    }

    /// The metadata of an R3D NE or N-RAW frame from its frame header's atoms (`frame_header`): the header's version
    /// (`NRHM`) and an `NRMT` atom per item, of the item's tag, a pad byte and its value. The walk stops at an atom it
    /// can't step past. With the clip's metadata already in `map` (the first sample's), the frame's is added to it
    pub fn parse_nev_frame_metadata(&self, data: &[u8], map: &mut GroupedTagMap, options: &crate::InputOptions) -> Result<()> {
        let mut md = serde_json::Map::<String, serde_json::Value>::new();
        if let Some(map) = map.get(&GroupId::Default) {
            if let Some(v) = (map.get_t(TagId::Metadata) as Option<&serde_json::Value>).and_then(|v| v.as_object()) {
                md = v.clone();
            }
        }
        let mut at = 0;
        while let Some(head) = data.get(at..at + 8) {
            let atom_size = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as usize;
            let Some(atom) = data[at..].get(..atom_size).filter(|_| atom_size >= 8) else { break };

            if &atom[4..8] == b"NRMT" && atom_size >= 13 {
                // NRMT structure: [size:4]["NRMT":4][tag_id:4][pad:1][value:N]
                let tag_id = u32::from_be_bytes([atom[8], atom[9], atom[10], atom[11]]);
                let value_bytes = &atom[13..];
                let mut value_cursor = Cursor::new(value_bytes);

                match tag_id {
                    0x0110_0100 => { // ImageWidth
                        if let Ok(v) = value_cursor.read_u32::<BigEndian>() {
                            md.insert("image_width".into(), (v as u64).into());
                        }
                    }
                    0x0110_0101 => { // ImageHeight
                        if let Ok(v) = value_cursor.read_u32::<BigEndian>() {
                            md.insert("image_height".into(), (v as u64).into());
                        }
                    }
                    0x0110_A302 => { // CFAPattern
                        let arr: Vec<serde_json::Value> = value_bytes.iter().map(|&b| (b as u64).into()).collect();
                        md.insert("cfa_pattern".into(), serde_json::Value::Array(arr));
                    }
                    // EXIF-style tags (group 0x0110)
                    0x0110_829A => { // Exposure Time (float)
                        if let Ok(val) = value_cursor.read_f32::<BigEndian>() {
                            util::insert_tag(map, tag!(parsed GroupId::Default, TagId::ExposureTime, "Exposure time", f32, |v| format!("{:.6}", v), val, vec![]), options);
                            md.insert("exposure_time".into(), (val as f64).into());
                        }
                    }
                    0x0110_829D => { // F-Number (float)
                        if let Ok(val) = value_cursor.read_f32::<BigEndian>() {
                            util::insert_tag(map, tag!(parsed GroupId::Lens, TagId::IrisFStop, "Aperture", f32, |v| format!("f/{:.1}", v), val, vec![]), options);
                            md.insert("f_number".into(), (val as f64).into());
                        }
                    }
                    0x0110_8832 => { // ISO (u32)
                        if let Ok(val) = value_cursor.read_u32::<BigEndian>() {
                            util::insert_tag(map, tag!(parsed GroupId::Default, TagId::ISOValue, "ISO", u32, |v| v.to_string(), val, vec![]), options);
                            md.insert("iso".into(), val.into());
                        }
                    }
                    0x0110_9204 => { // Exposure Compensation (float)
                        if let Ok(val) = value_cursor.read_f32::<BigEndian>() {
                            md.insert("exposure_compensation".into(), (val as f64).into());
                        }
                    }
                    0x0110_920A => { // Focal Length (float)
                        if let Ok(val) = value_cursor.read_f32::<BigEndian>() {
                            util::insert_tag(map, tag!(parsed GroupId::Lens, TagId::FocalLength, "Focal length", f32, |v| format!("{:.1} mm", v), val, vec![]), options);
                            md.insert("lens_focal_length".into(), (val as f64).into());
                        }
                    }
                    0x0110_0112 => { // Orientation
                        if let Ok(val) = value_cursor.read_u16::<BigEndian>() {
                            md.insert("orientation".into(), val.into());
                        }
                    }
                    // Nikon-specific tags (group 0x0190)
                    0x0190_0010 => { // White Balance Kelvin
                        if let Ok(val) = value_cursor.read_u16::<BigEndian>() {
                            md.insert("white_balance_kelvin".into(), val.into());
                        }
                    }
                    0x0190_0012 => { // Color/Orientation Matrix (3x3 floats)
                        if value_bytes.len() >= 36 {
                            let mut matrix = Vec::with_capacity(9);
                            for _ in 0..9 {
                                if let Ok(f) = value_cursor.read_f32::<BigEndian>() {
                                    matrix.push(f as f64);
                                }
                            }
                            if matrix.len() == 9 {
                                md.insert("color_matrix".into(), serde_json::to_value(matrix).unwrap_or_default());
                            }
                        }
                    }
                    _ => {
                        // Store unknown with hex ID
                        let key = format!("tag_0x{:08x}", tag_id);
                        if let Ok(v) = value_cursor.read_f32::<BigEndian>() {
                            if v.is_finite() && v.abs() < 1e10 {
                                md.insert(key, (v as f64).into());
                            }
                        }
                    }
                }
            }
            at += atom_size;
        }

        // Insert frame metadata as JSON
        if !md.is_empty() {
            let (sensor_size, pixel_pitch) = match self.model.as_deref() {
                Some("NIKON ZR") => (Some((6060, 4032)), Some((5930, 5930))),
                _ => (None, None)
            };
            if let Some(pp) = pixel_pitch {
                if let Some(ss) = sensor_size {
                    util::insert_tag(map, tag!(parsed GroupId::Imager, TagId::SensorSizePixels, "Sensor Size Pixels", u32x2, |v| format!("{v:?}"), ss, vec![]), &options);

                    if let Some(iw) = md.get("image_width").and_then(|v| v.as_u64()).map(|v| v as u32) {
                        if let Some(ih) = md.get("image_height").and_then(|v| v.as_u64()).map(|v| v as u32) {
                            util::insert_tag(map, tag!(parsed GroupId::Imager, TagId::CaptureAreaSize, "Capture Area Size", f32x2, |v| format!("{v:?}"), (iw as f32, ih as f32), vec![]), &options);
                            // Set origin to center
                            util::insert_tag(map, tag!(parsed GroupId::Imager, TagId::CaptureAreaOrigin, "Capture Area Origin", f32x2, |v| format!("{v:?}"), (((ss.0 as f32 - iw as f32)) / 2.0, ((ss.1 as f32 - ih as f32)) / 2.0), vec![]), &options);
                        }
                    }
                }
                util::insert_tag(map, tag!(parsed GroupId::Imager, TagId::PixelPitch, "Pixel pitch", u32x2, |v| format!("{v:?}"), pp, vec![]), &options);
            }

            util::insert_tag(map, tag!(parsed GroupId::Default, TagId::Metadata, "Metadata", Json, |v| serde_json::to_string(v).unwrap(), serde_json::Value::Object(md), vec![]), options);
        }

        Ok(())
    }
}
