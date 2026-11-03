// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2021 Adrian <adrian.eddy at gmail>

use std::collections::BTreeMap;
use std::io::*;
use std::sync::{ Arc, atomic::{ AtomicBool, Ordering } };
use byteorder::{ ReadBytesExt, BigEndian };

use crate::*;
use crate::tags_impl::*;
use crate::mxf::{ next_klv, next_key };

/// Metadata, Ancillary, SMPTE ST 436
const ANCILLARY: [u8; 16] = [0x06, 0x0e, 0x2b, 0x34, 0x01, 0x02, 0x01, 0x01, 0x0d, 0x01, 0x03, 0x01, 0x17, 0x01, 0x02, 0x01];
/// The longest an ST 436 ANC element's value can be: a count of 16 bits, and that many lines (ST 436 Table 7), each 14
/// bytes and a payload array of at most 348 bytes: an ST 291 packet of 255 user data words, and its DID, SDID, data count
/// and checksum, in the coding that needs the most, 3 10-bit samples in 4 bytes (as FFmpeg's
/// `AV_SMPTE_436M_CODED_ANC_PAYLOAD_CAPACITY`)
const ANCILLARY_MAX: u64 = 2 + 65535 * (14 + 348);
const BUFFER_SIZE: usize = 128 * 1024;

pub fn parse<T: Read + Seek, F: Fn(f64), P>(stream: &mut T, size: u64, progress_cb: F, cancel_flag: Arc<AtomicBool>, metadata_only: Option<&mut util::VideoMetadata>, options: &crate::InputOptions, parse_fn: P) -> Result<Vec<SampleInfo>>
where P: Fn(&[u8], &crate::InputOptions, &mut GroupedTagMap) -> Result<()> {
    let start = stream.stream_position()?;
    let size = if size > 0 { size } else { stream.seek(SeekFrom::End(0))? };

    // The video's descriptor is an item of the file's header metadata, and nothing else is read for its metadata
    let descriptor = picture_descriptor(stream, size, &cancel_flag);
    let sample_rate = descriptor.as_ref().and_then(|data| data.get(&MxfMetaTag::SampleRate)).and_then(|x| x.as_f64());
    if let Some(md) = metadata_only {
        if let Some(data) = descriptor {
            // The frames the video presents, for its duration: its material package's picture track's
            // (`segment_table::mxf_video_frames`: of the copies of the header metadata, the footer partition's first, as
            // FFmpeg and bmx read it)
            let video = crate::segment_table::mxf_video_frames(stream, size);
            *md = util::VideoMetadata {
                duration_s: video.map_or(0.0, |(frames, rate)| frames as f64 * rate.1 as f64 / rate.0 as f64),
                fps: sample_rate.unwrap_or(25.0),
                width: data.get(&MxfMetaTag::DisplayWidth).and_then(|x| x.as_u64()).unwrap_or_default() as usize,
                height: data.get(&MxfMetaTag::DisplayHeight).and_then(|x| x.as_u64()).unwrap_or_default() as usize,
                rotation: 0,
            };
        }
        return Ok(Vec::new());
    }

    stream.seek(SeekFrom::Start(start))?;
    let mut stream = BufReader::with_capacity(BUFFER_SIZE, stream);
    let mut samples = Vec::new();

    let mut frame_rate = sample_rate.unwrap_or(25.0);

    let mut index = 0;
    let progress = |position: u64| progress_cb(position as f64 / size as f64);
    // The essence is anywhere in the file's body, so all of the file is walked
    while let Some((id, length)) = next_klv(&mut stream, size, &cancel_flag, &progress)? {
        // log::debug!("{}: {}", util::to_hex(&id), length);

        if id != ANCILLARY {
            // Each KLV's value is read or stepped over once, so the next key is where this one ends
            stream.seek_relative(length as i64)?;
            continue;
        }

        // The element is frame-wrapped, one per frame: a frame whose metadata can't be read still counts, so the samples
        // keep the numbers of their frames
        let frame = index;
        index += 1;
        if length > ANCILLARY_MAX {
            // A length no element can have isn't stepped over, which would lose the frames it runs over
            log::warn!("An ST 436 element of {length} bytes at 0x{:08x}", stream.stream_position()?);
            next_key(&mut stream, size, &cancel_flag, &progress)?;
            continue;
        }
        let mut data = vec![0; length as usize];
        if stream.read_exact(&mut data).is_err() { break; }
        let data = parse_ancillary(&data);

        let mut map = GroupedTagMap::new();
        if parse_fn(&data, options, &mut map).is_ok() {
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

            // log::debug!("Index: {}, Duration: {}, Frame rate: {}, Timestamp: {}", frame, duration_ms, frame_rate, frame as f64 * duration_ms);

            samples.push(SampleInfo {
                sample_index: frame,
                duration_ms,
                timestamp_ms: frame as f64 * duration_ms,
                tag_map: Some(map),
                ..Default::default()
            });

            if options.probe_only {
                cancel_flag.store(true, Ordering::Relaxed);
            }
        }
    }

    Ok(samples)
}

/// The first picture essence descriptor of an MXF file's header metadata that can be read, of its copies in the order
/// they're read (`segment_table::mxf_header_metadata`). Only the header metadata is read, each copy's within its bounds,
/// and damage in it is passed by the next key, so a damaged file costs no more than a whole one's header metadata
fn picture_descriptor<T: Read + Seek>(stream: &mut T, size: u64, cancel: &AtomicBool) -> Option<BTreeMap<MxfMetaTag, serde_json::Value>> {
    let copies = crate::segment_table::mxf_header_metadata(stream, size);
    let mut stream = BufReader::with_capacity(BUFFER_SIZE, stream);
    for copy in copies {
        stream.seek(SeekFrom::Start(copy.start)).ok()?;
        while let Some((id, length)) = next_klv(&mut stream, copy.end, cancel, &|_| ()).ok()? {
            let value = stream.stream_position().ok()?;
            if crate::segment_table::mxf_is_picture_descriptor(&id) && let Ok(data) = parse_set(&mut stream, length) {
                return Some(data);
            }
            let next = (value + length) as i64 - stream.stream_position().ok()? as i64;
            stream.seek_relative(next).ok()?;
        }
    }
    None
}

/// The RDD 18 acquisition metadata (ST 291 DID 0x43, SDID 0x05) of an ST 436 ANC element: a count of lines, each a header
/// of 14 bytes and a payload array of bytes (ST 436 Table 7), together. The lines are read up to one that isn't whole in
/// the element or whose array isn't of bytes, the end of what can be told of it (as FFmpeg's
/// `av_smpte_436m_anc_iter_next`; bmx refuses all the element)
fn parse_ancillary(buffer: &[u8]) -> Vec<u8> {
    let mut full_data = Vec::with_capacity(buffer.len());
    let Some((count, mut lines)) = buffer.split_first_chunk::<2>() else {
        log::warn!("An ST 436 element of {} bytes", buffer.len());
        return full_data;
    };
    for _ in 0..u16::from_be_bytes(*count) {
        let line = lines.split_first_chunk::<14>().and_then(|(header, rest)| {
            // let line_number = u16::from_be_bytes([header[0], header[1]]);
            // let wrapping_type = header[2];
            // let payload_sample_coding = header[3];
            let sample_count = u16::from_be_bytes([header[4], header[5]]) as usize;
            let array_count = u32::from_be_bytes([header[6], header[7], header[8], header[9]]) as usize;
            let array_length = u32::from_be_bytes([header[10], header[11], header[12], header[13]]);
            if array_count > 0 && array_length != 1 { return None; }
            let (array, rest) = rest.split_at_checked(array_count)?;
            Some((&array[..sample_count.min(array_count)], rest))
        });
        let Some((array_data, rest)) = line else {
            log::warn!("An ST 436 element of {} bytes has a line it can't hold", buffer.len());
            break;
        };
        lines = rest;
        // let size = array_data[2] as usize;
        // let idx = array_data[3];
        if let [0x43, 0x05, _, _, payload @ ..] = array_data {
            full_data.extend_from_slice(payload);
        }
    }
    full_data
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

/// The items of a local set (ST 377-1 §9.6) of `length` bytes at the stream's position, as they come, nothing kept but
/// their values below, so a set takes no more memory for what it declares. An item that runs past the set's end is
/// damage: such a set isn't used
fn parse_set<R: Read + Seek>(stream: &mut BufReader<R>, length: u64) -> Result<BTreeMap<MxfMetaTag, serde_json::Value>> {
    let mut map = BTreeMap::<MxfMetaTag, serde_json::Value>::new();
    let mut left = length;

    while left >= 4 {
        let tag = stream.read_u16::<BigEndian>()?;
        let length = stream.read_u16::<BigEndian>()? as usize;
        left = left.checked_sub(4 + length as u64).ok_or(ErrorKind::InvalidData)?;
        // No item read here is longer
        if length > 8 {
            stream.seek_relative(length as i64)?;
            continue;
        }
        let mut item = [0u8; 8];
        stream.read_exact(&mut item[..length])?;
        // An item too short for its type isn't one
        let _ = parse_item(tag, &mut Cursor::new(&item[..length]), &mut map);
    }
    Ok(map)
}

fn parse_item(tag: u16, slice: &mut Cursor<&[u8]>, map: &mut BTreeMap<MxfMetaTag, serde_json::Value>) -> Result<()> {
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
        _ => { }
    }
    Ok(())
}
