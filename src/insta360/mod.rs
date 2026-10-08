// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2021 Adrian <adrian.eddy at gmail>

pub mod extra_info;
pub mod record;
pub(crate) mod rig;
mod rig_body_masks;
mod rig_mask_coefficients;
pub use rig_body_masks::Insta360MaskProfile;

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
    /// Every generation of the `offset` calibration string the clip carries.
    offsets: rig::Offsets,
    /// `ExtraMetadata.stream_type` — which video track holds which lens.
    stream_type: i32,
    bullet_time: bool,
    /// The other file of a two-file body, when the clip declares a pair.
    sibling: Option<crate::rig::SiblingHint>,
    /// The coded size of one video track.
    dimension: Option<(u32, u32)>,
    /// The vendor's window crop, `(src, dst)`.
    window_crop: Option<((u32, u32), (u32, u32))>,
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

    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], _filepath: P, _options: &crate::InputOptions) -> Option<Self> {
        if buffer.len() > MAGIC.len() && &buffer[buffer.len()-MAGIC.len()..] == MAGIC {
            return Some(Insta360::default());
        }
        None
    }

    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, size: usize, progress_cb: F, cancel_flag: Arc<AtomicBool>, options: crate::InputOptions) -> Result<Vec<SampleInfo>> {
        let mut tag_map = self.parse_file(stream, size, progress_cb, cancel_flag, &options)?;
        self.process_map(&mut tag_map, &options);
        Ok(vec![SampleInfo { tag_map: Some(tag_map), ..std::default::Default::default() }])
    }

    fn parse_file<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, size: usize, progress_cb: F, cancel_flag: Arc<AtomicBool>, options: &crate::InputOptions) -> Result<GroupedTagMap> {
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
                self.parse_record(first_id, 0, version, &buf, Some(&mut offsets), options)?;

                if !offsets.is_empty() {
                    // The table is the index, and the ONLY index. Two layouts
                    // carry one:
                    //
                    //  * an X4 / X5 packs its records back to back and also
                    //    closes each with the same six-byte descriptor the
                    //    table-less layout below walks, so the descriptor
                    //    after a record restates the table's entry;
                    //  * an Antigravity A1 places each record at the start of
                    //    a 256 KiB slot and writes NO descriptors at all —
                    //    the bytes after a record are the next slot's, and a
                    //    reader that insists on a descriptor there rejects
                    //    every record and sees an empty file.
                    //
                    // So a table entry is trusted on its own terms — it names
                    // where the record is, how long it is and what format it
                    // is in — and checked only against the trailer it lives
                    // in. The descriptor, where one exists, says nothing the
                    // table did not.
                    for (id, (offset, record_size, format)) in &offsets {
                        if cancel_flag.load(Relaxed) { break; }
                        if size > 0 {
                            progress_cb(stream.stream_position()? as f64 / size as f64);
                        }
                        let end = *offset as u64 + *record_size as u64;
                        if end > extra_size as u64 {
                            log::warn!("Insta360 record {id} runs past the trailer ({end} of {extra_size} bytes); skipped");
                            continue;
                        }

                        stream.seek(SeekFrom::Start(extra_start as u64 + *offset as u64))?;
                        buf.resize(*record_size as usize, 0);
                        stream.read_exact(&mut buf)?;

                        for (g, v) in self.parse_record(*id, *format, version, &buf, None, options)? {
                            map.entry(g).or_insert_with(TagMap::new).extend(v);
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

                for (g, v) in self.parse_record(id, format, version, &buf, None, options)? {
                    let group_map = map.entry(g).or_insert_with(TagMap::new);
                    group_map.extend(v);
                }

                offset += size + 4+1+1;
            }
            return Ok(map);
        }
        Err(ErrorKind::NotFound.into())
    }

    fn process_map(&mut self, tag_map: &mut GroupedTagMap, options: &crate::InputOptions) {
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
                Some("Insta360 GO Ultra") => "YxZ",
                Some("Insta360 OneR")  => "Xyz",
                Some("Insta360 OneRS") => "Xyz",
                Some("Insta360 X4")    => "yzX",
                Some("Insta360 X5")    => "yzX",
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

        if let Some(dimension) = self.dimension {
            if let Some((_src, dst)) = self.window_crop {
                self.insert_lens_profile(tag_map, dimension, dst, options);
            }
            // The rig goes at its own tag and never at `Lens/Data`: that one is
            // the single-lens JSON profile above, tags are last-wins per (group,
            // id), and a rig-shaped object there would deserialise into an empty
            // lens profile downstream and then suppress the lens-database lookup.
            if let Some(camera_rig) = rig::camera_rig(
                self.model.as_deref().unwrap_or_default(),
                &self.offsets,
                rig::StreamLayout::from_stream_type(self.stream_type),
                self.sibling.clone(),
                dimension,
                self.bullet_time,
                crate::rig::readout(self.frame_readout_time),
            ) {
                insert_tag(tag_map, tag!(parsed GroupId::Lens, TagId::Rig, "Camera rig", CameraRig, |v: &crate::rig::CameraRig| serde_json::to_string(v).unwrap_or_default(), camera_rig, vec![]), options);
            }
        }

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

    /// The single-lens JSON profile a lens database deserialises, built from
    /// lens 0 of the `offset_v3` string.
    ///
    /// **Not** from the newest string the clip carries: the profile names the
    /// `insta360` distortion model, which is the v3 Brown block, and a v6 or
    /// v2 coefficient list under that name is a different lens.
    fn insert_lens_profile(&self, tag_map: &mut GroupedTagMap, size: (u32, u32), dst: (u32, u32), options: &crate::InputOptions) {
        let model = self.model.clone().unwrap_or_default().replace("Insta360 ", "");
        let Some(offset) = self.offsets.v3.as_ref() else { return };
        let Some(lens) = offset.lenses.first() else { return };
        let (Some(&[k1, k2, k3]), Some(&[p1, p2])) = (lens.coeffs.get(..3), lens.coeffs.get(3..5)) else { return };
        let (xi, [fx, fy], [cx, cy]) = (lens.xi, lens.focal, lens.centre);

        let c_ratio = (
            size.0 as f64 / (lens.width / offset.lenses.len() as f64),
            size.1 as f64 / lens.height
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

        insert_tag(tag_map, tag!(parsed GroupId::Lens, TagId::Data, "Lens profile", Json, |v| serde_json::to_string(v).unwrap(), profile, vec![]), options);
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

