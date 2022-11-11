// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2022 Adrian <adrian.eddy at gmail>

use std::io::*;
use memchr::memmem;
use std::sync::{ Arc, atomic::AtomicBool };

use crate::tags_impl::*;
use crate::*;
use byteorder::{ ReadBytesExt, LittleEndian };

#[derive(Default)]
pub struct Vuze {
    pub model: Option<String>,
}

impl Vuze {
    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], _filepath: P) -> Option<Self> {
        if memmem::find(buffer, b"bmdt").is_some() &&
           memmem::find(buffer, b"modl").is_some() &&
           memmem::find(buffer, b"slno").is_some() &&
           memmem::find(buffer, b"cali").is_some() {
            return Some(Self { model: None });
        }
        None
    }

    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, _size: usize, progress_cb: F, cancel_flag: Arc<AtomicBool>) -> Result<Vec<SampleInfo>> {
        let mut gyro = Vec::new();
        let mut accl = Vec::new();

        let mut map = GroupedTagMap::new();

        let mut last_timestamp = 0.0;
        let mut width = 0;
        let mut height = 0;

        while let Ok((typ, _offs, size, header_size)) = util::read_box(stream) {
            if size == 0 || typ == 0 { break; }
            let org_pos = stream.stream_position()?;

            if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) { break; }

            if typ == fourcc("moov") || typ == fourcc("udta") {
                continue; // go inside these boxes
            } else {
                if typ == fourcc("modl") { // Model
                    let mut buf = vec![0u8; size as usize - header_size as usize];
                    stream.read_exact(&mut buf)?;
                    self.model = Some(String::from_utf8_lossy(&buf).trim_start_matches("Vuze").to_string());
                }
                if typ == fourcc("rcrp") { // Right camera crop
                    let mut buf = vec![0u8; size as usize - header_size as usize];
                    stream.read_exact(&mut buf)?;
                    let rect = String::from_utf8_lossy(&buf).split(' ').filter_map(|x| x.parse::<i32>().ok()).collect::<Vec<i32>>();
                    if rect.len() == 4 {
                        width = rect[2];
                        height = rect[3];
                    }
                }
                if typ == fourcc("cali") { // Calibration YAML
                    let mut buf = vec![0u8; size as usize - header_size as usize];
                    stream.read_exact(&mut buf)?;
                    let calib = String::from_utf8_lossy(&buf).to_string().replace("%YAML:1.0", "");

                    match serde_yaml::from_str(calib.trim()) as serde_yaml::Result<serde_json::Value> {
                        Ok(calib) if calib.get("CamModel_V2_Set").is_some() => {
                            util::insert_tag(&mut map, tag!(parsed GroupId::Default, TagId::Metadata, "Calibration", Json, |v| serde_json::to_string(v).unwrap(), calib.clone(), vec![]));

                            if let Some(profile) = self.get_lens_profile(&calib["CamModel_V2_Set"], width, height) {
                                util::insert_tag(&mut map, tag!(parsed GroupId::Lens, TagId::Data, "Lens profile", Json, |v| serde_json::to_string(v).unwrap(), profile, vec![]));
                            } else {
                                log::warn!("Failed to get lens profile");
                            }
                        },
                        Err(e) => log::warn!("Failed to parse YAML: {}\n{}", e, &calib),
                        _ => log::warn!("Failed to parse YAML: {}", &calib)
                    }
                }
                if typ == fourcc("bmdt") { // IMU data
                    let buflen = size as usize - header_size as usize;
                    let mut buf = vec![0u8; buflen];
                    stream.read_exact(&mut buf)?;

                    // Data example:
                    // 0C00 01 00 60EA0000 E9030000 5A 3D
                    // 0E00 03 00 0000000000000000 295C1642
                    // 2200 01 00 0000000000000000 00000020 880B4140 00000080 11945DC0 00000000 00000000
                    // 0E00 03 00 40420F0000000000 7B141642
                    // 2000 02 00 5682000000000000 17B7D13A 17B7513B 00 27 00 00 00 10 00 00 79 17 00 00 C8 00
                    let mut d = std::io::Cursor::new(buf);
                    while (d.position() as usize) < buflen {

                        if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) { break; }
                        if buflen > 0 {
                            progress_cb(d.position() as f64 / buflen as f64);
                        }

                        let len = d.read_u16::<LittleEndian>()?;
                        let _unkh1 = d.read_u8()?; // command?
                        let _unkh2 = d.read_u8()?; // camera ID?
                        match len {
                            0x0C => {
                                let _fps_num = d.read_u32::<LittleEndian>()?;
                                let _fps_den = d.read_u32::<LittleEndian>()?;
                                let _unk1 = d.read_u8()?;
                                let _unk2 = d.read_u8()?;

                                // println!("0x0C: {_unkh1} {_unkh2} {_fps_num}/{_fps_den} | {_unk1} {_unk1}");
                            },
                            0x0E => {
                                let _ts = d.read_u64::<LittleEndian>()?;
                                let _unkf = d.read_f32::<LittleEndian>()?;
                                // println!("0x0E: {_unkh1} {_unkh2} {} | {:.4}", _ts, _unkf);
                            },
                            0x22 => {
                                let ts = d.read_u64::<LittleEndian>()?;

                                let ax = d.read_f32::<LittleEndian>()?;
                                let ay = d.read_f32::<LittleEndian>()?;
                                let az = d.read_f32::<LittleEndian>()?;

                                let gx = d.read_f32::<LittleEndian>()?;
                                let gy = d.read_f32::<LittleEndian>()?;
                                let gz = d.read_f32::<LittleEndian>()?;

                                last_timestamp = ts as f64 / 1000.0;

                                if gx.abs() > 360.0 || gy.abs() > 360.0 || gz.abs() > 360.0 {
                                    log::warn!("Invalid gyro value {gx:.4} {gy:.4} {gz:.4}");
                                    continue;
                                }
                                if ax.abs() > 10.0 || ay.abs() > 10.0 || az.abs() > 10.0 {
                                    log::warn!("Invalid accel value {ax:.4} {ay:.4} {az:.4}");
                                    continue;
                                }

                                gyro.push(TimeVector3 {
                                    t: last_timestamp / 1000.0,
                                    x: gx as f64,
                                    y: gy as f64,
                                    z: gz as f64
                                });
                                accl.push(TimeVector3 {
                                    t: last_timestamp / 1000.0,
                                    x: ax as f64,
                                    y: ay as f64,
                                    z: az as f64
                                });

                                // println!("0x22: {_unkh1} {_unkh2} {} | {:.4} {:.4} {:.4} | {:.4} {:.4} {:.4}", ts, gx, gy, gz, ax, ay, az);
                            },
                            0x20 => {
                                let _ts = d.read_u64::<LittleEndian>()?;
                                let _unkf1 = d.read_f32::<LittleEndian>()?;
                                let _unkf2 = d.read_f32::<LittleEndian>()?;
                                let _unk1 = d.read_u32::<LittleEndian>()?; // data type not confirmed
                                let _unk2 = d.read_u32::<LittleEndian>()?; // data type not confirmed
                                let _unk3 = d.read_u32::<LittleEndian>()?; // data type not confirmed
                                let _unk4 = d.read_u16::<LittleEndian>()?; // data type not confirmed

                                // println!("0x20: {_unkh1} {_unkh2} {} | {:.4} {:.4} | {} {} {} {}", _ts, _unkf1, _unkf2, _unk1, _unk2, _unk3, _unk4);
                            },
                            _ => {
                                log::warn!("Unknown Vuze tag: {:04x}", len);
                                break;
                            }
                        }
                    }
                }

                stream.seek(SeekFrom::Start(org_pos + size - header_size as u64))?;
            }
        }

        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Data, "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), accl, vec![]));
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Data, "Gyroscope data",     Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]));

        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Unit, "Accelerometer unit", String, |v| v.to_string(), "g".into(), Vec::new()));
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Unit, "Gyroscope unit",     String, |v| v.to_string(), "deg/s".into(), Vec::new()));

        let imu_orientation = "xYz";
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()));
        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()));

        Ok(vec![
            SampleInfo { timestamp_ms: 0.0, duration_ms: last_timestamp, tag_map: Some(map), ..Default::default() }
        ])
    }

    pub fn normalize_imu_orientation(v: String) -> String {
        v
    }

    pub fn camera_type(&self) -> String {
        "Vuze".to_owned()
    }

    pub fn frame_readout_time(&self) -> Option<f64> {
        None
    }

    fn get_lens_profile(&self, data: &serde_json::Value, width: i32, height: i32) -> Option<serde_json::Value> {
        let model = self.model.clone()?;
        let matrix = data.get("CAM_0")?.get("K")?.get("data")?.as_array()?.into_iter().filter_map(|x| x.as_f64()).collect::<Vec<f64>>();
        let coeffs = data.get("CAM_0")?.get("DistortionCoeffs")?.as_array()?.into_iter().filter_map(|x| x.as_f64()).collect::<Vec<f64>>();
        if matrix.len() != 9 { return None; }
        if coeffs.len() < 4  { return None; }

        Some(serde_json::json!({
            "calibrated_by": "Vuze",
            "camera_brand": "Vuze",
            "camera_model": model,
            "calib_dimension": { "w": width, "h": height },
            "orig_dimension":  { "w": width, "h": height },
            "frame_readout_time": 0.0,
            "official": true,
            "fisheye_params": {
              "camera_matrix": [
                [ matrix[0], matrix[1], matrix[2] ],
                [ matrix[3], matrix[4], matrix[5] ],
                [ matrix[6], matrix[7], matrix[8] ]
              ],
              "distortion_coeffs": coeffs
            },
            "sync_settings": {
              "initial_offset": 0,
              "initial_offset_inv": false,
              "search_size": 0.5,
              "max_sync_points": 5,
              "every_nth_frame": 1,
              "time_per_syncpoint": 0.6,
              "do_autosync": false
            },
            "calibrator_version": "---"
        }))
    }
}
