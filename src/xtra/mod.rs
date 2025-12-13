// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2025 Adrian <adrian.eddy at gmail>

pub mod dvtm_ae2503;
pub mod dvtm_ae2504;

use std::io::*;
use std::sync::{ Arc, atomic::AtomicBool };

use crate::tags_impl::*;
use crate::*;
use crate::util::insert_tag;
use memchr::memmem;
use prost::Message;

#[derive(Default)]
pub struct Xtra {
    pub model: Option<String>,
    pub frame_readout_time: Option<f64>
}

#[derive(PartialEq, Debug, Clone, Copy)]
enum DeviceProtobuf {
    Unknown,
    Ae2503,
    Ae2504,
}

impl Xtra {
    pub fn camera_type(&self) -> String {
        "Xtra".to_owned()
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
        if memmem::find(buffer, b"xtmd").is_some() && (memmem::find(buffer, b"CAM meta").is_some()) {
            Some(Self {
                model: None,
                frame_readout_time: None
            })
        } else {
            None
        }
    }

    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, size: usize, progress_cb: F, cancel_flag: Arc<AtomicBool>, options: crate::InputOptions) -> Result<Vec<SampleInfo>> {
        let mut samples = Vec::new();
        let mut first_timestamp = 0;

        let mut focal_length = None;
        let mut distortion_coeffs = None;
        let mut exposure_time = 0.0;
        let mut fps = 59.94;
        let mut sensor_fps = 59.969295501708984;
        let mut sample_rate = 2000.0;
        // let mut global_quat_i = 0;

        // let mut first_vsync = 0;
        let mut prev_ts = 0.0;
        let mut prev_quat: Option<Quaternion<f64>> = None;
        let mut inv = false;

        let mut which_proto = DeviceProtobuf::Unknown;

        let cancel_flag2 = cancel_flag.clone();
        let ctx = util::get_metadata_track_samples(stream, size, true, |mut info: SampleInfo, data: &[u8], file_position: u64, _video_md: Option<&VideoMetadata>| {
            if size > 0 {
                progress_cb(file_position as f64 / size as f64);
            }

            if which_proto == DeviceProtobuf::Unknown {
                if data.len() > 64 && memmem::find(&data[0..64], b"ae2503").is_some() {
                    which_proto = DeviceProtobuf::Ae2503;
                } else if data.len() > 64 && memmem::find(&data[0..64], b"ae2504").is_some() {
                    which_proto = DeviceProtobuf::Ae2504;
                }
                log::debug!("Using device protobuf: {which_proto:?}");
            }

            macro_rules! handle_parsed {
                ($parsed:expr, $field:tt) => {
                    let mut tag_map = GroupedTagMap::new();

                    if let Some(ref clip) = $parsed.clip_meta {
                        self.model              = clip.clip_meta_header       .as_ref().map(|h| h.product_name.replace("XTRA ", "").replace("Xtra ", ""));
                        self.frame_readout_time = clip.sensor_readout_time    .as_ref().map(|h| h.readout_time as f64 / 1000_000.0);
                        focal_length            = clip.digital_focal_length   .as_ref().map(|h| h.focal_length as f64);
                        distortion_coeffs       = clip.distortion_coefficients.as_ref().map(|h| h.coeffients.clone());

                        if let Some(v) = clip.sensor_fps.as_ref().map(|h| h.sensor_frame_rate as f64) {
                            sensor_fps = v;
                        }
                        if let Some(v) = clip.imu_sampling_rate.as_ref().map(|h| h.imu_sampling_rate as f64) {
                            sample_rate = v;
                        }

                        let v = serde_json::to_value(&clip).map_err(|_| Error::new(ErrorKind::Other, "Serialize error"));
                        if let Ok(vv) = v {
                            log::debug!("Metadata: {:?}", &vv);
                            insert_tag(&mut tag_map, tag!(parsed GroupId::Default, TagId::Metadata, "Metadata", Json, |v| serde_json::to_string(v).unwrap(), vv, vec![]), &options);
                        }
                        if let Some(ref stream) = $parsed.stream_meta {
                            if let Some(ref meta) = stream.video_stream_meta {
                                fps = meta.framerate as f64;
                            }
                        }
                        if let Some(ref mut v) = self.frame_readout_time {
                            *v /= fps / sensor_fps;
                        }
                    }

                    let fps_ratio = fps / sensor_fps;

                    let mut quats = Vec::new();
                    if let Some(ref frame) = $parsed.frame_meta {
                        let frame_ts = frame.frame_meta_header.as_ref().unwrap().frame_timestamp as i64;
                        if info.sample_index == 0 { first_timestamp = frame_ts; }
                        let frame_relative_ts = frame_ts - first_timestamp;

                        if let Some(ref e) = frame.camera_frame_meta {
                            exposure_time = e.exposure_time.as_ref().and_then(|v| Some(*v.exposure_time.get(0)? as f64 / *v.exposure_time.get(1)? as f64)).unwrap_or_default() * 1000.0;
                        }

                        if let Some(ref imu) = frame.imu_frame_meta {
                            if let Some(ref attitude) = imu.$field {
                                let len = attitude.attitude.len() as f64;

                                let vsync_duration = 1000.0 / sensor_fps.max(1.0);

                                let frame_timestamp = (frame_relative_ts as f64) / 1000.0;

                                for (i, q) in attitude.attitude.iter().enumerate() {
                                    let index = i as f64 - attitude.offset as f64;
                                    let quat_ts = frame_timestamp + ((index / len) * vsync_duration);

                                    let ts = quat_ts / fps_ratio;

                                    prev_ts = ts;

                                    if q.quaternion_w.is_nan() || q.quaternion_x.is_nan() || q.quaternion_y.is_nan() || q.quaternion_z.is_nan() {
                                        continue;
                                    }

                                    let quat = util::multiply_quats(
                                        (q.quaternion_w as f64,
                                        q.quaternion_x as f64,
                                        q.quaternion_y as f64,
                                        q.quaternion_z as f64),
                                        (0.5, -0.5, -0.5, 0.5),
                                    );
                                    // Rotate Y axis 180 deg for horizon lock
                                    let quat = util::multiply_quats((0.0, 0.0, 1.0, 0.0), (quat.w, quat.x, quat.y, quat.z));

                                    if quat.w == 0.0 && quat.x == 0.0 && quat.y == 0.0 && quat.z == 0.0 {
                                        continue;
                                    }

                                    if prev_quat.is_some() && (prev_quat.unwrap() - quat).norm_squared().sqrt() > 1.5 {
                                        inv = !inv;
                                    }
                                    prev_quat = Some(quat.clone());

                                    quats.push(TimeQuaternion {
                                        t: ts,
                                        v: if inv { -quat } else { quat },
                                    });
                                }

                                if info.sample_index == 0 { log::debug!("Quaternions: {:?}", &quats); }
                                util::insert_tag(&mut tag_map, tag!(parsed GroupId::Quaternion, TagId::Data, "Quaternion data",  Vec_TimeQuaternion_f64, |v| format!("{:?}", v), quats, vec![]), &options);
                            }
                        }
                    }

                    info.tag_map = Some(tag_map);

                    samples.push(info);

                    if options.probe_only {
                        cancel_flag2.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                };
            }

            match which_proto {
                DeviceProtobuf::Unknown => { },
                DeviceProtobuf::Ae2504 => match dvtm_ae2503::ProductMeta::decode(data) {
                    Ok(parsed) => { handle_parsed!(parsed, imu_attitude_after_fusion); },
                    Err(e) => { log::warn!("Failed to parse protobuf: {:?}", e); }
                },
                DeviceProtobuf::Ae2503 => match dvtm_ae2504::ProductMeta::decode(data) {
                    Ok(parsed) => { handle_parsed!(parsed, imu_attitude_after_fusion); },
                    Err(e) => { log::warn!("Failed to parse protobuf: {:?}", e); }
                },
            }
        }, cancel_flag)?;

        match (samples.first_mut(), focal_length, distortion_coeffs) {
            (Some(sample), Some(focal_length), Some(coeffs)) if coeffs.len() >= 4 => {
                if let Some(tkhd) = ctx.tracks.iter().filter(|x| x.track_type == mp4parse::TrackType::Video).filter_map(|x| x.tkhd.as_ref()).next() {
                    let (w, h) = (tkhd.width >> 16, tkhd.height >> 16);

                    let profile = self.get_lens_profile(w, h, focal_length, &coeffs);
                    if let Some(ref mut tag_map) = sample.tag_map {
                        insert_tag(tag_map, tag!(parsed GroupId::Lens, TagId::Data, "Lens profile", Json, |v| serde_json::to_string(v).unwrap(), profile, vec![]), &options);
                    }
                }
            },
            _ => { }
        }

        Ok(samples)
    }

    fn get_lens_profile(&self, width: u32, height: u32, focal_length: f64, coeffs: &[f32]) -> serde_json::Value {
        let model = self.model.clone().unwrap_or_default();
        let half_width = width as f64 / 2.0;
        let half_height = height as f64 / 2.0;
        let output_size = Self::get_output_size(width, height);
        serde_json::json!({
            "calibrated_by": "Xtra",
            "camera_brand": "Xtra",
            "camera_model": model,
            "calib_dimension":  { "w": width, "h": height },
            "orig_dimension":   { "w": width, "h": height },
            "output_dimension": { "w": output_size.0, "h": output_size.1 },
            "frame_readout_time": self.frame_readout_time,
            "official": true,
            "fisheye_params": {
              "camera_matrix": [
                [ focal_length, 0.0, half_width ],
                [ 0.0, focal_length, half_height ],
                [ 0.0, 0.0, 1.0 ]
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
        })
    }

    fn get_output_size(width: u32, height: u32) -> (u32, u32) {
        let aspect = (width as f64 / height as f64 * 100.0) as u32;
        match aspect {
            133 => (width, (width as f64 / 1.7777777777777).round() as u32), // 4:3 -> 16:9
            _   => (width, height)
        }
    }
}
