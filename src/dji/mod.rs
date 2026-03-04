// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2022 Adrian <adrian.eddy at gmail>

pub mod dvtm_wm169;
pub mod dvtm_eagle4_wa530;
pub mod dvtm_ac203;
pub mod dvtm_ac204;
pub mod dvtm_ow001;

use std::io::*;
use std::sync::{ Arc, atomic::AtomicBool };

use chrono::NaiveDateTime;
use crate::tags_impl::*;
use crate::*;
use crate::util::insert_tag;
use memchr::memmem;
use prost::Message;

mod csv;

fn parse_gps_datetime(s: &str) -> Option<f64> {
    let s = s.trim();
    let formats = [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H-%M-%S",
        "%Y:%m:%d %H:%M:%S",
        "%Y:%m:%d %H-%M-%S",
    ];
    for fmt in formats {
        if let Ok(dt) = NaiveDateTime::parse_from_str(s, fmt) {
            return Some(dt.and_utc().timestamp() as f64);
        }
    }
    None
}

fn normalize_model_name(raw: &str) -> String {
    raw.replace("DJI ", "")
        .replace('\n', " ")
        .replace('\r', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Default)]
struct DjiCommonTags {
    serial: Option<String>,
    model: Option<String>,
    frame_width: Option<u32>,
    frame_height: Option<u32>,
    fps: Option<f64>,
    iso: Option<u32>,
    shutter_s: Option<f64>,
    color_temp_k: Option<f64>,
    accel: Option<TimeVector3<f64>>,
    gps: Option<GpsData>,
    gps_lat: Option<f64>,
    gps_lon: Option<f64>,
    gps_alt_m: Option<f64>,
    gps_status: Option<String>,
    gps_alt_type: Option<String>,
    has_gps_time: Option<bool>,
    gps_datetime: Option<String>,
}

fn gps_status_str(status: u64) -> String {
    match status {
        0 => "GPS_NORMAL".to_string(),
        1 => "GPS_INVALID".to_string(),
        2 => "GPS_RTK".to_string(),
        _ => format!("GPS_UNKNOWN({})", status),
    }
}

fn gps_alt_type_str(alt_type: u64) -> String {
    match alt_type {
        0 => "PRESSURE_ALTITUDE".to_string(),
        1 => "GPS_FUSION_ALTITUDE".to_string(),
        2 => "RTK_ALTITUDE".to_string(),
        _ => format!("ALTITUDE_UNKNOWN({})", alt_type),
    }
}

fn normalize_gps_coords(unit: i32, lat: f64, lon: f64) -> (f64, f64) {
    match unit {
        0 => (lat.to_degrees(), lon.to_degrees()), // UNIT_RAD
        _ => (lat, lon), // UNIT_DEG or unknown
    }
}

// NOTE: This helper intentionally covers only "lightweight" DJI formats (ow001/ac203/ac204)
// to avoid duplicating common tag insertion. wm169/wa530 use a separate macro because they
// include richer IMU/quaternion timing logic that does not fit this shared path.
fn insert_common_tags(tag_map: &mut GroupedTagMap, common: &DjiCommonTags, options: &crate::InputOptions) {
    if let Some(ref serial) = common.serial {
        insert_tag(tag_map, tag!(parsed GroupId::Default, TagId::SerialNumber, "Serial number", String, |v| v.to_string(), serial.clone(), vec![]), options);
    }
    if let Some(ref model) = common.model {
        insert_tag(tag_map, tag!(parsed GroupId::Default, TagId::Custom("Model".into()), "Model", String, |v| v.to_string(), model.clone(), vec![]), options);
    }

    if let Some(w) = common.frame_width {
        insert_tag(tag_map, tag!(parsed GroupId::Default, TagId::Custom("FrameWidth".into()), "Frame width", u32, |v| format!("{:?}", v), w, vec![]), options);
    }
    if let Some(h) = common.frame_height {
        insert_tag(tag_map, tag!(parsed GroupId::Default, TagId::Custom("FrameHeight".into()), "Frame height", u32, |v| format!("{:?}", v), h, vec![]), options);
    }
    if let Some(fps) = common.fps {
        insert_tag(tag_map, tag!(parsed GroupId::Default, TagId::FrameRate, "Frame rate", f64, |v| format!("{:.3}", v), fps, vec![]), options);
    }
    if common.frame_width.is_some() || common.frame_height.is_some() || common.fps.is_some() {
        let mut obj = serde_json::Map::new();
        if let Some(w) = common.frame_width { obj.insert("width".into(), serde_json::Value::from(w)); }
        if let Some(h) = common.frame_height { obj.insert("height".into(), serde_json::Value::from(h)); }
        if let Some(fps) = common.fps { obj.insert("fps".into(), serde_json::Value::from(fps)); }
        insert_tag(tag_map, tag!(parsed GroupId::Default, TagId::Custom("FrameInfo".into()), "Frame info", Json, |v| serde_json::to_string(v).unwrap(), serde_json::Value::Object(obj), vec![]), options);
    }

    if let Some(iso) = common.iso {
        insert_tag(tag_map, tag!(parsed GroupId::Exposure, TagId::ISOValue, "ISO", u32, |v| v.to_string(), iso, vec![]), options);
    }
    if let Some(shutter) = common.shutter_s {
        insert_tag(tag_map, tag!(parsed GroupId::Exposure, TagId::ShutterSpeed, "Shutter speed", f64, |v| format!("{:.6}", v), shutter, vec![]), options);
    }
    if let Some(temp) = common.color_temp_k {
        insert_tag(tag_map, tag!(parsed GroupId::Colors, TagId::Custom("ColorTemperature".into()), "Color temperature", f64, |v| format!("{:.1}", v), temp, vec![]), options);
    }

    if let Some(ref acc) = common.accel {
        insert_tag(tag_map, tag!(parsed GroupId::Accelerometer, TagId::Data, "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), vec![acc.clone()], vec![]), options);
        insert_tag(tag_map, tag!(parsed GroupId::Accelerometer, TagId::Unit, "Accelerometer unit", String, |v| v.to_string(), "g".into(), Vec::new()), options);
        insert_tag(tag_map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), "XYZ".into(), Vec::new()), options);
    }

    if let Some(lat) = common.gps_lat {
        insert_tag(tag_map, tag!(parsed GroupId::GPS, TagId::Custom("GPSLatitude".into()), "GPS latitude", f64, |v| format!("{:.7}", v), lat, vec![]), options);
    }
    if let Some(lon) = common.gps_lon {
        insert_tag(tag_map, tag!(parsed GroupId::GPS, TagId::Custom("GPSLongitude".into()), "GPS longitude", f64, |v| format!("{:.7}", v), lon, vec![]), options);
    }
    if let Some(alt) = common.gps_alt_m {
        insert_tag(tag_map, tag!(parsed GroupId::GPS, TagId::Custom("GPSAltitude".into()), "GPS altitude", f64, |v| format!("{:.3}", v), alt, vec![]), options);
    }

    if let Some(ref status) = common.gps_status {
        insert_tag(tag_map, tag!(parsed GroupId::GPS, TagId::Custom("GPSStatus".into()), "GPS status", String, |v| v.to_string(), status.clone(), vec![]), options);
    }
    if let Some(ref alt_type) = common.gps_alt_type {
        insert_tag(tag_map, tag!(parsed GroupId::GPS, TagId::Custom("GPSAltitudeType".into()), "GPS altitude type", String, |v| v.to_string(), alt_type.clone(), vec![]), options);
    }
    if let Some(has_time) = common.has_gps_time {
        insert_tag(tag_map, tag!(parsed GroupId::GPS, TagId::Custom("HasGpsTime".into()), "Has GPS time", bool, |v| format!("{:?}", v), has_time, vec![]), options);
    }
    if let Some(ref dt) = common.gps_datetime {
        insert_tag(tag_map, tag!(parsed GroupId::GPS, TagId::Custom("GPSDateTime".into()), "GPS datetime", String, |v| v.to_string(), dt.clone(), vec![]), options);
    }
    if let Some(ref gps) = common.gps {
        insert_tag(tag_map, tag!(parsed GroupId::GPS, TagId::Data, "GPS data", Vec_GpsData, |v| format!("{:?}", v), vec![gps.clone()], vec![]), options);
    }
}

#[derive(Default)]
pub struct Dji {
    pub model: Option<String>,
    pub frame_readout_time: Option<f64>,
}

#[derive(PartialEq, Debug, Clone, Copy)]
enum DeviceProtobuf {
    Unknown,
    Wm169,
    Wa530,
    Ac203,
    Ac204,
    Ow001,
}

impl Dji {
    pub fn camera_type(&self) -> String {
        "DJI".to_owned()
    }
    pub fn has_accurate_timestamps(&self) -> bool {
        true
    }
    pub fn possible_extensions() -> Vec<&'static str> {
        vec!["mp4", "mov", "csv"]
    }
    pub fn frame_readout_time(&self) -> Option<f64> {
        self.frame_readout_time
    }
    pub fn normalize_imu_orientation(v: String) -> String {
        v
    }

    fn parse_ac203(&mut self, parsed: &dvtm_ac203::ProductMeta, info: &SampleInfo, options: &crate::InputOptions, fps: &mut f64, _sensor_fps: &mut f64, _sample_rate: &mut f64) -> Option<GroupedTagMap> {
        let mut common = DjiCommonTags::default();

        if let Some(ref clip) = parsed.clip_meta {
            if let Some(ref header) = clip.clip_meta_header {
                if !header.product_sn.is_empty() {
                    common.serial = Some(header.product_sn.clone());
                }
                if !header.product_name.is_empty() {
                    let model = normalize_model_name(&header.product_name);
                    if self.model.is_none() {
                        self.model = Some(model.clone());
                    }
                    common.model = Some(model);
                }
            }

        }

        if let Some(ref stream) = parsed.stream_meta {
            if let Some(ref meta) = stream.video_stream_meta {
                *fps = meta.framerate as f64;
                common.frame_width = Some(meta.resolution_width);
                common.frame_height = Some(meta.resolution_height);
                common.fps = Some(meta.framerate as f64);
            }
        }

        if let Some(ref frame) = parsed.frame_meta {
            if let Some(ref camera) = frame.camera_frame_meta {
                if let Some(ref iso) = camera.iso {
                    common.iso = Some(iso.iso.round() as u32);
                }
                if let Some(ref shutter) = camera.exposure_time {
                    if shutter.exposure_time.len() >= 2 && shutter.exposure_time[1] != 0 {
                        common.shutter_s = Some(shutter.exposure_time[0] as f64 / shutter.exposure_time[1] as f64);
                    }
                }
                if let Some(ref temp) = camera.white_balance_cct {
                    common.color_temp_k = Some(temp.white_balance_cct as f64);
                }
                if let Some(ref acc) = camera.accelerometer {
                    let t = info.timestamp_ms / 1000.0;
                    common.accel = Some(TimeVector3 { t, x: acc.accelerometer_x as f64, y: acc.accelerometer_y as f64, z: acc.accelerometer_z as f64 });
                }
            }

            if let Some(ref gps_frame) = frame.gps_frame_meta {
                if let Some(ref gps_basic) = gps_frame.gps_basic {
                    if let Some(ref coord) = gps_basic.gps_coordinates {
                        let (lat, lon) = normalize_gps_coords(coord.position_coord_unit, coord.latitude, coord.longitude);
                        common.gps_lat = Some(lat);
                        common.gps_lon = Some(lon);
                    }

                    let gps_alt_m = gps_basic.gps_altitude_mm as f64 / 1000.0;
                    if gps_alt_m != 0.0 {
                        common.gps_alt_m = Some(gps_alt_m);
                    }

                    let status = gps_basic.gps_status as u64;
                    common.gps_status = Some(gps_status_str(status));
                    let alt_type = gps_basic.gps_altitude_type as u64;
                    common.gps_alt_type = Some(gps_alt_type_str(alt_type));
                    common.has_gps_time = Some(gps_basic.has_gps_time);

                    let gps_dt = gps_basic.gps_time.as_ref().map(|d| d.time.clone());
                    if let Some(ref dt) = gps_dt {
                        common.gps_datetime = Some(dt.clone());
                    }

                    if let (Some(lat), Some(lon)) = (common.gps_lat, common.gps_lon) {
                        let has_time = common.has_gps_time.unwrap_or(false);
                        let unix_ts = match (gps_dt.as_ref(), has_time) {
                            (Some(_), false) => 0.0,
                            (Some(v), _) => parse_gps_datetime(v).unwrap_or(0.0),
                            (None, _) => 0.0,
                        };
                        let is_acquired = matches!(status, 0 | 2);
                        common.gps = Some(GpsData {
                            is_acquired,
                            unix_timestamp: unix_ts,
                            lat,
                            lon,
                            speed: 0.0,
                            track: 0.0,
                            altitude: common.gps_alt_m.unwrap_or(0.0),
                        });
                    }
                }
            }
        }

        let mut tag_map = GroupedTagMap::new();
        insert_common_tags(&mut tag_map, &common, options);

        if tag_map.is_empty() {
            None
        } else {
            Some(tag_map)
        }
    }

    fn parse_ac204(&mut self, parsed: &dvtm_ac204::ProductMeta, info: &SampleInfo, options: &crate::InputOptions, fps: &mut f64, _sensor_fps: &mut f64, _sample_rate: &mut f64) -> Option<GroupedTagMap> {
        let mut common = DjiCommonTags::default();

        if let Some(ref clip) = parsed.clip_meta {
            if let Some(ref header) = clip.clip_meta_header {
                if !header.product_sn.is_empty() {
                    common.serial = Some(header.product_sn.clone());
                }
                if !header.product_name.is_empty() {
                    let model = normalize_model_name(&header.product_name);
                    if self.model.is_none() {
                        self.model = Some(model.clone());
                    }
                    common.model = Some(model);
                }
            }

        }

        if let Some(ref stream) = parsed.stream_meta {
            if let Some(ref meta) = stream.video_stream_meta {
                *fps = meta.framerate as f64;
                common.frame_width = Some(meta.resolution_width);
                common.frame_height = Some(meta.resolution_height);
                common.fps = Some(meta.framerate as f64);
            }
        }

        if let Some(ref frame) = parsed.frame_meta {
            if let Some(ref camera) = frame.camera_frame_meta {
                if let Some(ref iso) = camera.iso {
                    common.iso = Some(iso.iso.round() as u32);
                }
                if let Some(ref shutter) = camera.exposure_time {
                    if shutter.exposure_time.len() >= 2 && shutter.exposure_time[1] != 0 {
                        common.shutter_s = Some(shutter.exposure_time[0] as f64 / shutter.exposure_time[1] as f64);
                    }
                }
                if let Some(ref temp) = camera.white_balance_cct {
                    common.color_temp_k = Some(temp.white_balance_cct as f64);
                }
                if let Some(ref acc) = camera.accelerometer {
                    let t = info.timestamp_ms / 1000.0;
                    common.accel = Some(TimeVector3 { t, x: acc.accelerometer_x as f64, y: acc.accelerometer_y as f64, z: acc.accelerometer_z as f64 });
                }
            }

            if let Some(ref gps_frame) = frame.gps_frame_meta {
                if let Some(ref gps_basic) = gps_frame.gps_basic {
                    if let Some(ref coord) = gps_basic.gps_coordinates {
                        let (lat, lon) = normalize_gps_coords(coord.position_coord_unit, coord.latitude, coord.longitude);
                        common.gps_lat = Some(lat);
                        common.gps_lon = Some(lon);
                    }

                    let gps_alt_m = gps_basic.gps_altitude_mm as f64 / 1000.0;
                    if gps_alt_m != 0.0 {
                        common.gps_alt_m = Some(gps_alt_m);
                    }

                    let status = gps_basic.gps_status as u64;
                    common.gps_status = Some(gps_status_str(status));
                    let alt_type = gps_basic.gps_altitude_type as u64;
                    common.gps_alt_type = Some(gps_alt_type_str(alt_type));
                    common.has_gps_time = Some(gps_basic.has_gps_time);

                    let gps_dt = gps_basic.gps_time.as_ref().map(|d| d.time.clone());
                    if let Some(ref dt) = gps_dt {
                        common.gps_datetime = Some(dt.clone());
                    }

                    if let (Some(lat), Some(lon)) = (common.gps_lat, common.gps_lon) {
                        let has_time = common.has_gps_time.unwrap_or(false);
                        let unix_ts = match (gps_dt.as_ref(), has_time) {
                            (Some(_), false) => 0.0,
                            (Some(v), _) => parse_gps_datetime(v).unwrap_or(0.0),
                            (None, _) => 0.0,
                        };
                        let is_acquired = matches!(status, 0 | 2);
                        common.gps = Some(GpsData {
                            is_acquired,
                            unix_timestamp: unix_ts,
                            lat,
                            lon,
                            speed: 0.0,
                            track: 0.0,
                            altitude: common.gps_alt_m.unwrap_or(0.0),
                        });
                    }
                }
            }
        }

        let mut tag_map = GroupedTagMap::new();
        insert_common_tags(&mut tag_map, &common, options);

        if tag_map.is_empty() {
            None
        } else {
            Some(tag_map)
        }
    }

    fn parse_ow001(&mut self, parsed: &dvtm_ow001::ProductMeta, info: &SampleInfo, options: &crate::InputOptions, fps: &mut f64, sensor_fps: &mut f64, sample_rate: &mut f64) -> Option<GroupedTagMap> {
        let mut common = DjiCommonTags::default();

        if let Some(ref clip) = parsed.clip_meta {
            if let Some(ref header) = clip.clip_meta_header {
                if !header.product_sn.is_empty() {
                    common.serial = Some(header.product_sn.clone());
                }
                if !header.product_name.is_empty() {
                    let model = normalize_model_name(&header.product_name);
                    if self.model.is_none() {
                        self.model = Some(model.clone());
                    }
                    common.model = Some(model);
                }
            }

            if let Some(v) = clip.sensor_fps.as_ref().map(|h| h.sensor_frame_rate as f64) {
                *sensor_fps = v;
            }
            if let Some(v) = clip.imu_sampling_rate.as_ref().map(|h| h.imu_sampling_rate as f64) {
                *sample_rate = v;
            }
        }

        if let Some(ref stream) = parsed.stream_meta {
            if let Some(ref meta) = stream.video_stream_meta {
                *fps = meta.framerate as f64;
                common.frame_width = Some(meta.resolution_width);
                common.frame_height = Some(meta.resolution_height);
                common.fps = Some(meta.framerate as f64);
            }
        }

        if let Some(ref frame) = parsed.frame_meta {
            if let Some(ref camera) = frame.camera_frame_meta {
                if let Some(ref iso) = camera.iso {
                    common.iso = Some(iso.iso.round() as u32);
                }
                if let Some(ref shutter) = camera.exposure_time {
                    if shutter.exposure_time.len() >= 2 && shutter.exposure_time[1] != 0 {
                        common.shutter_s = Some(shutter.exposure_time[0] as f64 / shutter.exposure_time[1] as f64);
                    }
                }
                if let Some(ref temp) = camera.white_balance_cct {
                    common.color_temp_k = Some(temp.white_balance_cct as f64);
                }
                if let Some(ref acc) = camera.accelerometer {
                    let t = info.timestamp_ms / 1000.0;
                    common.accel = Some(TimeVector3 { t, x: acc.accelerometer_x as f64, y: acc.accelerometer_y as f64, z: acc.accelerometer_z as f64 });
                }
            }

            if let Some(ref gps_frame) = frame.gps_frame_meta {
                if let Some(ref gps_basic) = gps_frame.gps_basic {
                    if let Some(ref coord) = gps_basic.gps_coordinates {
                        let (lat, lon) = normalize_gps_coords(coord.position_coord_unit, coord.latitude, coord.longitude);
                        common.gps_lat = Some(lat);
                        common.gps_lon = Some(lon);
                    }

                    let gps_alt_m = gps_basic.gps_altitude_mm as f64 / 1000.0;
                    if gps_alt_m != 0.0 {
                        common.gps_alt_m = Some(gps_alt_m);
                    }

                    let status = gps_basic.gps_status as u64;
                    common.gps_status = Some(gps_status_str(status));
                    let alt_type = gps_basic.gps_altitude_type as u64;
                    common.gps_alt_type = Some(gps_alt_type_str(alt_type));
                    common.has_gps_time = Some(gps_basic.has_gps_time);

                    let gps_dt = gps_basic.gps_time.as_ref().map(|d| d.time.clone());
                    if let Some(ref dt) = gps_dt {
                        common.gps_datetime = Some(dt.clone());
                    }

                    if let (Some(lat), Some(lon)) = (common.gps_lat, common.gps_lon) {
                        let has_time = common.has_gps_time.unwrap_or(false);
                        let unix_ts = match (gps_dt.as_ref(), has_time) {
                            (Some(_), false) => 0.0,
                            (Some(v), _) => parse_gps_datetime(v).unwrap_or(0.0),
                            (None, _) => 0.0,
                        };
                        let is_acquired = matches!(status, 0 | 2);
                        common.gps = Some(GpsData {
                            is_acquired,
                            unix_timestamp: unix_ts,
                            lat,
                            lon,
                            speed: 0.0,
                            track: 0.0,
                            altitude: common.gps_alt_m.unwrap_or(0.0),
                        });
                    }
                }
            }
        }

        let mut tag_map = GroupedTagMap::new();
        insert_common_tags(&mut tag_map, &common, options);

        if tag_map.is_empty() {
            None
        } else {
            Some(tag_map)
        }
    }

    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], _filepath: P, _options: &crate::InputOptions) -> Option<Self> {
        if memmem::find(buffer, b"djmd").is_some() && (memmem::find(buffer, b"DJI meta").is_some() || memmem::find(buffer, b"CAM meta").is_some()) {
            Some(Self {
                model: None,
                frame_readout_time: None,
            })
        } else if memmem::find(buffer, b"Clock:Tick").is_some() && memmem::find(buffer, b"IMU_ATTI(0):gyroX").is_some() {
            Some(Self {
                model: Some("CSV flight log".into()),
                frame_readout_time: None,
            })
        } else {
            None
        }
    }

    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, size: usize, progress_cb: F, cancel_flag: Arc<AtomicBool>, options: crate::InputOptions) -> Result<Vec<SampleInfo>> {
        if self.model.is_some() {
            return csv::parse(stream, size, options);
        }

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
                let head = if data.len() > 128 { &data[0..128] } else { data };
                if memmem::find(head, b"ac203").is_some() || memmem::find(head, b"ac204").is_some() {
                    which_proto = if memmem::find(head, b"ac203").is_some() {
                        DeviceProtobuf::Ac203
                    } else {
                        DeviceProtobuf::Ac204
                    };
                } else if memmem::find(head, b"OW001").is_some() || memmem::find(head, b"ow001").is_some() {
                    which_proto = DeviceProtobuf::Ow001;
                } else if memmem::find(head, b"WA530").is_some() || memmem::find(head, b"wa530").is_some() {
                    which_proto = DeviceProtobuf::Wa530;
                } else {
                    which_proto = DeviceProtobuf::Wm169;
                }
                log::debug!("Using device protobuf: {which_proto:?}");
            }

            macro_rules! handle_parsed {
                ($parsed:expr, $field:tt) => {
                    let mut tag_map = GroupedTagMap::new();

                    if let Some(ref clip) = $parsed.clip_meta {
                        self.model              = clip.clip_meta_header       .as_ref().map(|h| h.product_name.replace("DJI ", ""));
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

                            // log::debug!("Exposure time: {:?}", &exposure_time);
                        }

                        if let Some(ref imu) = frame.imu_frame_meta {
                            if let Some(ref attitude) = imu.$field {
                                // let ts = attitude.timestamp as i64;
                                // println!("{} {} {} {}, vsync: {}", frame_ts, ts, frame_relative_ts, ts - frame_ts, attitude.vsync);
                                let len = attitude.attitude.len() as f64;

                                let vsync_duration = 1000.0 / sensor_fps.max(1.0);
                                // if first_vsync == 0 {
                                //     first_vsync = attitude.vsync;
                                // }

                                let frame_timestamp = (frame_relative_ts as f64) / 1000.0;

                                // let frame_timestamp = (attitude.vsync - first_vsync) as f64 * vsync_duration;
                                // println!("fps: {fps}, sensor_fps: {sensor_fps}, ratio: {fps_ratio}, exp: {exposure_time}, ts: {frame_timestamp}, diff: {}", frame_timestamp);
                                // println!("vsync: {}, ts: {:.3}, ts2: {:.3}, diff: {:.3}", (attitude.vsync - first_vsync), frame_timestamp * 1000.0, (frame_relative_ts as f64 / ratio), (frame_relative_ts as f64 / ratio) - (frame_timestamp * 1000.0));

                                // let frame_ratio = self.frame_readout_time.unwrap() / vsync_duration;

                                // let offset_ms = (1000.0 / sample_rate) * attitude.offset as f64;

                                for (i, q) in attitude.attitude.iter().enumerate() {
                                    let index = i as f64 - attitude.offset as f64;
                                    let quat_ts = frame_timestamp + ((index / len) * vsync_duration);

                                    /*let ts = match std::env::var("OFFSET_METHOD").as_deref() {
                                        Ok("1.3.0") => {
                                            quat_ts - (exposure_time / 2.0)
                                        },
                                        Ok("no-exp") => {
                                            quat_ts
                                        },
                                        Ok("global-quat-index") => {
                                            (global_quat_i as f64 - attitude.offset as f64) * (1000.0 / sample_rate)
                                        },
                                        Ok("global-quat-index-with-readout-time") => {
                                            (global_quat_i as f64 - attitude.offset as f64) * (1000.0 / sample_rate) - (self.frame_readout_time.unwrap() / 2.0)
                                        },
                                        Ok("with-readout-time") => {
                                            quat_ts - (self.frame_readout_time.unwrap() / 2.0)
                                        },
                                        // Default, if no env var
                                        _ => {
                                            quat_ts - exposure_time
                                        }
                                    };*/

                                    let ts = quat_ts / fps_ratio;

                                    // let ts = (quat_ts1 - exposure_time) / fps_ratio;
                                    // println!("ts: {:.2}, diff: {:.4}, vsync: {}, frame_timestamp: {}, fts: {frame_timestamp}, fts2: {frame_timestamp2}", ts, ts - prev_ts, attitude.vsync, frame_ts);
                                    prev_ts = ts;

                                    // global_quat_i += 1;

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

                    // if info.index == 0 { dbg!(&parsed); }

                    info.tag_map = Some(tag_map);

                    samples.push(info);

                    if options.probe_only {
                        cancel_flag2.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                };
            }

            match which_proto {
                DeviceProtobuf::Unknown => { },
                DeviceProtobuf::Wm169 => match dvtm_wm169::ProductMeta::decode(data) {
                    Ok(parsed) => { handle_parsed!(parsed, imu_attitude_after_fusion); },
                    Err(e) => { log::warn!("Failed to parse protobuf: {:?}", e); }
                },
                DeviceProtobuf::Wa530 => match dvtm_eagle4_wa530::ProductMeta::decode(data) {
                    Ok(parsed) => { handle_parsed!(parsed, imu_single_attitude_after_fusion); },
                    Err(e) => { log::warn!("Failed to parse protobuf: {:?}", e); }
                },
                DeviceProtobuf::Ac203 => match dvtm_ac203::ProductMeta::decode(data) {
                    Ok(parsed) => {
                        if let Some(tag_map) = self.parse_ac203(&parsed, &info, &options, &mut fps, &mut sensor_fps, &mut sample_rate) {
                            info.tag_map = Some(tag_map);
                            samples.push(info);
                            if options.probe_only {
                                cancel_flag2.store(true, std::sync::atomic::Ordering::Relaxed);
                            }
                        }
                    },
                    Err(e) => { log::warn!("Failed to parse protobuf: {:?}", e); }
                },
                DeviceProtobuf::Ac204 => match dvtm_ac204::ProductMeta::decode(data) {
                    Ok(parsed) => {
                        if let Some(tag_map) = self.parse_ac204(&parsed, &info, &options, &mut fps, &mut sensor_fps, &mut sample_rate) {
                            info.tag_map = Some(tag_map);
                            samples.push(info);
                            if options.probe_only {
                                cancel_flag2.store(true, std::sync::atomic::Ordering::Relaxed);
                            }
                        }
                    },
                    Err(e) => { log::warn!("Failed to parse protobuf: {:?}", e); }
                },
                DeviceProtobuf::Ow001 => match dvtm_ow001::ProductMeta::decode(data) {
                    Ok(parsed) => {
                        if let Some(tag_map) = self.parse_ow001(&parsed, &info, &options, &mut fps, &mut sensor_fps, &mut sample_rate) {
                            info.tag_map = Some(tag_map);
                            samples.push(info);
                            if options.probe_only {
                                cancel_flag2.store(true, std::sync::atomic::Ordering::Relaxed);
                            }
                        }
                    },
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
            "calibrated_by": "DJI",
            "camera_brand": "DJI",
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
