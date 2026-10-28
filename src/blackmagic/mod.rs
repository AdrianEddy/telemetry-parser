// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2022 Adrian <adrian.eddy at gmail>

use std::io::*;
use std::sync::{ Arc, atomic::AtomicBool };

use crate::tags_impl::*;
use crate::*;
use byteorder::{ ReadBytesExt, LittleEndian, BigEndian };
use memchr::memmem;

#[derive(Default)]
pub struct BlackmagicBraw {
    pub model: Option<String>,
    frame_readout_time: Option<f64>
}

impl BlackmagicBraw {
    pub fn camera_type(&self) -> String {
        if self.model.is_some() {
            "Blackmagic".to_owned()
        } else {
            "Blackmagic RAW".to_owned()
        }
    }
    pub fn has_accurate_timestamps(&self) -> bool {
        match self.model.as_deref() {
            Some("Micro Studio Camera 4K G2") => false,
            _ => true
        }
    }
    pub fn possible_extensions() -> Vec<&'static str> {
        vec!["braw"]
    }
    pub fn frame_readout_time(&self) -> Option<f64> {
        self.frame_readout_time
    }
    pub fn normalize_imu_orientation(v: String) -> String {
        v
    }

    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], _filepath: P, _options: &crate::InputOptions) -> Option<Self> {
        if memmem::find(buffer, b"Blackmagic Design").is_some() && memmem::find(buffer, b"braw_codec_bitrate").is_some() {
            Some(Self::default())
        } else {
            None
        }
    }

    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, size: usize, progress_cb: F, cancel_flag: Arc<AtomicBool>, options: crate::InputOptions) -> Result<Vec<SampleInfo>> {
        let mut gyro = Vec::new();
        let mut accl = Vec::new();
        // The frame each reading was taken during (`util::READING_FRAMES`)
        let (mut gyro_frames, mut accl_frames) = (Vec::new(), Vec::new());
        let mut container_fps = None;

        let mut map = GroupedTagMap::new();

        let mut samples = Vec::new();
        let mut frame_rate = None;

        let mut firmware_version = String::new();
        // let mut crop_factor = 1.0;
        if let Ok(meta) = self.parse_meta(stream, size) {
            if let Some(cam) = meta.get("camera_type").and_then(|x| x.as_str()) {
                self.model = Some(cam.trim_start_matches("Blackmagic ").to_string());
            }
            if let Some(fw) = meta.get("firmware_version").and_then(|x| x.as_str()) {
                firmware_version = fw.to_string();
            }
            if let Some(v) = meta.get("crop_origin").and_then(|v| v.as_array()).and_then(|x| Some((x.get(0)?.as_f64()? as f32, x.get(1)?.as_f64()? as f32))) {
                util::insert_tag(&mut map, tag!(parsed GroupId::Imager, TagId::CaptureAreaOrigin, "Capture area origin", f32x2, |v| format!("{v:?}"), v, vec![]), &options);
            }
            if let Some(v) = meta.get("sensor_area_captured").and_then(|v| v.as_array()).and_then(|x| Some((x.get(0)?.as_f64()? as f32, x.get(1)?.as_f64()? as f32))) {
                util::insert_tag(&mut map, tag!(parsed GroupId::Imager, TagId::CaptureAreaSize, "Capture area size", f32x2, |v| format!("{v:?}"), v, vec![]), &options);
            }
            match self.model.as_deref() {
                Some("Pocket Cinema Camera 6K Pro") |
                Some("Pocket Cinema Camera 6K G2") |
                Some("Pocket Cinema Camera 6K") => {
                    util::insert_tag(&mut map, tag!(parsed GroupId::Imager, TagId::PixelPitch, "Pixel pitch", u32x2, |v| format!("{v:?}"), (3759, 3759), vec![]), &options);
                    // crop_factor = 1.5;
                },
                Some("Pocket Cinema Camera 4K") => {
                    util::insert_tag(&mut map, tag!(parsed GroupId::Imager, TagId::PixelPitch, "Pixel pitch", u32x2, |v| format!("{v:?}"), (4628, 4628), vec![]), &options);
                    // crop_factor = 2.0;
                },
                Some("Micro Studio Camera 4K G2") => {
                    // TODO: this is not confirmed
                    util::insert_tag(&mut map, tag!(parsed GroupId::Imager, TagId::PixelPitch, "Pixel pitch", u32x2, |v| format!("{v:?}"), (4628, 4628), vec![]), &options);
                    // crop_factor = 2.0;
                },
                _ => { }
            }

            util::insert_tag(&mut map, tag!(parsed GroupId::Default, TagId::Metadata, "Metadata", Json, |v| serde_json::to_string(v).unwrap(), meta, vec![]), &options);
        }

        let _ = util::get_track_samples(stream, size, mp4parse::TrackType::Video, true, Some(8192), |mut info: SampleInfo, data: &[u8], file_position: u64, video_md: Option<&VideoMetadata>| {
            if size > 0 {
                progress_cb(file_position as f64 / size as f64 / 3.0);
            }
            if container_fps.is_none() { container_fps = video_md.map(|md| md.fps).filter(|fps| *fps > 0.0); }
            if let Ok(md) = Self::parse_per_frame_meta(data) {
                let mut map = GroupedTagMap::new();

                if let Some(v) = md.get("sensor_rate").and_then(|v| v.as_array()) {
                    if v.len() == 2 {
                        frame_rate = v[0].as_u64().zip(v[1].as_u64()).map(|(a, b)| a as f64 / b.max(1) as f64);
                    }
                }
                if let Some(v) = md.get("focal_length").and_then(|v| v.as_str()) {
                    let v = v.replace("mm", "");
                    if let Ok(v) = v.parse::<f32>() {
                        util::insert_tag(&mut map, tag!(parsed GroupId::Lens, TagId::FocalLength, "Focal length", f32, |v| format!("{v:.2} mm"), v, vec![]), &options);
                    }
                }

                util::insert_tag(&mut map, tag!(parsed GroupId::Default, TagId::Metadata, "Metadata", Json, |v| serde_json::to_string(v).unwrap(), md, vec![]), &options);
                info.tag_map = Some(map);
                samples.push(info);
                if options.probe_only {
                    cancel_flag.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
        }, cancel_flag.clone());

        if let Some(fr) = frame_rate {
            util::insert_tag(&mut map, tag!(parsed GroupId::Default, TagId::FrameRate, "Frame rate", f64, |v| format!("{:?}", v), fr, vec![]), &options);
            if let Some(rs) = self.frame_readout_time {
                if firmware_version == "7.9" && rs > (1000.0 / fr) {
                    self.frame_readout_time = Some(rs / 2.0); // Bug in firmware v7.9.0
                }
            }
        }
        // The readings are timed on the capture timeline, as the sensor captures the frames: frame k from k / its rate
        // (the clip's sensor rate, the container's frame rate when it states none). A reading is the frame's being
        // captured at the middle of its sample's time. Without either rate the frames aren't known
        let capture_rate = frame_rate.or(container_fps);
        let frame_of = |info: &SampleInfo| capture_rate.map(|rate| ((info.timestamp_ms + info.duration_ms.max(0.0) / 2.0) * rate / 1000.0).floor().max(0.0) as u64);
        let cancel_flag2 = cancel_flag.clone();
        util::get_metadata_track_samples(stream, size, false, |info: SampleInfo, data: &[u8], file_position: u64, _video_md: Option<&VideoMetadata>| {
            if size > 0 {
                progress_cb(((info.track_index as f64 - 1.0) + (file_position as f64 / size as f64)) / 3.0);
            }

            if data.len() >= 4+4+3*4 {
                let mut d = Cursor::new(data);
                crate::try_block!({
                    d.seek(SeekFrom::Start(8)).ok()?;
                    if &data[4..8] == b"mogy" {
                        gyro_frames.extend(frame_of(&info));
                        gyro.push(TimeVector3 { t: (info.timestamp_ms - self.frame_readout_time.unwrap_or(0.0) / 2.0) / 1000.0,
                            x: d.read_f32::<LittleEndian>().ok()? as f64,
                            y: d.read_f32::<LittleEndian>().ok()? as f64,
                            z: d.read_f32::<LittleEndian>().ok()? as f64
                        });
                    } else if &data[4..8] == b"moac" {
                        accl_frames.extend(frame_of(&info));
                        accl.push(TimeVector3 { t: (info.timestamp_ms - self.frame_readout_time.unwrap_or(0.0) / 2.0) / 1000.0,
                            x: -d.read_f32::<LittleEndian>().ok()? as f64,
                            y: -d.read_f32::<LittleEndian>().ok()? as f64,
                            z: -d.read_f32::<LittleEndian>().ok()? as f64
                        });
                    }
                });
            }
            if options.probe_only {
                cancel_flag2.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }, cancel_flag)?;


        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Data, "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), accl, vec![]), &options);
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Data, "Gyroscope data",     Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]), &options);
        if capture_rate.is_some() {
            util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Custom(util::READING_FRAMES.into()), "Frame of each reading", Vec_u64, |v| format!("{:?}", v), accl_frames, Vec::new()), &options);
            util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Custom(util::READING_FRAMES.into()), "Frame of each reading", Vec_u64, |v| format!("{:?}", v), gyro_frames, Vec::new()), &options);
        }

        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Unit, "Accelerometer unit", String, |v| v.to_string(), "m/s²".into(),  Vec::new()), &options);
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Unit, "Gyroscope unit",     String, |v| v.to_string(), "rad/s".into(), Vec::new()), &options);

        let imu_orientation = match self.model.as_deref() {
            Some("Micro Studio Camera 4K G2") if firmware_version == "8.4" => "yXZ",
            _ => "yxz"
        };
        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()), &options);
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()), &options);

        samples.insert(0, SampleInfo { tag_map: Some(map), ..Default::default() });

        Ok(samples)
    }

    pub fn parse_meta<T: Read + Seek>(&mut self, stream: &mut T, size: usize) -> Result<serde_json::Value> {
        let all = read_beginning_and_end(stream, size, 4*1024*1024)?;
        let mut offs = 0;
        let mut meta = None;
        while let Some(pos) = memchr::memmem::find(&all[offs..], b"meta") {
            if all.len() > offs+pos+12 && &all[offs+pos+8..offs+pos+12] == b"hdlr" {
                // The box starts with its size, before its type
                let start = (offs+pos).checked_sub(4).ok_or(ErrorKind::InvalidData)?;
                let size = (&all[start..]).read_u32::<BigEndian>()? as usize;
                meta = Some(all[start..].get(8..size).ok_or(ErrorKind::InvalidData)?);
                break;
            }
            offs += pos + 4;
        }

        if let Some(meta) = meta {
            let mut keys = Vec::new();
            let mut md = serde_json::Map::<String, serde_json::Value>::new();
            Self::iter_boxes(meta, false, |name, d, _| {
                if name == "keys" {
                    // After its version, flags and count
                    Self::iter_boxes(d.get(8..).ok_or(ErrorKind::UnexpectedEof)?, false, |_, d, _| {
                        if let Ok(key) = std::str::from_utf8(&d) {
                            keys.push(key.to_string());
                        }
                        Ok(())
                    })?;
                }
                if name == "ilst" {
                    Self::iter_boxes(&d, true, |_, d, i| {
                        let typ = d.get(..4).ok_or(ErrorKind::UnexpectedEof)?.read_u32::<BigEndian>()?;
                        if let Some(key) = keys.get(i).cloned() {
                            // https://developer.apple.com/library/archive/documentation/QuickTime/QTFF/Metadata/Metadata.html#//apple_ref/doc/uid/TP40000939-CH1-SW35
                            let mut d = d.get(8..).ok_or(ErrorKind::UnexpectedEof)?; // after the type and the locale
                            let v = match typ {
                                1  => serde_json::to_value(std::str::from_utf8(d).unwrap_or(&"")),
                                23 => serde_json::to_value(d.read_f32::<BigEndian>()? as f64),
                                24 => serde_json::to_value(d.read_f64::<BigEndian>()?),
                                65 => serde_json::to_value(d.read_i8()?),
                                66 => serde_json::to_value(d.read_i16::<BigEndian>()?),
                                67 => serde_json::to_value(d.read_i32::<BigEndian>()?),
                                70 |
                                71 => serde_json::to_value([d.read_f32::<BigEndian>()? as f64, d.read_f32::<BigEndian>()? as f64]),
                                74 => serde_json::to_value(d.read_i64::<BigEndian>()?),
                                75 => serde_json::to_value(d.read_u8()?),
                                76 => serde_json::to_value(d.read_u16::<BigEndian>()?),
                                77 => serde_json::to_value(d.read_u32::<BigEndian>()?),
                                78 => serde_json::to_value(d.read_u64::<BigEndian>()?),
                                _ => {
                                    log::debug!("{}({}): {}", key, typ, pretty_hex::pretty_hex(&d[..128.min(d.len().saturating_sub(1))].to_vec()));
                                    Err(serde_json::Error::io(ErrorKind::InvalidData.into()))
                                }
                            };
                            if let Ok(v) = v {
                                md.insert(key, v);
                            }
                        }
                        Ok(())
                    })?;
                }
                Ok(())
            })?;

            if let Some(sensor_area_height) = md.get("sensor_area_captured").and_then(|v| v.as_array()).and_then(|v| v.get(1)).and_then(|v| v.as_f64()) {
                if let Some(sensor_line_time) = md.get("sensor_line_time").and_then(|v| v.as_f64()) {
                    self.frame_readout_time = Some((sensor_area_height * sensor_line_time) / 1000.0);
                }
            }

            return Ok(serde_json::Value::Object(md));
        }
        Err(ErrorKind::InvalidData.into())
    }

    fn parse_per_frame_meta(data: &[u8]) -> Result<serde_json::Value> {
        if data.len() > 8 && &data[4..8] == b"bmdf" {
            let size = (&data[..8]).read_u32::<BigEndian>()? as usize;
            let meta = data.get(8..size).ok_or(ErrorKind::InvalidData)?;
            let mut md = serde_json::Map::<String, serde_json::Value>::new();
            Self::iter_boxes(meta, false, |name, mut d, _| {
                fn get_str<'a>(d: &'a [u8]) -> serde_json::Result<&'a str> {
                    Ok(std::str::from_utf8(d).map_err(|_| serde_json::Error::io(ErrorKind::InvalidData.into()))?.trim_end_matches('\0'))
                }
                let v = match name {
                    "srte" => (Some("sensor_rate"),          serde_json::to_value([d.read_u32::<BigEndian>()?, d.read_u32::<BigEndian>()?])),
                    "innd" => (Some("internal_nd"),          serde_json::to_value(d.read_f32::<BigEndian>()? as f64)),
                    "agpf" => (Some("analog_gain"),          serde_json::to_value(d.read_f32::<BigEndian>()? as f64)),
                    "expo" => (Some("exposure"),             serde_json::to_value(d.read_f32::<BigEndian>()? as f64)),
                    "isoe" => (Some("iso"),                  serde_json::to_value(d.read_u32::<BigEndian>()?)),
                    "wkel" => (Some("white_balance_kelvin"), serde_json::to_value(d.read_u32::<BigEndian>()?)),
                    "wtin" => (Some("white_balance_tint"),   serde_json::to_value(d.read_u16::<BigEndian>()?)),
                    "asct" => (Some("as_shot_kelvin"),       serde_json::to_value(d.read_u32::<BigEndian>()?)),
                    "asti" => (Some("as_shot_tint"),         serde_json::to_value(d.read_u16::<BigEndian>()?)),
                    "shtv" => (Some("shutter_value"),        serde_json::to_value(get_str(d)?)),
                    "aptr" => (Some("aperture"),             serde_json::to_value(get_str(d)?)),
                    "dsnc" => (Some("distance"),             serde_json::to_value(get_str(d)?)),
                    "fcln" => (Some("focal_length"),         serde_json::to_value(get_str(d)?)),
                    _ => {
                        // log::debug!("{name}: {}", pretty_hex::pretty_hex(&d));
                        (None, Err(serde_json::Error::io(ErrorKind::InvalidData.into())))
                    }
                };
                if let Ok(vv) = v.1 {
                    md.insert(v.0.unwrap_or(name).to_string(), vv);
                }
                Ok(())
            })?;
            return Ok(serde_json::Value::Object(md));
        }
        Err(ErrorKind::InvalidData.into())
    }

    // Calls `cb` with the name and contents of each box in `data`. The items of an array (an `ilst`) are an index from 1
    // and a `data` box, whose name and contents `cb` gets with the index from 0. A box of size 0 extends to the end of
    // `data`, and the walk stops at a box it can't step past: smaller than its header, or past the end of `data`
    fn iter_boxes<F: FnMut(&str, &[u8], usize) -> Result<()>>(data: &[u8], is_array: bool, mut cb: F) -> Result<()> {
        let mut offs = 0;
        while data.len().saturating_sub(offs) > 8 {
            let size = match (&data[offs..offs+4]).read_u32::<BigEndian>()? as usize {
                0 => data.len() - offs,
                size => size
            };
            let Some(d) = data[offs..].get(8..size) else { break };
            if is_array {
                let index = (&data[offs+4..offs+8]).read_u32::<BigEndian>()? as usize;
                let size2 = d.get(..4).map_or(0, |mut x| x.read_u32::<BigEndian>().unwrap_or_default() as usize);
                if let (Some(index), Some(d), Some(Ok(name))) = (index.checked_sub(1), d.get(8..size2), d.get(4..8).map(std::str::from_utf8)) {
                    cb(name, d, index)?;
                }
            } else {
                if let Ok(name) = std::str::from_utf8(&data[offs+4..offs+8]) {
                    cb(name, d, 0)?;
                }
            }
            offs += size;
        }
        Ok(())
    }
}
