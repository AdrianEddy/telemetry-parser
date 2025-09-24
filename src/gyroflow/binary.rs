// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2025 Adrian <adrian.eddy at gmail>

use std::io::*;
use std::sync::{ Arc, atomic::AtomicBool };

use crate::tags_impl::*;
use crate::*;
use crate::util::insert_tag;
use memchr::memmem;
use prost::Message;

#[derive(Default)]
pub struct GyroflowProtobuf {
    pub model: Option<String>,
    vendor: String,
    pub frame_readout_time: Option<f64>,
    imu_orientation: String,
}

impl GyroflowProtobuf {
    pub fn camera_type(&self) -> String {
        self.vendor.clone()
    }
    pub fn has_accurate_timestamps(&self) -> bool {
        true
    }
    pub fn possible_extensions() -> Vec<&'static str> {
        vec!["mp4", "mov"]
    }
    pub fn frame_readout_time(&self) -> Option<f64> {
        self.frame_readout_time
    }
    pub fn normalize_imu_orientation(v: String) -> String {
        v
    }

    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], _filepath: P, _options: &crate::InputOptions) -> Option<Self> {
        if memmem::find(buffer, b"GyroflowProtobuf").is_some() {
            Some(Self {
                model: None,
                vendor: "Gyroflow".into(),
                frame_readout_time: None,
                imu_orientation: "XYZ".into(),
            })
        } else {
            None
        }
    }

    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, size: usize, progress_cb: F, cancel_flag: Arc<AtomicBool>, options: crate::InputOptions) -> Result<Vec<SampleInfo>> {
        let mut samples = Vec::new();

        let cancel_flag2 = cancel_flag.clone();
        // let mut first_timestamp = None;
        util::get_metadata_track_samples(stream, size, true, |mut info: SampleInfo, data: &[u8], file_position: u64, _video_md: Option<&VideoMetadata>| {
            if size > 0 {
                progress_cb(file_position as f64 / size as f64);
            }
            if !memmem::find(data, b"GyroflowProtobuf").is_some() {
                log::warn!("Unexpected data: {}", pretty_hex::pretty_hex(&data));
            }

            match super::gyroflow_proto::Main::decode(data) {
                Ok(parsed) => {
                    let mut tag_map = GroupedTagMap::new();
                    //dbg!(&parsed);

                    if let Some(ref header) = parsed.header {
                        if let Some(ref cam) = header.camera {
                            self.vendor = cam.camera_brand.clone();
                            self.model = Some(cam.camera_model.clone()).filter(|x| !x.is_empty());
                            if let Some(ref profile) = cam.lens_profile {
                                if profile.starts_with('{') && let Ok(profile_json) = serde_json::from_str(profile) {
                                    insert_tag(&mut tag_map, tag!(parsed GroupId::Lens, TagId::Data, "Lens profile", Json, |v| serde_json::to_string(v).unwrap(), profile_json, vec![]), &options);
                                } else {
                                    insert_tag(&mut tag_map, tag!(parsed GroupId::Lens, TagId::Name, "Lens profile", String, |v| v.clone(), profile.clone(), vec![]), &options);
                                }
                            }
                            if let Some(ref imuo) = cam.imu_orientation {
                                self.imu_orientation = imuo.clone();
                            }
                        }
                        if let Some(ref clip) = header.clip {
                            self.frame_readout_time = Some(clip.frame_readout_time_us as f64 / 1000.0);
                        }
                    }

                    if let Some(ref frame) = parsed.frame {
                        let ts = frame.start_timestamp_us as f64 * 1e-6;
                        // if first_timestamp.is_none() {
                        //     first_timestamp = Some(ts);
                        // }
                        // let ts = ts - first_timestamp.unwrap();

                        let mut gyro = Vec::with_capacity(frame.imu.len());
                        let mut acc  = Vec::with_capacity(frame.imu.len());
                        let mut mag  = Vec::with_capacity(frame.imu.len());
                        for imu in &frame.imu {
                            gyro.push(TimeVector3 {
                                t: ts,
                                x: imu.gyroscope_x as f64,
                                y: imu.gyroscope_y as f64,
                                z: imu.gyroscope_z as f64
                            });
                            acc.push(TimeVector3 {
                                t: ts,
                                x: imu.accelerometer_x as f64,
                                y: imu.accelerometer_y as f64,
                                z: imu.accelerometer_z as f64
                            });
                            match (imu.magnetometer_x, imu.magnetometer_y, imu.magnetometer_z) {
                                (Some(x), Some(y), Some(z)) => {
                                    mag.push(TimeVector3 {
                                        t: ts,
                                        x: x as f64,
                                        y: y as f64,
                                        z: z as f64
                                    });
                                },
                                _ => { }
                            }
                        }
                        util::insert_tag(&mut tag_map, tag!(parsed GroupId::Accelerometer, TagId::Data, "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), acc, vec![]), &options);
                        util::insert_tag(&mut tag_map, tag!(parsed GroupId::Gyroscope,     TagId::Data, "Gyroscope data",     Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]), &options);
                        if !mag.is_empty() {
                            util::insert_tag(&mut tag_map, tag!(parsed GroupId::Magnetometer,  TagId::Data, "Magnetometer data",  Vec_TimeVector3_f64, |v| format!("{:?}", v), mag, vec![]), &options);
                            util::insert_tag(&mut tag_map, tag!(parsed GroupId::Magnetometer,  TagId::Unit, "Magnetometer unit",  String, |v| v.to_string(), "µT".into(), Vec::new()), &options);
                            util::insert_tag(&mut tag_map, tag!(parsed GroupId::Magnetometer,  TagId::Orientation, "IMU orientation", String, |v| v.to_string(), self.imu_orientation.clone(), Vec::new()), &options);
                        }

                        util::insert_tag(&mut tag_map, tag!(parsed GroupId::Accelerometer, TagId::Unit, "Accelerometer unit", String, |v| v.to_string(), "m/s²".into(),  Vec::new()), &options);
                        util::insert_tag(&mut tag_map, tag!(parsed GroupId::Gyroscope,     TagId::Unit, "Gyroscope unit",     String, |v| v.to_string(), "rad/s".into(), Vec::new()), &options);

                        util::insert_tag(&mut tag_map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), self.imu_orientation.clone(), Vec::new()), &options);
                        util::insert_tag(&mut tag_map, tag!(parsed GroupId::Gyroscope,     TagId::Orientation, "IMU orientation", String, |v| v.to_string(), self.imu_orientation.clone(), Vec::new()), &options);
                    }

                    info.tag_map = Some(tag_map);

                    samples.push(info);

                    if options.probe_only {
                        cancel_flag2.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                },
                Err(e) => {
                    log::error!("Failed to parse protobuf: {e:?}");
                    log::error!("Data: {}", pretty_hex::pretty_hex(&data));
                }
            }
        }, cancel_flag)?;

        Ok(samples)
    }
}
