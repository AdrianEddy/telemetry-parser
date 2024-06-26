// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2023 Adrian <adrian.eddy at gmail>

use crate::*;
use crate::tags_impl::*;
use memchr::memmem;

pub mod bin;

#[derive(Default)]
pub struct Cooke {
    pub model: Option<String>,
}

impl Cooke {
    pub fn camera_type(&self) -> String { "Cooke /i".to_owned() }
    pub fn has_accurate_timestamps(&self) -> bool { false }
    pub fn possible_extensions() -> Vec<&'static str> { vec!["yml", "yaml"] }
    pub fn frame_readout_time(&self) -> Option<f64> { None }
    pub fn normalize_imu_orientation(v: String) -> String { v }
    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], _filepath: P) -> Option<Self> {
        if memmem::find(buffer, b"RecordType: rt.header.lens.info").is_some() || memmem::find(buffer, b"RecordType: rt.header.recorder.info").is_some() {
            Some(Self {
                model: Some("YAML metadata".into()),
            })
        } else {
            None
        }
    }

    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, _size: usize, _progress_cb: F, cancel_flag: Arc<AtomicBool>) -> Result<Vec<SampleInfo>> {
        let mut samples = Vec::new();
        let mut all_data = String::new();
        stream.read_to_string(&mut all_data)?;

        // let mut map = GroupedTagMap::new();
        let imu_orientation = "XYZ";

        let mut calibration_accl = None;
        let mut calibration_gyro = None;
        let mut calibration_magn = None;

        let mut map = GroupedTagMap::new();
        let mut last_timecode = None;

        let mut prev_timestamp = [0i64; 4];
        let mut prev_absolute_timestamp = [0i64; 4];
        let mut timestamp = [0i64; 4];

        for chunk in all_data.split("\n\n") {
            if chunk.trim().is_empty() { continue; }
            if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) { break; }

            match serde_yaml::from_str(chunk) as serde_yaml::Result<serde_json::Value> {
                Ok(data) => {
                    let rtype = data.get("RecordType").and_then(|x| x.as_str());
                    let timecode = Self::get_timecode(&data);
                    let tsi = match rtype {
                        Some("rt.temporal.lens.accelerometer.raw") => 0,
                        Some("rt.temporal.lens.gyro.raw")          => 1,
                        Some("rt.temporal.lens.magnetometer.raw")  => 2,
                        Some("rt.temporal.lens.general")           => 3,
                        _ => 4
                    };
                    if let Some(ts) = data.get("Timestamp").and_then(|x| x.as_i64()) {
                        timestamp[tsi] += ts + (if prev_timestamp[tsi] > ts { std::u16::MAX as i64 } else { 0 }) - prev_timestamp[tsi];
                        prev_timestamp[tsi] = ts;
                    }
                    //if tsi == 1 { println!("{:?}, ts: {:.2}", timecode, timestamp[tsi] as f64 / 150000.0); }
                    match rtype {
                        Some("rt.header.lens.info") => {
                            util::insert_tag(&mut map, tag!(parsed GroupId::Lens, TagId::Metadata, "Lens info", Json, |v| format!("{:?}", v), data, vec![]));
                        },
                        Some("rt.header.lens.shading") => {
                            util::insert_tag(&mut map, tag!(parsed GroupId::Lens, TagId::Shading, "Lens shading", Json, |v| format!("{:?}", v), data, vec![]));
                        },
                        Some("rt.header.lens.distortion") => {
                            util::insert_tag(&mut map, tag!(parsed GroupId::Lens, TagId::Distortion, "Lens distortion", Json, |v| format!("{:?}", v), data, vec![]));
                        },
                        Some("rt.header.lens.cal.accelerometer") => { calibration_accl = Self::get_mtrx::<4>(&data); },
                        Some("rt.header.lens.cal.gyro")          => { calibration_gyro = Self::get_mtrx::<7>(&data); },
                        Some("rt.header.lens.cal.magnetometer")  => { calibration_magn = Self::get_mtrx::<4>(&data); },
                        Some("rt.temporal.lens.accelerometer.raw") => {
                            if let Some(vals) = Self::get_datavals(&data) {
                                let mut accl = Vec::with_capacity(vals.len());
                                let num_vals = vals.len() as f64;
                                let timestamp_frac = (timestamp[0] as f64 - prev_absolute_timestamp[0] as f64) / num_vals;
                                for (i, x) in vals.into_iter().enumerate() {
                                    let data = if let Some(calib) = calibration_accl {
                                        (
                                            x.0 * calib[0][0] + x.1 * calib[0][1] + x.2 * calib[0][2] + 1.0 * calib[0][3],
                                            x.0 * calib[1][0] + x.1 * calib[1][1] + x.2 * calib[1][2] + 1.0 * calib[1][3],
                                            x.0 * calib[2][0] + x.1 * calib[2][1] + x.2 * calib[2][2] + 1.0 * calib[2][3]
                                        )
                                    } else {
                                        x
                                    };
                                    accl.push(TimeVector3 { t: ((timestamp[0] as f64 - ((num_vals - 1.0 - i as f64) * timestamp_frac)) / 150000.0),
                                        x: data.0,
                                        y: data.1,
                                        z: data.2
                                    });
                                }
                                util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Data,        "Accelerometer data",  Vec_TimeVector3_f64, |v| format!("{:?}", v), accl, vec![]));
                                util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Unit,        "Accelerometer unit",  String, |v| v.to_string(), "m/s²".into(),  Vec::new()));
                                util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation",     String, |v| v.to_string(), imu_orientation.into(), Vec::new()));
                            }
                        },
                        Some("rt.temporal.lens.gyro.raw") => {
                            if let Some(vals) = Self::get_datavals(&data) {
                                //dbg!(&vals);
                                let mut gyro = Vec::with_capacity(vals.len());
                                let num_vals = vals.len() as f64;
                                let timestamp_frac = (timestamp[1] as f64 - prev_absolute_timestamp[1] as f64) / num_vals;
                                for (i, x) in vals.into_iter().enumerate() {
                                    let data = if let Some(calib) = calibration_gyro {
                                        (
                                            x.0 * calib[0][0] + x.1 * calib[0][1] + x.2 * calib[0][2] + x.0.powi(2) * calib[0][3] + x.1.powi(2) * calib[0][4] + x.2.powi(2) * calib[0][5] + 1.0 * calib[0][6],
                                            x.0 * calib[1][0] + x.1 * calib[1][1] + x.2 * calib[1][2] + x.0.powi(2) * calib[1][3] + x.1.powi(2) * calib[1][4] + x.2.powi(2) * calib[1][5] + 1.0 * calib[1][6],
                                            x.0 * calib[2][0] + x.1 * calib[2][1] + x.2 * calib[2][2] + x.0.powi(2) * calib[2][3] + x.1.powi(2) * calib[2][4] + x.2.powi(2) * calib[2][5] + 1.0 * calib[2][6]
                                        )
                                    } else {
                                        x
                                    };
                                    // println!("{}\t{}\t{}", data.0, data.1, data.2);
                                    gyro.push(TimeVector3 { t: ((timestamp[1] as f64 - ((num_vals - 1.0 - i as f64) * timestamp_frac)) / 150000.0),
                                        x: data.0,
                                        y: data.1,
                                        z: data.2
                                    });
                                }
                                util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope, TagId::Data,        "Gyroscope data",  Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]));
                                util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope, TagId::Unit,        "Gyroscope unit",  String, |v| v.to_string(), "rad/s".into(), Vec::new()));
                                util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()));
                            }
                        },
                        Some("rt.temporal.lens.magnetometer.raw") => {
                            if let Some(vals) = Self::get_datavals(&data) {
                                let mut magn = Vec::with_capacity(vals.len());
                                let num_vals = vals.len() as f64;
                                let timestamp_frac = (timestamp[2] as f64 - prev_absolute_timestamp[2] as f64) / num_vals;
                                for (i, x) in vals.into_iter().enumerate() {
                                    let data = if let Some(calib) = calibration_magn {
                                        (
                                            x.0 * calib[0][0] + x.1 * calib[0][1] + x.2 * calib[0][2] + 1.0 * calib[0][3],
                                            x.0 * calib[1][0] + x.1 * calib[1][1] + x.2 * calib[1][2] + 1.0 * calib[1][3],
                                            x.0 * calib[2][0] + x.1 * calib[2][1] + x.2 * calib[2][2] + 1.0 * calib[2][3]
                                        )
                                    } else {
                                        x
                                    };
                                    magn.push(TimeVector3 { t: ((timestamp[2] as f64 - ((num_vals - 1.0 - i as f64) * timestamp_frac)) / 150000.0),
                                        x: data.0,
                                        y: data.1,
                                        z: data.2
                                    });
                                }
                                util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer, TagId::Data,        "Magnetometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), magn, vec![]));
                                util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer, TagId::Unit,        "Magnetometer unit", String, |v| v.to_string(), "T".into(), Vec::new()));
                                util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer, TagId::Orientation, "IMU orientation",   String, |v| v.to_string(), imu_orientation.into(), Vec::new()));
                            }
                        },
                        Some("rt.temporal.lens.general") => {
                            util::insert_tag(&mut map, tag!(parsed GroupId::Lens, TagId::Data, "Lens data", Json, |v| format!("{:?}", v), data, vec![]));
                        },
                        Some("rt.header.recorder.info") => {
                            util::insert_tag(&mut map, tag!(parsed GroupId::Default, TagId::Metadata, "Recorder info", Json, |v| format!("{:?}", v), data, vec![]));
                        },
                        _ => {
                            panic!("Unknown record: {data:?}");
                        }
                    }

                    if last_timecode != timecode {
                        samples.push(SampleInfo {
                            sample_index: samples.len() as u64,
                            track_index: 0,
                            timestamp_ms: 0.0,
                            duration_ms: 0.0,
                            tag_map: Some(map),
                            ..Default::default()
                        });
                        map = GroupedTagMap::new();
                    }
                    prev_absolute_timestamp = timestamp;
                    last_timecode = timecode;
                },
                Err(e) => {
                    log::error!("Failed to parse YAML: {}", e);
                    break;
                }
            }
        }

        Ok(samples)
    }

    fn get_timecode(data: &serde_json::Value) -> Option<String> {
        let obj = data.get("Timecode")?.as_object()?;
        Some(format!("{:02}:{:02}:{:02}:{:02}", obj.get("hh")?.as_i64()?, obj.get("mm")?.as_i64()?, obj.get("ss")?.as_i64()?, obj.get("ff")?.as_i64()?))
    }

    fn get_datavals(data: &serde_json::Value) -> Option<Vec<(f64, f64, f64)>> {
        let arr = data.get("Datavals")?;
        let arr = if arr.is_object() { serde_json::to_value(vec![arr]).unwrap() } else { arr.clone() };
        let arr = arr.as_array()?;
        let mut ret: Vec<_> = Vec::with_capacity(arr.len());
        for v in arr {
            let v = v.as_object()?;
            ret.push((
                v.get("X")?.as_f64()?,
                v.get("Y")?.as_f64()?,
                v.get("Z")?.as_f64()?
            ));
        }
        Some(ret)
    }
    fn get_mtrx<const N: usize>(data: &serde_json::Value) -> Option<[[f64; N]; 3]> {
        let mut ret = [[0.0f64; N]; 3];
        for r in 1..=3 {
            let arr = data.get(&format!("Row_{r}"))?.as_array()?;
            for (i, x) in arr.iter().enumerate() {
                ret[r - 1][i] = x.as_f64()?;
            }
        }
        Some(ret)
    }
}
