// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2024 Adrian <adrian.eddy at gmail>

use std::io::*;
use memchr::memmem;
use std::sync::{ Arc, atomic::AtomicBool };
use std::collections::HashMap;

use crate::tags_impl::*;
use crate::*;
use byteorder::{ LittleEndian, ReadBytesExt };

#[derive(Default)]
pub struct QoocamEgo {
    pub model: Option<String>,
}

#[derive(Clone, Debug)]
enum Value {
    Buffer(Vec<u8>),
    Json(serde_json::Value),
}

impl QoocamEgo {
    pub fn camera_type(&self) -> String {
        "KanDao".to_owned()
    }
    pub fn has_accurate_timestamps(&self) -> bool {
        false
    }
    pub fn possible_extensions() -> Vec<&'static str> {
        vec!["mp4", "mov"]
    }
    pub fn frame_readout_time(&self) -> Option<f64> {
        None
    }
    pub fn normalize_imu_orientation(v: String) -> String {
        v
    }

    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], _filepath: P) -> Option<Self> {
        if memmem::find(buffer, b"QooCam EGO").is_some() {
            return Some(Self { model: Some("QooCam EGO".into()) });
        }
        None
    }

    fn parse_data<T: Read + Seek>(stream: &mut T, _size: usize) -> Result<HashMap<String, Value>> {
        let mut map = HashMap::new();
        let count = stream.read_u32::<LittleEndian>()?;
        for _ in 0..count {
            let mut name = [0u8; 32];
            let mut typ = [0u8; 8];
            stream.read_exact(&mut name)?;
            stream.read_exact(&mut typ)?;
            let name = String::from_utf8_lossy(&name).trim_matches(char::from(0)).to_string();
            let typ = String::from_utf8_lossy(&typ).trim_matches(char::from(0)).to_string();
            let length = stream.read_u32::<LittleEndian>()?;
            let v = match typ.as_ref() {
                "CHAR" => {
                    let mut buf = vec![0u8; length as usize];
                    stream.read_exact(&mut buf)?;
                    Value::Json(serde_json::Value::String(String::from_utf8_lossy(&buf).trim_matches(char::from(0)).to_string()))
                },
                "U32" => {
                    if length == 1 {
                        Value::Json(serde_json::Value::Number(stream.read_u32::<LittleEndian>()?.into()))
                    } else {
                        let mut arr = Vec::new();
                        for _ in 0..length {
                            arr.push(serde_json::Value::Number(stream.read_u32::<LittleEndian>()?.into()))
                        }
                        Value::Json(serde_json::Value::Array(arr))
                    }
                }
                "U8" => {
                    let mut buf = vec![0u8; length as usize];
                    stream.read_exact(&mut buf)?;
                    Value::Buffer(buf)
                }
                _ => {
                    log::error!("Unknown type: {typ}");
                    Value::Json(serde_json::Value::Null)
                }
            };
            map.insert(name, v);
        }
        Ok(map)
    }

    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, _size: usize, _progress_cb: F, cancel_flag: Arc<AtomicBool>) -> Result<Vec<SampleInfo>> {
        let mut gyro = Vec::new();
        let mut accl = Vec::new();
        let mut exp = Vec::new();
        let mut first_timestamp = None;
        let mut last_timestamp = None;
        let mut metadata = HashMap::new();

        let mut map = GroupedTagMap::new();

        while let Ok((typ, _offs, size, header_size)) = util::read_box(stream) {
            if size == 0 || typ == 0 { break; }
            let org_pos = stream.stream_position()?;

            if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) { break; }

            if typ == fourcc("kfix") { // Fixed metadata
                let md = Self::parse_data(stream, size as usize - header_size as usize)?;
                for (k, v) in md.iter() {
                    if let Value::Json(v) = v {
                        metadata.insert(k.clone(), v.clone());
                    }
                }
            }
            if typ == fourcc("kvar") { // Variable metadata
                let md = Self::parse_data(stream, size as usize - header_size as usize)?;
                //println!("Variable metadata: {:#?}", md);
                if let Some(Value::Buffer(imu)) = md.get("IMU") {
                    let mut d = std::io::Cursor::new(&imu);
                    while d.position() < imu.len() as u64 {
                        let timestamp_ms = d.read_u64::<LittleEndian>()? as f64 / 1000.0;
                        let gx = d.read_i16::<LittleEndian>()? as f64 / 16384.0;
                        let gy = d.read_i16::<LittleEndian>()? as f64 / 16384.0;
                        let gz = d.read_i16::<LittleEndian>()? as f64 / 16384.0;
                        let ax = d.read_i16::<LittleEndian>()? as f64 / 16384.0;
                        let ay = d.read_i16::<LittleEndian>()? as f64 / 16384.0;
                        let az = d.read_i16::<LittleEndian>()? as f64 / 16384.0;
                        if first_timestamp.is_none() {
                            first_timestamp = Some(timestamp_ms);
                        }
                        last_timestamp = Some(timestamp_ms);
                        gyro.push(TimeVector3 {
                            t: timestamp_ms / 1000.0,
                            x: gx as f64,
                            y: gy as f64,
                            z: gz as f64
                        });
                        accl.push(TimeVector3 {
                            t: timestamp_ms / 1000.0,
                            x: ax as f64,
                            y: ay as f64,
                            z: az as f64
                        });
                    }
                }
                if let Some(Value::Buffer(buf)) = md.get("EXP") {
                    let mut d = std::io::Cursor::new(&buf);
                    while d.position() < buf.len() as u64 {
                        exp.push(TimeScalar {
                            t: d.read_u32::<LittleEndian>()? as f64,
                            v: d.read_u32::<LittleEndian>()? as f64 / 1000.0,
                        });
                    }
                }
                for (k, v) in md.iter() {
                    if let Value::Json(v) = v {
                        metadata.insert(k.clone(), v.clone());
                    }
                }
            }

            stream.seek(SeekFrom::Start(org_pos + size - header_size as u64))?;
        }

        util::insert_tag(&mut map,
            tag!(parsed GroupId::Default, TagId::Metadata, "Extra metadata", Json, |v| format!("{:?}", v), serde_json::to_value(metadata).map_err(|_| Error::new(ErrorKind::Other, "Serialize error"))?, vec![])
        );

        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Data, "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), accl, vec![]));
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Data, "Gyroscope data",     Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]));
        util::insert_tag(&mut map, tag!(parsed GroupId::Exposure,      TagId::Data, "Exposure data",      Vec_TimeScalar_f64,  |v| format!("{:?}", v), exp, vec![]));

        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Unit, "Accelerometer unit", String, |v| v.to_string(), "g".into(), Vec::new()));
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Unit, "Gyroscope unit",     String, |v| v.to_string(), "rad/s".into(), Vec::new()));

        let imu_orientation = "xYz";
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()));
        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()));

        Ok(vec![
            SampleInfo { timestamp_ms: 0.0, duration_ms: last_timestamp.unwrap_or_default() - first_timestamp.unwrap_or_default(), tag_map: Some(map), ..Default::default() }
        ])
    }
}
