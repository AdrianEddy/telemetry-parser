// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2021 Adrian <adrian.eddy at gmail>

use std::{ io::*, collections::{ BTreeSet, BTreeMap } };
use std::sync::{ Arc, atomic::AtomicBool };
use byteorder::{ ReadBytesExt, BigEndian };
use mp4parse::{ MediaContext, TrackType };
use memchr::memmem;

use crate::tags_impl::*;

pub fn to_hex(data: &[u8]) -> String {
    let mut ret = String::with_capacity(data.len() * 3);
    for b in data {
        ret.push_str(&format!("{:02x} ", b));
    }
    ret
}

#[derive(Debug, Clone, Default)]
pub struct SampleInfo {
    pub sample_index: u64,
    pub track_index: usize,
    pub timestamp_ms: f64,
    pub duration_ms: f64,
    pub video_rotation: Option<i32>,
    pub tag_map: Option<GroupedTagMap>
}

// Read all boxes and make sure all top-level boxes are named using ascii and have correct size.
// If there's any garbage at the end of the file, it is removed.
pub fn get_mp4_good_size<T: Read + Seek>(stream: &mut T, size: u64) -> Result<usize> {
    let mut good_size = 0;
    while stream.stream_position()? < size {
        let start_pos = stream.stream_position()?;
        let mut len = stream.read_u32::<BigEndian>()? as u64;
        let mut name = [0u8; 4];
        let name_read_ok = stream.read_exact(&mut name).is_ok();
        let name_good = name_read_ok && name.iter().all(|x| x.is_ascii() && *x > 13);
        if len == 1 { // Large box
            len = stream.read_u64::<BigEndian>()?;
        }
        let end_pos = start_pos + len;
        let size_good = size >= end_pos;
        if name_good && size_good {
            good_size = end_pos as usize;
            stream.seek(SeekFrom::Start(end_pos))?;
        } else {
            break;
        }
    }
    stream.seek(SeekFrom::Start(0))?;
    Ok(good_size)
}

// if mdhd timescale is 0, try to patch it if we know valid value
pub fn patch_mdhd_timescale(all: &mut [u8]) {
    let mut offs = 0;
    while let Some(pos) = memchr::memmem::find(&all[offs..], b"mdhd") {
        if all.len() > offs+pos+70 && &all[offs+pos+32..offs+pos+36] == b"hdlr" {
            let typ = unsafe { std::str::from_utf8_unchecked(&all[offs+pos+61..offs+pos+70] ) };
            let version = all[offs + 5];
            let dates = match version { 1 => 16, _ => 8 }; // creation and modification dates size
            // Skip 4 bytes fourcc
            // Skip 4 bytes version + flags
            // Skip 8 or 16 bytes creation/modification dates
            let ts_offset = offs+pos+4+4+dates;
            let timescale = (&all[ts_offset..]).read_u32::<BigEndian>().unwrap();
            if timescale == 0 {
                let patch = match typ {
                    "GoPro AAC" => 48000u32,
                    "GoPro MET" => 1000u32,
                    _ => 0u32
                };
                log::warn!("Track {typ} timescale is 0, trying patching it to {patch}");
                if patch > 0 {
                    all[ts_offset..ts_offset+4].copy_from_slice(&patch.to_be_bytes());
                }
            }
        }
        offs += pos + 4;
    }
}

pub struct PatchingLimitingStream<R: Read + Seek> {
    pub inner: R,
    pub stream_size: usize,
    pub total: usize,
    pub limit: usize
}
impl<R: Read + Seek> Read for PatchingLimitingStream<R> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let read = self.inner.read(buf)?;
        patch_mdhd_timescale(buf);
        self.total += read;
        if self.total > self.limit {
            return Ok(read.saturating_sub(self.total - self.limit));
        }
        Ok(read)
    }
}
impl<R: Read + Seek> Seek for PatchingLimitingStream<R> {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64> {
        let res = self.inner.seek(pos);
        if res.is_ok() {
            self.total = match pos {
                SeekFrom::Start(pos) => pos as usize,
                SeekFrom::End(pos) => self.stream_size.saturating_add(pos as usize),
                SeekFrom::Current(pos) => self.total.saturating_add(pos as usize),
            };
        }
        res
    }
}

pub fn parse_mp4<T: Read + Seek>(stream: &mut T, size: usize) -> mp4parse::Result<mp4parse::MediaContext> {
    if size > 10*1024*1024 {
        // With large files we can save a lot of time by only parsing actual MP4 box structure, skipping track data itself.
        // We do that by reading 15 MB from each end of the file, then patching `mdat` box to make the 30 MB buffer a correct MP4 file.
        // This is hacky, but it's worth a try and if we fail we fallback to full parsing anyway.
        let mut read_mb = if size as u64 > 100u64*1024*1024*1024 { // If file is greater than 100 GB, read 500 MB header/footer
            500
        } else if size as u64 > 60u64*1024*1024*1024 { // If file is greater than 60 GB, read 100 MB header/footer
            100
        } else if size as u64 > 30u64*1024*1024*1024 { // If file is greater than 30 GB, read 50 MB header/footer
            50
        } else if size as u64 > 5u64*1024*1024*1024 { // If file is greater than 5 GB, read 25 MB header/footer
            25
        } else {
            15
        };

        { // Check if it's Insta360 to account for the data at the end of file
            use crate::insta360;
            let mut buf = vec![0u8; insta360::HEADER_SIZE];
            stream.seek(SeekFrom::End(-(insta360::HEADER_SIZE as i64)))?;
            stream.read_exact(&mut buf)?;
            if &buf[insta360::HEADER_SIZE-32..] == insta360::MAGIC {
                let extra_size = (&buf[32..]).read_u32::<byteorder::LittleEndian>()? as f32;
                read_mb += (extra_size / 1024.0 / 1024.0).ceil() as usize;
            }
            stream.seek(SeekFrom::Start(0))?;
        }

        let mut all = read_beginning_and_end(stream, size, read_mb*1024*1024)?;
        if let Some(pos) = memchr::memmem::find(&all, b"mdat") {
            let how_much_less = (size - all.len()) as u64;
            let mut len = (&all[pos-4..]).read_u32::<BigEndian>()? as u64;
            if len == 1 {
                len = (&all[pos+4..]).read_u64::<BigEndian>()?;
            }
            if how_much_less > len {
                // Something went wrong, we need the full data
                log::warn!("Reading full mp4 {how_much_less} {len} {size} {}", all.len());
                if let Ok(good_size) = get_mp4_good_size(stream, size as u64) {
                    let mut limited_stream = BufReader::with_capacity(512 * 1024, PatchingLimitingStream { inner: stream, stream_size: size, total: 0, limit: good_size });
                    return mp4parse::read_mp4(&mut limited_stream);
                }
                return mp4parse::read_mp4(stream);
            } else {
                if len == 1 { // Large box
                    len = (&all[pos+4..]).read_u64::<BigEndian>()? - how_much_less;
                    all[pos+4..pos+12].copy_from_slice(&len.to_be_bytes());
                } else {
                    len -= how_much_less;
                    all[pos-4..pos].copy_from_slice(&(len as u32).to_be_bytes());
                }
            }

            if let Ok(good_size) = get_mp4_good_size(&mut std::io::Cursor::new(&all), all.len() as u64) {
                if all.len() > good_size {
                    log::warn!("Garbage found at the end of the file, removing {} bytes from the end.", all.len() - good_size);
                    all.resize(good_size, 0);
                }
            }
            patch_mdhd_timescale(&mut all);

            return mp4parse::read_mp4(&mut std::io::Cursor::new(&all));
        }
    }
    mp4parse::read_mp4(stream)
}

pub fn get_track_samples<F, T: Read + Seek>(stream: &mut T, size: usize, typ: mp4parse::TrackType, single: bool, max_sample_size: Option<usize>, mut callback: F, cancel_flag: Arc<AtomicBool>) -> Result<MediaContext>
    where F: FnMut(SampleInfo, &[u8], u64, Option<&VideoMetadata>)
{

    let ctx = parse_mp4(stream, size).or_else(|_| mp4parse::read_mp4(stream))?;

    let mut track_index = 0;
    // let mut sample_delta = 0u32;
    // let mut timestamp_ms = 0f64;

    let mut video_md = None;
    let mut video_rotation = None;

    for x in &ctx.tracks {
        if x.track_type == mp4parse::TrackType::Video && video_md.is_none() {
            video_md = get_video_metadata_from_track(x).ok();
            video_rotation = video_md.as_ref().map(|x| x.rotation);
        }
        if x.track_type == typ {
            if let Some(timescale) = x.timescale {
                // if let Some(ref stts) = x.stts {
                //     sample_delta = stts.samples[0].sample_delta;
                // }
                // let duration_ms = sample_delta as f64 * 1000.0 / timescale.0 as f64;

                if let Some(samples) = mp4parse::unstable::create_sample_table(&x, 0.into()) {
                    let mut sample_data = Vec::new();
                    let mut sample_index = 0u64;
                    for s in samples {
                        if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) { break; }

                        let mut sample_size = (s.end_offset.0 - s.start_offset.0) as usize;
                        if let Some(max_sample_size) = max_sample_size {
                            if sample_size > max_sample_size {
                                sample_size = max_sample_size;
                            }
                        }
                        let start_comp_ms = mp4parse::unstable::track_time_to_us(mp4parse::TrackScaledTime::<i64>(s.start_composition.0, x.id), mp4parse::TrackTimeScale::<i64>(timescale.0 as i64, timescale.1)).ok_or(mp4parse::Error::InvalidData(mp4parse::Status::MvhdBadTimescale))?.0 as f64 / 1000.0;
                        let end_comp_ms   = mp4parse::unstable::track_time_to_us(mp4parse::TrackScaledTime::<i64>(s.end_composition.0,   x.id), mp4parse::TrackTimeScale::<i64>(timescale.0 as i64, timescale.1)).ok_or(mp4parse::Error::InvalidData(mp4parse::Status::MvhdBadTimescale))?.0 as f64 / 1000.0;
                        let sample_timestamp_ms = start_comp_ms;
                        let sample_duration_ms = end_comp_ms - start_comp_ms;
                        if sample_size > 4 {
                            if sample_data.len() != sample_size {
                                sample_data.resize(sample_size, 0u8);
                            }

                            stream.seek(SeekFrom::Start(s.start_offset.0 as u64))?;
                            stream.read_exact(&mut sample_data[..])?;

                            callback(SampleInfo { sample_index, track_index, timestamp_ms: sample_timestamp_ms, duration_ms: sample_duration_ms, tag_map: None, video_rotation }, &sample_data, s.start_offset.0 as u64, video_md.as_ref());

                            //timestamp_ms += duration_ms;
                            sample_index += 1;
                        }
                    }
                    if single {
                        break;
                    }
                }
            }
        }
        track_index += 1;
    }
    Ok(ctx)
}

pub fn get_metadata_track_samples<F, T: Read + Seek>(stream: &mut T, size: usize, single: bool, callback: F, cancel_flag: Arc<AtomicBool>) -> Result<MediaContext>
    where F: FnMut(SampleInfo, &[u8], u64, Option<&VideoMetadata>)
{
    get_track_samples(stream, size, mp4parse::TrackType::Metadata, single, None, callback, cancel_flag)
}
pub fn get_other_track_samples<F, T: Read + Seek>(stream: &mut T, size: usize, single: bool, callback: F, cancel_flag: Arc<AtomicBool>) -> Result<MediaContext>
    where F: FnMut(SampleInfo, &[u8], u64, Option<&VideoMetadata>)
{
    get_track_samples(stream, size, mp4parse::TrackType::Unknown, single, None, callback, cancel_flag)
}

pub fn read_beginning_and_end<T: Read + Seek>(stream: &mut T, stream_size: usize, read_size: usize) -> Result<Vec<u8>> {
    let mut all = vec![0u8; read_size*2];

    stream.seek(SeekFrom::Start(0))?;

    if stream_size > read_size * 2 {
        let read1 = stream.read(&mut all[..read_size])?;

        stream.seek(SeekFrom::End(-(read_size as i64)))?;
        let read2 = stream.read(&mut all[read1..])?;

        all.resize(read1+read2, 0);
    } else {
        let read = stream.read(&mut all)?;
        all.resize(read, 0);
    }

    stream.seek(SeekFrom::Start(0))?;

    Ok(all)
}

#[derive(Default, serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct IMUData {
    pub timestamp_ms: f64,
    pub gyro: Option<[f64; 3]>,
    pub accl: Option<[f64; 3]>,
    pub magn: Option<[f64; 3]>
}


pub fn normalized_imu(input: &crate::Input, orientation: Option<String>) -> Result<Vec<IMUData>> {
    let mut timestamp = 0f64;
    let mut first_timestamp = None;
    let accurate_ts = input.has_accurate_timestamps();

    let mut final_data = Vec::<IMUData>::with_capacity(10000);
    let mut data_index = 0;

    let mut fix_timestamps = false;

    if let Some(ref samples) = input.samples {
        for info in samples {
            if info.tag_map.is_none() { continue; }

            let grouped_tag_map = info.tag_map.as_ref().unwrap();

            for (group, map) in grouped_tag_map {
                if group == &GroupId::Gyroscope || group == &GroupId::Accelerometer || group == &GroupId::Magnetometer {
                    let raw2unit = crate::try_block!(f64, {
                        match &map.get(&TagId::Scale)?.value {
                            TagValue::i16(v) => *v.get() as f64,
                            TagValue::f32(v) => *v.get() as f64,
                            TagValue::f64(v) => *v.get(),
                            _ => 1.0
                        }
                    }).unwrap_or(1.0);

                    let unit2deg = crate::try_block!(f64, {
                        match (map.get_t(TagId::Unit) as Option<&String>)?.as_str() {
                            "rad/s" => 180.0 / std::f64::consts::PI, // rad to deg
                            "g" => 9.80665, // g to m/s²
                            _ => 1.0
                        }
                    }).unwrap_or(1.0);

                    let mut io = match map.get_t(TagId::Orientation) as Option<&String> {
                        Some(v) if v.len() == 3 => v.clone(),
                        _ => "XYZ".into()
                    };
                    io = input.normalize_imu_orientation(io);
                    if let Some(imuo) = &orientation {
                        io = imuo.clone();
                    }
                    let io = io.as_bytes();

                    if let Some(taginfo) = map.get(&TagId::Data) {
                        match &taginfo.value {
                            // Sony and GoPro
                            TagValue::Vec_Vector3_i16(arr) => {
                                let arr = arr.get();
                                let reading_duration = info.duration_ms / arr.len() as f64;
                                fix_timestamps = true;

                                for (j, v) in arr.iter().enumerate() {
                                    if final_data.len() <= data_index + j {
                                        final_data.resize_with(data_index + j + 1, Default::default);
                                        final_data[data_index + j].timestamp_ms = timestamp;
                                        timestamp += reading_duration;
                                    }
                                    let itm = v.clone().into_scaled(&raw2unit, &unit2deg).orient(io);
                                         if group == &GroupId::Gyroscope     { final_data[data_index + j].gyro = Some([ itm.x, itm.y, itm.z ]); }
                                    else if group == &GroupId::Accelerometer { final_data[data_index + j].accl = Some([ itm.x, itm.y, itm.z ]); }
                                    else if group == &GroupId::Magnetometer  { final_data[data_index + j].magn = Some([ itm.x, itm.y, itm.z ]); }
                                }
                            },
                            // Insta360
                            TagValue::Vec_TimeVector3_f64(arr) => {
                                for (j, v) in arr.get().iter().enumerate() {
                                    if final_data.len() <= data_index + j {
                                        final_data.resize_with(data_index + j + 1, Default::default);
                                        final_data[data_index + j].timestamp_ms = v.t * 1000.0;
                                        if !accurate_ts {
                                            if first_timestamp.is_none() {
                                                first_timestamp = Some(final_data[data_index + j].timestamp_ms);
                                            }
                                            final_data[data_index + j].timestamp_ms -= first_timestamp.unwrap();
                                        }
                                    }
                                    let itm = v.clone().into_scaled(&raw2unit, &unit2deg).orient(io);
                                         if group == &GroupId::Gyroscope     { final_data[data_index + j].gyro = Some([ itm.x, itm.y, itm.z ]); }
                                    else if group == &GroupId::Accelerometer { final_data[data_index + j].accl = Some([ itm.x, itm.y, itm.z ]); }
                                    else if group == &GroupId::Magnetometer  { final_data[data_index + j].magn = Some([ itm.x, itm.y, itm.z ]); }
                                }
                            },
                            _ => ()
                        }
                    }
                }
            }
            data_index = final_data.len();
        }
    }

    if fix_timestamps && !final_data.is_empty() {
        let avg_diff = {
            if input.camera_type() == "GoPro" {
                crate::gopro::GoPro::get_avg_sample_duration(input.samples.as_ref().unwrap(), &GroupId::Gyroscope)
            } else {
                let mut total_duration_ms = 0.0;
                for info in input.samples.as_ref().unwrap() {
                    total_duration_ms += info.duration_ms;
                }
                Some(total_duration_ms / final_data.len() as f64)
            }
        };
        if let Some(avg_diff) = avg_diff {
            if avg_diff > 0.0 {
                for (i, x) in final_data.iter_mut().enumerate() {
                    x.timestamp_ms = avg_diff * i as f64;
                }
            }
        }
    }

    Ok(final_data)
}

pub fn normalized_imu_interpolated(input: &crate::Input, orientation: Option<String>) -> Result<Vec<IMUData>> {
    let mut first_timestamp = None;

    let accurate_ts = input.has_accurate_timestamps();

    let mut timestamp = (0.0, 0.0, 0.0);

    let mut gyro_map = BTreeMap::new();
    let mut accl_map = BTreeMap::new();
    let mut magn_map = BTreeMap::new();

    let mut gyro_timestamps = BTreeSet::new();

    if let Some(ref samples) = input.samples {
        let mut reading_duration =
        if input.camera_type() == "GoPro" {
            (
                crate::gopro::GoPro::get_avg_sample_duration(samples, &GroupId::Gyroscope),
                crate::gopro::GoPro::get_avg_sample_duration(samples, &GroupId::Accelerometer),
                crate::gopro::GoPro::get_avg_sample_duration(samples, &GroupId::Magnetometer),
            )
        } else {
            let mut total_len = (0, 0, 0);
            for grouped_tag_map in samples.iter().filter_map(|v| v.tag_map.as_ref()) {
                for (group, map) in grouped_tag_map {
                    if let Some(taginfo) = map.get(&TagId::Data) {
                        if let TagValue::Vec_Vector3_i16(arr) = &taginfo.value {
                            match group {
                                GroupId::Gyroscope     => total_len.0 += arr.get().len(),
                                GroupId::Accelerometer => total_len.1 += arr.get().len(),
                                GroupId::Magnetometer  => total_len.2 += arr.get().len(),
                                _ => {}
                            }
                        }
                    }
                }
            }

            let mut total_duration_ms = 0.0;
            for info in samples {
                total_duration_ms += info.duration_ms;
            }
            (
                if total_len.0 > 0 { Some(total_duration_ms / total_len.0 as f64) } else { None },
                if total_len.1 > 0 { Some(total_duration_ms / total_len.1 as f64) } else { None },
                if total_len.2 > 0 { Some(total_duration_ms / total_len.2 as f64) } else { None }
            )
        };
        log::debug!("Reading duration: {:?}", reading_duration);
        if let Some(grd) = reading_duration.0 {
            if let Some(ard) = reading_duration.1 {
                if (grd - ard).abs() < 0.1 {
                    reading_duration.0 = Some(grd.max(ard));
                    reading_duration.1 = Some(grd.max(ard));
                }
            }
            if let Some(mrd) = reading_duration.2 {
                if (grd - mrd).abs() < 0.1 {
                    reading_duration.0 = Some(grd.max(mrd));
                    reading_duration.2 = Some(grd.max(mrd));
                }
            }
        }

        for info in samples {
            if info.tag_map.is_none() { continue; }

            let grouped_tag_map = info.tag_map.as_ref().unwrap();

            for (group, map) in grouped_tag_map {
                if group == &GroupId::Gyroscope || group == &GroupId::Accelerometer || group == &GroupId::Magnetometer {
                    let raw2unit = crate::try_block!(f64, {
                        match &map.get(&TagId::Scale)?.value {
                            TagValue::i16(v) => *v.get() as f64,
                            TagValue::f32(v) => *v.get() as f64,
                            TagValue::f64(v) => *v.get(),
                            _ => 1.0
                        }
                    }).unwrap_or(1.0);

                    let unit2deg = crate::try_block!(f64, {
                        match (map.get_t(TagId::Unit) as Option<&String>)?.as_str() {
                            "rad/s" => 180.0 / std::f64::consts::PI, // rad to deg
                            "g" => 9.80665, // g to m/s²
                            _ => 1.0
                        }
                    }).unwrap_or(1.0);

                    let mut io = match map.get_t(TagId::Orientation) as Option<&String> {
                        Some(v) if v.len() == 3 => v.clone(),
                        _ => "XYZ".into()
                    };
                    io = input.normalize_imu_orientation(io);
                    if let Some(imuo) = &orientation {
                        io = imuo.clone();
                    }
                    let io = io.as_bytes();

                    if let Some(taginfo) = map.get(&TagId::Data) {
                        match &taginfo.value {
                            // Sony and GoPro
                            TagValue::Vec_Vector3_i16(arr) => {
                                let arr = arr.get();

                                for v in arr {
                                    let itm = v.clone().into_scaled(&raw2unit, &unit2deg).orient(io);
                                         if group == &GroupId::Gyroscope     { let ts = (timestamp.0 * 1000.0f64).round() as i64; gyro_map.insert(ts, itm); timestamp.0 += reading_duration.0.unwrap(); gyro_timestamps.insert(ts); }
                                    else if group == &GroupId::Accelerometer { let ts = (timestamp.1 * 1000.0f64).round() as i64; accl_map.insert(ts, itm); timestamp.1 += reading_duration.1.unwrap(); }
                                    else if group == &GroupId::Magnetometer  { let ts = (timestamp.2 * 1000.0f64).round() as i64; magn_map.insert(ts, itm); timestamp.2 += reading_duration.2.unwrap(); }
                                }
                            },
                            TagValue::Vec_TimeVector3_f64(arr) => {
                                for v in arr.get() {
                                    let mut timestamp_ms = v.t * 1000.0;
                                    if !accurate_ts {
                                        if first_timestamp.is_none() {
                                            first_timestamp = Some(timestamp_ms);
                                        }
                                        timestamp_ms -= first_timestamp.unwrap();
                                    }

                                    let timestamp_us = (timestamp_ms * 1000.0).round() as i64;

                                    let itm = v.clone().into_scaled(&raw2unit, &unit2deg).orient(io);
                                         if group == &GroupId::Gyroscope     { gyro_map.insert(timestamp_us, itm);  gyro_timestamps.insert(timestamp_us); }
                                    else if group == &GroupId::Accelerometer { accl_map.insert(timestamp_us, itm); }
                                    else if group == &GroupId::Magnetometer  { magn_map.insert(timestamp_us, itm); }
                                }
                            },
                            _ => ()
                        }
                    }
                }
            }
        }
    }

    fn get_at_timestamp(ts: i64, map: &BTreeMap<i64, Vector3<f64>>) -> Option<[f64; 3]> {
        if map.is_empty() { return None; }
        if let Some(v) = map.get(&ts) { return Some([v.x, v.y, v.z]); }

        if let Some((k1, v1)) = map.range(..=ts).next_back() {
            if let Some((k2, v2)) = map.range(ts..).next() {
                let time_delta = (k2 - k1) as f64;
                let fract = (ts - k1) as f64 / time_delta;
                // dbg!(&fract);
                return Some([
                    v1.x * (1.0 - fract) + (v2.x * fract),
                    v1.y * (1.0 - fract) + (v2.y * fract),
                    v1.z * (1.0 - fract) + (v2.z * fract),
                ]);
            }
        }
        None
    }

    let mut final_data = Vec::with_capacity(gyro_map.len());
    for x in &gyro_timestamps {
        final_data.push(IMUData {
            timestamp_ms: *x as f64 / 1000.0,
            gyro: get_at_timestamp(*x, &gyro_map),
            accl: get_at_timestamp(*x, &accl_map),
            magn: get_at_timestamp(*x, &magn_map)
        });
    }

    Ok(final_data)
}

pub fn interpolate_at_timestamp(timestamp_us: i64, offsets: &BTreeMap<i64, f64>) -> f64 {
    match offsets.len() {
        0 => 0.0,
        1 => *offsets.values().next().unwrap(),
        _ => {
            if let Some(&first_ts) = offsets.keys().next() {
                if let Some(&last_ts) = offsets.keys().next_back() {
                    let lookup_ts = (timestamp_us).min(last_ts-1).max(first_ts+1);
                    if let Some(offs1) = offsets.range(..=lookup_ts).next_back() {
                        if *offs1.0 == lookup_ts {
                            return *offs1.1;
                        }
                        if let Some(offs2) = offsets.range(lookup_ts..).next() {
                            let time_delta = ((offs2.0 - offs1.0) as f64).max(1.0);
                            let fract = (timestamp_us - offs1.0) as f64 / time_delta;
                            return offs1.1 + (offs2.1 - offs1.1) * fract;
                        }
                    }
                }
            }
            0.0
        }
    }
}

pub fn multiply_quats(p: (f64, f64, f64, f64), q: (f64, f64, f64, f64)) -> Quaternion<f64> {
    Quaternion {
        w: p.0*q.0 - p.1*q.1 - p.2*q.2 - p.3*q.3,
        x: p.0*q.1 + p.1*q.0 + p.2*q.3 - p.3*q.2,
        y: p.0*q.2 - p.1*q.3 + p.2*q.0 + p.3*q.1,
        z: p.0*q.3 + p.1*q.2 - p.2*q.1 + p.3*q.0
    }
}

pub fn find_between_with_offset(buffer: &[u8], from: &[u8], to: u8, offset: i32) -> Option<String> {
    let pos = memmem::find(buffer, from)?;
    let end = memchr::memchr(to, &buffer[pos+from.len()..])?;
    Some(String::from_utf8_lossy(&buffer[(pos as i32 + from.len() as i32 + offset) as usize..pos+from.len()+end]).into())
}

pub fn find_between(buffer: &[u8], from: &[u8], to: u8) -> Option<String> {
    find_between_with_offset(buffer, from, to, 0)
}
pub fn find_from_to(buffer: &[u8], from: &[u8], to: &[u8]) -> Option<String> {
    let pos = memmem::find(buffer, from)?;
    let end = memmem::find(&buffer[pos+from.len()..], to)?;
    Some(String::from_utf8_lossy(&buffer[pos..pos+from.len()+end+to.len()]).into())
}

pub fn insert_tag(map: &mut GroupedTagMap, tag: TagDescription, options: &crate::InputOptions) {
    use crate::TagFilter;
    let match_tag = |x: &TagFilter| -> bool {
        match x {
            TagFilter::EntireGroup(g)    => g == &tag.group,
            TagFilter::EntireTag  (t)    => t == &tag.id,
            TagFilter::SpecificTag(g, t) => g == &tag.group && t == &tag.id,
        }
    };
    if !options.tag_whitelist.is_empty() && !options.tag_whitelist.iter().any(match_tag) {
        return;
    }
    if !options.tag_blacklist.is_empty() && options.tag_blacklist.iter().any(match_tag) {
        return;
    }

    let group_map = map.entry(tag.group.clone()).or_insert_with(TagMap::new);
    group_map.insert(tag.id.clone(), tag);
}

pub fn create_csv_map<'a, 'b>(row: &'b csv::StringRecord, headers: &'a Vec<String>) -> BTreeMap<&'a str, &'b str> {
    headers.iter().zip(row).map(|(a, b)| (&a[..], b.trim())).collect()
}
pub fn create_csv_map_hdr<'a, 'b>(row: &'b csv::StringRecord, headers: &'a csv::StringRecord) -> BTreeMap<&'a str, &'b str> {
    headers.iter().zip(row).map(|(a, b)| (a, b)).collect()
}

pub fn get_fps_from_track(track: &mp4parse::Track) -> Option<f64> {
    if let Some(ref stts) = track.stts {
        if !stts.samples.is_empty() {
            let samples: u32 = stts.samples.iter().map(|v| v.sample_count).sum();
            let timescale = track.timescale?;
            let duration = track.duration?;
            let duration_us = duration.0 as f64 * 1000_000.0 / timescale.0 as f64;
            let us_per_frame = duration_us / samples as f64;
            return Some(1000_000.0 / us_per_frame);
        }
    }
    None
}

#[derive(Default, Debug, Clone)]
pub struct VideoMetadata {
    pub width: usize,
    pub height: usize,
    pub fps: f64,
    pub duration_s: f64,
    pub rotation: i32
}

pub fn get_video_metadata_from_track(track: &mp4parse::Track) -> Result<VideoMetadata> {
    let mut duration_sec = 0.0;
    if let Some(d) = track.duration {
        if let Some(ts) = track.timescale {
            duration_sec = d.0 as f64 / ts.0 as f64;
        }
    }
    if let Some(ref tkhd) = track.tkhd {
        let w = (tkhd.width >> 16) as usize;
        let h = (tkhd.height >> 16) as usize;
        let matrix = (
            tkhd.matrix.a >> 16,
            tkhd.matrix.b >> 16,
            tkhd.matrix.c >> 16,
            tkhd.matrix.d >> 16,
        );
        let rotation = match matrix {
            (0, 1, -1, 0) => 90,   // rotate 90 degrees
            (-1, 0, 0, -1) => 180, // rotate 180 degrees
            (0, -1, 1, 0) => 270,  // rotate 270 degrees
            _ => 0,
        };
        let fps = get_fps_from_track(&track).unwrap_or_default();
        Ok(VideoMetadata {
            width: w,
            height: h,
            fps,
            duration_s: duration_sec,
            rotation
        })
    } else {
        Err(ErrorKind::Other.into())
    }
}

pub fn get_video_metadata<T: Read + Seek>(stream: &mut T, filesize: usize) -> Result<VideoMetadata> { // -> (width, height, fps, duration_s, rotation)
    let mut header = [0u8; 4];
    let mut last16kb = vec![0u8; 16384];
    stream.read_exact(&mut header)?;
    if filesize > 16384 {
        stream.seek(SeekFrom::End(-16384))?;
        stream.read_exact(&mut last16kb)?;
    }
    stream.seek(SeekFrom::Start(0))?;

    if header == [0x06, 0x0E, 0x2B, 0x34] { // MXF header
        let mut md = VideoMetadata::default();
        crate::sony::mxf::parse(stream, filesize, |_|(), Arc::new(AtomicBool::new(false)), Some(&mut md), &crate::InputOptions::default())?;
        return Ok(md);
    }

    // Special case for BRAW
    let mut override_size = None;
    if memmem::find(&last16kb, b"Blackmagic Design").is_some() {
        let mut bmd = crate::blackmagic::BlackmagicBraw::default();
        if let Ok(md) = bmd.parse_meta(stream, filesize) {
            if let Some(size) = md.get("crop_size").and_then(|x| x.as_array()).filter(|x| x.len() == 2).and_then(|x| Some((x[0].as_f64()? as usize, x[1].as_f64()? as usize))) {
                override_size = Some(size);
            }
        }
    }

    let mp = parse_mp4(stream, filesize)?;
    for track in mp.tracks {
        if track.track_type == TrackType::Video {
            let mut md = get_video_metadata_from_track(&track)?;
            if let Some(os) = override_size {
                md.width = os.0;
                md.height = os.1;
            }
            return Ok(md);
        }
    }
    Err(ErrorKind::Other.into())
}

pub const fn fourcc(s: &str) -> u32 {
    let s = s.as_bytes();
    (s[3] as u32) | ((s[2] as u32) << 8) | ((s[1] as u32) << 16) | ((s[0] as u32) << 24)
}
pub fn read_box<R: Read + Seek>(reader: &mut R) -> Result<(u32, u64, u64, i64)> {
    let pos = reader.stream_position()?;
    let size = reader.read_u32::<BigEndian>()?;
    let typ = reader.read_u32::<BigEndian>()?;
    if size == 1 {
        let largesize = reader.read_u64::<BigEndian>()?;
        Ok((typ, pos, largesize, 16))
    } else {
        Ok((typ, pos, size as u64, 8))
    }
}

#[macro_export]
macro_rules! try_block {
    ($type:ty, $body:block) => {
        (|| -> Option<$type> {
            Some($body)
        }())
    };
    ($body:block) => {
        (|| -> Option<()> {
            $body
            Some(())
        }())
    };
}