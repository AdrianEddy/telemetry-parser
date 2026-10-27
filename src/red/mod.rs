// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2022 Adrian <adrian.eddy at gmail>

use std::io::*;
use std::sync::{ Arc, atomic::AtomicBool };
use std::collections::BTreeMap;

use crate::tags_impl::*;
use crate::*;
use byteorder::{ ReadBytesExt, BigEndian, ByteOrder };

#[derive(Default)]
pub struct RedR3d {
    pub model: Option<String>,
    sensor_name: Option<String>,
    record_framerate: Option<f64>,
    all_parts: Vec<String>,
    cooke_error_logged: bool,
}

/// The atoms of a part of a clip its data is read from, in the part's order
struct PartAtoms {
    /// The frame atoms (offset, size), each a frame of the clip
    frames: Vec<(u64, u64)>,
    /// The data atoms (name, offset, size)
    data: Vec<([u8; 4], u64, u64)>,
    /// Whether they were found through the part's index, else by following its atoms
    indexed: bool,
    /// The camera's number of the part's first frame, from 1 through the clip's parts, by the index
    first_frame_number: Option<u32>,
}

/// The camera clock of the frames read, as far as their timestamps establish it, by the rules of R3D frame clock. A
/// frame's timestamp is the camera clock's time of it, but KOMODO firmware 1.7.0, 1.7.5 and 1.8.3 writes other bytes in the
/// field of single frames, ASCII text or a time seconds off, so it is the frame's time only where the frames around it say so:
/// - two frames of a stretch (below) agree when the time between their timestamps is the frames between them at the record frame
///   rate, to within half a frame;
/// - a timestamp is validated when it agrees with most of the up to five frames of its stretch around it, itself included: the
///   two before and the two after it, more on one side at the end of a stretch. One or two damaged frames next to it can't
///   out-vote the clock, and two damaged frames that agree with each other don't validate each other;
/// - the time of a frame is its own timestamp when that agrees with the nearest validated frame of its stretch, the later one on
///   a tie, and otherwise that frame's timestamp moved by the frames between them at the record frame rate. In a stretch
///   without a validated frame it's the frame's own timestamp.
///
/// A stretch is the frames read from one where the camera's numbers jump to the next, but for a jump where the timestamps of the
/// frames on either side agree: then the numbers are off, not the clock. Frames on either side of missing ones can't vouch for
/// each other's time
struct FrameClock<'a> {
    timestamps: &'a [u64],
    /// The frames' numbers in the clip
    indices: Vec<u64>,
    fps: Option<f64>,
    /// Where each stretch after the first starts, by the frames read before it
    breaks: Vec<usize>,
}

/// The time of frame `frame` read on the camera clock, in µs, and the frame read it's taken from ([`FrameClock::time`])
#[derive(Clone, Copy)]
struct FrameTime { us: f64, frame: usize, from: usize }

impl<'a> FrameClock<'a> {
    /// The clock of frames read with these timestamps and numbers in the clip, at the record frame rate `fps`, where the camera's
    /// numbers jump before the frames `jumps` (by the frames read before them)
    fn new(timestamps: &'a [u64], indices: Vec<u64>, fps: Option<f64>, jumps: &[(usize, String, i64)]) -> Self {
        let mut clock = Self { timestamps, indices, fps, breaks: Vec::new() };
        clock.breaks = jumps.iter().map(|jump| jump.0).filter(|&j| !clock.agree(j - 1, j)).collect();
        clock
    }

    /// Whether the timestamps of frames `a` and `b` read, `a` first, agree
    fn agree(&self, a: usize, b: usize) -> bool {
        let (ta, tb) = (self.timestamps[a], self.timestamps[b]);
        self.fps.is_some_and(|fps| tb > ta && ((tb - ta) as f64 * fps / 1000000.0).round() as u64 == self.indices[b] - self.indices[a])
    }

    /// The frames read of the stretch of frame `j`
    fn stretch(&self, j: usize) -> std::ops::Range<usize> {
        let k = self.breaks.partition_point(|&start| start <= j);
        k.checked_sub(1).map_or(0, |k| self.breaks[k])..self.breaks.get(k).copied().unwrap_or(self.timestamps.len())
    }

    /// Whether the timestamp of frame `j` is validated
    fn validated(&self, j: usize) -> bool {
        let stretch = self.stretch(j);
        let mut others = Vec::with_capacity(4);
        for d in 1..stretch.len() {
            others.extend([j + d, j.wrapping_sub(d)].into_iter().filter(|c| stretch.contains(c)));
            if others.len() >= 4 { break; }
        }
        others.truncate(4);
        let agreeing = others.iter().filter(|&&c| self.agree(c.min(j), c.max(j))).count();
        !others.is_empty() && 2 * (1 + agreeing) > 1 + others.len()
    }

    /// The time of frame `j` read
    fn time(&self, j: usize) -> FrameTime {
        let own = FrameTime { us: self.timestamps[j] as f64, frame: j, from: j };
        let Some(fps) = self.fps else { return own };
        let stretch = self.stretch(j);
        for d in 0..stretch.len() {
            for c in [j + d, j.wrapping_sub(d)].into_iter().take(if d == 0 { 1 } else { 2 }) {
                if !stretch.contains(&c) || !self.validated(c) { continue; }
                if c == j || self.agree(c.min(j), c.max(j)) { return own; }
                return FrameTime { us: self.timestamps[c] as f64 + (self.indices[j] as f64 - self.indices[c] as f64) * 1000000.0 / fps, frame: j, from: c };
            }
        }
        own
    }
}

impl RedR3d {
    pub fn camera_type(&self) -> String {
        if self.model.is_some() {
            "RED".to_owned()
        } else {
            "RED RAW".to_owned()
        }
    }
    pub fn has_accurate_timestamps(&self) -> bool {
        true
    }
    pub fn possible_extensions() -> Vec<&'static str> {
        vec!["r3d", "mp4", "mov", "mxf"]
    }
    pub fn frame_readout_time(&self) -> Option<f64> {
        None
    }
    pub fn normalize_imu_orientation(v: String) -> String {
        v
    }

    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], filepath: P, options: &crate::InputOptions) -> Option<Self> {
        let path = filepath.as_ref().to_str().unwrap_or_default().to_owned();

        let ext = filesystem::get_extension(&path);
        if ext != "r3d" && !options.dont_look_for_sidecar_files {
            if let Some(p) = filesystem::file_with_extension(&path, "R3D") {
                return Some(Self {
                    all_parts: Self::detect_all_parts(&p).unwrap_or_default(),
                    ..Default::default()
                })
            }
            if let Some(p) = filesystem::file_with_extension(&path, "") {
                let all_parts = Self::detect_all_parts(&p).unwrap_or_default();
                if all_parts.is_empty() { return None; }
                return Some(Self { all_parts, ..Default::default() });
            }
            return None;
        }
        if buffer.len() > 8 && &buffer[4..8] == b"RED2" {
            Some(Self {
                all_parts: Self::detect_all_parts(&path).unwrap_or_default(),
                ..Default::default()
            })
        } else {
            None
        }
    }

    /// The files of the clip the R3D file `path` is part of, as the R3D SDK opens the clip: its RMD sidecar, then its parts in
    /// order. The parts of a clip are `<clip>_<NNN>.R3D` in one folder, NNN being the part number in 3 digits (R3DSDK.h:
    /// "A001_C001_1231XY_001...999.R3D"). The SDK opens the part it's given, then the parts below it down to part 001, and
    /// part 000 when the part given is 001, then the parts above it, each time until one is missing, by the name of the part
    /// given with the number changed. Any other file is a clip of its own, e.g. a copy `<clip>_001 - Copy.R3D`. The sidecar
    /// is `<clip>.RMD`, or `<name>.RMD` for a file that isn't a part (`Clip::GetRmdPath`). Established with R3D SDK 9.2 and
    /// REDline (R3D SDK 8.3) on clips with parts missing, added, renamed and copied
    fn detect_all_parts(path: &str) -> Result<Vec<String>> {
        let filename = filesystem::get_filename(path);
        let Some(stem) = filename.len().checked_sub(4).filter(|&i| filename.get(i..).is_some_and(|ext| ext.eq_ignore_ascii_case(".r3d"))).map(|i| &filename[..i]) else {
            return Ok(Vec::new());
        };
        let ext = &filename[stem.len()..];
        let files = filesystem::list_folder(&filesystem::get_folder(path));
        if files.is_empty() {
            log::warn!("Failed to read directory of file {path}");
        }

        let part = stem.rsplit_once('_')
            .filter(|(clip, number)| !clip.is_empty() && number.len() == 3 && number.bytes().all(|x| x.is_ascii_digit()))
            .and_then(|(clip, number)| Some((clip, number.parse::<u32>().ok()?)));
        // The clip's name, and the tail of the file name the SDK changes for the names of the clip's other files: the part
        // number and the extension, or the extension of a file that isn't a part
        let clip = part.map_or(stem, |(clip, _)| clip);
        let tail = &filename[clip.len()..];
        // The file the SDK opens by the clip's name and this tail. It changes the tail of the path it's given, so the file
        // system decides whether a name differing in case is the same file, as it is on Windows, most macOS volumes and exFAT
        // media. The file of exactly this name is the one; a file whose name differs only in case is the one where the file
        // system opens it by this name. A path that doesn't end with its file name (an opaque URL) has no such tail to change
        let file = |new_tail: &str| -> Option<String> {
            let name = format!("{clip}{new_tail}");
            if let Some(x) = files.iter().find(|x| x.0 == name) {
                return Some(x.1.clone());
            }
            let name = name.to_lowercase();
            if !files.iter().any(|x| x.0.to_lowercase() == name) {
                return None;
            }
            let path = format!("{}{new_tail}", path.strip_suffix(tail)?);
            filesystem::open_file(&path).is_ok().then_some(path)
        };
        let Some((_, number)) = part else {
            let mut ret: Vec<String> = file(".RMD").into_iter().collect();
            ret.push(path.to_owned());
            return Ok(ret);
        };
        let part_tail = |n: u32| format!("_{n:03}{ext}");
        let mut below = Vec::new();
        let mut n = number;
        while n > 1 {
            let Some(p) = file(&part_tail(n - 1)) else { break };
            below.push(p);
            n -= 1;
        }
        if number == 1 {
            below.extend(file(&part_tail(0)));
        }
        let mut ret: Vec<String> = file(".RMD").into_iter().collect();
        ret.extend(below.into_iter().rev());
        ret.push(path.to_owned());
        let mut n = number;
        while let Some(p) = file(&part_tail(n + 1)) {
            ret.push(p);
            n += 1;
        }
        Ok(ret)
    }
    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, _stream: &mut T, _size: usize, progress_cb: F, cancel_flag: Arc<AtomicBool>, options: crate::InputOptions) -> Result<Vec<SampleInfo>> {
        // The IMU samples and the camera timestamp of each, by which they're timed once all the clip's frames are read
        let mut gyro: Vec<TimeVector3<f64>> = Vec::new();
        let mut accl: Vec<TimeVector3<f64>> = Vec::new();
        let mut imu_timestamps: Vec<u64> = Vec::new();
        // The camera time of the clip's time 0 for the frames read: µs on the clock of the frame timestamps and the IMU
        // samples, up to where the clip's time is joined after missing frames (`junctions`)
        let mut time_base = None;
        // The last frame read: its number in the clip and the camera's number of it (from 1, through the clip's parts)
        let mut last_frame: Option<(u64, u32)> = None;
        // Whether frames can be missing before the next frame read: after frames found by following the atoms of a part
        let mut check_continuity = false;
        // Where the camera's numbers of the frames read jump where frames can be missing: the frame, by the frames read before
        // it, the part it's in, and the frames missing by the numbers (fewer than none where they go back)
        let mut number_jumps: Vec<(usize, String, i64)> = Vec::new();
        // Where the clip's time is joined after missing frames, in the clip's order: the time base before and after, and the
        // time of the clip half a frame before the frame after the missing ones, where the time base after starts
        let mut junctions: Vec<(u64, u64, f64)> = Vec::new();
        // How many frames were read before the first junction: those timed by the clip's first time base
        let mut frames_before_junction = None;
        let mut frame_index = 0u64;
        let mut frame_timestamps = Vec::new();
        let mut map = GroupedTagMap::new();
        let mut samples = Vec::new();

        let all_parts = self.all_parts.clone();
        let mut atom_head = [0u8; 8];
        let mut rdi_head = Vec::new();
        // A data atom is read in parts of this size, of whole IMU records
        let mut chunk = vec![0u8; 3276 * 20];
        let mut clip_header_parsed = false;

        let mut csv = String::new();
        // The RMD sidecar's values, merged into the clip metadata in key order
        let mut rmd = BTreeMap::<String, String>::new();

        let total_count = all_parts.len() as f64;

        'files: for (i, path) in all_parts.into_iter().enumerate() {
            let ext = filesystem::get_extension(path.as_str());
            if ext == "rmd" {
                rmd.extend(Self::parse_rmd(&path));
                continue;
            }

            let mut file = filesystem::open_file(&path)?;
            let stream = &mut *file.file;
            let filesize = stream.seek(SeekFrom::End(0))?;

            // A part starts with its header. The R3D SDK ends the clip before a file that doesn't, while a part cut short
            // anywhere after its header's name is still a part of the clip
            let header_size = if filesize >= 8 {
                Self::read_at(stream, 0, &mut atom_head)?;
                BigEndian::read_u32(&atom_head) as u64
            } else {
                0
            };
            if header_size < 8 || &atom_head[4..] != b"RED2" {
                log::warn!("RED: {path} doesn't start with a RED2 header, the clip ends before it");
                break 'files;
            }
            // The bytes of the header its metadata can be in: its descriptors, up to 255 of each kind, and a metadata block of
            // up to 64 KB
            let mut header = vec![0u8; header_size.min(filesize).min(0x44 + 255 * (24 + 20 + 16) + 2 + 0xffff) as usize];
            Self::read_at(stream, 0, &mut header)?;
            // The clip's metadata is the header of its first part, as in the RED SDK's clip metadata. The headers of the
            // next parts describe those parts: their file name, start timecodes and lens state
            if !clip_header_parsed {
                match Self::header_metadata(&header) {
                    Ok(metadata) => {
                        let _ = self.parse_meta(metadata, &mut map, &options);
                        clip_header_parsed = true;
                    },
                    Err(e) => log::warn!("RED: {e} in the RED2 header of {path}"),
                }
            }
            if options.probe_only {
                break 'files;
            }

            // The part's frames and data atoms, through its index as the R3D SDK finds them, else, in a part cut short or
            // damaged, by following its atoms as the SDK then does
            let header_end = header_size.div_ceil(4096) * 4096;
            let atoms = match Self::indexed_atoms(stream, filesize, header_end)? {
                Ok(atoms) => atoms,
                Err(why) => Self::walked_atoms(stream, filesize, header_end, &path, &why)?,
            };
            check_continuity |= !atoms.indexed;
            let items = (atoms.frames.len() + atoms.data.len()).max(1) as f64;

            for (k, &(offset, size)) in atoms.frames.iter().enumerate() {
                if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) { break 'files; }
                progress_cb((i as f64 + k as f64 / items) / total_count);
                let index = frame_index + k as u64;
                let atom_data_len = (size - 8) as usize;
                if atom_data_len < 88 {
                    // It's still a frame of the clip: the R3D SDK finds and numbers the frames by the clip's index of them,
                    // not by their atoms, so the frames after it keep their number
                    log::warn!("RED: the atom of frame {index} in {path}, {size} bytes, is too small for its header, the frame is skipped");
                    continue;
                }
                // The frame header and the per-frame metadata, which fit in the first 4 KB of the atom when the metadata
                // is shorter than 4008 bytes
                rdi_head.resize(atom_data_len.min(4096), 0);
                stream.seek(SeekFrom::Start(offset))?;
                stream.read_exact(&mut atom_head)?;
                stream.read_exact(&mut rdi_head)?;
                let frame_timestamp = (&rdi_head[56..]).read_u64::<BigEndian>()?;
                let frame_number = BigEndian::read_u32(&rdi_head[68..]);
                if atoms.indexed {
                    // The index's entry is checked against the atom it gives: a frame atom, or the frame of this number
                    let expected = atoms.first_frame_number.and_then(|first| first.checked_add(k as u32));
                    if &atom_head[4..] != b"RDI\x01" && expected != Some(frame_number) {
                        log::warn!("RED: the index of {path} has frame {index} at 0x{offset:x}, where there's neither a frame atom nor its frame number, the frame is skipped");
                        continue;
                    }
                    let atom_size = BigEndian::read_u32(&atom_head) as u64;
                    if atom_size != size {
                        log::warn!("RED: the atom of frame {index} in {path} has a size of {atom_size} bytes, its index {size}: the index's is used, as in the R3D SDK");
                    }
                }
                let metadata_end = 88 + (&rdi_head[86..]).read_u16::<BigEndian>()? as usize;
                if metadata_end > rdi_head.len() && metadata_end <= atom_data_len {
                    let read = rdi_head.len();
                    rdi_head.resize(metadata_end, 0);
                    stream.read_exact(&mut rdi_head[read..])?;
                }
                // Frames before this one can be missing from a part whose atoms were followed, e.g. the part before cut short
                // (which the R3D SDK follows with the next part), as the camera's frame numbers and its clock tell. The clock
                // is that of the frames around this one and the frame before, so it's read once all frames are
                if check_continuity {
                    if let Some((index_before, number_before)) = last_frame {
                        let missing = frame_number as i64 - number_before as i64 - (index - index_before) as i64;
                        if missing != 0 {
                            number_jumps.push((samples.len(), path.clone(), missing));
                        }
                    }
                    check_continuity = !atoms.indexed;
                }
                if time_base.is_none() {
                    // The time of frame 0, so that frames read after one that can't be read keep their time too. Whether this
                    // frame's timestamp can be the clip's time base is checked once the frames after it are read
                    let frames_before_us = self.record_framerate.filter(|fps| *fps > 0.0).map(|fps| (index as f64 * 1000000.0 / fps).round() as u64).unwrap_or_default();
                    time_base = Some(frame_timestamp.saturating_sub(frames_before_us));
                }
                let mut per_frame_map = GroupedTagMap::new();
                util::insert_tag(&mut per_frame_map, tag!(parsed GroupId::Default, TagId::TimestampUs, "Frame timestamp", u64, |v| v.to_string(), frame_timestamp, vec![]), &options);
                if metadata_end <= rdi_head.len() {
                    let _ = self.parse_meta(&rdi_head[88..metadata_end], &mut per_frame_map, &options);
                } else {
                    log::warn!("RED: the metadata of frame {index} in {path} ends after its atom");
                }
                // The per-frame metadata (lens focal length, exposure) is keyed by the frame time relative to the
                // first frame, the same time base the IMU samples below use, so a zoom lens is seen frame by frame. The
                // difference is taken in i128, which holds that of any two timestamps, a damaged one too
                let timestamp_ms = (frame_timestamp as i128 - time_base.unwrap() as i128) as f64 / 1000.0;
                samples.push(SampleInfo { sample_index: index, timestamp_ms, tag_map: Some(per_frame_map), ..Default::default() });
                frame_timestamps.push(frame_timestamp);
                last_frame = Some((index, frame_number));
            }
            frame_index += atoms.frames.len() as u64;

            // The data atoms after a 4 KB header: CSV text in RDX\x01, the IMU in RDX\x02, records of a timestamp (u64, µs on
            // the clock of the frame timestamps), the acceleration and the rotation (i16 x, y, z each). An atom cut short
            // gives the complete records the file has of it, as in the R3D SDK. The atom's header is checked before its
            // contents are read, and these are read in parts up to where they end, so that a damaged size reads no garbage
            // and takes no more memory than a part
            for (k, &(name, offset, size)) in atoms.data.iter().enumerate() {
                if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) { break 'files; }
                progress_cb((i as f64 + (atoms.frames.len() + k) as f64 / items) / total_count);
                let end = (offset + size).min(filesize);
                if end <= offset + 4096 {
                    continue;
                }
                Self::read_at(stream, offset, &mut atom_head)?;
                if atom_head[4..] != name {
                    log::warn!("RED: the index of {path} has a {} atom at 0x{offset:x}, where there's none, it's skipped", name.escape_ascii());
                    continue;
                }
                let atom_size = BigEndian::read_u32(&atom_head) as u64;
                if atom_size != size {
                    log::warn!("RED: the {} atom at 0x{offset:x} in {path} has a size of {atom_size} bytes, its index {size}: the index's is used, as in the R3D SDK", name.escape_ascii());
                }
                let mut pos = offset + 4096;
                if &name == b"RDX\x01" {
                    // The text up to a NUL or a byte that isn't UTF-8, which only a damaged atom has: the camera's text is
                    // ASCII, and zeros follow it in the atom's last 4 KB. A character the part read ends in is read again
                    // with the next part
                    let mut carry = 0;
                    while pos < end {
                        let n = (end - pos).min((chunk.len() - carry) as u64) as usize;
                        Self::read_at(stream, pos, &mut chunk[carry..carry + n])?;
                        let (start, read) = (pos - carry as u64, carry + n);
                        pos += n as u64;
                        let (text, error) = match std::str::from_utf8(&chunk[..read]) {
                            Ok(text) => (text, None),
                            Err(e) => (std::str::from_utf8(&chunk[..e.valid_up_to()]).unwrap_or_default(), Some(e)),
                        };
                        if let Some(nul) = text.find('\0') {
                            csv.push_str(&text[..nul]);
                            log::warn!("RED: the {} atom at 0x{offset:x} in {path} has a NUL at 0x{:x}, its text ends there", name.escape_ascii(), start + nul as u64);
                            break;
                        }
                        csv.push_str(text);
                        match error {
                            None => carry = 0,
                            Some(e) if e.error_len().is_none() && pos < end => {
                                carry = read - e.valid_up_to();
                                chunk.copy_within(e.valid_up_to()..read, 0);
                            },
                            Some(e) => {
                                // A character the end of the file cuts is that of an atom cut short
                                if e.error_len().is_some() || end == offset + size {
                                    log::warn!("RED: the {} atom at 0x{offset:x} in {path} has a byte at 0x{:x} that isn't UTF-8 text, its text ends there", name.escape_ascii(), start + e.valid_up_to() as u64);
                                }
                                break;
                            },
                        }
                    }
                } else {
                    // The records up to one timed more than 1 s from the one before, which isn't one of the camera's. The
                    // camera's records of an atom follow each other by at most a few ms, not always forward: by 3.2 ms back
                    // to 0.8 ms on in the 157 atoms of the 110 files here, most with repeated timestamps. Where a damaged size
                    // runs past the records, the zeros after them read as the start of the camera's clock (the records here
                    // are 39 s or more after it), and the atoms after them as times far off. A record with the timestamp of
                    // the one before is that record written again, with the same values, and the camera's next record follows
                    // it by one regular step: all 9894 such records of 78 files. It's kept, as the R3D SDK's IMU samples keep
                    // it, and the samples by time keep one of them (`util::normalized_imu_interpolated`)
                    let mut timestamp_before: Option<u64> = None;
                    'records: while end - pos >= 20 {
                        let n = ((end - pos) / 20).min(chunk.len() as u64 / 20) as usize * 20;
                        Self::read_at(stream, pos, &mut chunk[..n])?;
                        for (j, record) in chunk[..n].as_chunks::<20>().0.iter().enumerate() {
                            let timestamp = BigEndian::read_u64(record);
                            if let Some(before) = timestamp_before.filter(|&before| timestamp.abs_diff(before) > 1000000) {
                                log::warn!("RED: the {} atom at 0x{offset:x} in {path} has a record at 0x{:x} timed {:.6} s from the one before, the atom's records end there", name.escape_ascii(), pos + j as u64 * 20, (timestamp as f64 - before as f64) / 1000000.0);
                                break 'records;
                            }
                            timestamp_before = Some(timestamp);
                            let value = |i: usize| BigEndian::read_i16(&record[8 + i * 2..]) as i32;
                            imu_timestamps.push(timestamp);
                            accl.push(TimeVector3 { t: 0.0, x: -value(0) as f64 / 100.0, y: -value(1) as f64 / 100.0, z: -value(2) as f64 / 100.0 });
                            gyro.push(TimeVector3 { t: 0.0, x: value(3) as f64 / 10.0, y: value(4) as f64 / 10.0, z: value(5) as f64 / 10.0 });
                        }
                        pos += n as u64;
                    }
                }
            }
        }
        let fps = self.record_framerate.filter(|fps| *fps > 0.0);
        // Where the camera's numbers say frames are missing, the camera clock tells how many, by the frames on either side
        // ([`FrameClock`]). Where it says as many as the numbers do, the clip's time is joined there: as in the SDK's video the
        // frame after the missing ones follows the one before them, the clip's time goes on from that one's, and the IMU
        // samples of the time missing are left out
        let clock = FrameClock::new(&frame_timestamps, samples.iter().map(|s| s.sample_index).collect(), fps, &number_jumps);
        if !number_jumps.is_empty() {
            let mut base_before = time_base.unwrap_or_default();
            let mut junction_frames = Vec::new();
            for (j, path, missing) in &number_jumps {
                let (j, missing) = (*j, *missing);
                let (index, index_before) = (samples[j].sample_index, samples[j - 1].sample_index);
                if missing <= 0 { continue; }
                let Some(fps) = fps else {
                    log::warn!("RED: by the frame numbers, {missing} frames of the clip are missing before frame {index} in {path}, but the clip has no record frame rate: the clip's time isn't changed there");
                    continue;
                };
                let frames_between = (index - index_before) as i64;
                let (before, now) = (clock.time(j - 1), clock.time(j));
                let missing_by_clock = match (frame_timestamps[j - 1], frame_timestamps[j]) {
                    // The clip has no timestamps: its frames are timed by their number
                    (0, 0) => Some(missing),
                    _ if now.us > before.us => Some(((now.us - before.us) * fps / 1000000.0).round() as i64 - frames_between),
                    _ => None,
                };
                if missing_by_clock != Some(missing) {
                    log::warn!("RED: by the frame numbers, {missing} frames of the clip are missing before frame {index} in {path}, but the camera clock of the frames around it doesn't agree: the clip's time isn't changed there");
                    continue;
                }
                for time in [before, now].iter().filter(|t| t.from != t.frame) {
                    log::warn!("RED: the timestamp of frame {} is off the camera clock of the frames around it, at the record frame rate, {fps} fps: its time is that of frame {}, moved by the frames between them", samples[time.frame].sample_index, samples[time.from].sample_index);
                }
                let base = frame_timestamps[now.from].saturating_sub((samples[now.from].sample_index as f64 * 1000000.0 / fps).round() as u64);
                frames_before_junction.get_or_insert(j);
                junctions.push((base_before, base, (index as f64 - 0.5) / fps));
                junction_frames.push(j);
                base_before = base;
                log::warn!("RED: {missing} frames of the clip are missing before frame {index} in {path}. As in the R3D SDK's video, it follows frame {index_before}, and the clip's time goes on from there");
            }
            // The frames read after a junction are timed by its time base
            for (k, &j) in junction_frames.iter().enumerate() {
                let end = junction_frames.get(k + 1).copied().unwrap_or(samples.len());
                for (sample, &timestamp) in samples[j..end].iter_mut().zip(&frame_timestamps[j..end]) {
                    sample.timestamp_ms = (timestamp as i128 - junctions[k].1 as i128) as f64 / 1000.0;
                }
            }
        }
        // The clip's first time base is that of its first frame read, back to frame 0 at the record frame rate, when this
        // frame's timestamp is the camera clock's: when the clock validates it ([`FrameClock`]), by the same rule as the
        // frames on either side of a junction. A damaged timestamp, or other bytes such as KOMODO firmware writes in the
        // field of single frames (ASCII text), would put the clip's time 0, and with it the IMU samples, anywhere, and two
        // damaged frames that agree with each other can't vouch for each other. The time base is then that of the first
        // frame read before the first junction that the clock validates, and the frames are timed by the record frame rate.
        // Without one (no timestamps, or no record frame rate) it stays
        let first_frames = frames_before_junction.unwrap_or(samples.len());
        let anchor = (0..first_frames).find(|&a| clock.validated(a));
        let rebased = match (anchor, fps) {
            (Some(a), Some(fps)) if a > 0 => {
                let base = frame_timestamps[a].saturating_sub((samples[a].sample_index as f64 * 1000000.0 / fps).round() as u64);
                match junctions.first_mut() {
                    Some(junction) => junction.0 = base,
                    None => time_base = Some(base),
                }
                log::warn!("RED: the timestamps of the frames read before frame {} don't agree with the camera clock of the frames after them, at the record frame rate, {fps} fps: the clip's time is that of frame {0}, back to frame 0 at that rate, and the frames are timed by the record frame rate", samples[a].sample_index);
                true
            },
            _ => false,
        };
        // Each IMU sample is timed by its own camera time, by the time base of the stretch of the clip that time is in: the
        // clip's first up to the time missing at its first junction, then each junction's after it. The samples of the time
        // missing at a junction, from half a frame after the frame before the missing frames to half a frame before the one
        // after them, are left out. They're timed once all junctions are known: a part's data atoms are read after its
        // frames, and its first samples can be seconds older than its first frame. A clip none of whose frames is read is
        // timed from its first sample. The time is the difference in i128, as the frames' is, so a damaged timestamp can't
        // overflow it. Each record's time is taken once, for both of its samples
        if let Some(&first) = imu_timestamps.first() {
            let first_base = junctions.first().map(|j| j.0).or(time_base).unwrap_or(first);
            let time = |timestamp: u64| -> Option<f64> {
                let t = |base: u64| (timestamp as i128 - base as i128) as f64 / 1000000.0;
                let stretch = junctions.iter().take_while(|&&(_, base, from)| t(base) >= from).count();
                let t = t(if stretch == 0 { first_base } else { junctions[stretch - 1].1 });
                (!junctions.get(stretch).is_some_and(|&(_, _, from)| t >= from)).then_some(t)
            };
            let mut kept = 0;
            for (i, &timestamp) in imu_timestamps.iter().enumerate() {
                if let Some(t) = time(timestamp) {
                    accl[kept] = TimeVector3 { t, ..accl[i] };
                    gyro[kept] = TimeVector3 { t, ..gyro[i] };
                    kept += 1;
                }
            }
            accl.truncate(kept);
            gyro.truncate(kept);
        }
        // The frames are timed by the camera's frame timestamps (the R3D SDK's per-frame `frame_timestamp`, in µs on the clock
        // of the IMU samples) when these increase from frame to frame and the first agrees with the camera clock (above), and
        // otherwise by the record frame rate, as the video is. DSMC2 bodies (RED RAVEN, DSMC2 DRAGON-X) and KOMODO firmware
        // 1.5.3 write no timestamp: the field is 0, and REDline lists no timestamp for these frames. KOMODO clips of firmware
        // 1.7.0, 1.7.5 and 1.8.3 have other bytes, or a time seconds off, in the field of single frames
        let unordered = frame_timestamps.windows(2).position(|w| w[0] >= w[1]);
        if unordered.is_some() || rebased {
            match fps {
                Some(fps) => {
                    if frame_timestamps.iter().all(|&t| t == 0) {
                        log::debug!("RED: the frames have no timestamps, they're timed by the record frame rate, {fps} fps");
                    } else if let Some(i) = unordered {
                        log::warn!("RED: the timestamp of frame {} doesn't follow the one before, the frames are timed by the record frame rate, {fps} fps", samples[i + 1].sample_index);
                    }
                    for sample in &mut samples {
                        sample.timestamp_ms = sample.sample_index as f64 * 1000.0 / fps;
                    }
                },
                None => log::warn!("RED: the frame timestamps don't increase and the clip has no record frame rate, the frames can't be timed"),
            }
        }
        if !csv.is_empty() {
            util::insert_tag(&mut map, tag!(parsed GroupId::Default,   TagId::Custom("CSV".into()), "Custom CSV data", String, |v| v.clone(), csv, vec![]), &options);
        }
        if !rmd.is_empty() {
            /*if let Some(Ok(fps)) = rmd.get("frame_rate_override").map(|x| x.parse::<f64>()) {
                self.record_framerate = Some(fps);
            }*/
            if let Some(v) = rmd.get("lens") {
                util::insert_tag(&mut map, tag!(parsed GroupId::Lens, TagId::Name, "Lens name", String, |v| v.clone(), v.into(), vec![]), &options);
            }
            crate::try_block!({
                if let TagValue::Json(ref mut md) = map.get_mut(&GroupId::Default)?.get_mut(&TagId::Metadata)?.value {
                    if let Some(md) = md.get_mut().as_object_mut() {
                        for (k, v) in std::mem::take(&mut rmd) {
                            if k == "fittype" {
                                if v.starts_with("Fit Width ") || v.starts_with("Fit Height ") {
                                    if let Ok(num) = v.replace("Fit Width ", "").replace("Fit Height ", "").replace("x", "").parse::<f64>() {
                                        if v.starts_with("Fit Width") {
                                            md.insert("horizontal_stretch".into(), num.into());
                                        } else {
                                            md.insert("vertical_stretch".into(), num.into());
                                        }
                                    }
                                }
                            } else {
                                md.insert(k, v.into());
                            }
                        }
                    }
                }
            });
        }

        // Try to get the sync data, if no async data present
        if accl.is_empty() && gyro.is_empty() && !samples.is_empty() {
            let frame_rate = self.record_framerate.filter(|fps| *fps > 0.0);
            for sample in &samples {
                if let Some(ref map) = sample.tag_map {
                    if let Some(g) = map.get(&GroupId::Default) {
                        if let Some(arr) = g.get_t(TagId::Metadata) as Option<&serde_json::Value> {
                            // One sample per frame, at its frame's time: its number at the record frame rate, as frames
                            // without timestamps are timed, or the frame's own time when the clip doesn't give the rate
                            let t = match frame_rate { Some(fps) => sample.sample_index as f64 / fps, None => sample.timestamp_ms / 1000.0 };
                            if let Some(camera_acceleration) = arr.get("camera_acceleration").and_then(|x| x.as_array()) {
                                if camera_acceleration.len() == 3 {
                                    accl.push(TimeVector3 { t,
                                        x: -camera_acceleration[0].as_f64().unwrap_or(0.0),
                                        y: -camera_acceleration[1].as_f64().unwrap_or(0.0),
                                        z: -camera_acceleration[2].as_f64().unwrap_or(0.0),
                                    });
                                }
                            }
                            if let Some(camera_rotation) = arr.get("camera_rotation").and_then(|x| x.as_array()) {
                                if camera_rotation.len() == 3 {
                                    gyro.push(TimeVector3 { t,
                                        x: camera_rotation[0].as_f64().unwrap_or(0.0),
                                        y: camera_rotation[1].as_f64().unwrap_or(0.0),
                                        z: camera_rotation[2].as_f64().unwrap_or(0.0)
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Data, "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), accl, vec![]), &options);
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Data, "Gyroscope data",     Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]), &options);

        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Unit, "Accelerometer unit", String, |v| v.to_string(), "m/s²".into(),  Vec::new()), &options);
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Unit, "Gyroscope unit",     String, |v| v.to_string(), "deg/s".into(), Vec::new()), &options);

        if let Some(fr) = self.record_framerate {
            util::insert_tag(&mut map, tag!(parsed GroupId::Default,   TagId::FrameRate, "Frame rate", f64, |v| format!("{:?}", v), fr, vec![]), &options);
        }

        let imu_orientation = "zyx";
        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()), &options);
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()), &options);

        samples.insert(0, SampleInfo { tag_map: Some(map), ..Default::default() });

        Ok(samples)
    }

    pub fn parse_meta(&mut self, mut data: &[u8], map: &mut GroupedTagMap, options: &crate::InputOptions) -> Result<()> {
        let mut md = serde_json::Map::<String, serde_json::Value>::new();
        while let Ok(size) = data.read_u16::<BigEndian>() {
            // An entry is its size (counting the size itself), type, id and value. A smaller size ends the metadata
            if size >= 4 {
                let Some(d) = data.get(..size as usize - 2) else {
                    log::warn!("RED: a metadata entry of {size} bytes runs past the end of its block, the entries after are lost");
                    break;
                };
                data = &data[d.len()..];
                let mut id = match d[1] {
                    0x06 => "camera_pin",
                    0x08 => "exposure_time",
                    0x0d => "white_balance_kelvin",
                    0x0e => "white_balance_tint",
                    0x0F => "saturation",
                    0x11 => "brightness",
                    0x13 => "contrast",
                    0x19 => "camera_id",
                    0x1A => "reel_id",
                    0x1B => "clip_id",
                    0x23 => "local_date",
                    0x24 => "local_time",
                    0x25 => "camera_firmware_version",
                    0x30 => "gmt_date",
                    0x31 => "gmt_time",
                    0x39 => "lens_cooke_i_static",
                    0x3A => "lens_cooke_i_dynamic",
                    0x3b => "iso",
                    0x56 => "file_name",
                    0x65 => "firmware_revision",
                    0x66 => "record_framerate",
                    0x6B => "focal_length",
                    0x6C => "focus_distance",
                    0x74 => "lens_focus_distance_near",
                    0x75 => "lens_focus_distance_far",
                    0x6E => "lens_brand",
                    0x70 => "lens_name",
                    0x71 => "camera_network_name",
                    0x76 => "user_production_name",
                    0x77 => "user_director",
                    0x78 => "user_director_of_photography",
                    0x79 => "user_copyright",
                    0x7A => "user_unit",
                    0x7B => "user_location",
                    0x7C => "user_camera_operator",
                    0x7D => "user_scene",
                    0x7E => "user_take",
                    0x7F => "camera_acceleration", // x/y/z
                    0x80 => "camera_rotation", // x/y/z
                    0x86 => "resolution_format_name",
                    0x9D => "lens_serial_number",
                    0x9E => "lens_owner",
                    0xA0 => "camera_model",
                    0xA1 => "sensor_name",
                    0xAB => "3d_lut1",
                    0xB0 => "fps", // / 1001
                    0xBE => "redcode",
                    0xBF => "record_fps", // / 1001
                    0xC1 => "3d_lut2",
                    0xD0 => "lens_focal_length_micrometers",
                    0xD2 => "frame_ptp_timestamp",
                    _ => "",
                }.to_string();
                if id.is_empty() { id = format!("0x{:x}", d[1]); };

                let num_items = match id.as_str() {
                    "camera_acceleration" => 3, // x/y/z
                    "camera_rotation"     => 3, // x/y/z
                    _ => 1,
                };
                if id.starts_with("lens_cooke") {
                    // The lens responses, as the camera received them from a Cooke /i lens, as far as they can be decoded: a field
                    // none of which can be decoded is left out. The problems are logged once per clip, they usually repeat in
                    // every frame
                    let responses = crate::cooke::bin::parse(&d[2..]);
                    for e in &responses.errors {
                        if !self.cooke_error_logged {
                            log::warn!("RED: {id}: {e}");
                            self.cooke_error_logged = true;
                        } else {
                            log::debug!("RED: {id}: {e}");
                        }
                    }
                    if !responses.records.is_empty() || responses.errors.is_empty() {
                        md.insert(id.clone(), responses.records.into());
                    }
                    continue;
                }
                //println!("Type: {}, id: {}", d[0], id);

                let value = &d[2..];
                let item = |i: usize| -> Option<serde_json::Value> {
                    let mut v = value.get(i * match d[0] { 0x20 | 0x60 => 4, 0x40 => 2, _ => 1 }..)?;
                    Some(match d[0] {
                        0x10 => Self::entry_str(value).into(),
                        0x20 => (v.read_f32::<BigEndian>().ok()? as f64).into(),
                        0x30 => v.read_u8().ok()?.into(),
                        0x40 => v.read_i16::<BigEndian>().ok()?.into(),
                        0x60 => v.read_u32::<BigEndian>().ok()?.into(),
                        // Other types aren't decoded, nor a value shorter than its type
                        _ => return None
                    })
                };
                let mut items = vec![];
                for i in 0..num_items {
                    if let Some(v) = item(i) {
                        if id == "camera_model" { self.model = v.as_str().map(|x| x.to_string()); }
                        if id == "sensor_name" { self.sensor_name = v.as_str().map(|x| x.to_string()); }
                        if id == "record_framerate" { self.record_framerate = v.as_f64(); }

                        items.push(v);
                        // log::debug!("{}: {:?}", id, v);
                    }
                }
                if id == "exposure_time" && items.len() == 1 {
                    if let Some(v) = items[0].as_f64() {
                        util::insert_tag(map, tag!(parsed GroupId::Default, TagId::ExposureTime, "Exposure time", f32, |v| format!("{v:.6}"), v as f32, vec![]), &options);
                    }
                }
                if items.len() == 1 {
                    md.insert(id.clone(), items.into_iter().next().unwrap());
                } else {
                    md.insert(id.clone(), serde_json::to_value(items)?);
                }
            } else {
                break;
            }
        }
        if !md.is_empty() {
            if let Some(v) = md.get("focal_length").and_then(|v| v.as_f64()) {
                util::insert_tag(map, tag!(parsed GroupId::Lens, TagId::FocalLength, "Focal length", f32, |v| format!("{v:.3}"), v as f32, vec![]), &options);
            }
            if let Some(v) = md.get("lens_name").and_then(|v| v.as_str()) {
                util::insert_tag(map, tag!(parsed GroupId::Lens, TagId::Name, "Lens name", String, |v| v.clone(), v.into(), vec![]), &options);
            }

            if let Some(pp) = Self::pixel_pitch(self.sensor_name.as_deref(), self.model.as_deref()) {
                util::insert_tag(map, tag!(parsed GroupId::Imager, TagId::PixelPitch, "Pixel pitch", u32x2, |v| format!("{v:?}"), (pp, pp), vec![]), &options);
            }

            util::insert_tag(map, tag!(parsed GroupId::Default, TagId::Metadata, "Metadata", Json, |v| serde_json::to_string(v).unwrap(), serde_json::Value::Object(md), vec![]), &options);
        }
        Ok(())
    }

    /// The text of a string (0x10) entry: its bytes up to the first NUL. The entry size bounds the string, and some cameras
    /// also count a C terminator in it, e.g. the V-RAPTOR XE writes its model as `V-RAPTOR XE 8K VV\0`
    fn entry_str(v: &[u8]) -> &str {
        let end = v.iter().position(|&c| c == 0).unwrap_or(v.len());
        std::str::from_utf8(&v[..end]).unwrap_or("")
    }

    /// The clip metadata block of a `RED2` header atom, `atom` being the whole atom. At 0x40 the header has the number of its
    /// video, audio and data stream descriptors (then a byte that is 1 in every clip seen), and from 0x44 on the descriptors:
    /// an `rdi` of 24 bytes per video stream, an `rda` of 20 bytes per audio stream and an `rdx` of 16 bytes per kind of RDX
    /// atom, each starting with its tag. The metadata block follows them: its size, a big-endian u16, and its entries.
    /// Established on 124 clips of the RED RAVEN, DSMC2 DRAGON-X, KOMODO 6K, KOMODO-X, V-RAPTOR 8K VV, V-RAPTOR [X] and
    /// V-RAPTOR XE, the counts are 1, 0 to 1 and 0 to 2. The RED SDK's clip metadata has the values of this block in the
    /// clip's first part (start timecodes, original file name, lens state), and those of the first frame for what it lacks
    fn header_metadata(atom: &[u8]) -> std::result::Result<&[u8], String> {
        const DESCRIPTORS: [(&[u8; 3], usize); 3] = [(b"rdi", 24), (b"rda", 20), (b"rdx", 16)];
        let counts = atom.get(0x40..0x43).ok_or("no stream descriptor counts")?;
        let mut pos = 0x44;
        for (&(tag, len), &count) in DESCRIPTORS.iter().zip(counts) {
            for _ in 0..count {
                if atom.get(pos..pos + 3) != Some(&tag[..]) {
                    return Err(format!("no {} stream descriptor at 0x{pos:x} (descriptor counts {counts:?})", String::from_utf8_lossy(tag)));
                }
                pos += len;
            }
        }
        let size = atom.get(pos..pos + 2).map(|v| u16::from_be_bytes([v[0], v[1]]) as usize).ok_or("no metadata block")?;
        atom.get(pos + 2..pos + 2 + size).ok_or_else(|| format!("the metadata block of {size} bytes at 0x{pos:x} runs past the end of the atom"))
    }

    /// The frames and data atoms of a part through its index, as the R3D SDK finds them. The part's last 4 KB are its
    /// footer: REOB in a clip of one file, REOF in a part another part follows, REOC in the clip's last part. It has its
    /// file's size (u64 at 0x40), its numbers of video, audio and data streams (bytes at 0x4C) and from 0x50 a descriptor of
    /// each stream, `rdi` of 24 bytes, `rda` of 40 and `rdx` of 24: its tag, then u32s, the 4 KB block of the stream's index
    /// atom, the size of its largest atom, the number of its atoms in the part, 0 and their number in the clip up to the end
    /// of the part (an `rda` then has the audio samples of the part and of the clip, u64 each). An index atom (RII frames,
    /// RIA audio, RIX\x01 and RIX\x02 data) has at 0x40 how many u32s an entry has after its first, and from 0x44 an entry
    /// per atom: its 4 KB block, a value only RED RAVEN and DSMC2 frame indexes have, and its size. The camera numbers the
    /// frames from 1 through the clip's parts, in their atoms (u32 at 0x4C). Established on the 124 R3D files here, of the
    /// RED RAVEN, DSMC2 DRAGON-X, KOMODO 6K, KOMODO-X, V-RAPTOR 8K VV, V-RAPTOR [X] and V-RAPTOR XE, in all of which the
    /// index has exactly the atoms that following the atoms' sizes finds, and with R3D SDK 9.2, which reads the frames where
    /// the index has them: a frame's own atom size isn't used. `Err` with what's wrong when the part has no footer or its
    /// index doesn't agree with itself and the file, as in a part cut short
    fn indexed_atoms<S: Read + Seek + ?Sized>(stream: &mut S, filesize: u64, header_end: u64) -> Result<std::result::Result<PartAtoms, String>> {
        let Some(footer_at) = filesize.checked_sub(4096).filter(|&at| at >= header_end) else {
            return Ok(Err("it's too short for a footer".into()));
        };
        let mut footer = vec![0u8; 4096];
        Self::read_at(stream, footer_at, &mut footer)?;
        if ![b"REOB", b"REOF", b"REOC"].iter().any(|x| footer[4..8] == x[..]) {
            return Ok(Err("its last 4 KB aren't a footer".into()));
        }
        let size = BigEndian::read_u32(&footer) as usize;
        let Some(footer) = footer.get(..size).filter(|_| size >= 0x50) else {
            return Ok(Err(format!("its footer has a size of {size} bytes")));
        };
        let footer_filesize = BigEndian::read_u64(&footer[0x40..]);
        if footer_filesize != filesize {
            return Ok(Err(format!("its footer is of a file of {footer_filesize} bytes")));
        }
        let mut frames = None;
        let mut data = Vec::new();
        let mut pos = 0x50;
        for (kind, len, count) in [(b"rdi", 24, footer[0x4C]), (b"rda", 40, footer[0x4D]), (b"rdx", 24, footer[0x4E])] {
            for _ in 0..count {
                let Some(d) = footer.get(pos..pos + len).filter(|d| d[..3] == kind[..]) else {
                    return Ok(Err(format!("its footer has no {} stream descriptor at 0x{pos:x}", kind.escape_ascii())));
                };
                pos += len;
                // The frames of the (first) video stream and the data atoms; the audio isn't read
                let (index_name, atom_name) = match &d[..4] {
                    b"rdi\x01" => (b"RII\x01", b"RDI\x01"),
                    b"rdx\x01" => (b"RIX\x01", b"RDX\x01"),
                    b"rdx\x02" => (b"RIX\x02", b"RDX\x02"),
                    _ => continue,
                };
                let (block, count, cumulative) = (BigEndian::read_u32(&d[4..]), BigEndian::read_u32(&d[12..]), BigEndian::read_u32(&d[20..]));
                // A data atom's size in the index is that of the contents read, so it's checked against the largest atom the
                // footer has of its stream, the largest in the clip up to the end of the part (in every part here). A frame's
                // isn't: only its header and metadata are read, where the index has the frame
                let largest = (atom_name != b"RDI\x01").then(|| BigEndian::read_u32(&d[8..]) as u64);
                let entries = match Self::index_entries(stream, footer_at, header_end, block, index_name, count, largest)? {
                    Ok(entries) => entries,
                    Err(e) => return Ok(Err(e)),
                };
                if atom_name == b"RDI\x01" {
                    if frames.is_none() {
                        frames = Some((entries, cumulative.checked_sub(count).and_then(|x| x.checked_add(1))));
                    }
                } else {
                    data.extend(entries.into_iter().map(|(offset, size)| (*atom_name, offset, size)));
                }
            }
        }
        let Some((frames, first_frame_number)) = frames else {
            return Ok(Err("its footer has no index of its frames".into()));
        };
        data.sort_by_key(|x| x.1);
        Ok(Ok(PartAtoms { frames, data, indexed: true, first_frame_number }))
    }

    /// The atoms (offset, size) an index atom of a part has, in the part's order, each after the header and the one before
    /// it and before the footer, and none larger than `largest` when given. `Err` with what's wrong when they aren't, or
    /// the index doesn't have the number of atoms the footer has
    fn index_entries<S: Read + Seek + ?Sized>(stream: &mut S, footer_at: u64, header_end: u64, block: u32, name: &[u8; 4], count: u32, largest: Option<u64>) -> Result<std::result::Result<Vec<(u64, u64)>, String>> {
        let at = block as u64 * 4096;
        let mut head = [0u8; 0x44];
        if at < header_end || at + head.len() as u64 > footer_at {
            return Ok(Err(format!("its footer has its {} index at 0x{at:x}, outside of the part", name.escape_ascii())));
        }
        Self::read_at(stream, at, &mut head)?;
        // The values of an entry: the atom's block, the values some indexes have besides, its size
        let width = 4 * (1 + head[0x40] as u64);
        let size = BigEndian::read_u32(&head) as u64;
        // An atom takes at least 4 KB of the part
        if head[4..8] != name[..] || width < 8 || count as u64 > (footer_at - header_end) / 4096 || size != head.len() as u64 + count as u64 * width || at + size > footer_at {
            return Ok(Err(format!("its {} index at 0x{at:x} isn't one of the {count} atoms its footer has", name.escape_ascii())));
        }
        let mut entries = vec![0u8; (size - head.len() as u64) as usize];
        Self::read_at(stream, at + head.len() as u64, &mut entries)?;
        let mut end = header_end;
        let mut ret = Vec::with_capacity(count as usize);
        for entry in entries.chunks_exact(width as usize) {
            let offset = BigEndian::read_u32(entry) as u64 * 4096;
            let size = BigEndian::read_u32(&entry[entry.len() - 4..]) as u64;
            if offset < end || size < 8 || offset + size.div_ceil(4096) * 4096 > footer_at {
                return Ok(Err(format!("its {} index has an atom of {size} bytes at 0x{offset:x}, which isn't in the part after the one before it", name.escape_ascii())));
            }
            if let Some(largest) = largest.filter(|&largest| size > largest) {
                return Ok(Err(format!("its {} index has an atom of {size} bytes at 0x{offset:x}, larger than the largest of its stream its footer has, {largest} bytes", name.escape_ascii())));
            }
            end = offset + size.div_ceil(4096) * 4096;
            ret.push((offset, size));
        }
        Ok(Ok(ret))
    }

    /// The frames and data atoms of a part by following its atoms by their sizes from its header on, as the R3D SDK does in
    /// a part without a usable index, such as one cut short: it skips the atoms it doesn't know, and stops at the end of
    /// the file or at an atom smaller than its header or going past the end of the file. A frame atom is a frame when the
    /// file goes on after it with an atom header, and a data atom gives the complete records the file has of it.
    /// Established with R3D SDK 9.2 on clips cut short in their frames, data atoms, index and footer, and damaged in an
    /// atom's size or name, its footer or its index
    fn walked_atoms<S: Read + Seek + ?Sized>(stream: &mut S, filesize: u64, header_end: u64, path: &str, why: &str) -> Result<PartAtoms> {
        let mut atoms = PartAtoms { frames: Vec::new(), data: Vec::new(), indexed: false, first_frame_number: None };
        let mut head = [0u8; 8];
        let mut pos = header_end;
        let end = loop {
            if pos + 8 > filesize {
                break if pos < filesize { format!("an atom header cut short at 0x{pos:x}") } else { "the end of the file".to_owned() };
            }
            Self::read_at(stream, pos, &mut head)?;
            let (size, name) = (BigEndian::read_u32(&head) as u64, [head[4], head[5], head[6], head[7]]);
            if size < 8 {
                break format!("an atom size of {size} at 0x{pos:x}, which isn't one");
            }
            let padded = size.div_ceil(4096) * 4096;
            match &name {
                b"RDI\x01" if pos + padded + 8 <= filesize => atoms.frames.push((pos, size)),
                b"RDX\x01" | b"RDX\x02" => atoms.data.push((name, pos, size)),
                _ => {}
            }
            if pos + size > filesize {
                break format!("its {} atom at 0x{pos:x} of {size} bytes, which goes past the end of the file", name.escape_ascii());
            }
            pos += padded;
        };
        log::warn!("RED: {path} has no usable index, {why}: it was cut short or is damaged. As the R3D SDK does, its atoms are followed from its header on: {} frames, up to {end}", atoms.frames.len());
        Ok(atoms)
    }

    fn read_at<S: Read + Seek + ?Sized>(stream: &mut S, offset: u64, buf: &mut [u8]) -> Result<()> {
        stream.seek(SeekFrom::Start(offset))?;
        stream.read_exact(buf)
    }

    /// The pixel pitch of the sensor in nanometres (square pixels), by the sensor name the camera records or, in clips of
    /// firmware that records none (KOMODO 6K before 1.7.3, V-RAPTOR 8K VV 1.3.4), by the camera model. The names of the
    /// sensors without a clip here are those of the R3D SDK: its library (9.2) lists the sensor names it gives clips that
    /// record a sensor ID only, and R3DSDKMetadata.h has "HELIUM 8K S35" as an example sensor name. The names the RED RAVEN
    /// and V-RAPTOR 8K VV clips here record, "DRAGON 4.5K S35" and "V-RAPTOR VV", are on that list too.
    ///
    /// RED's specifications give the sensor size and its effective pixels, "Technical Specifications" in each camera's
    /// operation guide; the pitch is their ratio, the sizes being rounded to their last digit
    fn pixel_pitch(sensor_name: Option<&str>, model: Option<&str>) -> Option<u32> {
        let pitch = match sensor_name {
            // V-RAPTOR 8K VV (955-0199 V1.2), V-RAPTOR [X] 8K VV (955-0225 V1.7), V-RAPTOR XL [X] 8K VV (955-0227 V1.7),
            // V-RAPTOR XE 8K VV (955-0233 V2.1): 40.96 mm × 21.60 mm, 8192 × 4320
            Some("V-RAPTOR VV" | "V-RAPTOR [X] VV" | "V-RAPTOR XE VV") => 5000,
            // V-RAPTOR 8K S35 (955-0215 V1.4): 26.21 mm × 13.82 mm, 8192 × 4320
            Some("V-RAPTOR S35") => 3200,
            // KOMODO 6K (955-0196 v1.6), KOMODO-X (955-0219 V1.0): 27.03 mm × 14.26 mm, 6144 × 3240
            Some("KOMODO S35" | "KOMODO-X S35") => 4400,
            // DSMC2 DRAGON-X 6K (955-0182 V7.4): 30.7 mm × 15.8 mm, 6144 × 3160
            Some("DRAGON-X 6K S35") => 5000,
            // RED RAVEN (955-0154 V7.1): 23.0 mm × 10.8 mm, 4608 × 2160
            Some("DRAGON 4.5K S35") => 5000,
            // DSMC2 MONSTRO (955-0170 V7.1): 40.96 mm × 21.60 mm, 8192 × 4320
            Some("MONSTRO 8K VV") => 5000,
            // DSMC2 HELIUM (955-0171 V7.4), WEAPON / EPIC-W with HELIUM 8K S35 (955-0155 V7.0): 29.90 mm × 15.77 mm, 8192 × 4320
            Some("HELIUM 8K S35") => 3650,
            // DSMC2 GEMINI (955-0172 V7.0): 30.72 mm × 18.00 mm, 5120 × 3000
            Some("GEMINI 5K S35") => 6000,
            // EPIC DRAGON, SCARLET DRAGON (955-0156 V6.3): 30.7 mm × 15.8 mm, 6144 × 3160
            Some("DRAGON 6K S35") => 5000,
            // EPIC MYSTERIUM-X (955-0156 V6.3): 27.7 mm × 14.6 mm, 5120 × 2700
            Some("MYSTERIUM-X 5K S35") => 5410,
            _ => match model? {
                "KOMODO 6K" | "KOMODO-X 6K S35" => 4400,
                "V-RAPTOR 8K VV" | "V-RAPTOR [X] 8K VV" | "V-RAPTOR XE 8K VV" => 5000,
                // V-RAPTOR 8K S35 (955-0215 V1.4): 26.21 mm × 13.82 mm, 8192 × 4320
                "V-RAPTOR 8K S35" => 3200,
                "Raven" | "RED RAVEN 4.5K" | "DSMC2 DRAGON-X 6K S35" => 5000,
                _ => return None
            }
        };
        Some(pitch)
    }

    fn parse_rmd(file: &str) -> BTreeMap<String, String> {
        let mut rmd = BTreeMap::<String, String>::new();
        if let Ok(contents) = filesystem::read_file(file) {
            let mut find = |name: &str, typ| {
                if let Some(v) = util::find_between(&contents, format!("<{} type=\"{}\" value=\"", name, typ).as_bytes(), b'"') {
                    if !v.is_empty() {
                        rmd.insert(name.to_string(), v
                            .replace("&quot;", "\"")
                            .replace("&amp;", "&")
                            .replace("&lt;", "<")
                            .replace("&gt;", ">")
                        );
                    }
                }
            };
            find("fittype", "string");
            find("unit", "string");
            find("location", "string");
            find("focal_length", "string");
            find("production_name", "string");
            find("aperture", "string");
            find("director", "string");
            find("camera_operator", "string");
            find("focus_distance", "string");
            find("copyright", "string");
            find("director_of_photography", "string");
            find("take", "string");
            find("lens", "string");
            find("scene", "string");
            find("shot", "string");
            find("label", "string");
            find("video_slate_position", "int");
            find("poster_frame", "int");
            find("added_r3d_markers", "bool");

            if let Some(n) = util::find_between(&contents, b"<frame_rate_override num=\"", b'"') {
                if let Some(d) = util::find_between(&contents, format!("<frame_rate_override num=\"{n}\" den=\"").as_bytes(), b'"') {
                    match (n.parse::<u32>(), d.parse::<u32>()) {
                        (Ok(n), Ok(d)) if n > 0 && d > 0 => { rmd.insert("frame_rate_override".into(), format!("{:.3}", n as f64 / d as f64)); }
                        _ => { }
                    }
                }
            }
        }

        rmd
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_damaged_frames_that_agree_dont_validate_each_other() {
        // 25 fps from 10 s on the camera clock, but for the first two frames, a second off it
        let mut timestamps: Vec<u64> = (0..8).map(|k| 10_000_000 + k * 40_000).collect();
        timestamps[0] -= 1_000_000;
        timestamps[1] -= 1_000_000;
        let clock = FrameClock::new(&timestamps, (0..8).collect(), Some(25.0), &[]);
        assert_eq!((0..8).find(|&k| clock.validated(k)), Some(2));
        // Frame 0 is at the time of frame 2, moved by the frames between them
        let time = clock.time(0);
        assert_eq!((time.us, time.from), (10_000_000.0, 2));
    }
}
