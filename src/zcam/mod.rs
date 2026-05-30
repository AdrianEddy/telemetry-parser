// SPDX-License-Identifier: MIT OR Apache-2.0

pub mod imvtmeta;

use std::io::*;
use std::sync::{Arc, atomic::AtomicBool};

use crate::tags_impl::*;
use crate::*;
use memchr::memmem;
use prost::Message;

#[derive(Default)]
pub struct Zcam {
    pub model: Option<String>,
    pub frame_readout_time: Option<f64>,
}

#[derive(PartialEq, Debug, Clone, Copy)]
enum DeviceProtobuf {
    Unknown,
    IMVTCam,
}

impl Zcam {
    pub fn camera_type(&self) -> String {
        "ZCAM".to_owned()
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

    pub fn detect<P: AsRef<std::path::Path>>(
        buffer: &[u8],
        _filepath: P,
        _options: &crate::InputOptions,
    ) -> Option<Self> {
        if memmem::find(buffer, b"IMVT meta").is_some() {
            Some(Self {
                model: None,
                frame_readout_time: None,
            })
        } else {
            None
        }
    }

    pub fn parse<T: Read + Seek, F: Fn(f64)>(
        &mut self,
        stream: &mut T,
        size: usize,
        progress_cb: F,
        cancel_flag: Arc<AtomicBool>,
        options: crate::InputOptions,
    ) -> Result<Vec<SampleInfo>> {
        let mut samples = Vec::new();
        let mut first_timestamp = 0;

        let mut fps = 59.94;
        let mut sensor_fps = 59.969295501708984;
        let mut sample_rate = 500.0;
        let mut imu_time_offset_us = 0.0;
        let mut gyro_scale = 1.0;
        let mut accel_scale = 1.0;
        let mut orientation_matrix: Option<Vec<f32>> = None;

        let mut which_proto = DeviceProtobuf::Unknown;

        let cancel_flag2 = cancel_flag.clone();
        let _ctx = util::get_metadata_track_samples(
            stream,
            size,
            true,
            |mut info: SampleInfo,
             data: &[u8],
             file_position: u64,
             _video_md: Option<&VideoMetadata>| {
                if size > 0 {
                    progress_cb(file_position as f64 / size as f64);
                }

                if which_proto == DeviceProtobuf::Unknown {
                    if data.len() > 64 && (memmem::find(&data[0..64], b"imvt_cam.proto").is_some())
                    {
                        which_proto = DeviceProtobuf::IMVTCam;
                    } else {
                        which_proto = DeviceProtobuf::Unknown;
                    }
                    log::debug!("Using device protobuf: {which_proto:?}");
                }

                macro_rules! handle_parsed {
                ($parsed:expr, $field:tt) => {
                    let mut tag_map = GroupedTagMap::new();

                    if let Some(ref clip) = $parsed.clip_meta {
                        self.model = clip.clip_meta_header.as_ref().map(|h| h.product_name.clone());
                        self.frame_readout_time = clip.image_sensor_info.as_ref().map(|h| h.read_out_time as f64 / 1000_000.0);

                        if let Some(ref lens) = clip.lens_info {
                            let lens_name = lens.lens_name.trim().to_owned();
                            if !lens_name.is_empty() {
                                util::insert_tag(&mut tag_map, tag!(parsed GroupId::Lens, TagId::Name, "Lens name", String, |v| v.clone(), lens_name.clone(), vec![]), &options);
                            }
                        }

                        if let Some(ref sensor) = clip.image_sensor_info {
                            if sensor.pixel_size_nm > 0 {
                                util::insert_tag(&mut tag_map, tag!(parsed GroupId::Imager, TagId::PixelPitch, "Pixel pitch", u32x2, |v| format!("{v:?}"), (sensor.pixel_size_nm, sensor.pixel_size_nm), vec![]), &options);
                            }
                            if sensor.sensor_pixel_width > 0 && sensor.sensor_pixel_height > 0 {
                                util::insert_tag(&mut tag_map, tag!(parsed GroupId::Imager, TagId::SensorSizePixels, "Sensor size pixels", u32x2, |v| format!("{v:?}"), (sensor.sensor_pixel_width, sensor.sensor_pixel_height), vec![]), &options);
                            }
                            if let (Some(w), Some(h)) = (sensor.crop_width, sensor.crop_height) {
                                if w > 0 && h > 0 {
                                    util::insert_tag(&mut tag_map, tag!(parsed GroupId::Imager, TagId::CaptureAreaSize, "Capture area size", f32x2, |v| format!("{v:?}"), (w as f32, h as f32), vec![]), &options);
                                }
                            }
                        }

                        let has_clip_metadata = clip
                            .clip_meta_header
                            .as_ref()
                            .map(|h| {
                                !h.product_name.trim().is_empty()
                                    || !h.product_firmware_version.trim().is_empty()
                                    || !h.proto_file_name.trim().is_empty()
                            })
                            .unwrap_or(false)
                            || clip
                                .lens_info
                                .as_ref()
                                .map(|l| !l.lens_name.trim().is_empty() || l.image_circle_diameter > 0.0)
                                .unwrap_or(false);
                        if has_clip_metadata {
                            let metadata = serde_json::json!({
                            "product_name": clip.clip_meta_header.as_ref().map(|h| h.product_name.trim()).unwrap_or_default(),
                            "product_firmware_version": clip.clip_meta_header.as_ref().map(|h| h.product_firmware_version.trim()).unwrap_or_default(),
                            "product_firmware_build_time": clip.clip_meta_header.as_ref().map(|h| h.product_firmware_build_time.trim()).unwrap_or_default(),
                            "product_firmware_commit_id": clip.clip_meta_header.as_ref().map(|h| h.product_firmware_commit_id.trim()).unwrap_or_default(),
                            "product_hardware_version": clip.clip_meta_header.as_ref().map(|h| h.product_hardware_version.trim()).unwrap_or_default(),
                            "proto_file_name": clip.clip_meta_header.as_ref().map(|h| h.proto_file_name.trim()).unwrap_or_default(),
                            "proto_product_version": clip.clip_meta_header.as_ref().map(|h| h.proto_product_version.trim()).unwrap_or_default(),
                            "proto_library_version": clip.clip_meta_header.as_ref().map(|h| h.proto_library_version.trim()).unwrap_or_default(),
                            "lens_type": clip.lens_info.as_ref().map(|l| l.lens_name.trim()).unwrap_or_default(),
                            "image_circle_diameter": clip.lens_info.as_ref().map(|l| l.image_circle_diameter).unwrap_or_default(),
                        });
                            util::insert_tag(&mut tag_map, tag!(parsed GroupId::Default, TagId::Metadata, "Metadata", Json, |v| serde_json::to_string(v).unwrap(), metadata, vec![]), &options);
                        }

                        if let Some(v) = clip.image_sensor_info.as_ref().map(|h| h.frame_rate as f64) {
                            sensor_fps = v;
                        }
                        if let Some(v) = clip.imu_info.as_ref().map(|h| h.gyro_scale as f64) {
                            gyro_scale = v;
                        }
                        if let Some(v) = clip.imu_info.as_ref().map(|h| h.accel_scale as f64) {
                            accel_scale = v;
                        }
                        if let Some(v) = clip.imu_info.as_ref().map(|h| h.sampling_rate as f64) {
                            sample_rate = v;
                        }
                        if let Some(v) = clip.imu_info.as_ref().map(|h| h.time_offset as f64) {
                            imu_time_offset_us = v;
                        }
                        orientation_matrix = clip.imu_info.as_ref().map(|h| h.orientation_matrix.clone());

                        if let Some(ref stream) = $parsed.stream_meta {
                            if let Some(ref meta) = stream.video_stream_meta {
                                let stream_fps = if meta.shot_fps > 0.0 { meta.shot_fps } else { meta.project_fps };
                                fps = stream_fps as f64;
                            }
                        }
                        if let Some(ref mut v) = self.frame_readout_time {
                            *v /= fps / sensor_fps;
                        }
                    }

                    let mut gyro = Vec::new();
                    let mut accl = Vec::new();
                    if let Some(ref frame) = $parsed.frame_meta {
                        let frame_ts = frame.frame_meta_header.as_ref().unwrap().frame_timestamp as i64;
                        if info.sample_index == 0 { first_timestamp = frame_ts; }
                        if let Some(ref imu) = frame.$field {
                            if let Some(ref imu_data) = imu.imu_data {
                                let sample_interval_us = 1_000_000.0 / sample_rate.max(1.0);
                                for (index, item) in imu_data.items.iter().enumerate() {
                                    let item_ts = imu_data.timestamp as f64 + imu_time_offset_us + (index as f64 * sample_interval_us);
                                    let ts = (item_ts - first_timestamp as f64) / 1_000_000.0;

                                    // IMU raw values are in LSB, convert them with scale from clip IMU info.
                                    gyro.push(TimeVector3 {
                                        t: ts,
                                        x: item.gx as f64 * gyro_scale,
                                        y: item.gy as f64 * gyro_scale,
                                        z: item.gz as f64 * gyro_scale,
                                    });
                                    accl.push(TimeVector3 {
                                        t: ts,
                                        x: item.ax as f64 * accel_scale,
                                        y: item.ay as f64 * accel_scale,
                                        z: item.az as f64 * accel_scale,
                                    });
                                }
                            }
                        }
                    }
                    if info.sample_index == 0 {
                        log::debug!("IMU samples: gyro={}, accel={}", gyro.len(), accl.len());
                    }
                    if !gyro.is_empty() {
                        util::insert_tag(&mut tag_map, tag!(parsed GroupId::Gyroscope,     TagId::Data, "Gyroscope data",     Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]), &options);
                        if let Some(matrix) = orientation_matrix.as_ref() {
                            util::insert_tag(&mut tag_map, tag!(parsed GroupId::Gyroscope, TagId::Matrix, "IMU orientation matrix", Vec_Vec_f32, |v| format!("{:?}", v), vec![matrix.clone()], vec![]), &options);
                        }
                    }
                    if !accl.is_empty() {
                        util::insert_tag(&mut tag_map, tag!(parsed GroupId::Accelerometer, TagId::Data, "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), accl, vec![]), &options);
                        if let Some(matrix) = orientation_matrix.as_ref() {
                            util::insert_tag(&mut tag_map, tag!(parsed GroupId::Accelerometer, TagId::Matrix, "IMU orientation matrix", Vec_Vec_f32, |v| format!("{:?}", v), vec![matrix.clone()], vec![]), &options);
                        }
                    }

                    // if info.index == 0 { dbg!(&parsed); }

                    info.tag_map = Some(tag_map);

                    samples.push(info);

                    if options.probe_only {
                        cancel_flag2.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                };
            }

                match which_proto {
                    DeviceProtobuf::Unknown => {}
                    DeviceProtobuf::IMVTCam => match imvtmeta::ProductMeta::decode(data) {
                        Ok(parsed) => {
                            handle_parsed!(parsed, frame_meta_of_imu);
                        }
                        Err(e) => {
                            log::warn!("Failed to parse protobuf: {:?}", e);
                        }
                    },
                }
            },
            cancel_flag,
        )?;

        Ok(samples)
    }
}
