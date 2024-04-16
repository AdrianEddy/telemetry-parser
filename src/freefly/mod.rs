// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2024 Adrian <adrian.eddy at gmail>

use std::io::*;
use std::sync::{ Arc, atomic::AtomicBool };

use crate::tags_impl::*;
use crate::*;
use crate::gopro::GoPro;
use memchr::memmem;

#[derive(Default)]
pub struct Freefly {
    pub model: Option<String>,
    frame_readout_time: Option<f64>
}

impl Freefly {
    pub fn camera_type(&self) -> String {
        "Freefly".to_owned()
    }
    pub fn has_accurate_timestamps(&self) -> bool {
        true
    }
    pub fn possible_extensions() -> Vec<&'static str> {
        vec!["mov", "mp4"]
    }
    pub fn frame_readout_time(&self) -> Option<f64> {
        self.frame_readout_time
    }
    pub fn normalize_imu_orientation(v: String) -> String {
        v
    }

    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], _filepath: P) -> Option<Self> {
        if memmem::find(buffer, b"com.freeflysystems.frame-metadata").is_some() {
            Some(Self::default())
        } else {
            None
        }
    }

    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, size: usize, progress_cb: F, cancel_flag: Arc<AtomicBool>) -> Result<Vec<SampleInfo>> {
        let mut samples = Vec::new();

        let mut acc_cal  = (0.00024414808, 0.00024414808, 0.00024414808);
        let mut gyro_cal = (0.00053264847, 0.00053264847, 0.00053264847);

        let mut frame_timestamps = Vec::new();
        let mut imu_timestamps   = Vec::new();

        util::get_metadata_track_samples(stream, size, true, |mut info: SampleInfo, data: &[u8], file_position: u64| {
            if size > 0 {
                progress_cb(file_position as f64 / size as f64);
            }

            if let Ok(mut map) = GoPro::parse_metadata(&data[8+8..], GroupId::Default, true) {
                let mut gyro = Vec::new();
                let mut accl = Vec::new();

                for v in map.values() {
                    if let Some(v) = v.get_t(TagId::Unknown(u32::from_be_bytes(*b"FRTS"))) as Option<&Vec<Scalar>> {
                        if v.len() == 2 {
                            let Scalar::u64(ts) = v[1] else { continue; };
                            frame_timestamps.push(ts);
                        }
                    }
                    if let Some(v) = v.get_t(TagId::Unknown(u32::from_be_bytes(*b"IMTS"))) as Option<&Vec<Scalar>> {
                        if v.len() == 2 {
                            let Scalar::u64(ts) = v[1] else { continue; };
                            imu_timestamps.push(ts);
                        }
                    }
                    if let Some(v) = v.get_t(TagId::Unknown(u32::from_be_bytes(*b"CAAC"))) as Option<&Vec<Vec<f32>>> {
                        if v.len() == 1 && v[0].len() >= 3 {
                            acc_cal = (v[0][0], v[0][1], v[0][2]);
                        }
                    }
                    if let Some(v) = v.get_t(TagId::Unknown(u32::from_be_bytes(*b"CAGY"))) as Option<&Vec<Vec<f32>>> {
                        if v.len() == 1 && v[0].len() >= 3 {
                            gyro_cal = (v[0][0], v[0][1], v[0][2]);
                        }
                    }

                    if let Some(v) = v.get_t(TagId::Unknown(u32::from_be_bytes(*b"ACGY"))) as Option<&Vec<Vec<Scalar>>> {
                        let first_frame_ts = frame_timestamps.first().cloned().unwrap_or(0) as f64;
                        let last_frame_ts  = frame_timestamps.last() .cloned().unwrap_or(0) as f64;
                        let last_imu_ts    = imu_timestamps  .last() .cloned().unwrap_or(0) as f64;

                        let num_samples = v.len();
                        for (i, sample) in v.into_iter().enumerate() {
                            if sample.len() == 7 {
                                let Scalar::u32(_t) = sample[0] else { continue; };
                                let Scalar::i16(ax) = sample[1] else { continue; };
                                let Scalar::i16(ay) = sample[2] else { continue; };
                                let Scalar::i16(az) = sample[3] else { continue; };
                                let Scalar::i16(gx) = sample[4] else { continue; };
                                let Scalar::i16(gy) = sample[5] else { continue; };
                                let Scalar::i16(gz) = sample[6] else { continue; };

                                let avg_frame_time = (last_frame_ts - first_frame_ts) as f64 / frame_timestamps.len() as f64 / 1000.0; // in ms
                                let playback_frame_time = 1000.0 / 24.0; // TODO

                                let ratio = playback_frame_time / avg_frame_time;

                                let imu_rate = 1000.0; // Hz

                                let t = (((last_imu_ts - first_frame_ts) / 1000_000.0) - (num_samples - i) as f64 / imu_rate) * ratio;
                                accl.push(TimeVector3 {
                                    t,
                                    x: ax as f64 * -acc_cal.0 as f64,
                                    y: ay as f64 * -acc_cal.1 as f64,
                                    z: az as f64 * -acc_cal.2 as f64,
                                });
                                gyro.push(TimeVector3 {
                                    t,
                                    x: gx as f64 * gyro_cal.0 as f64,
                                    y: gy as f64 * gyro_cal.1 as f64,
                                    z: gz as f64 * gyro_cal.2 as f64,
                                });
                            }
                        }
                    }
                }

                let imu_orientation = "ZyX"; // TODO
                if !gyro.is_empty() {
                    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Data,        "Gyroscope data",  Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]));
                    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Unit,        "Gyroscope unit",  String,              |v| v.to_string(), "rad/s".into(), Vec::new()));
                    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Orientation, "IMU orientation", String,              |v| v.to_string(), imu_orientation.into(), Vec::new()));
                }
                if !accl.is_empty() {
                    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Data,        "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), accl, vec![]));
                    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Unit,        "Accelerometer unit", String,              |v| v.to_string(), "m/s²".into(),  Vec::new()));
                    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation",    String,              |v| v.to_string(), imu_orientation.into(), Vec::new()));
                }

                info.tag_map = Some(map);
                samples.push(info);
            }
        }, cancel_flag)?;

        Ok(samples)
    }
}
