// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2025 Adrian <adrian.eddy at gmail>

use std::io::*;
use std::sync::{ Arc, atomic::AtomicBool };

use byteorder::{ BigEndian, LittleEndian, ReadBytesExt };
use crate::tags_impl::*;
use crate::*;
use memchr::memmem;
mod cndm_tags;
use cndm_tags::get_tag;

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

    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, size: usize, progress_cb: F, cancel_flag: Arc<AtomicBool>, options: crate::InputOptions) -> Result<Vec<SampleInfo>> {
        let mut header = [0u8; 4];
        stream.read_exact(&mut header)?;
        stream.seek(SeekFrom::Start(0))?;

        let mut samples = if header == [0x06, 0x0E, 0x2B, 0x34] { // MXF header
            crate::sony::mxf::parse(stream, size, progress_cb, cancel_flag, None, &options, parse_tags)?
        } else {
            let mut samples = Vec::new();
            let cancel_flag2 = cancel_flag.clone();
            util::get_metadata_track_samples(stream, size, true, |mut info: SampleInfo, data: &[u8], file_position: u64, _video_md: Option<&VideoMetadata>| {
                if size > 0 {
                    progress_cb(file_position as f64 / size as f64);
                }

                if data.len() > 8 {
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
                                        if let Ok(map) = parse_metadata(&mut d, length as usize, &options) {
                                            info.tag_map = Some(map);
                                            samples.push(info.clone());
                                            if options.probe_only {
                                                cancel_flag2.store(true, std::sync::atomic::Ordering::Relaxed);
                                            }
                                        }
                                    }
                                    _ => {
                                        // println!("Unknown CRM data: {metadata_id}, {}", pretty_hex::pretty_hex(&data_inner));
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
                                        let _reserved             = d.read_u8()?;
                                        let _drop_frame           = d.read_u8()?;
                                        let _number_of_frames     = d.read_u16::<LittleEndian>()?;
                                        let _timecode_sample_data = d.read_u32::<LittleEndian>()?;
                                        let _user_bit             = d.read_u32::<LittleEndian>()?;
                                    }
                                    2 => { // Acquisition metadata
                                        if let Ok(map) = parse_metadata(&mut d, length as usize, &options) {
                                            info.tag_map = Some(map);
                                            samples.push(info.clone());
                                            if options.probe_only {
                                                cancel_flag2.store(true, std::sync::atomic::Ordering::Relaxed);
                                            }
                                        }
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
                }
            }, cancel_flag)?;
            samples
        };

        self.process_map(&mut samples, &options);

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
                if let Some(x) = map.get(&GroupId::Default).and_then(|m| m.get(&TagId::Name)) {
                    let v = x.value.to_string();
                    self.model = Some(v.strip_prefix("Canon ").unwrap_or(&v).to_string());
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
    let mut slice = Cursor::new(data);
    let datalen = data.len() as usize;

    while slice.position() < datalen as u64 {
        let tag = slice.read_u16::<BigEndian>()?;
        if tag == 0x060e {
            slice.seek(SeekFrom::Current(14))?;
            continue;
        }
        if tag == 0 || tag == 0xffff { break; }
        let len = slice.read_u16::<BigEndian>()? as usize;
        let pos = slice.position() as usize;
        if pos + len > datalen {
            log::warn!("Invalid tag: {:02x}, len: {}, Available: {}", tag, len, datalen - pos);
            // log::warn!("{}", crate::util::to_hex(&data[pos-4..]));
            break;
        }
        let tag_data = &data[pos..(pos + len)];
        slice.seek(SeekFrom::Current(len as i64))?;
        if tag == 0x8300 { // Container
            parse_tags(tag_data, options, map)?;
            continue;
        }
        let mut tag_info = if tag > 0xe000 { //
            get_tag(tag, tag_data)
        } else {
            sony::rtmd_tags::get_tag(tag, tag_data)
        };
        tag_info.native_id = Some(tag as u32);

        util::insert_tag(map, tag_info, options);
    }
    Ok(())
}
