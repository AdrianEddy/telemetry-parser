use std::io::*;
use std::path::Path;
use std::sync::{ Arc, atomic::AtomicBool };

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
    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], filepath: P) -> Option<Self> {
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

                if let Some(parent) = path.parent() {
                    for x in parent.read_dir()? {
                        let x = x?;
                        let fname = x.file_name().to_string_lossy().to_string();
                        if fname.starts_with(filename_base) && fname.to_lowercase().ends_with(".r3d") {
                            if let Some(p) = x.path().to_str() {
                                ret.push(p.to_string());
                            }
                        }
                    }
                }
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

        let all_parts = self.all_parts.clone();

        let total_count = all_parts.len() as f64;

        for (i, file) in all_parts.into_iter().enumerate() {
            let stream = std::fs::File::open(file)?;
            let filesize = stream.metadata()?.len() as usize;

            let mut stream = std::io::BufReader::with_capacity(128*1024, stream);

            while let Ok(size) = stream.read_u32::<BigEndian>() {
                let mut name = [0u8; 4];
                stream.read_exact(&mut name)?;
                let aligned_size = ((size as f64 / 4096.0).ceil() * 4096.0) as usize;
                // log::debug!("Name: {}{}{}{}, size: {}", name[0] as char, name[1] as char, name[2] as char, name[3] as char, aligned_size);
                if &name == b"RDX\x02" {
                    let mut data = Vec::with_capacity(aligned_size);
                    data.resize(aligned_size, 0);
                    stream.seek(SeekFrom::Current(-8))?;
                    stream.read_exact(&mut data)?;
                    if data.len() > 4096 && (size as usize) <= data.len() {
                        let mut data = &data[4096..size as usize];

                        crate::try_block!({
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
                        });
                    }
                } else if &name == b"RED2" {
                    let mut data = Vec::with_capacity(aligned_size);
                    data.resize(aligned_size, 0);
                    stream.seek(SeekFrom::Current(-8))?;
                    stream.read_exact(&mut data)?;
                    if data.len() > 126 {
                        if let Ok(md) = self.parse_meta(&data[126..]) {
                            util::insert_tag(&mut map, tag!(parsed GroupId::Default, TagId::Metadata, "Metadata", Json, |v| serde_json::to_string(v).unwrap(), md, vec![]));
                        }
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

        Ok(vec![
            SampleInfo { tag_map: Some(map), ..Default::default() }
        ])
    }

    fn parse_meta(&mut self, mut data: &[u8]) -> Result<serde_json::Value> {
        let mut md = serde_json::Map::<String, serde_json::Value>::new();
        while let Ok(size) = data.read_u16::<BigEndian>() {
            if size > 2 {
                let mut d = Vec::with_capacity(size as usize - 2);
                d.resize(size as usize - 2, 0);
                data.read_exact(&mut d)?;
                let mut id = match d[1] {
                    0x00 => "start_edge_timecode",
                    0x01 => "start_absolute_timecode",
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
                    0x26 => "user_timecode_preference",
                    0x28 => "shutter_phase_offset",
                    0x2D => "genlock_setting",
                    0x2E => "jamsync_setting",
                    0x30 => "gmt_date",
                    0x31 => "gmt_time",
                    0x32 => "media_serial_number",
                    0x35 => "user_curve", // "user_curve_black_x", "user_curve_black_y", "user_curve_low_x", "user_curve_low_y", "user_curve_mid_x", "user_curve_mid_y", "user_curve_high_x", "user_curve_high_y", "user_curve_white_x", "user_curve_white_y"
                    0x36 => "frame_guide_name",
                    0x39 => "lens_cooke_i_static",
                    0x3A => "lens_cooke_i_dynamic",
                    0x3b => "iso",
                    0x49 => "flip_horizontal", // "flip_vertical"
                    0x4A => "shadow",
                    0x4D => "linked_camera_setup",
                    0x51 => "camera_position",
                    0x55 => "lens_mount", // 4: "P/L"; 8: "Nikon"; 9: "Canon"; 10: "Leica M"; 11: "P/L Motion Mount"; 12: "Canon Motion Mount"
                    0x56 => "file_name",
                    0x65 => "firmware_revision",
                    0x66 => "record_framerate",
                    0x69 => "motion_mount_nd_stops",
                    0x6D => "lens_distance_units", // "lens_aperture_units"
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
                    0x85 => "motion_mount_shutter_type", // 0 = ND only, 1 = Soft Shutter, 2 = Square Shutter
                    0x86 => "resolution_format_name",
                    0x92 => "monitor_sharpness",
                    0x97 => "user_shot",
                    0x9D => "lens_serial_number",
                    0x9E => "lens_owner",
                    0x9F => "temp_user_external_count", // "temp_user_external_changed"
                    0xA0 => "camera_model",
                    0xA1 => "sensor_name",
                    0xAA => "cdl_default_enabled",
                    0xAB => "3d_lut",
                    0xAC => "iso_calibration_version",
                    0xAD => "edge",
                    0xAE => "absolute",
                    0xAF => "sensor_sensitivity_id", // 0 = "Standard", 1 = "Low-light"
                    0xB0 => "fps", // / 1001
                    0xB1 => "output_transform",
                    0xBE => "redcode",
                    0xBF => "record_fps", // / 1001
                    0xC0 => "cdl_??",
                    0xC1 => "3d_lut",
                    0xC2 => "cdl_filename",
                    0xC4 => "wav_filename",
                    _ => "",
                }.to_string();
                if id.is_empty() { id = format!("0x{:x}", d[1]); };

                let v = match d[0] {
                    0x10 => serde_json::to_value(std::str::from_utf8(&d[2..]).unwrap_or(&"")),
                    0x20 => serde_json::to_value((&d[2..]).read_f32::<BigEndian>()? as f64),
                    0x30 => serde_json::to_value((&d[2..]).read_u8()?),
                    0x40 => serde_json::to_value((&d[2..]).read_i16::<BigEndian>()?),
                    0x60 => serde_json::to_value((&d[2..]).read_u32::<BigEndian>()?),
                    _ => {
                        // log::debug!("Type: {}, id: {}, hex: {}", d[0], id, pretty_hex::pretty_hex(&d));
                        Err(serde_json::Error::io(ErrorKind::InvalidData.into()))
                    }
                };
                if let Ok(v) = v {
                    if id == "camera_model" { self.model = v.as_str().map(|x| x.to_string()); }
                    if id == "record_framerate" { self.record_framerate = v.as_f64(); }

                    md.insert(id, v);
                    // log::debug!("{}: {:?}", id, v);
                }
            } else {
                break;
            }
        }
        Ok(serde_json::Value::Object(md))
    }

    pub fn normalize_imu_orientation(v: String) -> String {
        v
    }

    pub fn camera_type(&self) -> String {
        "RED RAW".to_owned()
    }

    pub fn frame_readout_time(&self) -> Option<f64> {
        None
    }
}
