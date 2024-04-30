// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2021 Adrian <adrian.eddy at gmail>

pub mod extra_info;
pub mod record;

use std::io::*;
use std::sync::{ Arc, atomic::AtomicBool, atomic::Ordering::Relaxed };
use byteorder::{ ReadBytesExt, LittleEndian };
use std::collections::BTreeMap;

use crate::{try_block, tag, tags_impl::*};
use crate::tags_impl::{GroupId::*, TagId::*};

pub const HEADER_SIZE: usize = 32 + 4 + 4 + 32; // padding(32), size(4), version(4), magic(32)
pub const MAGIC: &[u8] = b"8db42d694ccc418790edff439fe026bf";

use crate::util::*;

#[derive(Default)]
pub struct Insta360 {
    pub model: Option<String>,
    pub is_raw_gyro: bool,
    pub acc_range: Option<f64>,
    pub gyro_range: Option<f64>,
    pub frame_readout_time: Option<f64>,
    pub first_frame_timestamp: Option<f64>,
    pub gyro_timestamp: Option<f64>,
}

impl Insta360 {
    pub fn camera_type(&self) -> String {
        "Insta360".to_owned()
    }
    pub fn has_accurate_timestamps(&self) -> bool {
        true
    }
    pub fn possible_extensions() -> Vec<&'static str> {
        vec!["mp4", "mov", "insv"]
    }
    pub fn frame_readout_time(&self) -> Option<f64> {
        self.frame_readout_time
    }
    pub fn normalize_imu_orientation(v: String) -> String {
        v
    }

    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], _filepath: P) -> Option<Self> {
        if buffer.len() > MAGIC.len() && &buffer[buffer.len()-MAGIC.len()..] == MAGIC {
            return Some(Insta360::default());
        }
        None
    }

    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, size: usize, progress_cb: F, cancel_flag: Arc<AtomicBool>) -> Result<Vec<SampleInfo>> {
        let mut tag_map = self.parse_file(stream, size, progress_cb, cancel_flag)?;
        self.process_map(&mut tag_map);
        Ok(vec![SampleInfo { tag_map: Some(tag_map), ..std::default::Default::default() }])
    }

    fn parse_file<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, size: usize, progress_cb: F, cancel_flag: Arc<AtomicBool>) -> Result<GroupedTagMap> {
        let mut buf = vec![0u8; HEADER_SIZE];
        stream.seek(SeekFrom::End(-(HEADER_SIZE as i64)))?;
        stream.read_exact(&mut buf)?;
        let mut offsets = BTreeMap::new();
        if &buf[HEADER_SIZE-32..] == MAGIC {
            let mut map = GroupedTagMap::new();

            let extra_size = (&buf[32..]).read_u32::<LittleEndian>()? as i64;
            let version    = (&buf[36..]).read_u32::<LittleEndian>()?;
            let extra_start = size - extra_size as usize;

            let mut offset = (HEADER_SIZE + 4+1+1) as i64;

            stream.seek(SeekFrom::End(-offset + 1))?;
            let first_id = stream.read_u8()?;
            if first_id == record::RecordType::Offsets {
                let size = stream.read_u32::<LittleEndian>()? as i64;
                buf.resize(size as usize, 0);
                stream.seek(SeekFrom::End(-offset - size))?;
                stream.read_exact(&mut buf)?;
                self.parse_record(first_id, 0, version, &buf, Some(&mut offsets))?;

                if !offsets.is_empty() {
                    for (id, (offset, record_size)) in &offsets {
                        if cancel_flag.load(Relaxed) { break; }
                        if size > 0 {
                            progress_cb(stream.stream_position()? as f64 / size as f64);
                        }

                        stream.seek(SeekFrom::Start(extra_start as u64 + *offset as u64))?;
                        buf.resize(*record_size as usize, 0);
                        stream.read_exact(&mut buf)?;

                        let format = stream.read_u8()?;
                        let id2    = stream.read_u8()?;
                        let size2 = stream.read_u32::<LittleEndian>()?;
                        if size2 == *record_size && *id == id2 && id2 > 0 {
                            for (g, v) in self.parse_record(id2, format, version, &buf, None)? {
                                map.entry(g).or_insert_with(TagMap::new).extend(v);
                            }
                        }
                    }
                    return Ok(map);
                }
            }

            while offset < extra_size {
                stream.seek(SeekFrom::End(-offset))?;

                if cancel_flag.load(Relaxed) { break; }
                if size > 0 {
                    progress_cb(stream.stream_position()? as f64 / size as f64);
                }

                let format = stream.read_u8()?;
                let id     = stream.read_u8()?;
                let size   = stream.read_u32::<LittleEndian>()? as i64;

                buf.resize(size as usize, 0);

                stream.seek(SeekFrom::End(-offset - size))?;
                stream.read_exact(&mut buf)?;

                for (g, v) in self.parse_record(id, format, version, &buf, None)? {
                    let group_map = map.entry(g).or_insert_with(TagMap::new);
                    group_map.extend(v);
                }

                offset += size + 4+1+1;
            }
            return Ok(map);
        }
        Err(ErrorKind::NotFound.into())
    }

    fn process_map(&mut self, tag_map: &mut GroupedTagMap) {
        if let Some(x) = tag_map.get(&GroupId::Default) {
            self.model = try_block!(String, {
                (x.get_t(TagId::Metadata) as Option<&serde_json::Value>)?.as_object()?.get("camera_type")?.as_str()?.to_owned()
            });
        }

        let has_offset_v3 = crate::try_block!(bool, {
            (tag_map.get(&GroupId::Default)?.get_t(TagId::Metadata) as Option<&serde_json::Value>)?.as_object()?.get("offset_v3")?.as_array()?.len() >= 20
        }).unwrap_or_default();
        log::debug!("Has offset_v3: {has_offset_v3}");

        let imu_orientation = if has_offset_v3 {
            match self.model.as_deref() {
                Some("Insta360 GO 2")  => "XYZ",
                Some("Insta360 GO 3")  => "XYZ",
                Some("Insta360 GO 3S") => "yXZ",
                Some("Insta360 OneR")  => "Xyz",
                Some("Insta360 OneRS") => "Xyz",
                _                      => "Xyz"
            }
        } else {
            match self.model.as_deref() {
                Some("Insta360 Go")    => "xyZ",
                Some("Insta360 GO 2")  => "yXZ",
                Some("Insta360 OneR")  => "yXZ",
                Some("Insta360 OneRS") => "yxz",
                Some("Insta360 ONE X2")=> "xZy",
                _                      => "yXZ"
            }
        };

        if let Some(x) = tag_map.get_mut(&GroupId::Gyroscope) {
            x.insert(Orientation, tag!(parsed Gyroscope,     Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.to_string(), Vec::new()));
        }
        if let Some(x) = tag_map.get_mut(&GroupId::Accelerometer) {
            x.insert(Orientation, tag!(parsed Accelerometer, Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.to_string(), Vec::new()));
        }

        crate::try_block!({
            let md = (tag_map.get(&GroupId::Default)?.get_t(TagId::Metadata) as Option<&serde_json::Value>)?.as_object()?;
            match (md.get("dimension").and_then(|x| x.as_object()), md.get("window_crop_info").and_then(|x| x.as_object()), md.get("offset_v3").and_then(|x| x.as_array())) {
                (Some(dim), Some(crop_info), Some(offset_v3)) if offset_v3.len() >= 20 => {
                    let (w, h) = (dim.get("x")?.as_i64()? as u32, dim.get("y")?.as_i64()? as u32);
                    let sw = crop_info.get("src_width") ?.as_i64()? as u32;
                    let sh = crop_info.get("src_height")?.as_i64()? as u32;
                    let dw = crop_info.get("dst_width") ?.as_i64()? as u32;
                    let dh = crop_info.get("dst_height")?.as_i64()? as u32;

                    self.insert_lens_profile(tag_map, (w, h), (sw, sh), (dw, dh), &offset_v3.into_iter().filter_map(|x| x.as_f64()).collect::<Vec<f64>>());
                },
                _ => { }
            }
        });

        {
            let fft = self.first_frame_timestamp.unwrap_or_default() / 1000.0;
            let gyro_timestamp = self.gyro_timestamp.unwrap_or_default() / 1000.0;
            let mut update_timestamps = |group: &GroupId| {
                if let Some(g) = tag_map.get_mut(group) {
                    if let Some(g) = g.get_mut(&TagId::Data) {
                        match &mut g.value {
                            // Gyro/accel
                            TagValue::Vec_TimeVector3_f64(g) => {
                                for x in g.get_mut() {
                                    x.t -= fft;
                                    if self.is_raw_gyro {
                                        x.t /= 1000.0;
                                    }
                                    x.t -= gyro_timestamp;
                                }
                            },
                            // Exposure
                            TagValue::Vec_TimeScalar_f64(g) => {
                                let _ = g.get(); // make sure it's parsed
                                for x in g.get_mut() {
                                    x.t -= fft;
                                    if self.is_raw_gyro {
                                        x.t /= 1000.0;
                                    }
                                }
                            },
                            _ => { }
                        }
                    }
                }
            };
            update_timestamps(&GroupId::Gyroscope);
            update_timestamps(&GroupId::Accelerometer);
            update_timestamps(&GroupId::Exposure);
        }
    }

    fn insert_lens_profile(&self, tag_map: &mut GroupedTagMap, size: (u32, u32), _src: (u32, u32), dst: (u32, u32), offset_v3: &[f64]) {
        let model = self.model.clone().unwrap_or_default().replace("Insta360 ", "");

        // offset_v3: num_xi_fx_fy_cx_cy_yaw_pitch_roll_tx_ty_tz_k1_k2_k3_p1_p2_width_height_lensType_flag

        let (_num, xi, fx, fy, cx, cy, yaw, pitch, roll, _tx, _ty, _tz, k1, k2, k3, p1, p2, lens_width, lens_height, _lens_type, _flag) =
            (offset_v3[0], offset_v3[1], offset_v3[2], offset_v3[3], offset_v3[4], offset_v3[5], offset_v3[6], offset_v3[7],
            offset_v3[8], offset_v3[9], offset_v3[10], offset_v3[11], offset_v3[12], offset_v3[13], offset_v3[14], offset_v3[15],
            offset_v3[16], offset_v3[17], offset_v3[18], offset_v3[19], offset_v3[20]);

        let c_ratio = (
            size.0 as f64 / lens_width,
            size.1 as f64 / lens_height
        );
        let f_ratio = (
            dst.0 as f64 / size.0 as f64,
            dst.1 as f64 / size.1 as f64
        );

        let output_size = Self::get_output_size(size.0, size.1);

        let profile = serde_json::json!({
            "calibrated_by": "Insta360",
            "camera_brand": "Insta360",
            "camera_model": model,
            "calib_dimension": { "w": size.0, "h": size.1 },
            "orig_dimension":  { "w": size.0, "h": size.1 },
            "output_dimension": { "w": output_size.0, "h": output_size.1 },
            "frame_readout_time": self.frame_readout_time,
            "official": true,
            "asymmetrical": true,
            "fisheye_params": {
              "camera_matrix": [
                [ fx / f_ratio.0,   0.0,              cx * c_ratio.0 ],
                [ 0.0,              fy / f_ratio.1,   cy * c_ratio.1 ],
                [ 0.0,              0.0,              1.0 ]
              ],
              "distortion_coeffs": [k1, k2, k3, p1, p2, xi]
            },
            "distortion_model": "insta360",
            "sync_settings": {
              "initial_offset": 0,
              "initial_offset_inv": false,
              "search_size": 0.3,
              "max_sync_points": 5,
              "every_nth_frame": 1,
              "time_per_syncpoint": 0.5,
              "do_autosync": false
            },
            "calibrator_version": "---"
        });

        insert_tag(tag_map, tag!(parsed GroupId::Lens, TagId::Data, "Lens profile", Json, |v| serde_json::to_string(v).unwrap(), profile, vec![]));

        if pitch.abs() > 0.0 || roll.abs() > 0.0 || yaw.abs() > 0.0 {
            const DEG2RAD: f64 = std::f64::consts::PI / 180.0;
            let yaw = yaw * DEG2RAD;
            let pitch = pitch * DEG2RAD;
            let roll = roll * DEG2RAD;
            let (sr, cr) = (yaw.sin(), yaw.cos());
            let (sp, cp) = (pitch.sin(), pitch.cos());
            let (sy, cy) = (roll.sin(), roll.cos());
            let mat = [
                [cy * cp, cy * sp * sr - sy * cr, cy * sp * cr + sy * sr],
                [sy * cp, sy * sp * sr + cy * cr, sy * sp * cr - cy * sr],
                [-sp,     cp * sr,                cp * cr],
            ];
            let rotate = |vec: &mut TimeVector3<f64>| {
                let mut rotated = [0.0f64; 3];
                for i in 0..3 {
                    rotated[i] += mat[i][0] * vec.x;
                    rotated[i] += mat[i][1] * vec.y;
                    rotated[i] += mat[i][2] * vec.z;
                }
                vec.x = rotated[0];
                vec.y = rotated[1];
                vec.z = rotated[2];
            };

            for group in [GroupId::Gyroscope, GroupId::Accelerometer] {
                if let Some(x) = tag_map.get_mut(&group) {
                    if let Some(xx) = x.get_mut(&TagId::Data) {
                        if let TagValue::Vec_TimeVector3_f64(arr) = &mut xx.value {
                            for v in arr.get_mut().iter_mut() {
                                rotate(v);
                            }
                        }
                    }
                }
            }
        }
    }

    fn get_output_size(width: u32, height: u32) -> (u32, u32) {
        let aspect = (width as f64 / height as f64 * 100.0) as u32;
        match aspect {
            133 => (width, (width as f64 / 1.7777777777777).round() as u32), // 4:3 -> 16:9
            100 => (width, (width as f64 / 1.7777777777777).round() as u32), // 1:1 -> 16:9
            _   => (width, height)
        }
    }
}
