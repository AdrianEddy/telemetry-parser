// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2021 Adrian <adrian.eddy at gmail>

use std::collections::BTreeMap;
use std::io::*;
use std::sync::{ Arc, atomic::AtomicBool };
use byteorder::{ ReadBytesExt, BigEndian };

use crate::*;
use crate::tags_impl::*;

pub fn parse<T: Read + Seek, F: Fn(f64)>(stream: &mut T, size: usize, progress_cb: F, cancel_flag: Arc<AtomicBool>, metadata_only: Option<&mut util::VideoMetadata>, options: &crate::InputOptions) -> Result<Vec<SampleInfo>> {
    let mut stream = std::io::BufReader::with_capacity(128*1024, stream);
    let mut samples = Vec::new();

    let mut frame_rate = 25.0;

    let mut index = 0;
    let mut id = [0u8; 16];
    let mut max_duration = None;
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

        let length = read_ber(&mut stream)?;

        if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) { break; }
        if size > 0 {
            progress_cb(stream.stream_position()? as f64 / size as f64);
        }

        // log::debug!("{}: {}", util::to_hex(&id), length);

        if id == [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x53, 0x01, 0x01, 0x0D, 0x01, 0x01, 0x01, 0x01, 0x01, 0x11, 0x00] { // SourceClip
            let mut data = vec![0; length];
            stream.read_exact(&mut data)?;
            if let Ok(data) = parse_set(&data) {
                if let Some(duration) = data.get(&MxfMetaTag::ContainerDuration).or_else(|| data.get(&MxfMetaTag::Duration)).and_then(|x| x.as_u64()) {
                    if max_duration.is_none_or(|x| duration > x) {
                        max_duration = Some(duration);
                    }
                }
            }
        }
        if id == [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x53, 0x01, 0x01, 0x0D, 0x01, 0x01, 0x01, 0x01, 0x01, 0x28, 0x00] || // CDCIDescriptor
           id == [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x53, 0x01, 0x01, 0x0d, 0x01, 0x01, 0x01, 0x01, 0x01, 0x51, 0x00] || // MPEGPictureEssenceDescriptor
           id == [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x53, 0x01, 0x01, 0x0d, 0x01, 0x01, 0x01, 0x01, 0x01, 0x5f, 0x00] || // VC1VideoDescriptor
           id == [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x53, 0x01, 0x01, 0x0d, 0x01, 0x01, 0x01, 0x01, 0x01, 0x27, 0x00] || // GenericPictureEssenceDescriptor
           id == [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x53, 0x01, 0x01, 0x0D, 0x01, 0x01, 0x01, 0x01, 0x01, 0x29, 0x00] { // RGBAPictureEssenceDescriptor
            let mut data = vec![0; length];
            stream.read_exact(&mut data)?;
            if let Ok(data) = parse_set(&data) {
                if let Some(v) = data.get(&MxfMetaTag::SampleRate).and_then(|x| x.as_f64()) {
                    frame_rate = v;
                }
                if let Some(md) = metadata_only {
                    *md = util::VideoMetadata {
                        duration_s: data.get(&MxfMetaTag::ContainerDuration).or_else(|| data.get(&MxfMetaTag::Duration)).and_then(|x| x.as_u64()).unwrap_or(max_duration.unwrap_or_default()) as f64 / frame_rate,
                        fps: frame_rate,
                        width: data.get(&MxfMetaTag::DisplayWidth).and_then(|x| x.as_u64()).unwrap_or_default() as usize,
                        height: data.get(&MxfMetaTag::DisplayHeight).and_then(|x| x.as_u64()).unwrap_or_default() as usize,
                        rotation: 0,
                    };
                    return Ok(Vec::new());
                }
            }
        }

        if id == [0x06, 0x0e, 0x2b, 0x34, 0x01, 0x02, 0x01, 0x01, 0x0d, 0x01, 0x03, 0x01, 0x17, 0x01, 0x02, 0x01] { // Metadata, Ancillary, SMPTE ST 436
            let mut data = vec![0; length];
            stream.read_exact(&mut data)?;
            let data = parse_ancillary(&data)?;

            if let Ok(map) = super::Sony::parse_metadata(&data, options) {
                if let Some(group) = map.get(&GroupId::Default) {
                    if let Some(val) = group.get(&TagId::FrameRate) {
                        match &val.value {
                            TagValue::f32(vv) => frame_rate = *vv.get() as f64,
                            TagValue::f64(vv) => frame_rate = *vv.get(),
                            _ => {}
                        }
                    }
                }
                let duration_ms = 1000.0 / frame_rate;

                // log::debug!("Index: {}, Duration: {}, Frame rate: {}, Timestamp: {}", index, duration_ms, frame_rate, index as f64 * duration_ms);

                samples.push(SampleInfo {
                    sample_index: index,
                    duration_ms,
                    timestamp_ms: index as f64 * duration_ms,
                    tag_map: Some(map),
                    ..Default::default()
                });
                index += 1;

                if options.probe_only {
                    cancel_flag.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
        } else {
            stream.seek(SeekFrom::Current(length as i64))?;
        }
    }

    Ok(samples)
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

fn parse_ancillary(buffer: &[u8]) -> Result<Vec<u8>> {
    let mut slice = Cursor::new(&buffer);

    let count = slice.read_u16::<BigEndian>()?; // number of lines
    assert!(count as usize * 14 <= buffer.len());

    let mut full_data = Vec::with_capacity(buffer.len());
    for _ in 0..count {
        let _line_number = slice.read_u16::<BigEndian>()?;
        let _wrapping_type = slice.read_u8()?;
        let _payload_sample_coding = slice.read_u8()?;
        let sample_count = slice.read_u16::<BigEndian>()?;
        let array_count = slice.read_u32::<BigEndian>()?;
        let array_length = slice.read_u32::<BigEndian>()?;

        let pos = slice.position() as usize;

        let array_size = (array_count * array_length) as usize;

        let parsing_size = (sample_count as usize)
            .min(buffer.len() - pos)
            .min(array_size);

        let array_data = &buffer[pos..pos+parsing_size];
        if array_data[0] == 0x43 && array_data[1] == 0x05 {
            // let size = array_data[2] as usize;
            // let idx = array_data[3];
            let payload = &array_data[4..];

            full_data.extend_from_slice(&payload);
        }
        slice.seek(SeekFrom::Current(array_size as i64))?;
    }
    Ok(full_data)
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Debug)]
enum MxfMetaTag {
    Duration,
    RoundedTimecodeBase,
    StartTimecode,
    DropFrame,
    SampleRate,
    ContainerDuration,
    StoredHeight,
    StoredWidth,
    SampledHeight,
    SampledWidth,
    SampledXOffset,
    SampledYOffset,
    DisplayHeight,
    DisplayWidth,
    DisplayXOffset,
    DisplayYOffset,
    AspectRatio,
    ColorRange
}

fn parse_set(buffer: &[u8]) -> Result<BTreeMap<MxfMetaTag, serde_json::Value>> {
    let mut slice = Cursor::new(&buffer);
    let mut map = BTreeMap::<MxfMetaTag, serde_json::Value>::new();

    while slice.position() < buffer.len() as u64 {
        let tag = slice.read_u16::<BigEndian>()?;
        let length = slice.read_u16::<BigEndian>()?;

        match tag {
            0x0202 => { map.insert(MxfMetaTag::Duration,            slice.read_u64::<BigEndian>()?.into()); },
            0x1502 => { map.insert(MxfMetaTag::RoundedTimecodeBase, slice.read_u16::<BigEndian>()?.into()); },
            0x1501 => { map.insert(MxfMetaTag::StartTimecode,       slice.read_u64::<BigEndian>()?.into()); },
            0x1503 => { map.insert(MxfMetaTag::DropFrame,           slice.read_u8()?.into()); },
            0x3001 => { map.insert(MxfMetaTag::SampleRate,          (slice.read_u32::<BigEndian>()? as f64 / slice.read_u32::<BigEndian>()? as f64).into()); },
            0x3002 => { map.insert(MxfMetaTag::ContainerDuration,   slice.read_u64::<BigEndian>()?.into()); },
            0x3202 => { map.insert(MxfMetaTag::StoredHeight,        slice.read_u32::<BigEndian>()?.into()); },
            0x3203 => { map.insert(MxfMetaTag::StoredWidth,         slice.read_u32::<BigEndian>()?.into()); },
            0x3204 => { map.insert(MxfMetaTag::SampledHeight,       slice.read_u32::<BigEndian>()?.into()); },
            0x3205 => { map.insert(MxfMetaTag::SampledWidth,        slice.read_u32::<BigEndian>()?.into()); },
            0x3206 => { map.insert(MxfMetaTag::SampledXOffset,      slice.read_u32::<BigEndian>()?.into()); },
            0x3207 => { map.insert(MxfMetaTag::SampledYOffset,      slice.read_u32::<BigEndian>()?.into()); },
            0x3208 => { map.insert(MxfMetaTag::DisplayHeight,       slice.read_u32::<BigEndian>()?.into()); },
            0x3209 => { map.insert(MxfMetaTag::DisplayWidth,        slice.read_u32::<BigEndian>()?.into()); },
            0x320A => { map.insert(MxfMetaTag::DisplayXOffset,      slice.read_u32::<BigEndian>()?.into()); },
            0x320B => { map.insert(MxfMetaTag::DisplayYOffset,      slice.read_u32::<BigEndian>()?.into()); },
            0x320E => { map.insert(MxfMetaTag::AspectRatio,         (slice.read_u32::<BigEndian>()? as f64 / slice.read_u32::<BigEndian>()? as f64).into()); },
            0x3306 => { map.insert(MxfMetaTag::ColorRange,          slice.read_u32::<BigEndian>()?.into()); },
            _ => {
                slice.seek(SeekFrom::Current(length as i64))?;
            }
        }
    }
    Ok(map)
}
