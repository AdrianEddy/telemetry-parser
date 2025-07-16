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

    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], _filepath: P, _options: &crate::InputOptions) -> Option<Self> {
        if memmem::find(buffer, b"QooCam EGO").is_some() {
            return Some(Self { model: Some("QooCam EGO".into()) });
        }
        if memmem::find(buffer, b"QooCam 3 Ultra").is_some() {
            return Some(Self { model: Some("QooCam 3 Ultra".into()) });
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
                "S32" => {
                    if length == 1 {
                        Value::Json(serde_json::Value::Number(stream.read_i32::<LittleEndian>()?.into()))
                    } else {
                        let mut arr = Vec::new();
                        for _ in 0..length {
                            arr.push(serde_json::Value::Number(stream.read_i32::<LittleEndian>()?.into()))
                        }
                        Value::Json(serde_json::Value::Array(arr))
                    }
                }
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
                "U64" => {
                    if length == 1 {
                        Value::Json(serde_json::Value::Number(stream.read_u64::<LittleEndian>()?.into()))
                    } else {
                        let mut arr = Vec::new();
                        for _ in 0..length {
                            arr.push(serde_json::Value::Number(stream.read_u64::<LittleEndian>()?.into()))
                        }
                        Value::Json(serde_json::Value::Array(arr))
                    }
                }
                "DOUBLE" => {
                    if length == 1 {
                        let f = stream.read_f64::<LittleEndian>()?.into();
                        let n = serde_json::Number::from_f64(f);
                        Value::Json(serde_json::Value::Number(n.expect("Bad Double")))
                    } else {
                        let mut arr = Vec::new();
                        for _ in 0..length {
                            let f = stream.read_f64::<LittleEndian>()?.into();
                            let n = serde_json::Number::from_f64(f);
                            arr.push(serde_json::Value::Number(n.expect("Bad Double")))
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

    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, _size: usize, _progress_cb: F, cancel_flag: Arc<AtomicBool>, options: crate::InputOptions) -> Result<Vec<SampleInfo>> {
        let mut gyro = Vec::new();
        let mut accl = Vec::new();
        let mut exp = Vec::new();
        let mut first_timestamp = None;
        let mut last_timestamp = None;
        let mut metadata = HashMap::new();
        let mut rear_lens = false;

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
                if let Some(Value::Buffer(lens_index)) = md.get("LENS_INDEX") {
                    if lens_index[0] == 1 {
                        rear_lens = true;
                    }
                }
            }
            if typ == fourcc("kvar") { // Variable metadata
                let md = Self::parse_data(stream, size as usize - header_size as usize)?;
                //println!("Variable metadata: {:#?}", md);

                let mut g_range = 2000.0;
                let mut a_range = 4.0;
                if let Some(Value::Json(serde_json::Value::String(v))) = md.get("INFO") {
                    // v = "V_S_PTS=5812115123 V_E_PTS=5966273383 V_F_NUM=9252 V_FPS=60.0 V_MUX_FPS=60.0 V_IMU_F=6 V_G_RANGE=2000 V_A_RANGE=4 V_CROP_SCALE=0,0,0,0,1,1,0 V_VERSION=3"
                    let info = v.split_whitespace()
                                .map(|s| s.split('=').collect::<Vec<&str>>())
                                .filter(|v| v.len() == 2)
                                .map(|v| (v[0], v[1]))
                                .collect::<HashMap<&str, &str>>();
                    if let Some(gr) = info.get("V_G_RANGE").and_then(|x| x.parse::<f64>().ok()) {
                        g_range = gr;
                    }
                    if let Some(ar) = info.get("V_A_RANGE").and_then(|x| x.parse::<f64>().ok()) {
                        a_range = ar;
                    }
                }

                g_range /= 2.0;
                a_range /= 2.0;

                if let Some(Value::Buffer(imu)) = md.get("IMU") {
                    let mut d = std::io::Cursor::new(&imu);
                    while d.position() < imu.len() as u64 {
                        let timestamp_ms = d.read_u64::<LittleEndian>()? as f64 / 1000.0;
                        let gx = d.read_i16::<LittleEndian>()? as f64 / g_range;
                        let gy = d.read_i16::<LittleEndian>()? as f64 / g_range;
                        let gz = d.read_i16::<LittleEndian>()? as f64 / g_range;
                        let ax = d.read_i16::<LittleEndian>()? as f64 / a_range;
                        let ay = d.read_i16::<LittleEndian>()? as f64 / a_range;
                        let az = d.read_i16::<LittleEndian>()? as f64 / a_range;
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
            tag!(parsed GroupId::Default, TagId::Metadata, "Extra metadata", Json, |v| format!("{:?}", v), serde_json::to_value(metadata).map_err(|_| Error::new(ErrorKind::Other, "Serialize error"))?, vec![]),
            &options
        );

        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Data, "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), accl, vec![]), &options);
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Data, "Gyroscope data",     Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]), &options);
        util::insert_tag(&mut map, tag!(parsed GroupId::Exposure,      TagId::Data, "Exposure data",      Vec_TimeScalar_f64,  |v| format!("{:?}", v), exp, vec![]), &options);

        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Unit, "Accelerometer unit", String, |v| v.to_string(), "g".into(), Vec::new()), &options);
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Unit, "Gyroscope unit",     String, |v| v.to_string(), "rad/s".into(), Vec::new()), &options);

        let imu_orientation = match self.model.as_deref() {
            Some("QooCam 3 Ultra") => if rear_lens { "yxz" } else { "yXZ" },
            _ => "XYZ"
        };
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()), &options);
        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()), &options);

        Ok(vec![
            SampleInfo { timestamp_ms: 0.0, duration_ms: last_timestamp.unwrap_or_default() - first_timestamp.unwrap_or_default(), tag_map: Some(map), ..Default::default() }
        ])
    }
}
