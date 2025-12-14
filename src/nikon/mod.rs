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
        vec!["mp4", "mov", "nev"]
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
                    let mut buf = vec![0u8; size as usize - header_size as usize];
                    stream.read_exact(&mut buf)?;
                    self.parse_nev_clip_metadata(&buf[26..], &mut first_map, &options).unwrap();
                }

                stream.seek(SeekFrom::Start(org_pos + size - header_size as u64))?;
            }
        }
        stream.seek(SeekFrom::Start(0))?;

        util::get_track_samples(stream, size, mp4parse::TrackType::Video, true, None, |mut info: SampleInfo, data: &[u8], file_position: u64, _video_md: Option<&VideoMetadata>| {
            if size > 0 {
                progress_cb(file_position as f64 / size as f64);
            }

            if data.len() > 8 {
                let mut map = if info.sample_index == 0 { first_map.clone() } else { GroupedTagMap::new() };
                self.parse_nev_frame_metadata(&data, &mut map, &options).unwrap();
                info.tag_map = Some(map);
                samples.push(info);
            }
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

            // Helper: read rational as f64
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
                0x0000_0001 => { md.insert("make".into(), as_string().into()); }
                0x0000_0002 => {
                    let model = as_string();
                    self.model = Some(model.clone());
                    md.insert("camera_model".into(), model.into());
                }
                0x0000_0003 => { md.insert("camera_firmware_version".into(), as_string().into()); }
                0x0000_0011 => { md.insert("local_datetime".into(), as_string().into()); }
                0x0000_0012 => { md.insert("gmt_datetime".into(), as_string().into()); }
                0x0000_0013 => { // Unknown - possibly record_mode or similar
                    if let Some(v) = as_u32() { md.insert("record_mode".into(), v.into()); }
                }
                0x0000_0014 => { // flip_horizontal
                    if let Some(v) = as_u32() { md.insert("flip_horizontal".into(), v.into()); }
                }
                0x0000_0015 => { // flip_vertical
                    if let Some(v) = as_u32() { md.insert("flip_vertical".into(), v.into()); }
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
                0x0000_0019 => { md.insert("timezone".into(), as_string().into()); }
                0x0000_0021 => { // Color space/version
                    if let Some(v) = as_u32() { md.insert("color_space".into(), v.into()); }
                }
                0x0000_0022 => { // Image Width
                    if let Some(v) = as_u32() { md.insert("image_width".into(), v.into()); }
                }
                0x0000_0023 => { // Image Height
                    if let Some(v) = as_u32() { md.insert("image_height".into(), v.into()); }
                }
                0x0000_0024 => { // Possibly bits per component or channel count
                    if let Some(v) = as_u32() { md.insert("bits_per_component".into(), v.into()); }
                }
                0x0000_0025 => { // Bit depth
                    if let Some(v) = as_u32() { md.insert("bit_depth".into(), v.into()); }
                }
                0x0000_0026 => { // Audio channels
                    if let Some(v) = as_u32() { md.insert("audio_channels".into(), v.into()); }
                }
                0x0000_0027 => { // Audio format
                    if let Some(v) = as_u32() { md.insert("audio_format".into(), v.into()); }
                }
                0x0000_0031 => { // Channel mask (SDK: 3)
                    if let Some(v) = as_u32() { md.insert("channel_mask".into(), v.into()); }
                }
                0x0000_0032 => { // Audio related
                    if let Some(v) = as_u32() { md.insert("audio_codec".into(), v.into()); }
                }
                0x0000_0033 => { // Sample size (SDK: 32)
                    if let Some(v) = as_u32() { md.insert("sample_size".into(), v.into()); }
                }
                0x0000_0034 => { // Sample Rate
                    if let Some(v) = as_u32() { md.insert("samplerate".into(), v.into()); }
                }

                // White balance related (0x1xxx)
                0x0000_1017 => { // White balance kelvin
                    if let Some(v) = as_u32() { md.insert("white_balance_kelvin".into(), v.into()); }
                }
                0x0000_101A => { // Possibly color version
                    if let Some(v) = as_u32() { md.insert("clip_default_color_version".into(), v.into()); }
                }

                // Standard EXIF tags (0x01xxxxxx = EXIF IFD prefix)
                0x0100_0112 => { // Orientation
                    if let Some(v) = as_u32() { md.insert("orientation".into(), v.into()); }
                }
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
                0x0110_8822 => { // ExposureProgram
                    if let Some(v) = as_u32() { md.insert("exposure_program".into(), v.into()); }
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
                0x0110_9207 => { // MeteringMode
                    if let Some(v) = as_u32() { md.insert("metering_mode".into(), v.into()); }
                }
                0x0110_920A => { // Focal Length
                    if let Some(val) = as_rational() {
                        util::insert_tag(map, tag!(parsed GroupId::Lens, TagId::FocalLength, "Focal length", f32, |v| format!("{:.1} mm", v), val as f32, vec![]), options);
                        md.insert("lens_focal_length".into(), val.into());
                    }
                }
                0x0110_A431 => { // Camera Serial/PIN
                    md.insert("camera_pin".into(), as_string().into());
                }
                0x0110_A432 => { // LensInfo (min/max focal length, min/max aperture)
                    if let Some(val) = as_rational() {
                        md.insert("lens_info".into(), val.into());
                    }
                }
                0x0110_A433 => { // LensMake
                    md.insert("lens_make".into(), as_string().into());
                }
                0x0110_A434 => { // Lens Name
                    let name = as_string();
                    util::insert_tag(map, tag!(parsed GroupId::Lens, TagId::DisplayName, "Lens name", String, |v| v.clone(), name.clone(), vec![]), options);
                    md.insert("lens_name".into(), name.into());
                }
                0x0110_A435 => { // LensSerialNumber
                    md.insert("lens_serial_number".into(), as_string().into());
                }

                // Nikon MakerNotes (0x0200xxxx prefix)
                0x0200_0005 => { // White balance setting
                    md.insert("white_balance_setting".into(), as_string().trim().into());
                }
                0x0200_0007 => { // Focus Mode
                    md.insert("focus_mode".into(), as_string().trim().into());
                }
                0x0200_001B => { // Unknown
                    if let Some(v) = as_u32() { md.insert("nikon_0x1b".into(), v.into()); }
                }
                0x0200_002A => { // Possibly VR mode or similar
                    if let Some(v) = as_u32() { md.insert("nikon_0x2a".into(), v.into()); }
                }
                0x0200_003C => { // Unknown
                    if let Some(v) = as_u32() { md.insert("nikon_0x3c".into(), v.into()); }
                }
                0x0200_003F => { // Possibly exposure fine tuning
                    if let Some(val) = as_rational() { md.insert("exposure_fine_tune".into(), val.into()); }
                }
                0x0200_0084 => { // Nikon Lens info
                    if let Some(val) = as_rational() { md.insert("nikon_lens_info".into(), val.into()); }
                }
                0x0200_00A7 => { // Shutter Count
                    if let Some(v) = as_u32() { md.insert("shutter_count".into(), v.into()); }
                }
                0x0200_00AB => { // Variant program string
                    md.insert("variant_program".into(), as_string().into());
                }
                0x0200_00B1 => { // Unknown
                    if let Some(v) = as_u32() { md.insert("nikon_0xb1".into(), v.into()); }
                }

                // Audio / channel basics you already *see* as unknown tags:
                0x0000_0018 => { // audio_format
                    if let Some(v) = as_u32() { md.insert("audio_format".into(), v.into()); }
                }

                // --- Red/R3D-style tags (ExifTool "Red Tags") ---
                0x0000_1000 => { // StartEdgeCode
                    // prefer ASCII if that's what the file stores, else frames->timecode
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
                0x0000_1006 => { // SerialNumber
                    let s = as_string();
                    if !s.is_empty() { md.insert("camera_pin".into(), s.into()); }
                }
                0x0000_1023 => { // DateCreated (YYYYMMDD)
                    let d = as_string();
                    if !d.is_empty() { md.insert("local_date".into(), d.into()); }
                }
                0x0000_1024 => { // TimeCreated (HHMMSS)
                    let t = as_string();
                    if !t.is_empty() { md.insert("local_time".into(), t.into()); }
                }
                0x0000_1025 => { // FirmwareVersion
                    let fw = as_string();
                    if !fw.is_empty() { md.insert("camera_firmware_version".into(), fw.into()); }
                }
                0x0000_1036 => { // AspectRatio
                    if let Some(v) = as_f64() { md.insert("pixel_aspect_ratio".into(), v.into()); }
                }
                0x0000_106e => { // LensMake
                    let s = as_string();
                    if !s.is_empty() { md.insert("lens_mount".into(), s.into()); }
                }
                0x0000_1070 => { // LensModel
                    let s = as_string();
                    if !s.is_empty() { md.insert("lens_name".into(), s.into()); }
                }
                0x0000_1071 => { // Model
                    let s = as_string();
                    if !s.is_empty() {
                        self.model = Some(s.clone());
                        md.insert("camera_model".into(), s.into());
                    }
                }
                0x0000_10a1 => { // Sensor
                    let s = as_string();
                    if !s.is_empty() { md.insert("sensor_name".into(), s.into()); }
                }
                0x0000_200d => { // ColorTemperature
                    if let Some(v) = as_f64() { md.insert("white_balance_kelvin".into(), v.into()); }
                }
                0x0000_403b => { // ISO
                    if let Some(v) = as_u32() { md.insert("iso".into(), v.into()); }
                }
                0x0000_406a => { // FNumber
                    if let Some(v) = as_f64() { md.insert("f_number".into(), v.into()); }
                }
                0x0000_406b => { // FocalLength
                    if let Some(v) = as_f64() { md.insert("lens_focal_length".into(), v.into()); }
                }

                _ => {
                    // Improved unknown-tag storage:
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

    /// Parse per-frame metadata from NRAW frame data
    /// Input: Raw bytes of frame (contains NRFH with NRMT atoms inside)
    /// Returns per-frame tag map
    pub fn parse_nev_frame_metadata(&self, data: &[u8], map: &mut GroupedTagMap, options: &crate::InputOptions) -> Result<()> {
        let mut md = serde_json::Map::<String, serde_json::Value>::new();
        if let Some(map) = map.get(&GroupId::Default) {
            if let Some(v) = map.get_t(TagId::Metadata) as Option<&serde_json::Value> {
                md = v.as_object().unwrap().clone();
            }
        }
        let mut cursor = Cursor::new(data);
        let len = data.len() as u64;

        // Scan for NRMT atoms
        while cursor.position() + 12 <= len {
            let atom_start = cursor.position();

            let atom_size = cursor.read_u32::<BigEndian>()? as u64;

            // Validate size
            if atom_size < 12 || atom_start + atom_size > len {
                cursor.set_position(atom_start + 1);
                continue;
            }

            let mut magic = [0u8; 4];
            cursor.read_exact(&mut magic)?;
            // println!("{}{}{}{}", magic[0] as char, magic[1] as char, magic[2] as char, magic[3] as char);

            if &magic == b"NRMT" && atom_size >= 13 {
                // NRMT structure: [size:4]["NRMT":4][tag_id:4][pad:1][value:N]
                let tag_id = cursor.read_u32::<BigEndian>()?;
                let _padding = cursor.read_u8()?; // Skip padding byte
                let value_size = (atom_size - 13) as usize; // 4+4+4+1 = 13 bytes header

                let mut value_bytes = vec![0u8; value_size];
                cursor.read_exact(&mut value_bytes)?;
                let mut value_cursor = Cursor::new(&value_bytes);

                match tag_id {
                    0x0110_0100 => { // ImageWidth
                        // Prefer u32 if possible; fallback to heuristic decode
                        let v = u32::from_be_bytes(value_bytes[0..4].try_into().unwrap());
                        md.insert("image_width".into(), (v as u64).into());
                    }
                    0x0110_0101 => { // ImageHeight (ImageLength)
                        let v = u32::from_be_bytes(value_bytes[0..4].try_into().unwrap());
                        md.insert("image_height".into(), (v as u64).into());
                    }

                    // ---- NEW: CFAPattern is UNDEFINED bytes; don't parse as float ----
                    0x0110_A302 => { // CFAPattern
                        // Also store as byte array for convenience
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
            } else if &magic == b"NRAW" || &magic == b"NRFM" || &magic == b"NRFH" || &magic == b"NRHM" || &magic == b"NRTH" {
                // Container atoms - parse contents (don't skip, just continue from current position)
            } else if &magic == b"NRTI" {
                // Thumbnail atom - skip entire atom
                let mut _unknown = [0u8; 4];
                cursor.read_exact(&mut _unknown)?;
                let thumb_size = cursor.read_u32::<BigEndian>()? as u64;
                cursor.set_position(atom_start + thumb_size + 8);
            } else {
                // Unknown atom with valid size - skip it
                if atom_size >= 8 {
                    cursor.set_position(atom_start + atom_size);
                } else {
                    // Invalid size - advance by 1 byte and try again
                    cursor.set_position(atom_start + 1);
                }
            }
        }

        // Insert frame metadata as JSON
        if !md.is_empty() {
            let (sensor_size, pixel_pitch) = match self.model.as_deref() {
                Some("NIKON ZR") => (Some((6048, 4032)), Some((5930, 5930))),
                _ => (None, None)
            };
            if let Some(pp) = pixel_pitch {
                if let Some(ss) = sensor_size {
                    util::insert_tag(map, tag!(parsed GroupId::Imager, TagId::SensorSizePixels, "Sensor Size Pixels", u32x2, |v| format!("{v:?}"), ss, vec![]), &options);

                    if let Some(iw) = md.get("image_width").and_then(|v| v.as_u64()).map(|v| v as u32) {
                        if let Some(ih) = md.get("image_height").and_then(|v| v.as_u64()).map(|v| v as u32) {
                            util::insert_tag(map, tag!(parsed GroupId::Imager, TagId::CaptureAreaSize, "Capture Area Size", f32x2, |v| format!("{v:?}"), (iw as f32, ih as f32), vec![]), &options);
                            // Set origin to center
                            util::insert_tag(map, tag!(parsed GroupId::Imager, TagId::CaptureAreaOrigin, "Capture Area Origin", f32x2, |v| format!("{v:?}"), (((ss.0 - iw) as f32) / 2.0, ((ss.1 - ih) as f32) / 2.0), vec![]), &options);
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
