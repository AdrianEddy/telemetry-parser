// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2021-2023 Adrian <adrian.eddy at gmail>

mod rtmd_tags;
pub mod mxf;

#[cfg(feature="sony-xml")]
pub mod xml_metadata;

use std::io::*;
use std::sync::{ Arc, atomic::AtomicBool };

use byteorder::{ ReadBytesExt, BigEndian };
use rtmd_tags::*;
use crate::tags_impl::*;
use crate::*;
use memchr::memmem;

#[derive(Default)]
pub struct Sony {
    pub model: Option<String>,
    frame_readout_time: Option<f64>,
}
impl Sony {
    pub fn camera_type(&self) -> String {
        "Sony".to_owned()
    }
    pub fn has_accurate_timestamps(&self) -> bool {
        true
    }
    pub fn possible_extensions() -> Vec<&'static str> {
        vec!["mp4", "mov", "mxf"]
    }
    pub fn frame_readout_time(&self) -> Option<f64> {
        self.frame_readout_time
    }
    pub fn normalize_imu_orientation(v: String) -> String {
        fn invert_case(x: char) -> char {
            if x.is_ascii_lowercase() { x.to_ascii_uppercase() } else { x.to_ascii_lowercase() }
        }
        assert!(v.len() == 3);
        let mut v = v.chars().collect::<Vec<char>>();

        // Normalize to common orientation - swap X/Y and invert Z
        v.swap(0, 1);
        v[2] = invert_case(v[2]);

        v.iter().collect()
    }

    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], _filepath: P) -> Option<Self> {
        if let Some(p1) = memmem::find(buffer, b"manufacturer=\"Sony\"") {
            return Some(Self {
                model: util::find_between(&buffer[p1..(p1+1024).min(buffer.len())], b"modelName=\"", b'"'),
                frame_readout_time: None
            });
        }
        None
    }

    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, size: usize, progress_cb: F, cancel_flag: Arc<AtomicBool>) -> Result<Vec<SampleInfo>> {
        let mut header = [0u8; 4];
        stream.read_exact(&mut header)?;
        stream.seek(SeekFrom::Start(0))?;

        let mut samples = if header == [0x06, 0x0E, 0x2B, 0x34] { // MXF header
            mxf::parse(stream, size, progress_cb, cancel_flag, None)?
        } else {
            let mut samples = Vec::new();
            util::get_metadata_track_samples(stream, size, true, |mut info: SampleInfo, data: &[u8], file_position: u64| {
                if size > 0 {
                    progress_cb(file_position as f64 / size as f64);
                }
                if Self::detect_metadata(data) {
                    if let Ok(map) = Self::parse_metadata(&data[0x1C..]) {
                        info.tag_map = Some(map);
                        samples.push(info);
                    }
                }
            }, cancel_flag)?;
            samples
        };

        self.process_map(&mut samples);

        Ok(samples)
    }

    fn process_map(&mut self, samples: &mut Vec<SampleInfo>) {
        for sample in samples.iter_mut() {
            if let Some(ref mut map) = sample.tag_map {
                if map.contains_key(&GroupId::Accelerometer) {
                    util::insert_tag(map, tag!(parsed GroupId::Accelerometer, TagId::Unit, "Accelerometer unit", String, |v| v.to_string(), "g".into(), Vec::new()));
                }

                if let Some(imager) = map.get(&GroupId::Imager) {
                    if let Some(v) = imager.get_t(TagId::FrameReadoutTime) as Option<&f64> {
                        self.frame_readout_time = Some(*v);
                    }
                }
                if let Some(cooke) = map.get_mut(&GroupId::Cooke) {
                    let mut cooke_data: Vec<u8> = Vec::new();
                    if let Some(v) = cooke.get(&TagId::Unknown(0xe208)) { if let TagValue::Unknown(x) = &v.value { cooke_data.extend(&x.raw_data); } }
                    if let Some(v) = cooke.get(&TagId::Unknown(0xe209)) { if let TagValue::Unknown(x) = &v.value { cooke_data.extend(&x.raw_data); } }
                    if !cooke_data.is_empty() {
                        cooke.remove(&TagId::Unknown(0xe208));
                        cooke.remove(&TagId::Unknown(0xe209));
                        cooke.insert(TagId::Data2, tag!(GroupId::Cooke, TagId::Data2, "BinaryMetadata2", Json, "{:?}", |d| {
                            Ok(serde_json::Value::Array(crate::cooke::bin::parse(d.get_ref()).unwrap())) // TODO: unwrap
                        }, cooke_data));
                    }
                }
            }
        }
    }

    fn detect_metadata(data: &[u8]) -> bool {
        data.len() > 0x1C && data[0..2] == [0x00, 0x1C]
    }

    fn parse_metadata(data: &[u8]) -> Result<GroupedTagMap> {
        let mut slice = Cursor::new(data);
        let datalen = data.len() as usize;
        let mut map = GroupedTagMap::new();

        while slice.position() < datalen as u64 {
            let tag = slice.read_u16::<BigEndian>()?;
            if tag == 0x060e {
                /*let uuid = &data[slice.position() as usize - 2..slice.position() as usize + 14];
                log::debug!("--- {} ---", match &uuid[..16] {
                    &hex_literal::hex!("060E2B34 02530101 0C020101 01010000") => "LensUnitMetadata",
                    &hex_literal::hex!("060E2B34 02530101 0C020101 02010000") => "CameraUnitMetadata",
                    &hex_literal::hex!("060E2B34 02530101 0C020101 7F010000") => "UserDefinedAcquisitionMetadata",
                    _ => "Unknown"
                });*/
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
                // Since there's a lot of containers, this code cen be made more efficient by taking the TagMap by parameter, instead of creating new one for each container
                // Benchmarking will be a good idea
                for (g, v) in Self::parse_metadata(tag_data)? {
                    let group_map = map.entry(g).or_insert_with(TagMap::new);
                    group_map.extend(v);
                }
                continue;
            }
            let mut tag_info = get_tag(tag, tag_data);
            tag_info.native_id = Some(tag as u32);

            util::insert_tag(&mut map, tag_info);
        }
        Ok(map)
    }
}
