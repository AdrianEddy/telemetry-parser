// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2022 Adrian <adrian.eddy at gmail>

use std::io::*;
use memchr::memmem;
use std::sync::{ Arc, atomic::AtomicBool };

use crate::tags_impl::*;
use crate::*;
use byteorder::{ ReadBytesExt, LittleEndian };

#[derive(Default)]
pub struct KanDao {
    pub model: Option<String>,
}

// Input file is `imu.bin`

#[allow(unused_assignments)]
impl KanDao {
    pub fn camera_type(&self) -> String {
        "KanDao".to_owned()
    }
    pub fn has_accurate_timestamps(&self) -> bool {
        false
    }
    pub fn possible_extensions() -> Vec<&'static str> {
        vec!["bin"]
    }
    pub fn frame_readout_time(&self) -> Option<f64> {
        None
    }
    pub fn normalize_imu_orientation(v: String) -> String {
        v
    }

    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], _filepath: P) -> Option<Self> {
        if memmem::find(buffer, b"KANDAO_IMU_DATA").is_some() && memmem::find(buffer, b"GYROACC").is_some() {
            return Some(Self { model: None });
        }
        None
    }

    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, size: usize, progress_cb: F, cancel_flag: Arc<AtomicBool>) -> Result<Vec<SampleInfo>> {
        let mut stream = std::io::BufReader::new(stream);

        let mut ret = Vec::new();

        let mut gyro = Vec::new();
        let mut accl = Vec::new();
        let mut magn = Vec::new();

        let mut last_timestamp = None;
        let mut first_timestamp = None;

        let mut current_device_id = None;

        let mut map = GroupedTagMap::new();

        macro_rules! add_device {
            ($num:tt) => {
                util::insert_tag(&mut map, tag!(parsed GroupId::Default, TagId::Custom("DeviceID".into()), "Device ID", u32, |v| format!("{:?}", v), $num, vec![]));

                util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Data, "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), accl.clone(), vec![]));
                util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Data, "Gyroscope data",     Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro.clone(), vec![]));
                util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer,  TagId::Data, "Magnetometer data",  Vec_TimeVector3_f64, |v| format!("{:?}", v), magn.clone(), vec![]));

                util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Unit, "Accelerometer unit", String, |v| v.to_string(), "g"    .into(), Vec::new()));
                util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Unit, "Gyroscope unit",     String, |v| v.to_string(), "deg/s".into(), Vec::new()));
                util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer,  TagId::Unit, "Magnetometer unit",  String, |v| v.to_string(), "μT"   .into(), Vec::new()));

                let imu_orientation = "xYz";
                util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()));
                util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()));
                util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer,  TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()));

                ret.push(SampleInfo { timestamp_ms: first_timestamp.unwrap_or_default(), duration_ms: last_timestamp.unwrap_or_default() - first_timestamp.unwrap_or_default(), tag_map: Some(map.clone()), ..Default::default() });

                first_timestamp = None;
                last_timestamp = None;
                gyro.clear();
                accl.clear();
                magn.clear();
                map.clear();
            };
        }

        let mut buf = vec![0u8; 17];
        while (size as i64 - stream.stream_position()? as i64) >= 17 {
            if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) { break; }
            if size > 0 {
                progress_cb(stream.stream_position()? as f64 / size as f64);
            }

            stream.read_exact(&mut buf)?;
            if let Some(pos) = memmem::find(&buf, b"NARWAL_DEVICE_ID=") {
                stream.seek(SeekFrom::Current(-17 + pos as i64 + 17))?;
                let c1 = stream.read_u8()? as char;
                let c2 = stream.read_u8()? as char;
                if let Ok(num) = format!("{c1}{c2}").trim().parse::<u32>() {
                    if let Some(current_num) = current_device_id.take() {
                        add_device!(current_num);
                    }
                    current_device_id = Some(num);
                }
            }
            if let Some(pos) = memmem::find(&buf, b"KANDAO_IMU_DATA=") {
                stream.seek(SeekFrom::Current(-17 + pos as i64 + 16))?;
                for _ in 0..15 {
                    let _unkf1 = stream.read_f32::<LittleEndian>()?; // print!("{_unkf1:.8} ");
                }
                // println!("");
            }
            if let Some(pos) = memmem::find(&buf, b"GYROACC=") {
                stream.seek(SeekFrom::Current(-17 + pos as i64 + 8))?;

                stream.seek(SeekFrom::Current(5))?; // Unknown data
                let gyro_range = stream.read_u32::<LittleEndian>()? as f64;

                stream.seek(SeekFrom::Current(5))?; // Unknown data
                let acc_range = stream.read_u32::<LittleEndian>()? as f64;

                let gyro_scale = 32768.0 / gyro_range; // 1000 dps
                let accl_scale = 32768.0 / acc_range; // ± 2g

                let sample_num = stream.read_u32::<LittleEndian>()?;
                for _ in 0..sample_num {
                    let ts = stream.read_u64::<LittleEndian>()? as f64 / 1000.0;
                    let gx = (stream.read_i16::<LittleEndian>()? as f64) / gyro_scale;
                    let gy = (stream.read_i16::<LittleEndian>()? as f64) / gyro_scale;
                    let gz = (stream.read_i16::<LittleEndian>()? as f64) / gyro_scale;

                    let ax = (stream.read_i16::<LittleEndian>()? as f64) / accl_scale;
                    let ay = (stream.read_i16::<LittleEndian>()? as f64) / accl_scale;
                    let az = (stream.read_i16::<LittleEndian>()? as f64) / accl_scale;

                    if first_timestamp.is_none() { first_timestamp = Some(ts); }
                    last_timestamp = Some(ts);

                    accl.push(TimeVector3 { t: ts as f64 / 1000.0, x: ax, y: ay, z: az });
                    gyro.push(TimeVector3 { t: ts as f64 / 1000.0, x: gx, y: gy, z: gz });

                    // println!("GYRO {ts} | {gx:.4} {gy:.4} {gz:.4} | {ax:.4} {ay:.4} {az:.4}");
                }
            }
            if let Some(pos) = memmem::find(&buf, b"MAG=") {
                stream.seek(SeekFrom::Current(-17 + pos as i64 + 4))?;
                stream.seek(SeekFrom::Current(9))?; // Unknown data

                let sample_num = stream.read_u32::<LittleEndian>()?;
                for _ in 0..sample_num {
                    let ts = stream.read_u64::<LittleEndian>()? as f64 / 1000.0;
                    let mx = stream.read_i16::<LittleEndian>()? as f64;
                    let my = stream.read_i16::<LittleEndian>()? as f64;
                    let mz = stream.read_i16::<LittleEndian>()? as f64;

                    if first_timestamp.is_none() { first_timestamp = Some(ts); }
                    last_timestamp = Some(ts);

                    magn.push(TimeVector3 { t: ts as f64 / 1000.0, x: mx, y: my, z: mz });

                    // println!("MAGN {ts} | {mx} {my} {mz}");
                }
            }
            if let Some(pos) = memmem::find(&buf, b"EXPO=") {
                stream.seek(SeekFrom::Current(-17 + pos as i64 + 5))?;

                let sample_num = stream.read_u32::<LittleEndian>()?;
                for _ in 0..sample_num {
                    let ts = stream.read_u64::<LittleEndian>()? as f64 / 1000.0;
                    let _exp = stream.read_u32::<LittleEndian>()?;

                    if first_timestamp.is_none() { first_timestamp = Some(ts); }
                    last_timestamp = Some(ts);

                    // println!("EXPO {ts} | {_exp}");
                }
            }
        }
        if let Some(num) = current_device_id.take() {
            add_device!(num);
        }

        Ok(ret)
    }
}
