// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2021 Adrian <adrian.eddy at gmail>

use std::{ io::*, collections::BTreeMap };
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

/// The end of a box at `pos` that is `size` bytes long with its header of `header_size` (ISO/IEC 14496-12 §4.2): none
/// for a size smaller than the header, which no walk can step past, or for an end past the largest stream position (an
/// `i64`, as file offsets are). So a walk that goes from each box to the next one at its end always moves forward, by a
/// header at least. Nor can the end plus a header overflow, the sum the `read_box` walks seek by (`org_pos + size`)
fn box_end(pos: u64, size: u64, header_size: u64) -> Option<u64> {
    pos.checked_add(size).filter(|end| size >= header_size && *end <= i64::MAX as u64)
}

// Read all boxes and make sure all top-level boxes are named using ascii and have correct size.
// If there's any garbage at the end of the file, it is removed.
// A box of size 0 extends to the end of the file, so it's the last one (ISO/IEC 14496-12 §4.2). A recording that was
// never finished is left that way: FFmpeg writes its `mdat` with size 0 and only sets it when it writes the `moov` after
// it. Any other size that can't be stepped past (`box_end`) is garbage, like a box past the end of the file
pub fn get_mp4_good_size<T: Read + Seek>(stream: &mut T, size: u64) -> Result<usize> {
    let mut good_size = 0;
    while stream.stream_position()? < size {
        let start_pos = stream.stream_position()?;
        let mut len = stream.read_u32::<BigEndian>()? as u64;
        let mut name = [0u8; 4];
        let name_read_ok = stream.read_exact(&mut name).is_ok();
        let name_good = name_read_ok && name.iter().all(|x| x.is_ascii() && *x > 13);
        let mut header_size = 8;
        if len == 1 { // Large box
            len = stream.read_u64::<BigEndian>()?;
            header_size = 16;
        } else if len == 0 { // Up to the end of the file
            len = size - start_pos;
        }
        let end_pos = box_end(start_pos, len, header_size).filter(|end_pos| size >= *end_pos);
        if let (true, Some(end_pos)) = (name_good, end_pos) {
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
//
// The mdhd box (ISO/IEC 14496-12 §8.4.2) has its size before its type, and its version after it: 32 bytes in version 0,
// 44 in version 1, whose creation and modification dates and duration are 64-bit rather than 32-bit. Bytes that read
// "mdhd" without that size aren't one. The track's hdlr box follows it, its name 32 bytes in: a counted string in GoPro's
// files, "GoPro MET" after its length
pub fn patch_mdhd_timescale(all: &mut [u8]) {
    let mut offs = 0;
    while let Some(pos) = memchr::memmem::find(&all[offs..], b"mdhd") {
        let at = offs + pos;
        offs = at + 4;
        let Some(start) = at.checked_sub(4) else { continue };
        let size = (&all[start..at]).read_u32::<BigEndian>().unwrap() as usize;
        let dates = match (all.get(at + 4).copied(), size) {
            (Some(0), 32) => 8,
            (Some(1), 44) => 16,
            _ => continue,
        };
        let hdlr = start + size;
        if all.get(hdlr + 4..hdlr + 8) != Some(b"hdlr") { continue; }
        let Some(typ) = all.get(hdlr + 33..hdlr + 42) else { continue };
        // After the type, the version and flags, and the dates
        let ts_offset = at + 4 + 4 + dates;
        let timescale = (&all[ts_offset..]).read_u32::<BigEndian>().unwrap();
        if timescale == 0 {
            let patch = match typ {
                b"GoPro AAC" => 48000u32,
                b"GoPro MET" => 1000u32,
                _ => 0u32
            };
            log::warn!("Track {} timescale is 0, trying patching it to {patch}", String::from_utf8_lossy(typ));
            if patch > 0 {
                all[ts_offset..ts_offset+4].copy_from_slice(&patch.to_be_bytes());
            }
        }
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
        } else if size as u64 > 60u64*1024*1024*1024 { // If file is greater than 60 GB, read 220 MB header/footer
            220
        } else if size as u64 > 30u64*1024*1024*1024 { // If file is greater than 30 GB, read 180 MB header/footer
            180
        } else if size as u64 > 5u64*1024*1024*1024 { // If file is greater than 5 GB, read 40 MB header/footer
            40
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
        if let Some(pos) = memchr::memmem::find(&all, b"mdat").filter(|pos| *pos >= 4) { // after its size
            let how_much_less = (size - all.len()) as u64;
            let mut len = (&all[pos-4..]).read_u32::<BigEndian>()? as u64;
            let is_large_box = len == 1;
            if is_large_box {
                len = (&all[pos+4..]).read_u64::<BigEndian>()?;
            }
            if how_much_less > len {
                // Something went wrong, we need the full data
                log::warn!("Reading full mp4 {how_much_less} {len} {size} {}", all.len());
                if let Ok(good_size) = get_mp4_good_size(stream, size as u64) {
                    let mut limited_stream = BufReader::with_capacity(512 * 1024, PatchingLimitingStream { inner: stream, stream_size: size, total: 0, limit: good_size });
                    return mp4parse::read_mp4(&mut limited_stream, mp4parse::ParseStrictness::Permissive);
                }
                return mp4parse::read_mp4(stream, mp4parse::ParseStrictness::Permissive);
            } else {
                len -= how_much_less;
                if is_large_box { // Large box
                    all[pos+4..pos+12].copy_from_slice(&len.to_be_bytes());
                } else {
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
            crate::zcam::deobfuscate_zraw_chunk_offsets(&mut all);

            return mp4parse::read_mp4(&mut std::io::Cursor::new(&all), mp4parse::ParseStrictness::Permissive);
        }
    }
    // The whole file, from its start rather than wherever the caller's reads left the stream
    stream.seek(SeekFrom::Start(0))?;
    mp4parse::read_mp4(stream, mp4parse::ParseStrictness::Permissive)
}

pub fn get_track_samples<F, T: Read + Seek>(stream: &mut T, size: usize, mut typ: mp4parse::TrackType, single: bool, max_sample_size: Option<usize>, mut callback: F, cancel_flag: Arc<AtomicBool>) -> Result<MediaContext>
    where F: FnMut(SampleInfo, &[u8], u64, Option<&VideoMetadata>)
{

    // When `parse_mp4`'s quick parse fails (of the file's two ends, or of its boxes up to the first bad one), the whole
    // file as mp4parse reads it. That is from the file's start: the failed parse leaves the stream wherever it stopped
    let ctx = parse_mp4(stream, size).or_else(|_| {
        stream.seek(SeekFrom::Start(0))?;
        mp4parse::read_mp4(stream, mp4parse::ParseStrictness::Permissive)
    })?;

    let mut track_index = 0;
    // let mut sample_delta = 0u32;
    // let mut timestamp_ms = 0f64;

    let mut video_md = None;
    let mut video_rotation = None;

    if typ == mp4parse::TrackType::Metadata && !ctx.tracks.iter().any(|x| x.track_type == mp4parse::TrackType::Metadata) && ctx.tracks.iter().any(|x| x.track_type == mp4parse::TrackType::Unknown) {
        // If we want a metadata track, but there's none, try to get unknown track instead
        typ = mp4parse::TrackType::Unknown;
    }

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
                    // A sample is numbered by its entry in the sample table, the ones too short to be delivered
                    // included: the parsers number the frames by it, so a frame whose sample is empty is a gap in the
                    // numbers rather than moving every frame after it
                    for (sample_index, s) in samples.iter().enumerate() {
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

                            callback(SampleInfo { sample_index: sample_index as u64, track_index, timestamp_ms: sample_timestamp_ms, duration_ms: sample_duration_ms, tag_map: None, video_rotation }, &sample_data, s.start_offset.0 as u64, video_md.as_ref());

                            //timestamp_ms += duration_ms;
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


pub fn camera_rig(input: &crate::Input) -> Option<crate::rig::CameraRig> {
    input.samples.as_ref()?.iter()
        .filter_map(|sample| sample.tag_map.as_ref()?.get(&GroupId::Lens))
        .find_map(|map| (map.get_t(TagId::Rig) as Option<&crate::rig::CameraRig>).cloned())
}

/// The time between two samples of a track that it has no samples for: `next` starts where `prev` ends in a track that
/// delivers every sample, and later by the samples it didn't deliver (`get_track_samples` skips the empty ones). A gap
/// is at least one whole sample long, so less than half of `prev`'s duration is rounding of the times, and no gap
pub(crate) fn time_missing_between(prev: &SampleInfo, next: &SampleInfo) -> f64 {
    let gap = next.timestamp_ms - (prev.timestamp_ms + prev.duration_ms);
    if prev.duration_ms > 0.0 && gap >= prev.duration_ms / 2.0 { gap } else { 0.0 }
}

/// A sample of evenly spaced readings (an IMU reading at a steady rate, whose samples give no time per reading): the
/// sample, the camera clock's time of its readings when the format has one (GoPro's stamps, in µs), and their count
type EvenlySpaced<'a> = (&'a SampleInfo, Option<i64>, usize);

/// The time from the end of `prev`'s readings to the start of `next`'s on their timeline, which spaces the readings
/// `reading_ms` apart: none for samples the track delivered one after another, whose readings continue the timeline.
/// Across samples it didn't deliver (`time_missing_between`) it's the time of those, so the readings after them stay at
/// their own sample's time instead of landing a frame early for every sample missing. That is on the clock the
/// readings are spaced by: the track's, or the camera's when the samples are stamped by it, from the two stamps less
/// the readings of `prev`
fn time_before_readings(prev: EvenlySpaced, next: EvenlySpaced, reading_ms: f64) -> f64 {
    let missing_ms = time_missing_between(prev.0, next.0);
    match (prev.1, next.1) {
        (Some(prev_us), Some(next_us)) if missing_ms > 0.0 => (next_us - prev_us) as f64 / 1000.0 - prev.2 as f64 * reading_ms,
        _ => missing_ms,
    }
}

pub fn normalized_imu(input: &crate::Input, orientation: Option<String>) -> Result<Vec<IMUData>> {
    let mut timestamp = 0f64;
    let mut first_timestamp = None;
    let accurate_ts = input.has_accurate_timestamps();

    let mut final_data = Vec::<IMUData>::with_capacity(10000);
    let mut data_index = 0;

    let mut fix_timestamps = false;
    // The samples with evenly spaced readings, by where their readings start in `final_data`, and their duration
    let mut evenly_spaced: Vec<(usize, EvenlySpaced)> = Vec::new();
    let mut evenly_spaced_duration_ms = 0.0;
    let is_gopro = input.camera_type() == "GoPro";

    if let Some(ref samples) = input.samples {
        for info in samples {
            if info.tag_map.is_none() { continue; }

            let grouped_tag_map = info.tag_map.as_ref().unwrap();
            let mut sample_evenly_spaced = false;

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
                                sample_evenly_spaced |= !arr.is_empty();

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
            if sample_evenly_spaced {
                // GoPro's readings are spaced by the camera's clock (`GoPro::get_avg_sample_duration`), its gyroscope's
                let stamp_us = grouped_tag_map.get(&GroupId::Gyroscope).filter(|_| is_gopro).and_then(crate::gopro::GoPro::stamp_us);
                evenly_spaced.push((data_index, (info, stamp_us, 0)));
                evenly_spaced_duration_ms += info.duration_ms;
            }
            data_index = final_data.len();
        }
    }

    if fix_timestamps && !final_data.is_empty() {
        let avg_diff = {
            if is_gopro {
                crate::gopro::GoPro::get_avg_sample_duration(input.samples.as_ref().unwrap(), &GroupId::Gyroscope)
            } else {
                // The readings span the samples they came in
                Some(evenly_spaced_duration_ms / final_data.len() as f64)
            }
        };
        if let Some(avg_diff) = avg_diff {
            if avg_diff > 0.0 {
                // Where each sample's readings start: past the evenly spaced timeline by the time before them
                // (`time_before_readings`), from the first sample's own time
                let mut sample_starts = Vec::with_capacity(evenly_spaced.len());
                let mut offset_ms = 0.0;
                for (k, &(start, sample)) in evenly_spaced.iter().enumerate() {
                    offset_ms = match k.checked_sub(1).map(|previous| evenly_spaced[previous]) {
                        Some((previous_start, (info, stamp_us, _))) => offset_ms + time_before_readings((info, stamp_us, start - previous_start), sample, avg_diff),
                        None => sample.0.timestamp_ms,
                    };
                    sample_starts.push((start, offset_ms));
                }
                let mut sample_starts = sample_starts.into_iter().peekable();
                let mut offset_ms = 0.0;
                for (i, x) in final_data.iter_mut().enumerate() {
                    while let Some((_, sample_offset_ms)) = sample_starts.next_if(|(start, _)| *start <= i) {
                        offset_ms = sample_offset_ms;
                    }
                    x.timestamp_ms = avg_diff * i as f64 + offset_ms;
                }
            }
        }
    }

    Ok(final_data)
}

pub fn normalized_imu_interpolated(input: &crate::Input, orientation: Option<String>) -> Result<Vec<IMUData>> {
    Ok(imu_readings(input, orientation)?.interpolated(|_| Some(())).0)
}

/// The tag of an IMU sensor's group (`TagId::Custom`, a `Vec_u64`) holding the video frame of each of its readings
/// (`TagId::Data`, in their order), for a format whose sample holds the readings of many frames but which records where
/// each reading is among the frames: see `ReadingOrigin::frame`. A group whose readings' frames can't be told has none
pub const READING_FRAMES: &str = "ReadingFrames";

/// Where a reading of the IMU was read from (`imu_readings`)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ReadingOrigin {
    /// Its sample: the index in `Input::samples`
    pub sample: usize,
    /// The video frame the reading is recorded with, where its sample holds the readings of many frames and the format
    /// records each reading's place among the frames (`READING_FRAMES`): a RED clip's IMU records by the frame the atom
    /// holding them follows, a Blackmagic RAW clip's and a MotionCam recording's readings by the frame being captured
    /// when they were taken. A file cut from recordings keeps each segment's readings with its frames, so this tells
    /// the readings of one segment from the next one's where their times don't: a reading is timed by its motion, which
    /// can be before the frame it's recorded with
    pub frame: Option<u64>,
}

/// The readings of one sensor of the IMU by their time (µs), each with where it came from: a later sample's reading at
/// the same time replaces the earlier one
pub type SensorReadings = BTreeMap<i64, (Vector3<f64>, ReadingOrigin)>;

/// A sensor of the IMU that's put at the gyroscope's times (`ImuReadings::interpolated`)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ImuSensor {
    Accelerometer,
    Magnetometer,
}

/// A value of the accelerometer or the magnetometer at a gyroscope reading's time that `ImuReadings::interpolated` takes
/// between two readings of the sensor of which the parts aren't all known (`ImuReadings::unplaced_values`)
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct UnplacedValue<P> {
    /// The gyroscope reading's time (µs, its key in `ImuReadings::gyro`), and its part, `None` where its origin doesn't
    /// tell
    pub at_us: i64,
    pub part: Option<P>,
    pub sensor: ImuSensor,
    /// The readings of the sensor the value is between: their times (µs) and parts
    pub before: (i64, Option<P>),
    pub after: (i64, Option<P>),
}

/// The IMU readings of a file, each sensor's on its own timeline (`imu_readings`), before they're put together at the
/// gyroscope's times (`interpolated`)
#[derive(Clone, Debug, Default)]
pub struct ImuReadings {
    pub gyro: SensorReadings,
    pub accl: SensorReadings,
    pub magn: SensorReadings,
}

impl ImuReadings {
    /// The readings at the gyroscope's times, with where each gyroscope reading came from. The accelerometer and the
    /// magnetometer are interpolated at a gyroscope reading's time between their nearest readings before and after it
    /// that are of the gyroscope reading's part (`part`, of a reading's origin): the readings of one part are the
    /// motion of one stretch of time and those of another part another one's, as the segments of a file cut from
    /// recordings are, so no value is interpolated from two of them. `None` where the part has no reading on one side,
    /// as at the ends of a file. A reading whose part its origin doesn't tell (`part` gives `None`) is of whichever part
    /// its time is in, which only the parts' times on the motion timeline tell: it's taken with every part's readings,
    /// and a gyroscope reading of an unknown part is interpolated from all of them. One part for all of them
    /// (`|_| Some(())`), or none known (`|_| None::<()>`), is `normalized_imu_interpolated`
    pub fn interpolated<P: PartialEq>(&self, part: impl Fn(&ReadingOrigin) -> Option<P>) -> (Vec<IMUData>, Vec<ReadingOrigin>) {
        // Each sensor's readings: those of no known part, each part's with those (the parts in the order of their first
        // readings), and all of them where that's another map than those
        struct ByPart<P> { unknown: BTreeMap<i64, Vector3<f64>>, parts: Vec<(P, BTreeMap<i64, Vector3<f64>>)>, all: Option<BTreeMap<i64, Vector3<f64>>> }
        let by_part = |sensor: &SensorReadings| {
            let mut s = ByPart { unknown: BTreeMap::new(), parts: Vec::new(), all: None };
            for (t, (v, origin)) in sensor {
                match part(origin) {
                    Some(p) => match s.parts.iter_mut().find(|(q, _)| *q == p) {
                        Some((_, map)) => { map.insert(*t, v.clone()); }
                        None => s.parts.push((p, BTreeMap::from([(*t, v.clone())]))),
                    },
                    None => { s.unknown.insert(*t, v.clone()); }
                }
            }
            if !s.parts.is_empty() && (!s.unknown.is_empty() || s.parts.len() > 1) {
                s.all = Some(sensor.iter().map(|(t, (v, _))| (*t, v.clone())).collect());
            }
            if !s.unknown.is_empty() {
                for (_, map) in &mut s.parts { map.extend(s.unknown.iter().map(|(t, v)| (*t, v.clone()))); }
            }
            s
        };
        let (accl, magn) = (by_part(&self.accl), by_part(&self.magn));
        let at = |s: &ByPart<P>, p: &Option<P>, ts: i64| get_at_timestamp(ts, match p {
            Some(p) => s.parts.iter().find(|(q, _)| q == p).map_or(&s.unknown, |(_, map)| map),
            None => s.all.as_ref().or(s.parts.first().map(|(_, map)| map)).unwrap_or(&s.unknown),
        });

        let mut final_data = Vec::with_capacity(self.gyro.len());
        let mut origins = Vec::with_capacity(self.gyro.len());
        for (x, (v, origin)) in &self.gyro {
            let p = part(origin);
            final_data.push(IMUData {
                timestamp_ms: *x as f64 / 1000.0,
                gyro: Some([v.x, v.y, v.z]),
                accl: at(&accl, &p, *x),
                magn: at(&magn, &p, *x),
            });
            origins.push(*origin);
        }
        (final_data, origins)
    }

    /// The values of the accelerometer and the magnetometer that `interpolated(part)` takes from a reading of no known
    /// part, or gives a gyroscope reading of no known part: each between two readings of the sensor (none at the
    /// gyroscope reading's own time), with the times and the parts of the gyroscope reading and of those two (`None` where
    /// the origin doesn't tell). A reading of no known part is of the part its time is in, which the parts' places on the
    /// motion timeline tell once they're known: such a value is of its gyroscope reading's part only if both readings
    /// it's between turn out to be of that part too. Where one isn't, the part has no reading of the sensor on that side
    /// (the readings of a part are those of its time, and those its origins tell, which are in its time too), and the
    /// value is `None`, as `interpolated` gives one of a known part
    pub fn unplaced_values<P: PartialEq + Copy>(&self, part: impl Fn(&ReadingOrigin) -> Option<P>) -> Vec<UnplacedValue<P>> {
        // Each sensor's readings by their time with their parts: all of them, those of no known part, and each part's with
        // those, the readings `interpolated` takes a gyroscope reading's value from
        struct Pools<P> { all: BTreeMap<i64, Option<P>>, unknown: BTreeMap<i64, Option<P>>, parts: Vec<(P, BTreeMap<i64, Option<P>>)> }
        let pools = |sensor: &SensorReadings| {
            let all: BTreeMap<i64, Option<P>> = sensor.iter().map(|(t, (_, origin))| (*t, part(origin))).collect();
            let unknown: BTreeMap<i64, Option<P>> = all.iter().filter(|(_, p)| p.is_none()).map(|(t, p)| (*t, *p)).collect();
            let mut parts: Vec<(P, BTreeMap<i64, Option<P>>)> = Vec::new();
            for (t, p) in &all {
                let Some(p) = p else { continue };
                match parts.iter_mut().find(|(q, _)| q == p) {
                    Some((_, map)) => { map.insert(*t, Some(*p)); }
                    None => parts.push((*p, unknown.iter().map(|(t, q)| (*t, *q)).chain([(*t, Some(*p))]).collect())),
                }
            }
            Pools { all, unknown, parts }
        };
        let sensors = [(ImuSensor::Accelerometer, pools(&self.accl)), (ImuSensor::Magnetometer, pools(&self.magn))];
        let mut values = Vec::new();
        for (at_us, (_, origin)) in &self.gyro {
            let p = part(origin);
            for (sensor, s) in &sensors {
                let pool = match &p {
                    Some(p) => s.parts.iter().find(|(q, _)| q == p).map_or(&s.unknown, |(_, map)| map),
                    None => &s.all,
                };
                if pool.contains_key(at_us) { continue; }
                let (Some((&before, &bp)), Some((&after, &ap))) = (pool.range(..*at_us).next_back(), pool.range(*at_us..).next()) else { continue };
                if p.is_none() || bp.is_none() || ap.is_none() {
                    values.push(UnplacedValue { at_us: *at_us, part: p, sensor: *sensor, before: (before, bp), after: (after, ap) });
                }
            }
        }
        values
    }
}

/// `map`'s value at `ts`: linear between its readings before and after it, `None` past either end
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

/// The readings of the IMU, each sensor's on its own timeline as `normalized_imu_interpolated` times them, with where
/// each one came from
pub fn imu_readings(input: &crate::Input, orientation: Option<String>) -> Result<ImuReadings> {
    let mut first_timestamp = None;

    let accurate_ts = input.has_accurate_timestamps();

    let mut timestamp = if input.camera_type() == "GoPro" {
        crate::gopro::GoPro::imu_start_times_ms(input.samples.as_deref().unwrap_or_default())
    } else { (0.0, 0.0, 0.0) };

    let mut readings = ImuReadings::default();

    if let Some(ref samples) = input.samples {
        let mut reading_duration =
        if input.camera_type() == "GoPro" {
            (
                crate::gopro::GoPro::get_avg_sample_duration(samples, &GroupId::Gyroscope),
                crate::gopro::GoPro::get_avg_sample_duration(samples, &GroupId::Accelerometer),
                crate::gopro::GoPro::get_avg_sample_duration(samples, &GroupId::Magnetometer),
            )
        } else {
            // Each sensor's readings span the samples they came in, which `time_before_readings` advances the readings
            // past the time of any others
            let mut total_len = (0, 0, 0);
            let mut total_duration_ms = (0.0, 0.0, 0.0);
            for info in samples {
                let Some(ref grouped_tag_map) = info.tag_map else { continue };
                for (group, map) in grouped_tag_map {
                    let len = match map.get(&TagId::Data).map(|taginfo| &taginfo.value) {
                        Some(TagValue::Vec_Vector3_i16(arr)) => arr.get().len(),
                        Some(TagValue::Vec_Vector3_f32(arr)) => arr.get().len(),
                        _ => 0
                    };
                    if len == 0 { continue; }
                    match group {
                        GroupId::Gyroscope     => { total_len.0 += len; total_duration_ms.0 += info.duration_ms; },
                        GroupId::Accelerometer => { total_len.1 += len; total_duration_ms.1 += info.duration_ms; },
                        GroupId::Magnetometer  => { total_len.2 += len; total_duration_ms.2 += info.duration_ms; },
                        _ => {}
                    }
                }
            }
            (
                if total_len.0 > 0 { Some(total_duration_ms.0 / total_len.0 as f64) } else { None },
                if total_len.1 > 0 { Some(total_duration_ms.1 / total_len.1 as f64) } else { None },
                if total_len.2 > 0 { Some(total_duration_ms.2 / total_len.2 as f64) } else { None }
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

        // Each sensor's last sample of evenly spaced readings (`time_before_readings`). GoPro's timelines start where the
        // camera's own stamps put them (`GoPro::imu_start_times_ms`), the others at their first sample's time
        let mut last_sample: (Option<EvenlySpaced>, Option<EvenlySpaced>, Option<EvenlySpaced>) = (None, None, None);
        let is_gopro = input.camera_type() == "GoPro";

        for (sample_pos, info) in samples.iter().enumerate() {
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

                    // The video frame of each reading, where the format records it (`READING_FRAMES`)
                    let frames = map.get_t(TagId::Custom(READING_FRAMES.into())) as Option<&Vec<u64>>;
                    let origin = |j: usize| ReadingOrigin { sample: sample_pos, frame: frames.and_then(|f| f.get(j).copied()) };
                    if let Some(taginfo) = map.get(&TagId::Data) {
                        let evenly_spaced = match &taginfo.value {
                            TagValue::Vec_Vector3_i16(arr) => arr.get().len(),
                            TagValue::Vec_Vector3_f32(arr) => arr.get().len(),
                            _ => 0
                        };
                        if evenly_spaced > 0 {
                            let (timeline_ms, last, reading_ms) = match group {
                                GroupId::Gyroscope     => (&mut timestamp.0, &mut last_sample.0, reading_duration.0),
                                GroupId::Accelerometer => (&mut timestamp.1, &mut last_sample.1, reading_duration.1),
                                _                      => (&mut timestamp.2, &mut last_sample.2, reading_duration.2),
                            };
                            let sample = (info, if is_gopro { crate::gopro::GoPro::stamp_us(map) } else { None }, evenly_spaced);
                            match last.replace(sample) {
                                Some(previous) => *timeline_ms += time_before_readings(previous, sample, reading_ms.unwrap_or_default()),
                                None if !is_gopro => *timeline_ms = info.timestamp_ms,
                                None => {}
                            }
                        }
                        let sensor = match group {
                            GroupId::Gyroscope     => &mut readings.gyro,
                            GroupId::Accelerometer => &mut readings.accl,
                            _                      => &mut readings.magn,
                        };
                        match &taginfo.value {
                            // Sony and GoPro
                            TagValue::Vec_Vector3_i16(arr) => {
                                let arr = arr.get();

                                for (j, v) in arr.iter().enumerate() {
                                    let itm = v.clone().into_scaled(&raw2unit, &unit2deg).orient(io);
                                    let (timeline_ms, reading_ms) = match group {
                                        GroupId::Gyroscope     => (&mut timestamp.0, reading_duration.0),
                                        GroupId::Accelerometer => (&mut timestamp.1, reading_duration.1),
                                        _                      => (&mut timestamp.2, reading_duration.2),
                                    };
                                    sensor.insert((*timeline_ms * 1000.0f64).round() as i64, (itm, origin(j)));
                                    *timeline_ms += reading_ms.unwrap();
                                }
                            },
                            // Canon
                            TagValue::Vec_Vector3_f32(arr) => {
                                let arr = arr.get();

                                for (j, v) in arr.iter().enumerate() {
                                    let itm = v.clone().into_scaled(&raw2unit, &unit2deg).orient(io);
                                    let (timeline_ms, reading_ms) = match group {
                                        GroupId::Gyroscope     => (&mut timestamp.0, reading_duration.0),
                                        GroupId::Accelerometer => (&mut timestamp.1, reading_duration.1),
                                        _                      => (&mut timestamp.2, reading_duration.2),
                                    };
                                    sensor.insert((*timeline_ms * 1000.0f64).round() as i64, (itm, origin(j)));
                                    *timeline_ms += reading_ms.unwrap();
                                }
                            },
                            TagValue::Vec_TimeVector3_f64(arr) => {
                                for (j, v) in arr.get().iter().enumerate() {
                                    let mut timestamp_ms = v.t * 1000.0;
                                    if !accurate_ts {
                                        if first_timestamp.is_none() {
                                            first_timestamp = Some(timestamp_ms);
                                        }
                                        timestamp_ms -= first_timestamp.unwrap();
                                    }

                                    let timestamp_us = (timestamp_ms * 1000.0).round() as i64;

                                    let itm = v.clone().into_scaled(&raw2unit, &unit2deg).orient(io);
                                    sensor.insert(timestamp_us, (itm, origin(j)));
                                }
                            },
                            _ => ()
                        }
                    }
                }
            }
        }
    }

    Ok(readings)
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

/// The samples of a track and the sum of their durations in the time-to-sample table, in the track's timescale
fn stts_samples_and_duration(track: &mp4parse::Track) -> Option<(u64, u64)> {
    let stts = track.stts.as_ref().filter(|stts| !stts.samples.is_empty())?;
    Some(stts.samples.iter().fold((0u64, 0u64), |(samples, duration), v| {
        (samples + v.sample_count as u64, duration + v.sample_count as u64 * v.sample_delta as u64)
    }))
}

/// The frame rate of a track: its samples over the sum of their durations in the time-to-sample table. That is the
/// average rate ffmpeg reads (`avg_frame_rate`, libavformat/mov.c) and the one MDK, Gyroflow's player, gives the video;
/// for a constant frame rate it's the nominal one. Not the media duration (`mdhd`): a muxer can count the composition
/// delay of B-frames into that (ffmpeg does), and a 900-frame 59.94 fps clip delayed by two frames reads as 59.81 fps
/// by it. Only a table that gives its samples no duration at all leaves the media duration to go by
pub fn get_fps_from_track(track: &mp4parse::Track) -> Option<f64> {
    let (samples, stts_duration) = stts_samples_and_duration(track)?;
    let timescale = track.timescale?;
    let duration = if stts_duration > 0 { stts_duration } else { track.duration?.0 };
    let duration_us = duration as f64 * 1000_000.0 / timescale.0 as f64;
    let us_per_frame = duration_us / samples as f64;
    Some(1000_000.0 / us_per_frame)
}

/// The duration of a track in its timescale: the media duration (`mdhd`), at most the sum of the sample durations, as
/// ffmpeg takes it (libavformat/mov.c). A muxer can count the composition delay of B-frames into the media duration,
/// which the samples don't play for
fn track_duration(track: &mp4parse::Track) -> Option<u64> {
    let stts_duration = stts_samples_and_duration(track).map(|(_, duration)| duration).filter(|duration| *duration > 0);
    match (track.duration.map(|d| d.0), stts_duration) {
        (Some(media), Some(samples)) => Some(media.min(samples)),
        (media, samples) => media.or(samples),
    }
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
    if let Some(d) = track_duration(track) {
        if let Some(ts) = track.timescale {
            duration_sec = d as f64 / ts.0 as f64;
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
        crate::sony::mxf::parse(stream, filesize, |_|(), Arc::new(AtomicBool::new(false)), Some(&mut md), &crate::InputOptions::default(), crate::sony::Sony::parse_metadata)?;
        return Ok(md);
    }

    // Special case for BRAW: the frame is the active area (`crop_size`, the track header includes the sensor border
    // around it), and the Blackmagic RAW SDK applies the camera's `rotation` when decoding (the default geometry,
    // used by Resolve and by Gyroflow's decoder), so a vertical clip decodes as a portrait frame with no rotation left
    let mut override_size = None;
    if memmem::find(&last16kb, b"Blackmagic Design").is_some() {
        let mut bmd = crate::blackmagic::BlackmagicBraw::default();
        if let Ok(md) = bmd.parse_meta(stream, filesize) {
            if let Some(size) = md.get("crop_size").and_then(|x| x.as_array()).filter(|x| x.len() == 2).and_then(|x| Some((x[0].as_f64()? as usize, x[1].as_f64()? as usize))) {
                let rotation = md.get("rotation").and_then(|x| x.as_u64()).unwrap_or_default();
                override_size = Some(if rotation == 90 || rotation == 270 { (size.1, size.0) } else { size });
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
/// The header of the box at the reader's position: its type, position and size, and the size of the header. The size
/// counts the header: it's the 32-bit one, or the 64-bit one after the type when that is 1. A size of 0 is a box that
/// extends to the end of the file (ISO/IEC 14496-12 §4.2), the last one. Any other size that no walk can step past
/// (`box_end`: smaller than the header, or ending past the largest stream position) is an error (`InvalidData`), so a
/// walk that goes from box to box by their sizes always moves forward
pub fn read_box<R: Read + Seek>(reader: &mut R) -> Result<(u32, u64, u64, i64)> {
    let pos = reader.stream_position()?;
    let size = reader.read_u32::<BigEndian>()?;
    let typ = reader.read_u32::<BigEndian>()?;
    let (size, header_size) = if size == 1 {
        (reader.read_u64::<BigEndian>()?, 16)
    } else if size == 0 {
        return Ok((typ, pos, 0, 8));
    } else {
        (size as u64, 8)
    };
    if box_end(pos, size, header_size).is_none() {
        return Err(Error::new(ErrorKind::InvalidData, format!("Box {} at {pos} has an invalid size {size}", String::from_utf8_lossy(&typ.to_be_bytes()))));
    }
    Ok((typ, pos, size, header_size as i64))
}

/// The data of the box whose header `read_box` just read: its `size` bytes but the `header_size` of the header
/// (`read_data`). A box that runs past the end of the stream is an error (`UnexpectedEof`)
pub fn read_box_data<R: Read + Seek>(reader: &mut R, size: u64, header_size: i64) -> Result<Vec<u8>> {
    read_data(reader, size.saturating_sub(header_size as u64))
}

/// The next `len` bytes of the stream, for a length the file gives: fewer is an error (`UnexpectedEof`). They are read
/// into a buffer of that size, or of the rest of the stream where that is shorter, so a damaged length allocates no
/// more than the stream holds, and a whole box is read in one allocation
pub fn read_data<R: Read + Seek>(reader: &mut R, len: u64) -> Result<Vec<u8>> {
    let pos = reader.stream_position()?;
    let rest = reader.seek(SeekFrom::End(0))?.saturating_sub(pos);
    reader.seek(SeekFrom::Start(pos))?;
    let mut data = Vec::new();
    usize::try_from(len.min(rest)).ok().and_then(|n| data.try_reserve_exact(n).ok()).ok_or(ErrorKind::OutOfMemory)?;
    if reader.by_ref().take(len).read_to_end(&mut data)? as u64 != len {
        return Err(ErrorKind::UnexpectedEof.into());
    }
    Ok(data)
}

/// The data of the first box of type `typ` among the boxes that fill `data`, which is a box's data. A box of size 0
/// extends to the end of `data` (the end of the box it is in, as FFmpeg reads it), and the search ends at a box it can't
/// step past (`box_end`) or that runs past the end of `data`
pub fn find_box_data<'a>(data: &'a [u8], typ: &[u8; 4]) -> Option<&'a [u8]> {
    let mut pos = 0;
    while let Some(mut header) = data.get(pos..pos + 8) {
        let (size, header_size) = match header.read_u32::<BigEndian>().ok()? {
            0 => ((data.len() - pos) as u64, 8),
            1 => (data.get(pos + 8..).and_then(|mut x| x.read_u64::<BigEndian>().ok())?, 16),
            size => (size as u64, 8)
        };
        let end = box_end(pos as u64, size, header_size).filter(|end| *end <= data.len() as u64)? as usize;
        if &data[pos + 4..pos + 8] == typ {
            return Some(&data[pos + header_size as usize..end]);
        }
        pos = end;
    }
    None
}

fn trim_sequence_suffix(stem: &str) -> Option<&str> {
    let stem_without_digits = stem.trim_end_matches(|c: char| c.is_ascii_digit());
    if stem_without_digits.len() == stem.len() {
        return None;
    }
    let sequence_base = stem_without_digits.trim_end_matches(|c: char| c == '_' || c == '-' || c == '.');
    if sequence_base.is_empty() {
        None
    } else {
        Some(sequence_base)
    }
}

pub fn find_gcsv_sidecar_for_image_sequence(path: &str, ext: Option<&str>) -> Option<String> {
    let ext = ext?;
    if !matches!(ext, "dng" | "exr" | "jpg" | "jpeg" | "png") {
        return None;
    }

    // First, check sidecar with the same basename (for names like 0000.ext -> 0000.gcsv).
    for try_ext in ["gcsv", "GCSV"] {
        if let Some(sidecar_path) = crate::filesystem::file_with_extension(path, try_ext) {
            return Some(sidecar_path);
        }
    }

    // Then check sequence sidecar stripped from frame index (for names like clip_00001.ext -> clip.gcsv).
    let filename = crate::filesystem::get_filename(path);
    let stem = filename.rsplit_once('.')?.0;
    let sequence_base = trim_sequence_suffix(stem)?;
    let sequence_base_path = std::path::Path::new(path)
        .with_file_name(format!("{sequence_base}.{ext}"))
        .to_string_lossy()
        .to_string();
    for try_ext in ["gcsv", "GCSV"] {
        if let Some(sidecar_path) = crate::filesystem::file_with_extension(&sequence_base_path, try_ext) {
            return Some(sidecar_path);
        }
    }
    None
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A version `version` mdhd box of timescale `timescale`, and the hdlr box after it, named as GoPro names its tracks
    fn mdhd_hdlr(version: u8, timescale: u32, name: &str) -> Vec<u8> {
        let dates = if version == 1 { 16 } else { 8 };
        let mut mdhd = vec![version, 0, 0, 0];
        mdhd.extend(std::iter::repeat_n(0x11, dates));
        mdhd.extend(timescale.to_be_bytes());
        mdhd.extend(std::iter::repeat_n(0x22, dates / 2)); // The duration
        mdhd.extend([0x55, 0xC4, 0, 0]); // The language and pre_defined
        let mut hdlr = vec![0; 8]; // The version, flags and pre_defined
        hdlr.extend(b"meta");
        hdlr.extend([0; 12]);
        hdlr.push(name.len() as u8);
        hdlr.extend(name.as_bytes());
        let boxed = |typ: &[u8], body: Vec<u8>| [&(body.len() as u32 + 8).to_be_bytes()[..], typ, &body].concat();
        [boxed(b"mdhd", mdhd), boxed(b"hdlr", hdlr)].concat()
    }

    #[test]
    fn a_zero_mdhd_timescale_is_patched_in_either_version() {
        let prefix = &b"bytes that read mdhd but aren't a box"[..];
        for version in [0, 1] {
            let mut data = [prefix, &mdhd_hdlr(version, 0, "GoPro MET")].concat();
            patch_mdhd_timescale(&mut data);
            assert_eq!(data, [prefix, &mdhd_hdlr(version, 1000, "GoPro MET")].concat());
        }
        let mut other = mdhd_hdlr(1, 0, "GoPro TCD");
        patch_mdhd_timescale(&mut other);
        assert_eq!(other, mdhd_hdlr(1, 0, "GoPro TCD"));
    }
}