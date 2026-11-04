// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2025 Adrian <adrian.eddy at gmail>

use std::io::*;
use std::sync::{ Arc, atomic::AtomicBool };

use byteorder::{ BigEndian, LittleEndian, ReadBytesExt };
use crate::tags_impl::*;
use crate::*;
use memchr::memmem;
mod clip_boxes;
mod cndm_tags;
mod crm_packs;
mod mxf_packs;
use cndm_tags::{ FRAME_METADATA, get_tag };

#[derive(Default)]
pub struct Canon {
    pub model: Option<String>,
    pub lens: Option<String>,
    is_crm: bool,
    frame_readout_time: Option<f64>,
}
impl Canon {
    pub fn camera_type(&self) -> String {
        "Canon".to_owned()
    }
    pub fn has_accurate_timestamps(&self) -> bool {
        true
    }
    pub fn possible_extensions() -> Vec<&'static str> {
        vec!["mp4", "mov", "mxf", "crm"]
    }
    pub fn frame_readout_time(&self) -> Option<f64> {
        self.frame_readout_time
    }
    pub fn normalize_imu_orientation(v: String) -> String {
        v
    }

    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], _filepath: P, _options: &crate::InputOptions) -> Option<Self> {
        if memmem::find(buffer, b"Canon EOS").is_some() {
            return Some(Self {
                model: None,
                lens: None,
                is_crm: memmem::find(buffer, b"ftypcrx").is_some(),
                frame_readout_time: None
            });
        }
        None
    }

    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, size: u64, progress_cb: F, cancel_flag: Arc<AtomicBool>, options: crate::InputOptions) -> Result<Vec<SampleInfo>> {
        let mut header = [0u8; 4];
        stream.read_exact(&mut header)?;
        stream.seek(SeekFrom::Start(0))?;

        let is_mxf = header == [0x06, 0x0E, 0x2B, 0x34];
        let mut samples = if is_mxf {
            crate::sony::mxf::parse_with_klvs(stream, size, progress_cb, cancel_flag, None, &options, parse_tags, &mxf_packs::KEYS, mxf_packs::parse_klv)?
        } else {
            let mut samples = Vec::new();
            let cancel_flag2 = cancel_flag.clone();
            util::get_metadata_track_samples(stream, size, true, |mut info: SampleInfo, data: &[u8], file_position: u64, _video_md: Option<&VideoMetadata>| {
                if size > 0 {
                    progress_cb(file_position as f64 / size as f64);
                }

                if data.len() > 8 {
                    // What the frame states apart from its acquisition metadata, which replaces what they both state
                    let mut map = GroupedTagMap::new();
                    let mut acquisition = None;
                    if let Err(e) = || -> Result<()> {
                        let mut slice = Cursor::new(&data);
                        if self.is_crm {
                            while let Ok(length) = slice.read_u32::<LittleEndian>() {
                                let length = (length - 8) as usize;
                                let metadata_id = slice.read_u32::<LittleEndian>()?;
                                if slice.position() as usize + length > data.len() {
                                    log::error!("Invalid CRM data!. Length: {length}, data len: {}, position: {}", data.len(), slice.position());
                                    break;
                                }
                                let data_inner = &data[slice.position() as usize..slice.position() as usize + length];
                                slice.seek_relative(length as _)?;
                                let mut d = Cursor::new(&data_inner);
                                match metadata_id {
                                    0x0000000D => { // AcquisitionMetadataPack
                                        let _version  = d.read_u16::<LittleEndian>()?;
                                        let _reserved = d.read_u16::<LittleEndian>()?;
                                        acquisition = parse_metadata(&mut d, length as usize, &options).ok();
                                    }
                                    id => {
                                        // println!("Unknown CRM data: {metadata_id}, {}", pretty_hex::pretty_hex(&data_inner));
                                        if let Err(e) = crm_packs::parse_pack(id, data_inner, &options, &mut map) {
                                            log::warn!("Failed to parse the CRM pack {id:08x}: {e:?}");
                                        }
                                    }
                                }
                            }
                        } else {
                            while let Ok(id) = slice.read_u32::<LittleEndian>() {
                                let length = slice.read_u32::<LittleEndian>()? as usize;
                                if slice.position() as usize + length > data.len() {
                                    log::error!("Invalid cndm data!. Length: {length}, data len: {}, position: {}", data.len(), slice.position());
                                    break;
                                }
                                let data_inner = &data[slice.position() as usize..slice.position() as usize + length];
                                slice.seek_relative(length as _)?;
                                let mut d = Cursor::new(&data_inner);
                                match id {
                                    1 => { // Timecode
                                        let _reserved  = d.read_u8()?;
                                        let drop_frame = d.read_u8()? != 0;
                                        let rate       = d.read_u16::<LittleEndian>()?; // Hundredths of frames per second: 2997
                                        let counter    = d.read_u32::<LittleEndian>()?;
                                        let user_bits  = d.read_u32::<LittleEndian>()?.to_le_bytes();
                                        if let Some(v) = timecode(counter, rate, drop_frame) {
                                            util::insert_tag(&mut map, tag!(parsed FRAME_METADATA, TagId::Custom("Timecode".into()), "Timecode", String, |v| v.to_string(), v, Vec::new()), &options);
                                        }
                                        // Each byte has the binary group of the higher number first
                                        let user_bits = user_bits.iter().map(|x| format!("{:02x}", x.rotate_left(4))).collect::<String>();
                                        util::insert_tag(&mut map, tag!(parsed FRAME_METADATA, TagId::Custom("UserBits".into()), "User Bits (Binary Groups 1 to 8)", String, |v| v.to_string(), user_bits, Vec::new()), &options);
                                    }
                                    2 => { // Acquisition metadata
                                        acquisition = parse_metadata(&mut d, length as usize, &options).ok();
                                    }
                                    _ => {
                                        log::warn!("Unknown cndm data: {id}, {}", pretty_hex::pretty_hex(&data_inner));
                                    }
                                }
                            }
                        }
                        Ok(())
                    }() {
                        log::warn!("Failed to parse Canon metadata: {e:?}");
                    }

                    // Cameras from before the acquisition metadata have only the packs of a CRM
                    if acquisition.is_some() || !map.is_empty() {
                        for (group, tags) in acquisition.unwrap_or_default() {
                            map.entry(group).or_default().extend(tags);
                        }
                        info.tag_map = Some(map);
                        samples.push(info.clone());
                        if options.probe_only {
                            cancel_flag2.store(true, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                }
            }, cancel_flag)?;
            samples
        };

        self.process_map(&mut samples, &options);

        if !is_mxf {
            match clip_boxes::parse(stream, size) {
                Ok(Some(metadata)) => {
                    // Its frames state it only on some cameras (E2.28): not on the R5 Mark II or the R5 C, nor in a proxy,
                    // and a stills camera's movie has no metadata in its frames
                    if self.model.is_none() {
                        self.model = metadata["Model"].as_str().map(model_name);
                    }
                    if let Some(map) = samples.first_mut().and_then(|x| x.tag_map.as_mut()) {
                        util::insert_tag(map, tag!(parsed clip_boxes::CLIP_METADATA, TagId::Metadata, "Clip metadata", Json, |v| v.to_string(), metadata, Vec::new()), &options);
                    }
                }
                Ok(None) => { }
                Err(e) => log::warn!("Failed to parse the Canon boxes: {e:?}"),
            }
        }

        Ok(samples)
    }

    fn process_map(&mut self, samples: &mut Vec<SampleInfo>, options: &crate::InputOptions) {
        let imu_orientation = "yxZ";
        for sample in samples.iter_mut() {
            if let Some(ref mut map) = sample.tag_map {
                if map.contains_key(&GroupId::Accelerometer) {
                    util::insert_tag(map, tag!(parsed GroupId::Accelerometer, TagId::Unit, "Accelerometer unit", String, |v| v.to_string(), "g".into(), Vec::new()), options);
                    util::insert_tag(map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()), &options);
                }
                if map.contains_key(&GroupId::Gyroscope) {
                    util::insert_tag(map, tag!(parsed GroupId::Gyroscope,     TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()), &options);
                }
                if map.contains_key(&GroupId::Magnetometer) {
                    util::insert_tag(map, tag!(parsed GroupId::Magnetometer, TagId::Unit, "Magnetometer unit", String, |v| v.to_string(), "T".into(), Vec::new()), options);
                    util::insert_tag(map, tag!(parsed GroupId::Magnetometer, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()), &options);
                }
                if let Some(x) = map.get(&GroupId::Default).and_then(|m| m.get(&TagId::Name)) {
                    self.model = Some(model_name(&x.value.to_string()));
                }

                if let Some(imager) = map.get_mut(&GroupId::Imager) {
                    if let Some(v) = imager.get_t(TagId::FrameReadoutTime) as Option<&f64> {
                        self.frame_readout_time = Some(*v);
                    }
                }
            }
        }
    }
}

/// A model as the camera names it, without the brand that `camera_type` states
fn model_name(model: &str) -> String {
    model.strip_prefix("Canon ").unwrap_or(model).to_string()
}

pub fn parse_metadata<T: Read + Seek>(stream: &mut T, _size: usize, options: &crate::InputOptions) -> Result<GroupedTagMap> {
    let mut map = GroupedTagMap::new();
    let mut id = [0u8; 16];
    while let Ok(_) = stream.read_exact(&mut id) {
        if &id[0..4] != &[0x06, 0x0e, 0x2b, 0x34] {
            log::warn!("Unknown ID {} at 0x{:08x}", util::to_hex(&id), stream.stream_position()? - 16);
            while let Ok(byte) = stream.read_u8() {
                if byte == 0x06 {
                    let mut id2 = [0u8; 3];
                    stream.read_exact(&mut id2)?;
                    if id2 == [0x0e, 0x2b, 0x34] {
                        stream.seek(SeekFrom::Current(-4))?;
                        break;
                    }
                    stream.seek(SeekFrom::Current(-3))?;
                }
            }
            continue;
        }

        let length = read_ber(stream)?;

        // println!("{}: {}", util::to_hex(&id), length);

        if id == hex_literal::hex!("060E2B34 02530101 0C020101 01010000") || // Lens Unit Metadata
           id == hex_literal::hex!("060E2B34 02530101 0C020101 02010000") || // Camera Unit Metadata
           id == hex_literal::hex!("060E2B34 02530101 0C020101 7F010000") || // User Defined Acquisition Metadata
           id == hex_literal::hex!("060E2B34 0401010D 0E150004 01000000") || // Canon Lens Metadata
           id == hex_literal::hex!("060E2B34 0401010D 0E150004 02000000") || // Canon Camera Metadata
           id == hex_literal::hex!("060E2B34 0401010D 0E150004 04000000") {  // Cooke /i Lens Metadata
            let mut data = vec![0; length];
            stream.read_exact(&mut data)?;

            parse_tags(&data, options, &mut map)?;
        } else {
            log::warn!("Unknown id: {}, length: {}", util::to_hex(&id), length);
            stream.seek(SeekFrom::Current(length as i64))?;
        }
    }
    Ok(map)
}

/// The timecode of a frame counter (SMPTE ST 12-1), which wraps after 24 hours. `rate` is in hundredths of frames per second, and
/// a drop-frame timecode skips the first frame numbers of every minute but each tenth
fn timecode(counter: u32, rate: u16, drop_frame: bool) -> Option<String> {
    let base = (rate as u64 + 50) / 100;
    if base == 0 { return None; }

    let mut frames = counter as u64;
    let skipped = if drop_frame { base / 15 } else { 0 };
    if skipped > 0 {
        let per_ten_minutes = base * 600 - skipped * 9;
        let per_minute = base * 60 - skipped;
        let (tens, rest) = (frames / per_ten_minutes, frames % per_ten_minutes);
        frames += skipped * 9 * tens + if rest > skipped { skipped * ((rest - skipped) / per_minute) } else { 0 };
    }
    let seconds = frames / base;
    let separator = if skipped > 0 { ';' } else { ':' };
    Some(format!("{:02}:{:02}:{:02}{separator}{:02}", seconds / 3600 % 24, seconds / 60 % 60, seconds % 60, frames % base))
}

fn read_ber<T: Read + Seek>(stream: &mut T) -> Result<usize> {
    let mut size = stream.read_u8()? as usize;

    if size & 0x80 != 0 {
        let bytes = size & 0x7f;
        assert!(bytes <= 8);
        size = 0;
        for _ in 0..bytes {
            size = size << 8 | (stream.read_u8()? as usize);
        }
    }
    Ok(size)
}


pub fn parse_tags(data: &[u8], options: &crate::InputOptions, map: &mut GroupedTagMap) -> Result<()> {
    // The ends of the containers being read, the innermost last: a stack of its own, as a file can nest them as deep as
    // its bytes allow
    let mut ends = vec![data.len()];
    let mut pos = 0;
    let mut stored = None; // E2.1E, ahead of the items it counts
    let mut tilt_roll = (None, None);

    while let Some(&datalen) = ends.last() {
        if pos >= datalen {
            pos = datalen;
            ends.pop();
            continue;
        }
        let mut slice = Cursor::new(&data[pos..datalen]);
        let tag = slice.read_u16::<BigEndian>()?;
        if tag == 0x060e {
            pos += 16;
            continue;
        }
        if tag == 0 || tag == 0xffff {
            pos = datalen;
            continue;
        }
        let len = slice.read_u16::<BigEndian>()? as usize;
        pos += 4;
        if pos + len > datalen {
            log::warn!("Invalid tag: {:02x}, len: {}, Available: {}", tag, len, datalen - pos);
            // log::warn!("{}", crate::util::to_hex(&data[pos-4..]));
            pos = datalen;
            continue;
        }
        if tag == 0x8300 { // Container
            ends.push(pos + len);
            continue;
        }
        let tag_data = &data[pos..(pos + len)];
        pos += len;
        let tag_data = match tag {
            0xe21e => { stored = Some(tag_data); tag_data }
            0xe21f => cndm_tags::imu_readings(tag_data, stored.and_then(|x| x.first().copied())),
            0xe220 => cndm_tags::imu_readings(tag_data, stored.and_then(|x| x.get(1).copied())),
            0xe221 => cndm_tags::imu_readings(tag_data, stored.and_then(|x| x.get(2).copied())),
            0xe21c | 0xe21d => {
                // 8000h is 0 degrees, a tenth of a degree the LSB, FFFFh no information
                let angle = <[u8; 2]>::try_from(tag_data).ok().map(u16::from_be_bytes).filter(|x| *x != 0xffff).map(|x| (x as i32 - 0x8000) as f32 / 10.0);
                if tag == 0xe21c { tilt_roll.0 = angle; } else { tilt_roll.1 = angle; }
                continue;
            }
            0xe227 => {
                if let [summer_time, behind_utc, hour, minute, ..] = *tag_data {
                    if let Some(zone) = cndm_tags::time_zone_tag(summer_time != 0, behind_utc != 0, hour, (minute != 0xff).then_some(minute)) {
                        util::insert_tag(map, zone, options);
                    }
                }
                tag_data
            }
            _ => tag_data
        };
        let mut tag_info = if tag > 0xe000 { //
            get_tag(tag, tag_data)
        } else {
            sony::rtmd_tags::get_tag(tag, tag_data)
        };
        tag_info.native_id = Some(tag as u32);

        util::insert_tag(map, tag_info, options);
    }
    if let (Some(tilt), Some(roll)) = tilt_roll {
        util::insert_tag(map, tag!(parsed GroupId::CameraOrientation, TagId::TiltRoll, "Camera tilt and roll", f32x2, |v| format!("tilt {:.1}°, roll {:.1}°", v.0, v.1), (tilt, roll), Vec::new()), options);
    }
    Ok(())
}
