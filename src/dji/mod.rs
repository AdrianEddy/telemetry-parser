// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2022 Adrian <adrian.eddy at gmail>

pub mod dvtm_wm169;
pub mod dvtm_eagle4_wa530;
pub mod dvtm_ow001;

use std::io::*;
use std::convert::TryInto;
use std::sync::{ Arc, atomic::AtomicBool };

use chrono::NaiveDateTime;
use crate::tags_impl::*;
use crate::*;
use crate::util::insert_tag;
use memchr::memmem;
use prost::Message;

mod csv;

fn read_varint(data: &[u8], pos: &mut usize) -> Option<u64> {
    let mut shift = 0;
    let mut result: u64 = 0;
    while *pos < data.len() {
        let byte = data[*pos];
        *pos += 1;
        result |= ((byte & 0x7f) as u64) << shift;
        if (byte & 0x80) == 0 {
            return Some(result);
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
    None
}

fn find_field<'a>(data: &'a [u8], target: u32) -> Option<(u8, &'a [u8])> {
    let mut pos = 0usize;
    while pos < data.len() {
        let key = read_varint(data, &mut pos)?;
        let field = (key >> 3) as u32;
        let wtype = (key & 7) as u8;
        let value = match wtype {
            0 => { // varint
                let start = pos;
                read_varint(data, &mut pos)?;
                &data[start..pos]
            },
            1 => { // 64-bit
                let start = pos;
                pos = pos.saturating_add(8);
                if pos > data.len() { return None; }
                &data[start..pos]
            },
            2 => { // length-delimited
                let len = read_varint(data, &mut pos)? as usize;
                let start = pos;
                pos = pos.saturating_add(len);
                if pos > data.len() { return None; }
                &data[start..pos]
            },
            5 => { // 32-bit
                let start = pos;
                pos = pos.saturating_add(4);
                if pos > data.len() { return None; }
                &data[start..pos]
            },
            _ => return None
        };
        if field == target {
            return Some((wtype, value));
        }
    }
    None
}

fn read_varint_from_slice(data: &[u8]) -> Option<u64> {
    let mut pos = 0usize;
    read_varint(data, &mut pos)
}

fn get_field_at_path<'a>(data: &'a [u8], path: &[u32]) -> Option<(u8, &'a [u8])> {
    let mut cur = data;
    for (i, field) in path.iter().enumerate() {
        let (wtype, value) = find_field(cur, *field)?;
        let last = i + 1 == path.len();
        if last {
            return Some((wtype, value));
        }
        if wtype != 2 {
            return None;
        }
        cur = value;
    }
    None
}

fn get_u64_at_path(data: &[u8], path: &[u32]) -> Option<u64> {
    let (wtype, value) = get_field_at_path(data, path)?;
    match wtype {
        0 => read_varint_from_slice(value),
        5 => {
            if value.len() != 4 { return None; }
            let bytes: [u8; 4] = value.try_into().ok()?;
            Some(u32::from_le_bytes(bytes) as u64)
        },
        1 => {
            if value.len() != 8 { return None; }
            let bytes: [u8; 8] = value.try_into().ok()?;
            Some(u64::from_le_bytes(bytes))
        },
        _ => None,
    }
}

fn get_f32_at_path(data: &[u8], path: &[u32]) -> Option<f32> {
    let (wtype, value) = get_field_at_path(data, path)?;
    if wtype != 5 || value.len() != 4 {
        return None;
    }
    let bytes: [u8; 4] = value.try_into().ok()?;
    Some(f32::from_le_bytes(bytes))
}

fn get_f64_at_path(data: &[u8], path: &[u32]) -> Option<f64> {
    let (wtype, value) = get_field_at_path(data, path)?;
    match wtype {
        1 => {
            if value.len() != 8 { return None; }
            let bytes: [u8; 8] = value.try_into().ok()?;
            Some(f64::from_le_bytes(bytes))
        },
        5 => {
            if value.len() != 4 { return None; }
            let bytes: [u8; 4] = value.try_into().ok()?;
            Some(f32::from_le_bytes(bytes) as f64)
        },
        0 => read_varint_from_slice(value).map(|v| v as f64),
        _ => None,
    }
}

fn get_string_at_path(data: &[u8], path: &[u32]) -> Option<String> {
    let (wtype, value) = get_field_at_path(data, path)?;
    if wtype != 2 {
        return None;
    }
    Some(String::from_utf8_lossy(value).trim_end_matches('\0').to_string())
}

fn get_altitude_m_at_path(data: &[u8], path: &[u32]) -> Option<f64> {
    let (wtype, value) = get_field_at_path(data, path)?;
    match wtype {
        0 => read_varint_from_slice(value).map(|v| (v as i64) as f64 / 1000.0), // mm -> m
        5 => {
            if value.len() != 4 { return None; }
            let bytes: [u8; 4] = value.try_into().ok()?;
            Some(f32::from_le_bytes(bytes) as f64)
        },
        1 => {
            if value.len() != 8 { return None; }
            let bytes: [u8; 8] = value.try_into().ok()?;
            Some(f64::from_le_bytes(bytes))
        },
        _ => None,
    }
}

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

fn parse_ac20x_accel(data: &[u8]) -> Option<(f32, f32, f32)> {
    let x = get_f32_at_path(data, &[3, 2, 10, 2])?;
    let y = get_f32_at_path(data, &[3, 2, 10, 3])?;
    let z = get_f32_at_path(data, &[3, 2, 10, 4])?;
    Some((x, y, z))
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
    Ac20x,
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

    fn parse_ac20x_like(&mut self, data: &[u8], info: &SampleInfo, options: &crate::InputOptions) -> Option<GroupedTagMap> {
        let mut tag_map = GroupedTagMap::new();

        if let Some(serial) = get_string_at_path(data, &[1, 1, 5]) {
            util::insert_tag(&mut tag_map, tag!(parsed GroupId::Default, TagId::SerialNumber, "Serial number", String, |v| v.to_string(), serial, vec![]), options);
        }
        if let Some(model) = get_string_at_path(data, &[1, 1, 10]) {
            let model = model.replace("DJI ", "");
            let model = model.replace('\n', " ").replace('\r', " ");
            let model = model.split_whitespace().collect::<Vec<_>>().join(" ");
            if self.model.is_none() {
                self.model = Some(model.clone());
            }
            util::insert_tag(&mut tag_map, tag!(parsed GroupId::Default, TagId::Custom("Model".into()), "Model", String, |v| v.to_string(), model, vec![]), options);
        }

        let frame_width = get_u64_at_path(data, &[2, 3, 1]).map(|v| v as u32);
        let frame_height = get_u64_at_path(data, &[2, 3, 2]).map(|v| v as u32);
        let frame_rate = get_f64_at_path(data, &[2, 3, 3]);
        if let Some(w) = frame_width {
            util::insert_tag(&mut tag_map, tag!(parsed GroupId::Default, TagId::Custom("FrameWidth".into()), "Frame width", u32, |v| format!("{:?}", v), w, vec![]), options);
        }
        if let Some(h) = frame_height {
            util::insert_tag(&mut tag_map, tag!(parsed GroupId::Default, TagId::Custom("FrameHeight".into()), "Frame height", u32, |v| format!("{:?}", v), h, vec![]), options);
        }
        if let Some(fps) = frame_rate {
            util::insert_tag(&mut tag_map, tag!(parsed GroupId::Default, TagId::FrameRate, "Frame rate", f64, |v| format!("{:.3}", v), fps, vec![]), options);
        }
        if frame_width.is_some() || frame_height.is_some() || frame_rate.is_some() {
            let mut obj = serde_json::Map::new();
            if let Some(w) = frame_width { obj.insert("width".into(), serde_json::Value::from(w)); }
            if let Some(h) = frame_height { obj.insert("height".into(), serde_json::Value::from(h)); }
            if let Some(fps) = frame_rate { obj.insert("fps".into(), serde_json::Value::from(fps)); }
            util::insert_tag(&mut tag_map, tag!(parsed GroupId::Default, TagId::Custom("FrameInfo".into()), "Frame info", Json, |v| serde_json::to_string(v).unwrap(), serde_json::Value::Object(obj), vec![]), options);
        }

        if let Some(iso) = get_u64_at_path(data, &[3, 2, 2, 1]) {
            util::insert_tag(&mut tag_map, tag!(parsed GroupId::Exposure, TagId::ISOValue, "ISO", u32, |v| v.to_string(), iso as u32, vec![]), options);
        }
        if let Some(shutter) = get_f64_at_path(data, &[3, 2, 4, 1]) {
            util::insert_tag(&mut tag_map, tag!(parsed GroupId::Exposure, TagId::ShutterSpeed, "Shutter speed", f64, |v| format!("{:.6}", v), shutter, vec![]), options);
        }
        if let Some(temp) = get_f64_at_path(data, &[3, 2, 6, 1]) {
            util::insert_tag(&mut tag_map, tag!(parsed GroupId::Colors, TagId::Custom("ColorTemperature".into()), "Color temperature", f64, |v| format!("{:.1}", v), temp, vec![]), options);
        }

        let coord_units = get_f64_at_path(data, &[3, 4, 2, 1, 1]);
        let mut gps_lat = get_f64_at_path(data, &[3, 4, 2, 1, 2]);
        let mut gps_lon = get_f64_at_path(data, &[3, 4, 2, 1, 3]);
        if let (Some(units), Some(lat), Some(lon)) = (coord_units, gps_lat, gps_lon) {
            if units.abs() > 0.0 && (lat.abs() > 180.0 || lon.abs() > 180.0) {
                let scaled_lat = if units > 1.0 { lat / units } else { lat * units };
                let scaled_lon = if units > 1.0 { lon / units } else { lon * units };
                gps_lat = Some(scaled_lat);
                gps_lon = Some(scaled_lon);
            }
        }

        if let Some(units) = coord_units {
            util::insert_tag(&mut tag_map, tag!(parsed GroupId::GPS, TagId::Custom("CoordinateUnits".into()), "Coordinate units", f64, |v| format!("{:?}", v), units, vec![]), options);
        }
        if let Some(lat) = gps_lat {
            util::insert_tag(&mut tag_map, tag!(parsed GroupId::GPS, TagId::Custom("GPSLatitude".into()), "GPS latitude", f64, |v| format!("{:.7}", v), lat, vec![]), options);
        }
        if let Some(lon) = gps_lon {
            util::insert_tag(&mut tag_map, tag!(parsed GroupId::GPS, TagId::Custom("GPSLongitude".into()), "GPS longitude", f64, |v| format!("{:.7}", v), lon, vec![]), options);
        }

        let gps_alt_m = get_altitude_m_at_path(data, &[3, 4, 2, 2]);
        if let Some(alt) = gps_alt_m {
            util::insert_tag(&mut tag_map, tag!(parsed GroupId::GPS, TagId::Custom("GPSAltitude".into()), "GPS altitude", f64, |v| format!("{:.3}", v), alt, vec![]), options);
        }

        let gps_status = get_u64_at_path(data, &[3, 4, 2, 3]);
        if let Some(status) = gps_status {
            let status_str = match status {
                0 => "GPS_NORMAL".to_string(),
                1 => "GPS_INVALID".to_string(),
                2 => "GPS_RTK".to_string(),
                _ => format!("GPS_UNKNOWN({})", status),
            };
            util::insert_tag(&mut tag_map, tag!(parsed GroupId::GPS, TagId::Custom("GPSStatus".into()), "GPS status", String, |v| v.to_string(), status_str, vec![]), options);
        }
        let gps_alt_type = get_u64_at_path(data, &[3, 4, 2, 4]);
        if let Some(alt_type) = gps_alt_type {
            let alt_type_str = match alt_type {
                0 => "PRESSURE_ALTITUDE".to_string(),
                1 => "GPS_FUSION_ALTITUDE".to_string(),
                2 => "RTK_ALTITUDE".to_string(),
                _ => format!("ALTITUDE_UNKNOWN({})", alt_type),
            };
            util::insert_tag(&mut tag_map, tag!(parsed GroupId::GPS, TagId::Custom("GPSAltitudeType".into()), "GPS altitude type", String, |v| v.to_string(), alt_type_str, vec![]), options);
        }
        let has_gps_time = get_u64_at_path(data, &[3, 4, 2, 5]).map(|v| v != 0);
        if let Some(has_time) = has_gps_time {
            util::insert_tag(&mut tag_map, tag!(parsed GroupId::GPS, TagId::Custom("HasGpsTime".into()), "Has GPS time", bool, |v| format!("{:?}", v), has_time, vec![]), options);
        }

        let gps_dt = get_string_at_path(data, &[3, 4, 2, 6, 1]);
        if let Some(dt) = gps_dt.clone() {
            util::insert_tag(&mut tag_map, tag!(parsed GroupId::GPS, TagId::Custom("GPSDateTime".into()), "GPS datetime", String, |v| v.to_string(), dt, vec![]), options);
        }

        if let (Some(lat), Some(lon)) = (gps_lat, gps_lon) {
            let unix_ts = match (gps_dt.as_ref(), has_gps_time) {
                (Some(_), Some(false)) => 0.0,
                (Some(v), _) => parse_gps_datetime(v).unwrap_or(0.0),
                (None, _) => 0.0,
            };
            let is_acquired = match gps_status {
                Some(0) | Some(2) => true,
                Some(_) => false,
                None => false,
            };
            let gps = vec![GpsData {
                is_acquired,
                unix_timestamp: unix_ts,
                lat,
                lon,
                speed: 0.0,
                track: 0.0,
                altitude: gps_alt_m.unwrap_or(0.0),
            }];
            util::insert_tag(&mut tag_map, tag!(parsed GroupId::GPS, TagId::Data, "GPS data", Vec_GpsData, |v| format!("{:?}", v), gps, vec![]), options);
        }

        if let Some((ax, ay, az)) = parse_ac20x_accel(data) {
            let t = info.timestamp_ms / 1000.0;
            let acc = vec![TimeVector3 { t, x: ax as f64, y: ay as f64, z: az as f64 }];
            util::insert_tag(&mut tag_map, tag!(parsed GroupId::Accelerometer, TagId::Data, "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), acc, vec![]), options);
            util::insert_tag(&mut tag_map, tag!(parsed GroupId::Accelerometer, TagId::Unit, "Accelerometer unit", String, |v| v.to_string(), "g".into(), Vec::new()), options);
            util::insert_tag(&mut tag_map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), "XYZ".into(), Vec::new()), options);
        }

        if tag_map.is_empty() {
            None
        } else {
            Some(tag_map)
        }
    }

    fn parse_ow001(&mut self, parsed: &dvtm_ow001::ProductMeta, info: &SampleInfo, options: &crate::InputOptions, fps: &mut f64, sensor_fps: &mut f64, sample_rate: &mut f64) -> Option<GroupedTagMap> {
        let mut tag_map = GroupedTagMap::new();

        if let Some(ref clip) = parsed.clip_meta {
            if let Some(ref header) = clip.clip_meta_header {
                if !header.product_sn.is_empty() {
                    util::insert_tag(&mut tag_map, tag!(parsed GroupId::Default, TagId::SerialNumber, "Serial number", String, |v| v.to_string(), header.product_sn.clone(), vec![]), options);
                }
                if !header.product_name.is_empty() {
                    let model = header.product_name.replace("DJI ", "").replace('\n', " ").replace('\r', " ");
                    if self.model.is_none() {
                        self.model = Some(model.clone());
                    }
                    util::insert_tag(&mut tag_map, tag!(parsed GroupId::Default, TagId::Custom("Model".into()), "Model", String, |v| v.to_string(), model, vec![]), options);
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

                let frame_width = meta.resolution_width;
                let frame_height = meta.resolution_height;
                let frame_rate = meta.framerate as f64;

                util::insert_tag(&mut tag_map, tag!(parsed GroupId::Default, TagId::Custom("FrameWidth".into()), "Frame width", u32, |v| format!("{:?}", v), frame_width, vec![]), options);
                util::insert_tag(&mut tag_map, tag!(parsed GroupId::Default, TagId::Custom("FrameHeight".into()), "Frame height", u32, |v| format!("{:?}", v), frame_height, vec![]), options);
                util::insert_tag(&mut tag_map, tag!(parsed GroupId::Default, TagId::FrameRate, "Frame rate", f64, |v| format!("{:.3}", v), frame_rate, vec![]), options);

                let mut obj = serde_json::Map::new();
                obj.insert("width".into(), serde_json::Value::from(frame_width));
                obj.insert("height".into(), serde_json::Value::from(frame_height));
                obj.insert("fps".into(), serde_json::Value::from(frame_rate));
                util::insert_tag(&mut tag_map, tag!(parsed GroupId::Default, TagId::Custom("FrameInfo".into()), "Frame info", Json, |v| serde_json::to_string(v).unwrap(), serde_json::Value::Object(obj), vec![]), options);
            }
        }

        if let Some(ref frame) = parsed.frame_meta {
            if let Some(ref camera) = frame.camera_frame_meta {
                if let Some(ref iso) = camera.iso {
                    util::insert_tag(&mut tag_map, tag!(parsed GroupId::Exposure, TagId::ISOValue, "ISO", u32, |v| v.to_string(), iso.iso.round() as u32, vec![]), options);
                }
                if let Some(ref shutter) = camera.exposure_time {
                    if shutter.exposure_time.len() >= 2 && shutter.exposure_time[1] != 0 {
                        let val = shutter.exposure_time[0] as f64 / shutter.exposure_time[1] as f64;
                        util::insert_tag(&mut tag_map, tag!(parsed GroupId::Exposure, TagId::ShutterSpeed, "Shutter speed", f64, |v| format!("{:.6}", v), val, vec![]), options);
                    }
                }
                if let Some(ref temp) = camera.white_balance_cct {
                    util::insert_tag(&mut tag_map, tag!(parsed GroupId::Colors, TagId::Custom("ColorTemperature".into()), "Color temperature", f64, |v| format!("{:.1}", v), temp.white_balance_cct as f64, vec![]), options);
                }
                if let Some(ref acc) = camera.accelerometer {
                    let t = info.timestamp_ms / 1000.0;
                    let acc_vec = vec![TimeVector3 { t, x: acc.accelerometer_x as f64, y: acc.accelerometer_y as f64, z: acc.accelerometer_z as f64 }];
                    util::insert_tag(&mut tag_map, tag!(parsed GroupId::Accelerometer, TagId::Data, "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), acc_vec, vec![]), options);
                    util::insert_tag(&mut tag_map, tag!(parsed GroupId::Accelerometer, TagId::Unit, "Accelerometer unit", String, |v| v.to_string(), "g".into(), Vec::new()), options);
                    util::insert_tag(&mut tag_map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), "XYZ".into(), Vec::new()), options);
                }
            }

            if let Some(ref gps_frame) = frame.gps_frame_meta {
                if let Some(ref gps_basic) = gps_frame.gps_basic {
                    let mut gps_lat = gps_basic.gps_coordinates.as_ref().map(|c| c.latitude);
                    let mut gps_lon = gps_basic.gps_coordinates.as_ref().map(|c| c.longitude);
                    if let Some(ref coord) = gps_basic.gps_coordinates {
                        if coord.position_coord_unit == dvtm_ow001::position_coord::PositionCoordUnit::UnitRad as i32 {
                            if let Some(lat) = gps_lat { gps_lat = Some(lat.to_degrees()); }
                            if let Some(lon) = gps_lon { gps_lon = Some(lon.to_degrees()); }
                        }
                    }

                    if let Some(lat) = gps_lat {
                        util::insert_tag(&mut tag_map, tag!(parsed GroupId::GPS, TagId::Custom("GPSLatitude".into()), "GPS latitude", f64, |v| format!("{:.7}", v), lat, vec![]), options);
                    }
                    if let Some(lon) = gps_lon {
                        util::insert_tag(&mut tag_map, tag!(parsed GroupId::GPS, TagId::Custom("GPSLongitude".into()), "GPS longitude", f64, |v| format!("{:.7}", v), lon, vec![]), options);
                    }

                    let gps_alt_m = gps_basic.gps_altitude_mm as f64 / 1000.0;
                    if gps_alt_m != 0.0 {
                        util::insert_tag(&mut tag_map, tag!(parsed GroupId::GPS, TagId::Custom("GPSAltitude".into()), "GPS altitude", f64, |v| format!("{:.3}", v), gps_alt_m, vec![]), options);
                    }

                    let status = gps_basic.gps_status as u64;
                    let status_str = match status {
                        0 => "GPS_NORMAL".to_string(),
                        1 => "GPS_INVALID".to_string(),
                        2 => "GPS_RTK".to_string(),
                        _ => format!("GPS_UNKNOWN({})", status),
                    };
                    util::insert_tag(&mut tag_map, tag!(parsed GroupId::GPS, TagId::Custom("GPSStatus".into()), "GPS status", String, |v| v.to_string(), status_str, vec![]), options);

                    let alt_type = gps_basic.gps_altitude_type as u64;
                    let alt_type_str = match alt_type {
                        0 => "PRESSURE_ALTITUDE".to_string(),
                        1 => "GPS_FUSION_ALTITUDE".to_string(),
                        2 => "RTK_ALTITUDE".to_string(),
                        _ => format!("ALTITUDE_UNKNOWN({})", alt_type),
                    };
                    util::insert_tag(&mut tag_map, tag!(parsed GroupId::GPS, TagId::Custom("GPSAltitudeType".into()), "GPS altitude type", String, |v| v.to_string(), alt_type_str, vec![]), options);

                    let has_gps_time = gps_basic.has_gps_time;
                    util::insert_tag(&mut tag_map, tag!(parsed GroupId::GPS, TagId::Custom("HasGpsTime".into()), "Has GPS time", bool, |v| format!("{:?}", v), has_gps_time, vec![]), options);

                    let gps_dt = gps_basic.gps_time.as_ref().map(|d| d.time.clone());
                    if let Some(ref dt) = gps_dt {
                        util::insert_tag(&mut tag_map, tag!(parsed GroupId::GPS, TagId::Custom("GPSDateTime".into()), "GPS datetime", String, |v| v.to_string(), dt.clone(), vec![]), options);
                    }

                    if let (Some(lat), Some(lon)) = (gps_lat, gps_lon) {
                        let unix_ts = match (gps_dt.as_ref(), has_gps_time) {
                            (Some(_), false) => 0.0,
                            (Some(v), _) => parse_gps_datetime(v).unwrap_or(0.0),
                            (None, _) => 0.0,
                        };
                        let is_acquired = matches!(status, 0 | 2);
                        let gps = vec![GpsData {
                            is_acquired,
                            unix_timestamp: unix_ts,
                            lat,
                            lon,
                            speed: 0.0,
                            track: 0.0,
                            altitude: gps_alt_m,
                        }];
                        util::insert_tag(&mut tag_map, tag!(parsed GroupId::GPS, TagId::Data, "GPS data", Vec_GpsData, |v| format!("{:?}", v), gps, vec![]), options);
                    }
                }
            }
        }

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
                    which_proto = DeviceProtobuf::Ac20x;
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
                DeviceProtobuf::Ac20x => {
                    if let Some(tag_map) = self.parse_ac20x_like(data, &info, &options) {
                        info.tag_map = Some(tag_map);
                        samples.push(info);
                        if options.probe_only {
                            cancel_flag2.store(true, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
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
