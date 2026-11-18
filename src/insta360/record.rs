// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2021 Adrian <adrian.eddy at gmail>

use std::io::*;
use byteorder::{ReadBytesExt, LittleEndian};
use std::collections::BTreeMap;

use crate::insta360::extra_info;
use crate::tag;
use crate::tags_impl::*;
use crate::tags_impl::TagId::*;
use crate::tags_impl::GroupId::*;
use crate::util::insert_tag;

#[allow(non_snake_case, non_upper_case_globals)]
pub mod RecordType {
    pub const Offsets            : u8 = 0;
    pub const Metadata           : u8 = 1;
    pub const Thumbnail          : u8 = 2;
    pub const Gyro               : u8 = 3;
    pub const Exposure           : u8 = 4;
    pub const ThumbnailExt       : u8 = 5;
    pub const TimelapseTimestamp : u8 = 6;
    pub const Gps                : u8 = 7;
    pub const StarNum            : u8 = 8;
    pub const AAAData            : u8 = 9;
    pub const Anchors            : u8 = 10; // Highlights
    pub const AAASimulation      : u8 = 11;
    /// The secondary stream's exposure times, same layout as [`Exposure`].
    pub const ExposureTimeVin1   : u8 = 12;
    pub const Magnetic           : u8 = 13;
    pub const Euler              : u8 = 14;
    pub const SecGyro            : u8 = 15;
    pub const Speed              : u8 = 16;
    pub const TBox               : u8 = 17;
    pub const Editor             : u8 = 18;
    pub const TimelapseQuat      : u8 = 24;
    pub const Quaternion         : u8 = 25;
    pub const OnlineQuaternion   : u8 = 51;
    /// The exposures of the file's second, low-resolution stream (inferred from the name; no file has it yet).
    pub const SubStreamExposure  : u8 = 52;
    /// Per frame, a temperature the camera reads (inferred: the first one is `ExtraMetadata.sensor_module_temper`).
    pub const SensorTemperature  : u8 = 53;
    /// The exposures of the second stream's secondary sensor input (inferred from the name; no file has it yet).
    pub const SubStreamExposureTimeVin1: u8 = 54;
    pub const TimeMap            : u8 = 128;
}

#[allow(non_snake_case, non_upper_case_globals, dead_code)]
mod RecordFormat {
    pub const Binary   : u8 = 0;
    pub const Protobuf : u8 = 1;
    pub const Json     : u8 = 2;
}

impl super::Insta360 {
    pub fn parse_record(&mut self, id: u8, format: u8, _version: u32, data: &[u8], mut offsets: Option<&mut BTreeMap<u8, (u32, u32, u8)>>, options: &crate::InputOptions) -> Result<GroupedTagMap> {
        let mut map = GroupedTagMap::new();

        let mut d = Cursor::new(data);
        let len = data.len() as u64;

        match id {
            RecordType::Offsets => {
                // One ten-byte entry per record type, indexed by id — a slot
                // whose id is zero is a type this clip did not write. The
                // format is carried, because on a slotted trailer (below) the
                // table is the only place it is stated.
                while d.position() < len as u64 {
                    let id     = d.read_u8()?;
                    let format = d.read_u8()?;
                    let size   = d.read_u32::<LittleEndian>()?;
                    let offset = d.read_u32::<LittleEndian>()?;
                    if id > 0 {
                        if let Some(offsets) = offsets.as_mut() {
                            offsets.insert(id, (offset, size, format));
                        }
                    }
                }
            },
            RecordType::TimeMap => {
                insert_tag(&mut map, tag!(Default, TagId::Custom("TimeMap".into()), "TimeMap", Vec_TimeScalar_f64, |v| format!("{:?}", v), |d| {
                    let len = d.get_ref().len();
                    let mut tm = Vec::with_capacity(len as usize / (8+8));

                    let _unk1     = d.read_u32::<LittleEndian>()?;
                    let num_trims = d.read_u32::<LittleEndian>()?;
                    let _unk3     = d.read_u32::<LittleEndian>()?;
                    let _count    = d.read_u32::<LittleEndian>()?;
                    for _ in 0..num_trims {
                        let _trim_unk1 = d.read_f64::<LittleEndian>()?;
                        let _trim_unk2 = d.read_f64::<LittleEndian>()?;
                        let _trim_unk3 = d.read_f64::<LittleEndian>()?;
                        let _trim_unk4 = d.read_f64::<LittleEndian>()?;
                    }
                    let _unk5 = d.read_f64::<LittleEndian>()?;
                    while d.position() < len as u64 {
                        let v1 = d.read_f64::<LittleEndian>()?;
                        let v2 = d.read_f64::<LittleEndian>()?;
                        tm.push(TimeScalar {
                            t: v1,
                            v: v2
                        })
                    }
                    Ok(tm)
                }, data), options);
            },
            RecordType::Metadata => { // Metadata in protobuf format
                use prost::Message;
                let info = extra_info::ExtraMetadata::decode(data)?;

                self.is_raw_gyro = info.is_raw_gyro;
                if let Some(ref gyro_info) = info.gyro_cfg_info {
                    self.gyro_range = Some(gyro_info.gyro_range as f64);
                    self.acc_range  = Some(gyro_info.acc_range as f64);
                }

                // The calibration strings, kept as the parsed structures rather
                // than re-read out of the JSON below: the JSON is a flat array
                // of every token, which cannot say which lens a number belongs
                // to and is the shape the old single-lens reader mis-indexed.
                self.stream_type = info.stream_type;
                self.multi_track_order = info.pano_record_multi_track_order;
                self.pano_record_type = info.pano_record_type;
                self.recorded = crate::rig::Insta360Facts {
                    lens_types: Vec::new(),
                    bullet_time: info.file_group_info.as_ref().is_some_and(|g| g.r#type == 1),
                    offset_state: extra_info::OffsetState::try_from(info.offset_states).ok(),
                    cooling_shell_selected_state:
                        extra_info::extra_metadata::CoolingShellSelectedState::try_from(info.cooling_shell_selected_state).ok(),
                    cooling_shell_algo_state:
                        extra_info::extra_metadata::CoolingShellAlgoDetectedState::try_from(info.cooling_shell_algo_state).ok(),
                    imu_turn_ypr: None,
                };
                for text in [&info.offset, &info.offset_v2, &info.offset_v3, &info.offset_v6] {
                    if let Some(offset) = super::rig::parse(text) {
                        self.offsets.set(offset);
                    }
                }
                // A lens that focuses: its calibration follows the AF code, each frame's code is in the exposure
                // record, and the clip may have been shot through an attachment
                self.is_af_lens = info.is_af_lens;
                self.offset_v8 = super::offset_v8::parse(&info.offset_v8).or_else(|| {
                    info.offset_info.iter().filter(|o| o.offset_version == 8).find_map(|o| super::offset_v8::parse(&o.offset))
                });
                self.af_range = info.af_lens_info.as_ref().and_then(|r| super::offset_v8::af_range(r.range_near, r.range_far));
                self.lens_optics = super::offset_v8::Optics::of(info.lens_accessory_type);
                self.flowstate_online = info.is_flowstate_online;
                if let Some(mode) = info.shooting_param_info.as_ref()
                    .and_then(|s| extra_info::extra_metadata::shooting_param_info::FocusMode::try_from(s.focus_mode).ok())
                    .filter(|m| *m != extra_info::extra_metadata::shooting_param_info::FocusMode::Unknown) {
                    insert_tag(&mut map, tag!(parsed Autofocus, AutoFocusMode, "Autofocus mode", String, |v| v.to_string(), format!("{mode:?}"), vec![]), options);
                }
                self.sibling = info.file_group_info.as_ref()
                    .and_then(|g| super::rig::sibling_hint(g.index, g.total, &g.identify));
                self.dimension = info.dimension.as_ref()
                    .and_then(|d| Some((u32::try_from(d.x).ok()?, u32::try_from(d.y).ok()?)));
                self.window_crop = info.window_crop_info.as_ref().and_then(super::rig::WindowCrop::of);
                let mut v = serde_json::to_value(&info).map_err(|_| Error::new(ErrorKind::Other, "Serialize error"));
                if let Ok(vv) = &mut v {
                    if let Some(obj) = vv.as_object_mut() {
                        if let Ok(x) = extra_info::parse_gyro_calib(&info.gyro_calib)         { obj["gyro_calib"        ] = x; }
                        if let Ok(x) = extra_info::parse_gyro      (&info.gyro)               { obj["gyro"              ] = x; }
                        if let Ok(x) = extra_info::parse_offset    (&info.offset)             { obj["offset"            ] = x; }
                        if let Ok(x) = extra_info::parse_offset    (&info.offset_v2)          { obj["offset_v2"         ] = x; }
                        if let Ok(x) = extra_info::parse_offset    (&info.offset_v3)          { obj["offset_v3"         ] = x; }
                        if let Ok(x) = extra_info::parse_offset    (&info.original_offset)    { obj["original_offset"   ] = x; }
                        if let Ok(x) = extra_info::parse_offset    (&info.original_offset_v2) { obj["original_offset_v2"] = x; }
                        if let Ok(x) = extra_info::parse_offset    (&info.original_offset_v3) { obj["original_offset_v3"] = x; }
                        if let Ok(x) = extra_info::parse_offset    (&info.offset_v6)          { obj["offset_v6"         ] = x; }
                        if let Ok(x) = extra_info::parse_offset    (&info.original_offset_v6) { obj["original_offset_v6"] = x; }
                        if let Ok(x) = extra_info::parse_offset    (&info.offset_v8)          { obj["offset_v8"         ] = x; }
                        if let Ok(x) = extra_info::parse_offset    (&info.lens_accessory_offset) { obj["lens_accessory_offset"] = x; }

                        self.gyro_timestamp = if info.is_has_gyro_timestamp { Some(info.gyro_timestamp) } else { None };
                        self.first_frame_timestamp = Some(info.first_frame_timestamp as f64);
                        self.frame_readout_time = Some(info.rolling_shutter_time);
                    }
                }
                if let Ok(vv) = v {
                    insert_tag(&mut map, tag!(parsed Default, TagId::Metadata, "Extra metadata", Json, |v| serde_json::to_string(v).unwrap(), vv, data), options);
                }
            },
            RecordType::Thumbnail => { // video frame in h264
                insert_tag(&mut map, tag!(parsed Default, File("thumbnail.h264".into()), "Thumbnail", Vec_u8, |v| format!("{} bytes", v.len()), data.to_vec(), vec![]), options);
            },
            RecordType::ThumbnailExt => { // video frame in h264
                insert_tag(&mut map, tag!(parsed Default, File("thumbnail-ext.h264".into()), "ThumbnailExt", Vec_u8, |v| format!("{} bytes", v.len()), data.to_vec(), vec![]), options);
            },
            RecordType::Gyro => {
                let item_size = if self.is_raw_gyro { 8+6*2 } else { 8+6*8 };

                let mut acc_vec  = Vec::with_capacity(len as usize / item_size);
                let mut gyro_vec = Vec::with_capacity(len as usize / item_size);
                while d.position() < len as u64 {
                    let timestamp = d.read_u64::<LittleEndian>()? as f64 / 1000.0;
                    if !self.is_raw_gyro {
                        acc_vec.push(TimeVector3 {
                            t: timestamp,
                            x: d.read_f64::<LittleEndian>()?,
                            y: d.read_f64::<LittleEndian>()?,
                            z: d.read_f64::<LittleEndian>()?,
                        });
                        gyro_vec.push(TimeVector3 {
                            t: timestamp,
                            x: d.read_f64::<LittleEndian>()?,
                            y: d.read_f64::<LittleEndian>()?,
                            z: d.read_f64::<LittleEndian>()?,
                        });
                    } else {
                        acc_vec.push(TimeVector3 {
                            t: timestamp,
                            x: d.read_u16::<LittleEndian>()? as f64 - 32768.0,
                            y: d.read_u16::<LittleEndian>()? as f64 - 32768.0,
                            z: d.read_u16::<LittleEndian>()? as f64 - 32768.0,
                        });
                        gyro_vec.push(TimeVector3 {
                            t: timestamp,
                            x: d.read_u16::<LittleEndian>()? as f64 - 32768.0,
                            y: d.read_u16::<LittleEndian>()? as f64 - 32768.0,
                            z: d.read_u16::<LittleEndian>()? as f64 - 32768.0,
                        });
                    }
                }

                if self.is_raw_gyro {
                    let gyro_scale = 32768.0 / self.gyro_range.unwrap_or(2000.0); // 2000 dps
                    let accl_scale = 32768.0 / self.acc_range.unwrap_or(16.0); // ± 16g
                    insert_tag(&mut map, tag!(parsed Gyroscope,     Scale, "Gyroscope scale",     f64, |v| format!("{:?}", v), gyro_scale, vec![]), options);
                    insert_tag(&mut map, tag!(parsed Accelerometer, Scale, "Accelerometer scale", f64, |v| format!("{:?}", v), accl_scale, vec![]), options);

                    insert_tag(&mut map, tag!(parsed Gyroscope,     Unit, "Gyroscope unit",     String, |v| v.to_string(), "deg/s".into(), Vec::new()), options);
                } else {
                    insert_tag(&mut map, tag!(parsed Gyroscope,     Unit, "Gyroscope unit",     String, |v| v.to_string(), "rad/s".into(), Vec::new()), options);
                }

                insert_tag(&mut map, tag!(parsed Accelerometer, Data, "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), acc_vec, vec![]), options);
                insert_tag(&mut map, tag!(parsed Gyroscope,     Data, "Gyroscope data",     Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro_vec, vec![]), options);

                insert_tag(&mut map, tag!(parsed Accelerometer, Unit, "Accelerometer unit", String, |v| v.to_string(), "g".into(),  Vec::new()), options);
            },
            RecordType::Exposure | RecordType::ExposureTimeVin1 if self.is_af_lens => {
                // A lens that focuses states each frame's AF code after its exposure: 18-byte entries
                const ENTRY: usize = 8 + 8 + 2;
                let mut exp = Vec::with_capacity(data.len() / ENTRY);
                let mut af = Vec::with_capacity(data.len() / ENTRY);
                while d.position() + ENTRY as u64 <= len {
                    let timestamp_us = d.read_u64::<LittleEndian>()?;
                    let exposure = d.read_f64::<LittleEndian>()?;
                    let code = d.read_u16::<LittleEndian>()?;
                    let t = timestamp_us as f64 / 1000.0;
                    // Entry `i` is video frame `i`, so a frame whose exposure is no number keeps its place, with the
                    // exposure before it; its time and AF code still hold
                    let exposure = if exposure.is_finite() { exposure } else { exp.last().map_or(0.0, |e: &TimeScalar<f64>| e.v) };
                    exp.push(TimeScalar { t, v: exposure });
                    af.push(TimeScalar { t, v: f64::from(code) });
                    if id == RecordType::Exposure {
                        self.af_codes.push((timestamp_us, code));
                    }
                }
                insert_tag(&mut map, tag!(parsed Exposure,  Data, "Shutter speed", Vec_TimeScalar_f64, |v| format!("{:?}", v), exp, data.to_vec()), options);
                insert_tag(&mut map, tag!(parsed Autofocus, Data, "AF code",       Vec_TimeScalar_f64, |v| format!("{:?}", v), af,  vec![]), options);
            },
            RecordType::SensorTemperature => {
                insert_tag(&mut map, tag!(Imager, Temperature, "Sensor temperature (°C)", Vec_TimeScalar_f64, |v| format!("{:?}", v), |d| {
                    let len = d.get_ref().len() as u64;
                    let mut temp = Vec::with_capacity(len as usize / (8+4));
                    while d.position() + 8+4 <= len {
                        temp.push(TimeScalar {
                            t: d.read_u64::<LittleEndian>()? as f64 / 1000.0, // timestamp
                            v: d.read_f32::<LittleEndian>()? as f64 / 1000.0 // °C; the camera states thousandths of a degree (inferred: 29533 on a camera at room temperature)
                        })
                    }
                    Ok(temp)
                }, data), options);
            },
            RecordType::Exposure | RecordType::ExposureTimeVin1 => {
                insert_tag(&mut map, tag!(Exposure, Data, "Shutter speed", Vec_TimeScalar_f64, |v| format!("{:?}", v), |d| {
                    let len = d.get_ref().len();
                    let mut exp = Vec::with_capacity(len as usize / (8+8));
                    while d.position() < len as u64 {
                        exp.push(TimeScalar {
                            t: d.read_u64::<LittleEndian>()? as f64 / 1000.0, // timestamp
                            v: d.read_f64::<LittleEndian>()? // shutter speed
                        })
                    }
                    Ok(exp)
                }, data), options);
            },
            RecordType::TimelapseTimestamp => {
                insert_tag(&mut map, tag!(Default, TagId::Custom("Timestamps".into()), "Timelapse timestamps", Vec_f64, |v| format!("{:?}", v), |d| {
                    let len = d.get_ref().len();
                    let mut ts = Vec::with_capacity(len as usize / 8);
                    while d.position() < len as u64 {
                        ts.push(d.read_u64::<LittleEndian>()? as f64 / 1000.0); // timestamp
                    }
                    Ok(ts)
                }, data), options);
            },
            RecordType::Gps => {
                insert_tag(&mut map, tag!(GPS, Data, "GPS data", Vec_GpsData, |v| format!("{:?}", v), |d| {
                    let len = d.get_ref().len();
                    let mut gps = Vec::with_capacity(len as usize / 53); // item size: 53 bytes
                    while d.position() < len as u64 {
                        let unix_timestamp = (d.read_u64::<LittleEndian>()? as f64)
                                        + (d.read_u16::<LittleEndian>()? as f64) / 1000.0;
                        let fix      = d.read_u8()? as char; // A - Acquired / V - Void
                        let mut lat  = d.read_f64::<LittleEndian>()?;
                        let lat_dir  = d.read_u8()? as char; // N / S
                        let mut lon  = d.read_f64::<LittleEndian>()?;
                        let lon_dir  = d.read_u8()? as char; // E / W
                        let speed    = d.read_f64::<LittleEndian>()? * 3.6; // m/s to km/h
                        let track    = d.read_f64::<LittleEndian>()?;
                        let altitude = d.read_f64::<LittleEndian>()?; // Geoid undulation
                        if lat_dir == 'S' { lat = lat.abs() * -1.0; }
                        if lon_dir == 'W' { lon = lon.abs() * -1.0; }
                        gps.push(GpsData {
                            is_acquired: fix == 'A',
                            unix_timestamp,
                            lat,
                            lon,
                            speed,
                            track,
                            altitude
                        });
                    }
                    Ok(gps)
                }, data), options);
            },
            RecordType::AAAData => { // item size: 48 bytes
                insert_tag(&mut map, tag!(Default, TagId::Custom("AAAData".into()), "AAA data", Vec_TimeScalar_Json, |v| format!("{:?}", v), |d| {
                    let len = d.get_ref().len();
                    let mut aaa = Vec::with_capacity(len as usize / 48);
                    while d.position() < len as u64 {
                        let timestamp    = d.read_u32::<LittleEndian>()? as f64;
                        let ev_target    = d.read_f32::<LittleEndian>()?;
                        let exp_time     = d.read_f32::<LittleEndian>()?;
                        let data_stat    = d.read_u32::<LittleEndian>()?;
                        let luma_struct  = d.read_u32::<LittleEndian>()?;
                        for _ in 0..7 { d.read_u32::<LittleEndian>()?; } // temp_data

                        let luma_wg_grid = luma_struct & 0x7F;
                        let luma_wg_y    = (luma_struct & 0x3F80) >> 7;
                        let sum_wg_y     = (0x7C000 & luma_struct) >> 14;
                        let iso_value    = (100 * ((luma_struct & 0xFFF80000) >> 19)) >> 6;

                        // Just use JSON instead of creating a new structure
                        aaa.push(TimeScalar {
                            t: timestamp,
                            v: serde_json::json!({
                                "ev_target":    ev_target,
                                "exp_time":     exp_time,
                                "data_stat":    data_stat,

                                "luma_wg_grid": luma_wg_grid,
                                "luma_wg_y":    luma_wg_y,
                                "sum_wg_y":     sum_wg_y,
                                "iso_value":    iso_value
                            })
                        });
                    }
                    Ok(aaa)
                }, data), options);
            },
            RecordType::Anchors => {
                insert_tag(&mut map, tag!(Default, TagId::Custom("Anchors".into()), "Anchors (highlight) data", Vec_Json, |v| format!("{:?}", v), |d| {
                    let len = d.get_ref().len();
                    let mut anchors = Vec::new();
                    while d.position() < len as u64 {
                        let type_ = d.read_u8()?;
                        let count = d.read_u32::<LittleEndian>()?;
                        if count > 0 {
                            // Room for the timestamps the record's bytes can hold, not for what the count says
                            let item = if type_ != 2 && type_ != 18 { 8 } else { 16 };
                            let mut list = Vec::with_capacity((count as u64).min((len as u64 - d.position()) / item) as usize);
                            for _ in 0..count {
                                if type_ != 2 && type_ != 18 {
                                    list.push(vec![d.read_u64::<LittleEndian>()?]);
                                } else {
                                    list.push(vec![d.read_u64::<LittleEndian>()?, d.read_u64::<LittleEndian>()?]);
                                }
                            }
                            anchors.push(serde_json::json!({
                                "type": type_,
                                "timestampType": 0,
                                "timestampList": list
                            }));
                        }
                    }
                    Ok(anchors)
                }, data), options);
            },

            RecordType::StarNum | // Unknown format, item size: 11
            RecordType::AAASimulation | // Unknown format
            RecordType::Magnetic | // Unknown format
            RecordType::Euler | // Unknown format
            RecordType::SecGyro | // Unknown format
            RecordType::Speed | // Unknown format
            RecordType::TBox | // Unknown format
            RecordType::Editor | // Editing project state, not telemetry
            RecordType::TimelapseQuat | // Unknown format
            RecordType::Quaternion | // Unknown format
            RecordType::OnlineQuaternion | // Unknown format
            RecordType::SubStreamExposure | // Unknown format, no sample
            RecordType::SubStreamExposureTimeVin1 | // Unknown format, no sample
            _ => {
                log::warn!("Unknown Insta360 record: {}, size: {}, format: {}, {}", id, data.len(), format, pretty_hex::pretty_hex(&&data[0..data.len().min(256)]));
            }
        }
        Ok(map)
    }
}
