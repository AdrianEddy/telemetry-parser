// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2023 Adrian <adrian.eddy at gmail>

use std::io::*;
use std::sync::{ Arc, atomic::AtomicBool };

use crate::tags_impl::*;
use crate::*;
use byteorder::{ ReadBytesExt, LittleEndian };
use memchr::memmem;

#[derive(Default)]
pub struct Camm {
    pub model: Option<String>,
    frame_readout_time: Option<f64>
}

// We could parse that too
// https://github.com/google/spatial-media/blob/master/docs/spherical-video-v2-rfc.md

impl Camm {
    pub fn possible_extensions() -> Vec<&'static str> { vec!["mp4", "mov"] }

    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], _filepath: P) -> Option<Self> {
        for camm_pos in memmem::find_iter(buffer, b"camm") {
            if buffer.len() > 16 + camm_pos && &buffer[4..8] == b"ftyp" && &buffer[camm_pos-4-4-4-4..camm_pos-4-4-4] == b"stsd" {
                return Some(Self::default());
            }
        }
        None
    }

    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, size: usize, progress_cb: F, cancel_flag: Arc<AtomicBool>) -> Result<Vec<SampleInfo>> {
        let mut gyro = Vec::new();
        let mut accl = Vec::new();
        let mut magn = Vec::new();
        let mut pos = Vec::new();
        let mut quats = Vec::new();
        let mut gps = Vec::new();

        let mut samples = Vec::new();

        util::get_metadata_track_samples(stream, size, false, |info: SampleInfo, data: &[u8], file_position: u64| {
            if size > 0 {
                progress_cb(((info.track_index as f64 - 1.0) + (file_position as f64 / size as f64)) / 3.0);
            }

            if data.len() >= 4 {
                // https://developers.google.com/streetview/publish/camm-spec
                let mut d = Cursor::new(data);
                crate::try_block!({
                    let _reserved = d.read_u16::<LittleEndian>().ok()?;
                    let typ = d.read_u16::<LittleEndian>().ok()?;
                    match typ {
                        0 => { // angle_axis
                            let x = d.read_f32::<LittleEndian>().ok()? as f64;
                            let y = -d.read_f32::<LittleEndian>().ok()? as f64;
                            let z = -d.read_f32::<LittleEndian>().ok()? as f64;

                            // Separate axis and angle
                            let angle_rad = (x*x + y*y + z*z).sqrt();
                            let x = x / angle_rad;
                            let y = y / angle_rad;
                            let z = z / angle_rad;

                            // Convert to quaternion
                            let s = (angle_rad / 2.0).sin();
                            let quat = Quaternion {
                                x: x * s,
                                y: y * s,
                                z: z * s,
                                w: (angle_rad / 2.0).cos()
                            };

                            quats.push(TimeQuaternion { t: info.timestamp_ms, v: quat });
                        },
                        1 => {
                            let _pixel_exposure_time = d.read_i32::<LittleEndian>().ok()?;
                            let rolling_shutter_skew_time = d.read_i32::<LittleEndian>().ok()?;
                            self.frame_readout_time = Some(rolling_shutter_skew_time as f64 / 1000000.0); // nanoseconds to milliseconds
                        },
                        2 => { // gyro
                            gyro.push(TimeVector3 { t: info.timestamp_ms / 1000.0,
                                x: d.read_f32::<LittleEndian>().ok()? as f64,
                                y: d.read_f32::<LittleEndian>().ok()? as f64,
                                z: d.read_f32::<LittleEndian>().ok()? as f64
                            });
                        },
                        3 => { // acceleration
                            accl.push(TimeVector3 { t: info.timestamp_ms / 1000.0,
                                x: d.read_f32::<LittleEndian>().ok()? as f64,
                                y: d.read_f32::<LittleEndian>().ok()? as f64,
                                z: d.read_f32::<LittleEndian>().ok()? as f64
                            });
                        },
                        4 => { // position
                            pos.push(TimeVector3 { t: info.timestamp_ms,
                                x: d.read_f32::<LittleEndian>().ok()? as f64,
                                y: d.read_f32::<LittleEndian>().ok()? as f64,
                                z: d.read_f32::<LittleEndian>().ok()? as f64
                            });
                        },
                        5 => { // minimal gps
                            let latitude  = d.read_f64::<LittleEndian>().ok()?; // degrees
                            let longitude = d.read_f64::<LittleEndian>().ok()?; // degrees
                            let altitude  = d.read_f64::<LittleEndian>().ok()?; // degrees
                            gps.push(GpsData {
                                is_acquired: true,
                                unix_timestamp: info.timestamp_ms / 1000.0,
                                lat: latitude,
                                lon: longitude,
                                speed: 0.0,
                                track: 0.0,
                                altitude
                            });
                        },
                        6 => { // gps
                            let time_gps_epoch      = d.read_f64::<LittleEndian>().ok()?; // seconds
                            let gps_fix_type        = d.read_i32::<LittleEndian>().ok()?; // 0 (no fix), 2 (2D fix), 3 (3D fix)
                            let latitude            = d.read_f64::<LittleEndian>().ok()?; // degrees
                            let longitude           = d.read_f64::<LittleEndian>().ok()?; // degrees
                            let altitude            = d.read_f32::<LittleEndian>().ok()? as f64; // meters
                            let _horizontal_accuracy = d.read_f32::<LittleEndian>().ok()?; // meters
                            let _vertical_accuracy   = d.read_f32::<LittleEndian>().ok()?; // meters
                            let _velocity_east       = d.read_f32::<LittleEndian>().ok()?; // meters/seconds
                            let _velocity_north      = d.read_f32::<LittleEndian>().ok()?; // meters/seconds
                            let _velocity_up         = d.read_f32::<LittleEndian>().ok()?; // meters/seconds
                            let _speed_accuracy      = d.read_f32::<LittleEndian>().ok()?; // meters/seconds

                            gps.push(GpsData {
                                is_acquired: gps_fix_type > 0,
                                unix_timestamp: time_gps_epoch,
                                lat: latitude,
                                lon: longitude,
                                speed: 0.0, // TODO
                                track: 0.0, // TODO
                                altitude
                            });
                        },
                        7 => { // magnetic_field
                            magn.push(TimeVector3 { t: info.timestamp_ms / 1000.0,
                                x: d.read_f32::<LittleEndian>().ok()? as f64,
                                y: d.read_f32::<LittleEndian>().ok()? as f64,
                                z: d.read_f32::<LittleEndian>().ok()? as f64
                            });
                        },
                        _ => {
                            log::warn!("Unknown CAMM type: {typ}: {}", pretty_hex::pretty_hex(&data));
                        }
                    }
                });
            }
        }, cancel_flag)?;

        let mut map = GroupedTagMap::new();

        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Data, "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), accl, vec![]));
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Data, "Gyroscope data",     Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]));
        util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer,  TagId::Data, "Magnetometer data",  Vec_TimeVector3_f64, |v| format!("{:?}", v), magn, vec![]));
        util::insert_tag(&mut map, tag!(parsed GroupId::Position3D,    TagId::Data, "3D position data",   Vec_TimeVector3_f64, |v| format!("{:?}", v), pos, vec![]));
        util::insert_tag(&mut map, tag!(parsed GroupId::Quaternion,    TagId::Data, "Quaternion data",    Vec_TimeQuaternion_f64, |v| format!("{:?}", v), quats, vec![]));
        util::insert_tag(&mut map, tag!(parsed GroupId::GPS,           TagId::Data, "GPS data",           Vec_GpsData, |v| format!("{:?}", v), gps, vec![]));

        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Unit, "Accelerometer unit", String, |v| v.to_string(), "m/s²".into(),  Vec::new()));
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Unit, "Gyroscope unit",     String, |v| v.to_string(), "rad/s".into(), Vec::new()));
        util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer,  TagId::Unit, "Magnetometer unit",  String, |v| v.to_string(), "μT".into(), Vec::new()));

        let imu_orientation = "yxz";
        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()));
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()));
        util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer,  TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()));

        samples.insert(0, SampleInfo { tag_map: Some(map), ..Default::default() });

        Ok(samples)
    }

    pub fn normalize_imu_orientation(v: String) -> String {
        v
    }

    pub fn camera_type(&self) -> String {
        "CAMM".to_string()
    }

    pub fn frame_readout_time(&self) -> Option<f64> {
        self.frame_readout_time
    }
}
