// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2022 Adrian <adrian.eddy at gmail>

use std::io::*;
use std::path::Path;
use std::sync::{ Arc, atomic::AtomicBool };
use std::collections::HashMap;

use crate::tags_impl::*;
use crate::*;
use byteorder::{ ReadBytesExt, BigEndian };

#[derive(Default)]
pub struct RedR3d {
    pub model: Option<String>,
    record_framerate: Option<f64>,
    all_parts: Vec<String>,
}

impl RedR3d {
    pub fn camera_type(&self) -> String {
        if self.model.is_some() {
            "RED".to_owned()
        } else {
            "RED RAW".to_owned()
        }
    }
    pub fn has_accurate_timestamps(&self) -> bool {
        false
    }
    pub fn possible_extensions() -> Vec<&'static str> {
        vec!["r3d", "mp4", "mov", "mxf"]
    }
    pub fn frame_readout_time(&self) -> Option<f64> {
        None
    }
    pub fn normalize_imu_orientation(v: String) -> String {
        v
    }

    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], filepath: P) -> Option<Self> {
        if let Some(ext) = filepath.as_ref().extension() {
            if ext.to_ascii_lowercase() != "r3d" {
                if filepath.as_ref().with_extension("R3D").exists() {
                    return Some(Self {
                        model: None,
                        record_framerate: None,
                        all_parts: Self::detect_all_parts(filepath.as_ref().with_extension("R3D").as_path()).unwrap_or_default()
                    })
                }
                if filepath.as_ref().with_extension("").exists() {
                    let all_parts = Self::detect_all_parts(filepath.as_ref().with_extension("").as_path()).unwrap_or_default();
                    if all_parts.is_empty() { return None; }
                    return Some(Self { model: None, record_framerate: None, all_parts });
                }
                return None;
            }
        }
        if buffer.len() > 8 && &buffer[4..8] == b"RED2" {
            Some(Self {
                model: None,
                record_framerate: None,
                all_parts: Self::detect_all_parts(filepath.as_ref()).unwrap_or_default()
            })
        } else {
            None
        }
    }

    fn detect_all_parts(path: &Path) -> Result<Vec<String>> {
        let mut ret = Vec::new();
        if let Some(filename) = path.file_name().map(|x| x.to_string_lossy()) {
            if let Some(pos) = filename.rfind('_') {
                let filename_base = &filename[0..pos + 1];
                let rmd = format!("{}.rmd", &filename[0..pos]).to_ascii_lowercase();

                if let Some(parent) = path.parent() {
                    for x in parent.read_dir()? {
                        let x = x?;
                        let fname = x.file_name().to_string_lossy().to_string();
                        let fname_lower = fname.to_lowercase();
                        if (fname.starts_with(filename_base) && fname_lower.ends_with(".r3d")) || (fname_lower == rmd) {
                            if let Some(p) = x.path().to_str() {
                                ret.push(p.to_string());
                            }
                        }
                    }
                }
            }
        }
        if ret.is_empty() && path.extension().unwrap_or_default().to_ascii_lowercase().to_string_lossy().ends_with("r3d") {
            if let Some(p) = path.to_str() {
                ret.push(p.to_string());
            }
        }
        ret.sort_by(|a, b| human_sort::compare(a, b));
        Ok(ret)
    }
    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, _stream: &mut T, _size: usize, progress_cb: F, cancel_flag: Arc<AtomicBool>) -> Result<Vec<SampleInfo>> {
        let mut gyro = Vec::new();
        let mut accl = Vec::new();
        let mut first_timestamp = None;
        let mut map = GroupedTagMap::new();
        let mut samples = Vec::new();

        let all_parts = self.all_parts.clone();
        let mut data4096 = Vec::with_capacity(4096);
        data4096.resize(4096, 0);

        let mut csv = String::new();
        let mut rmd = HashMap::<String, String>::new();

        let total_count = all_parts.len() as f64;

        for (i, file) in all_parts.into_iter().enumerate() {
            if file.to_ascii_lowercase().ends_with(".rmd") {
                rmd.extend(Self::parse_rmd(&file));
                continue;
            }

            let stream = std::fs::File::open(file)?;
            let filesize = stream.metadata()?.len() as usize;

            let mut stream = std::io::BufReader::with_capacity(128*1024, stream);

            while let Ok(size) = stream.read_u32::<BigEndian>() {
                let mut name = [0u8; 4];
                stream.read_exact(&mut name)?;
                let aligned_size = ((size as f64 / 4096.0).ceil() * 4096.0) as usize;
                // log::debug!("Name: {}{}{}{}, size: {}", name[0] as char, name[1] as char, name[2] as char, name[3] as char, aligned_size);
                if &name == b"RDX\x01" || &name == b"RDX\x02" {
                    let mut data = Vec::with_capacity(aligned_size);
                    data.resize(aligned_size, 0);
                    stream.seek(SeekFrom::Current(-8))?;
                    stream.read_exact(&mut data)?;
                    if data.len() > 4096 && (size as usize) <= data.len() {
                        let mut data = &data[4096..size as usize];

                        crate::try_block!({
                            if &name == b"RDX\x01" {
                                csv.push_str(std::str::from_utf8(data).ok()?);
                            } else {
                                while let Ok(timestamp) = data.read_u64::<BigEndian>() {
                                    if first_timestamp.is_none() {
                                        first_timestamp = Some(timestamp);
                                    }
                                    let t = (timestamp - first_timestamp.unwrap()) as f64 / 1000000.0;
                                    accl.push(TimeVector3 { t,
                                        x: -data.read_i16::<BigEndian>().ok()? as f64 / 100.0,
                                        y: -data.read_i16::<BigEndian>().ok()? as f64 / 100.0,
                                        z: -data.read_i16::<BigEndian>().ok()? as f64 / 100.0
                                    });
                                    gyro.push(TimeVector3 { t,
                                        x: data.read_i16::<BigEndian>().ok()? as f64 / 10.0,
                                        y: data.read_i16::<BigEndian>().ok()? as f64 / 10.0,
                                        z: data.read_i16::<BigEndian>().ok()? as f64 / 10.0
                                    });
                                }
                            }
                        });
                    }
                } else if &name == b"RED2" {
                    let mut data = Vec::with_capacity(aligned_size);
                    data.resize(aligned_size, 0);
                    stream.seek(SeekFrom::Current(-8))?;
                    stream.read_exact(&mut data)?;
                    if data.len() > 126 {
                        if let Some(offs) = memchr::memmem::find(&data, b"rdx\x02\x00\x00\x00\x00\x00\x00\x00\x01RED ")
                                     .or_else(|| memchr::memmem::find(&data, b"rdx\x01\x00\x00\x00\x00\x00\x00\x00\x05REDT")) {
                            if let Ok(size) = (&data[offs + 16..]).read_u16::<BigEndian>() {
                                let _ = self.parse_meta(&data[offs + 16 + 2..offs + 16 + 2 + size as usize], &mut map);
                            }
                        }
                    }
                } else if &name == b"RDI\x01" {
                    if aligned_size >= 4096 {
                        stream.read_exact(&mut data4096)?;
                        stream.seek(SeekFrom::Current(aligned_size as i64 - 8 - 4096))?;
                        if let Ok(size) = (&data4096[86..]).read_u16::<BigEndian>() {
                            let mut per_frame_map = GroupedTagMap::new();
                            let _ = self.parse_meta(&data4096[88..88 + size as usize], &mut per_frame_map);
                            samples.push(SampleInfo { tag_map: Some(per_frame_map), ..Default::default() });
                        }
                    } else {
                        stream.seek(SeekFrom::Current(aligned_size as i64 - 8))?;
                    }
                } else {
                    stream.seek(SeekFrom::Current(aligned_size as i64 - 8))?;
                }

                if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) { break; }
                if filesize > 0 {
                    progress_cb((i as f64 + (stream.stream_position()? as f64 / filesize as f64)) / total_count);
                }
            }
        }
        if !csv.is_empty() {
            util::insert_tag(&mut map, tag!(parsed GroupId::Default,   TagId::Custom("CSV".into()), "Custom CSV data", String, |v| v.clone(), csv, vec![]));
        }
        if !rmd.is_empty() {
            /*if let Some(Ok(fps)) = rmd.get("frame_rate_override").map(|x| x.parse::<f64>()) {
                self.record_framerate = Some(fps);
            }*/
            if let Some(v) = rmd.get("lens") {
                util::insert_tag(&mut map, tag!(parsed GroupId::Lens, TagId::Name, "Lens name", String, |v| v.clone(), v.into(), vec![]));
            }
            crate::try_block!({
                if let TagValue::Json(ref mut md) = map.get_mut(&GroupId::Default)?.get_mut(&TagId::Metadata)?.value {
                    if let Some(md) = md.get_mut().as_object_mut() {
                        for (k, v) in rmd.drain() {
                            if k == "fittype" {
                                if v.starts_with("Fit Width ") || v.starts_with("Fit Height ") {
                                    if let Ok(num) = v.replace("Fit Width ", "").replace("Fit Height ", "").replace("x", "").parse::<f64>() {
                                        if v.starts_with("Fit Width") {
                                            md.insert("horizontal_stretch".into(), num.into());
                                        } else {
                                            md.insert("vertical_stretch".into(), num.into());
                                        }
                                    }
                                }
                            } else {
                                md.insert(k, v.into());
                            }
                        }
                    }
                }
            });
        }

        // Try to get the sync data, if no async data present
        if accl.is_empty() && gyro.is_empty() && !samples.is_empty() {
            let mut timestamp = 0.0;
            for sample in &samples {
                if let Some(ref map) = sample.tag_map {
                    if let Some(g) = map.get(&GroupId::Default) {
                        if let Some(arr) = g.get_t(TagId::Metadata) as Option<&serde_json::Value> {
                            if let Some(camera_acceleration) = arr.get("camera_acceleration").and_then(|x| x.as_array()) {
                                if camera_acceleration.len() == 3 {
                                    accl.push(TimeVector3 { t: timestamp,
                                        x: -camera_acceleration[0].as_f64().unwrap_or(0.0),
                                        y: -camera_acceleration[1].as_f64().unwrap_or(0.0),
                                        z: -camera_acceleration[2].as_f64().unwrap_or(0.0),
                                    });
                                }
                            }
                            if let Some(camera_rotation) = arr.get("camera_rotation").and_then(|x| x.as_array()) {
                                if camera_rotation.len() == 3 {
                                    gyro.push(TimeVector3 { t: timestamp,
                                        x: camera_rotation[0].as_f64().unwrap_or(0.0),
                                        y: camera_rotation[1].as_f64().unwrap_or(0.0),
                                        z: camera_rotation[2].as_f64().unwrap_or(0.0)
                                    });
                                }
                            }
                            timestamp += 1.0 / self.record_framerate.unwrap();
                        }
                    }
                }
            }
        }

        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Data, "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), accl, vec![]));
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Data, "Gyroscope data",     Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]));

        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Unit, "Accelerometer unit", String, |v| v.to_string(), "m/s²".into(),  Vec::new()));
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Unit, "Gyroscope unit",     String, |v| v.to_string(), "deg/s".into(), Vec::new()));

        if let Some(fr) = self.record_framerate {
            util::insert_tag(&mut map, tag!(parsed GroupId::Default,   TagId::FrameRate, "Frame rate", f64, |v| format!("{:?}", v), fr, vec![]));
        }

        let imu_orientation = "zyx";
        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()));
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()));

        samples.insert(0, SampleInfo { tag_map: Some(map), ..Default::default() });

        Ok(samples)
    }

    fn parse_meta(&mut self, mut data: &[u8], map: &mut GroupedTagMap) -> Result<()> {
        let mut md = serde_json::Map::<String, serde_json::Value>::new();
        while let Ok(size) = data.read_u16::<BigEndian>() {
            if size > 2 {
                let mut d = Vec::with_capacity(size as usize - 2);
                d.resize(size as usize - 2, 0);
                data.read_exact(&mut d)?;
                let mut id = match d[1] {
                    0x06 => "camera_pin",
                    0x08 => "exposure_time",
                    0x0d => "white_balance_kelvin",
                    0x0e => "white_balance_tint",
                    0x0F => "saturation",
                    0x11 => "brightness",
                    0x13 => "contrast",
                    0x19 => "camera_id",
                    0x1A => "reel_id",
                    0x1B => "clip_id",
                    0x23 => "local_date",
                    0x24 => "local_time",
                    0x25 => "camera_firmware_version",
                    0x30 => "gmt_date",
                    0x31 => "gmt_time",
                    0x3b => "iso",
                    0x56 => "file_name",
                    0x65 => "firmware_revision",
                    0x66 => "record_framerate",
                    0x6B => "focal_length",
                    0x6C => "focus_distance",
                    0x74 => "lens_focus_distance_near",
                    0x75 => "lens_focus_distance_far",
                    0x6E => "lens_brand",
                    0x70 => "lens_name",
                    0x71 => "camera_network_name",
                    0x76 => "user_production_name",
                    0x77 => "user_director",
                    0x78 => "user_director_of_photography",
                    0x79 => "user_copyright",
                    0x7A => "user_unit",
                    0x7B => "user_location",
                    0x7C => "user_camera_operator",
                    0x7D => "user_scene",
                    0x7E => "user_take",
                    0x7F => "camera_acceleration", // x/y/z
                    0x80 => "camera_rotation", // x/y/z
                    0x86 => "resolution_format_name",
                    0x9D => "lens_serial_number",
                    0x9E => "lens_owner",
                    0xA0 => "camera_model",
                    0xA1 => "sensor_name",
                    0xAB => "3d_lut1",
                    0xB0 => "fps", // / 1001
                    0xBE => "redcode",
                    0xBF => "record_fps", // / 1001
                    0xC1 => "3d_lut2",
                    _ => "",
                }.to_string();
                if id.is_empty() { id = format!("0x{:x}", d[1]); };

                let num_items = match id.as_str() {
                    "camera_acceleration" => 3, // x/y/z
                    "camera_rotation"     => 3, // x/y/z
                    _ => 1,
                };

                let mut items = vec![];
                for i in 0..num_items {
                    let v = match d[0] {
                        0x10 => serde_json::to_value(std::str::from_utf8(&d[2..]).unwrap_or(&"")),
                        0x20 => serde_json::to_value((&d[2 + i*4..]).read_f32::<BigEndian>()? as f64),
                        0x30 => serde_json::to_value((&d[2 + i*1..]).read_u8()?),
                        0x40 => serde_json::to_value((&d[2 + i*2..]).read_i16::<BigEndian>()?),
                        0x60 => serde_json::to_value((&d[2 + i*4..]).read_u32::<BigEndian>()?),
                        _ => {
                            // log::debug!("Type: {}, id: {}, hex: {}", d[0], id, pretty_hex::pretty_hex(&d));
                            Err(serde_json::Error::io(ErrorKind::InvalidData.into()))
                        }
                    };
                    if let Ok(v) = v {
                        if id == "camera_model" { self.model = v.as_str().map(|x| x.to_string()); }
                        if id == "record_framerate" { self.record_framerate = v.as_f64(); }

                        items.push(v);
                        // log::debug!("{}: {:?}", id, v);
                    }
                }
                if items.len() == 1 {
                    md.insert(id.clone(), items.into_iter().next().unwrap());
                } else {
                    md.insert(id.clone(), serde_json::to_value(items)?);
                }
            } else {
                break;
            }
        }
        if !md.is_empty() {
            if let Some(v) = md.get("focal_length").and_then(|v| v.as_f64()) {
                util::insert_tag(map, tag!(parsed GroupId::Lens, TagId::FocalLength, "Focal length", f32, |v| format!("{v:.3}"), v as f32, vec![]));
            }
            if let Some(v) = md.get("lens_name").and_then(|v| v.as_str()) {
                util::insert_tag(map, tag!(parsed GroupId::Lens, TagId::Name, "Lens name", String, |v| v.clone(), v.into(), vec![]));
            }

            let pixel_pitch = match self.model.as_deref() {
                Some("KOMODO 6K")       => Some((4400, 4400)),
                Some("V-RAPTOR 8K VV")  => Some((5000, 5000)),
                Some("V-RAPTOR 8K S35") => Some((3200, 3200)),
                Some("Raven")           => Some((5000, 5000)),
                Some("DSMC2 DRAGON-X 6K S35") => Some((5000, 5000)),
                _ => None
            };
            if let Some(pp) = pixel_pitch {
                util::insert_tag(map, tag!(parsed GroupId::Imager, TagId::PixelPitch, "Pixel pitch", u32x2, |v| format!("{v:?}"), pp, vec![]));
            }

            util::insert_tag(map, tag!(parsed GroupId::Default, TagId::Metadata, "Metadata", Json, |v| serde_json::to_string(v).unwrap(), serde_json::Value::Object(md), vec![]));
        }
        Ok(())
    }

    fn parse_rmd(file: &str) -> HashMap<String, String> {
        let mut rmd = HashMap::<String, String>::new();
        if let Ok(contents) = std::fs::read_to_string(file) {
            let contents = contents.as_bytes();
            let mut find = |name: &str, typ| {
                if let Some(v) = util::find_between(&contents, format!("<{} type=\"{}\" value=\"", name, typ).as_bytes(), b'"') {
                    if !v.is_empty() {
                        rmd.insert(name.to_string(), v
                            .replace("&quot;", "\"")
                            .replace("&amp;", "&")
                            .replace("&lt;", "<")
                            .replace("&gt;", ">")
                        );
                    }
                }
            };
            find("fittype", "string");
            find("unit", "string");
            find("location", "string");
            find("focal_length", "string");
            find("production_name", "string");
            find("aperture", "string");
            find("director", "string");
            find("camera_operator", "string");
            find("focus_distance", "string");
            find("copyright", "string");
            find("director_of_photography", "string");
            find("take", "string");
            find("lens", "string");
            find("scene", "string");
            find("shot", "string");
            find("label", "string");
            find("video_slate_position", "int");
            find("poster_frame", "int");
            find("added_r3d_markers", "bool");

            if let Some(n) = util::find_between(&contents, b"<frame_rate_override num=\"", b'"') {
                if let Some(d) = util::find_between(&contents, format!("<frame_rate_override num=\"{n}\" den=\"").as_bytes(), b'"') {
                    match (n.parse::<u32>(), d.parse::<u32>()) {
                        (Ok(n), Ok(d)) if n > 0 && d > 0 => { rmd.insert("frame_rate_override".into(), format!("{:.3}", n as f64 / d as f64)); }
                        _ => { }
                    }
                }
            }
        }

        rmd
    }
}
