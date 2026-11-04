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
    /// The frames read through all the clip's parts
    clip_frames: u64,
    /// The clip's video frame rate, `(numerator, denominator)`: the first video stream descriptor of the first part's header
    /// that has one
    video_rate: Option<(u64, u64)>,
    /// The clip's image size (width, height): the first video stream descriptor of the first part's header that has one
    image_size: Option<(u32, u32)>,
    /// The ID of the camera model: the byte at 0xc of the first part's header (e.g. 11 RED RAVEN, 23 DSMC2
    /// DRAGON-X, 25 KOMODO 6K, 26 V-RAPTOR 8K VV and [X], 28 KOMODO-X, 29 V-RAPTOR XE)
    camera_model_id: Option<u8>,
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

    /// The files of the clip the R3D file `path` is part of: its RMD sidecar, then its parts in order. The parts of a clip
    /// are `<clip>_<NNN>.R3D` in one folder, NNN being the part number in 3 digits ("A001_C001_1231XY_001...999.R3D"). The
    /// clip is the part given, then the parts below it down to part 001, and part 000 when the part given is 001, then the
    /// parts above it, each time until one is missing, by the name of the part given with the number changed. Any other file
    /// is a clip of its own, e.g. a copy `<clip>_001 - Copy.R3D`. The sidecar is `<clip>.RMD`, or `<name>.RMD` for a file
    /// that isn't a part. Established on clips with parts missing, added, renamed and copied
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
        // The clip's name, and the tail of the file name that changes for the names of the clip's other files: the part
        // number and the extension, or the extension of a file that isn't a part
        let clip = part.map_or(stem, |(clip, _)| clip);
        let tail = &filename[clip.len()..];
        // The file opened by the clip's name and this tail. The tail of the path given is changed, so the file
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
    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, _stream: &mut T, _size: u64, progress_cb: F, cancel_flag: Arc<AtomicBool>, options: crate::InputOptions) -> Result<Vec<SampleInfo>> {
        // The IMU samples and the camera timestamp of each, by which they're timed once all the clip's frames are read
        let mut gyro: Vec<TimeVector3<f64>> = Vec::new();
        let mut accl: Vec<TimeVector3<f64>> = Vec::new();
        let mut imu_timestamps: Vec<u64> = Vec::new();
        // The frame each IMU record is recorded with (`util::READING_FRAMES`): the frame its atom follows in the clip
        let mut imu_frames: Vec<u64> = Vec::new();
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

            // A part starts with its header. The clip ends before a file that doesn't, while a part cut short
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
            // The clip's metadata is the header of its first part. The headers of the
            // next parts describe those parts: their file name, start timecodes and lens state
            if !clip_header_parsed {
                // The metadata's guides are in fractions of the image, and its sensor is by the camera model in some clips
                self.image_size = self.image_size.or(Self::image_size(&header));
                self.camera_model_id = header.get(0xc).copied();
                match Self::header_metadata(&header) {
                    Ok(metadata) => {
                        let _ = self.parse_meta(metadata, &mut map, &options, false);
                        clip_header_parsed = true;
                    },
                    Err(e) => log::warn!("RED: {e} in the RED2 header of {path}"),
                }
            }
            // The descriptors come before the metadata block, a header without one has them too
            if self.video_rate.is_none() {
                self.video_rate = Self::video_rate(&header);
            }
            if options.probe_only {
                break 'files;
            }

            // The part's frames and data atoms, through its index, else, in a part cut short or damaged, by following its
            // atoms
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
                    // It's still a frame of the clip: the frames are found and numbered by the clip's index of them,
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
                        log::warn!("RED: the atom of frame {index} in {path} has a size of {atom_size} bytes, its index {size}: the index's is used");
                    }
                }
                let metadata_end = 88 + (&rdi_head[86..]).read_u16::<BigEndian>()? as usize;
                if metadata_end > rdi_head.len() && metadata_end <= atom_data_len {
                    let read = rdi_head.len();
                    rdi_head.resize(metadata_end, 0);
                    stream.read_exact(&mut rdi_head[read..])?;
                }
                // Frames before this one can be missing from a part whose atoms were followed, e.g. the part before cut short
                // (which the next part follows), as the camera's frame numbers and its clock tell. The clock
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
                    let _ = self.parse_meta(&rdi_head[88..metadata_end], &mut per_frame_map, &options, true);
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
            // The frames of the part by where they are in it, for the frame each data atom follows. The first atoms of a
            // part before its first frame follow the part before's last frame, the clip's first ones none: they're the
            // clip's first frame's
            let mut frames_at: Vec<(u64, u64)> = atoms.frames.iter().enumerate().map(|(k, &(offset, _))| (offset, frame_index + k as u64)).collect();
            frames_at.sort_unstable();
            let frame_before_part = frame_index.saturating_sub(1);
            let frame_of_atom = |offset: u64| match frames_at.partition_point(|(at, _)| *at < offset) {
                0 => frame_before_part,
                n => frames_at[n - 1].1,
            };
            frame_index += atoms.frames.len() as u64;
            self.clip_frames = frame_index;

            // The data atoms after a 4 KB header: CSV text in RDX\x01, the IMU in RDX\x02, records of a timestamp (u64, µs on
            // the clock of the frame timestamps), the acceleration and the rotation (i16 x, y, z each). An atom cut short
            // gives the complete records the file has of it. The atom's header is checked before its
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
                    log::warn!("RED: the {} atom at 0x{offset:x} in {path} has a size of {atom_size} bytes, its index {size}: the index's is used", name.escape_ascii());
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
                    // it by one regular step: all 9894 such records of 78 files. It's kept, and the samples by time keep one
                    // of them (`util::normalized_imu_interpolated`)
                    let mut timestamp_before: Option<u64> = None;
                    let atom_frame = frame_of_atom(offset);
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
                            imu_frames.push(atom_frame);
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
        // ([`FrameClock`]). Where it says as many as the numbers do, the clip's time is joined there: as in the clip's video
        // the frame after the missing ones follows the one before them, the clip's time goes on from that one's, and the IMU
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
                log::warn!("RED: {missing} frames of the clip are missing before frame {index} in {path}. As in the clip's video, it follows frame {index_before}, and the clip's time goes on from there");
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
                    imu_frames[kept] = imu_frames[i];
                    kept += 1;
                }
            }
            accl.truncate(kept);
            gyro.truncate(kept);
            imu_frames.truncate(kept);
        }
        // Each sensor's readings' frames: a record's, both of its samples
        let (mut accl_frames, mut gyro_frames) = (imu_frames.clone(), imu_frames);
        // The frames are timed by the camera's frame timestamps (in µs on the clock of the IMU samples) when these increase
        // from frame to frame and the first agrees with the camera clock (above), and otherwise by the record frame rate, as
        // the video is. DSMC2 bodies (RED RAVEN, DSMC2 DRAGON-X) and KOMODO firmware 1.5.3 write no timestamp: the field is 0.
        // KOMODO clips of firmware 1.7.0, 1.7.5 and 1.8.3 have other bytes, or a time seconds off, in the field of single
        // frames
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
                                    accl_frames.push(sample.sample_index);
                                    accl.push(TimeVector3 { t,
                                        x: -camera_acceleration[0].as_f64().unwrap_or(0.0),
                                        y: -camera_acceleration[1].as_f64().unwrap_or(0.0),
                                        z: -camera_acceleration[2].as_f64().unwrap_or(0.0),
                                    });
                                }
                            }
                            if let Some(camera_rotation) = arr.get("camera_rotation").and_then(|x| x.as_array()) {
                                if camera_rotation.len() == 3 {
                                    gyro_frames.push(sample.sample_index);
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
        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Custom(util::READING_FRAMES.into()), "Frame of each reading", Vec_u64, |v| format!("{:?}", v), accl_frames, Vec::new()), &options);
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Custom(util::READING_FRAMES.into()), "Frame of each reading", Vec_u64, |v| format!("{:?}", v), gyro_frames, Vec::new()), &options);

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

    /// Decodes a metadata block of the clip, `data`, into `map`: its values into the Default group's Metadata (JSON), and the
    /// typed tags they give. The block is the clip's, in its first part's RED2 header (`in_frame` false), or a frame's, which
    /// repeats the clip's items and adds the frame's own: its timecodes, lens state and motion.
    ///
    /// An entry is its size (u16, counting itself), its key (u16) and its value. The key's high nibble is the type of the
    /// value ([`Self::entry_value`]) and its low 11 bits the item. A key with bit 10 set (types 0x14 and 0x34) is an item's
    /// value for one of the camera's monitor outputs ([`Self::monitor_output_entry`]). Each item has its key and value
    /// ([`Self::insert_item`]). Established on the entries of 33 clips of the RED RAVEN, DSMC2 DRAGON-X, KOMODO 6K, KOMODO-X,
    /// V-RAPTOR 8K VV, V-RAPTOR [X] 8K VV and V-RAPTOR XE 8K VV
    pub fn parse_meta(&mut self, mut data: &[u8], map: &mut GroupedTagMap, options: &crate::InputOptions, in_frame: bool) -> Result<()> {
        let mut md = serde_json::Map::<String, serde_json::Value>::new();
        let mut derived = DerivedItems { camera_model_id: self.camera_model_id, ..Default::default() };
        // The camera model's ID is the clip's, from its header
        if let Some(id) = self.camera_model_id.filter(|_| !in_frame) {
            md.insert("camera_model_id".into(), id.into());
        }
        while data.len() >= 2 {
            let size = BigEndian::read_u16(data) as usize;
            // A smaller size ends the metadata
            if size < 4 {
                break;
            }
            let Some(d) = data.get(2..size) else {
                log::warn!("RED: a metadata entry of {size} bytes runs past the end of its block, the entries after are lost");
                break;
            };
            data = &data[size..];
            if d[0] & 0x04 != 0 {
                Self::monitor_output_entry(&mut md, d[0], d[1], &d[2..]);
            } else {
                self.insert_item(&mut md, &mut derived, ((d[0] as u16 & 0x07) << 8) | d[1] as u16, d[0], &d[2..], in_frame);
            }
        }
        derived.insert(&mut md, self.image_size);
        if !md.is_empty() {
            if let Some(v) = md.get("exposure_time").and_then(|v| v.as_f64()) {
                util::insert_tag(map, tag!(parsed GroupId::Default, TagId::ExposureTime, "Exposure time", f32, |v| format!("{v:.6}"), v as f32, vec![]), &options);
            }
            if let Some(v) = md.get("lens_focal_length").and_then(|v| v.as_f64()) {
                util::insert_tag(map, tag!(parsed GroupId::Lens, TagId::FocalLength, "Focal length", f32, |v| format!("{v:.3}"), v as f32, vec![]), &options);
            }
            if let Some(v) = md.get("lens_name").and_then(|v| v.as_str()) {
                util::insert_tag(map, tag!(parsed GroupId::Lens, TagId::Name, "Lens name", String, |v| v.clone(), v.into(), vec![]), &options);
            }
            Self::insert_recording_settings(&md, map, options);

            if let Some(pp) = Self::pixel_pitch(self.sensor_name.as_deref(), self.model.as_deref()) {
                util::insert_tag(map, tag!(parsed GroupId::Imager, TagId::PixelPitch, "Pixel pitch", u32x2, |v| format!("{v:?}"), (pp, pp), vec![]), &options);
            }

            util::insert_tag(map, tag!(parsed GroupId::Default, TagId::Metadata, "Metadata", Json, |v| serde_json::to_string(v).unwrap(), serde_json::Value::Object(md), vec![]), &options);
        }
        Ok(())
    }

    /// The recording settings of a metadata block's values `md`, in the tags other cameras give them in: ISO, shutter angle
    /// and speed, white balance, color space and gamma curve, and the iris. The shutter is that of the exposure time at the
    /// record frame rate, the speed of its whole fraction of a second, e.g. 1/48s for 180° at 23.976 fps
    fn insert_recording_settings(md: &serde_json::Map<String, serde_json::Value>, map: &mut GroupedTagMap, options: &crate::InputOptions) {
        let number = |key: &str| md.get(key).and_then(|v| v.as_f64());
        if let Some(iso) = number("iso").filter(|&x| x > 0.0) {
            util::insert_tag(map, tag!(parsed GroupId::Exposure, TagId::ISOValue, "ISO", u16, |v| v.to_string(), iso.round().min(u16::MAX as f64) as u16, vec![]), options);
        }
        if let Some(exposure_us) = number("exposure_time").filter(|&x| x > 0.0) {
            let fps = number("record_framerate_numerator").zip(number("record_framerate_denominator")).filter(|x| x.0 > 0.0 && x.1 > 0.0).map(|x| x.0 / x.1);
            if let Some(fps) = fps {
                let angle = (exposure_us * fps * 360.0 / 1_000_000.0) as f32;
                util::insert_tag(map, tag!(parsed GroupId::Exposure, TagId::ShutterAngle, "Shutter angle", f32, |v| format!("{v:.1}°"), angle, vec![]), options);
            }
            let speed = if exposure_us < 1_000_000.0 { (1, (1_000_000.0 / exposure_us).round() as u32) } else { ((exposure_us / 1_000_000.0).round() as u32, 1) };
            util::insert_tag(map, tag!(parsed GroupId::Exposure, TagId::ShutterSpeed, "Shutter speed", u32x2, |v| format!("{}/{}s", v.0, v.1), speed, vec![]), options);
        }
        if let Some(kelvin) = number("white_balance_kelvin").filter(|&x| x > 0.0) {
            util::insert_tag(map, tag!(parsed GroupId::Colors, TagId::WhiteBalance, "White balance", u16, |v| format!("{v}K"), kelvin.round().min(u16::MAX as f64) as u16, vec![]), options);
        }
        if let Some(name) = md.get("color_space_name").and_then(|v| v.as_str()) {
            util::insert_tag(map, tag!(parsed GroupId::Colors, TagId::ColorPrimaries, "Color space", String, |v| v.clone(), name.into(), vec![]), options);
        }
        if let Some(name) = md.get("gamma_curve_name").and_then(|v| v.as_str()) {
            util::insert_tag(map, tag!(parsed GroupId::Colors, TagId::CaptureGammaEquation, "Gamma curve", String, |v| v.clone(), name.into(), vec![]), options);
        }
        // The aperture × 10, in T-stops or f-stops. 0xffff is fully closed
        if let Some(aperture) = number("lens_aperture").filter(|&x| x > 0.0 && x < 65535.0) {
            let stop = (aperture / 10.0) as f32;
            if number("lens_aperture_units") == Some(1.0) {
                util::insert_tag(map, tag!(parsed GroupId::Lens, TagId::IrisTStop, "Iris T-stop", f32, |v| format!("T{v:.1}"), stop, vec![]), options);
            } else {
                util::insert_tag(map, tag!(parsed GroupId::Lens, TagId::IrisFStop, "Iris F-stop", f32, |v| format!("f/{v:.1}"), stop, vec![]), options);
            }
        }
    }

    /// Inserts the value `v` of metadata item `item` (of an entry of type `typ`) into `md`, under its key and in its format:
    /// each item has its own, whatever the entry's type says. The values that give keys together with other items are kept
    /// in `derived`. An item named by its values in the clips, where they tell what it is, is noted "inferred", and one whose
    /// meaning isn't known is `0x<item>`, its value by the entry's type
    fn insert_item(&mut self, md: &mut serde_json::Map<String, serde_json::Value>, derived: &mut DerivedItems, item: u16, typ: u8, v: &[u8], in_frame: bool) {
        use serde_json::Value;
        // A float whose magnitude is below 1e-5 is 0
        let f = |i: usize| v.get(i * 4..i * 4 + 4).map(|b| Some(BigEndian::read_f32(b)).filter(|x| x.abs() >= 1e-5).unwrap_or(0.0));
        let floats = |n: usize| (0..n).map_while(f).collect::<Vec<f32>>();
        let float_values = |x: &[f32]| Value::Array(x.iter().map(|&x| (x as f64).into()).collect());
        let u8_ = |i: usize| v.get(i).copied();
        let u16_ = |i: usize| v.get(i * 2..i * 2 + 2).map(BigEndian::read_u16);
        let u32_ = |i: usize| v.get(i * 4..i * 4 + 4).map(BigEndian::read_u32);
        let text = || Self::entry_text(v);
        let mut put = |key: &str, value: Value| { md.insert(key.to_owned(), value); };
        // A frame's timecode, or the clip's start timecode. One of 11 or 12 characters has ':' for a '!' before the minutes
        let timecode = |kind: &str| (if in_frame { format!("frame_{kind}_timecode") } else { format!("start_{kind}_timecode") }, {
            let mut tc = text();
            if (11..=12).contains(&tc.len()) && tc.as_bytes()[2] == b'!' {
                tc.replace_range(2..3, ":");
            }
            tc
        });

        match item {
            0x000 | 0x0ad => { let (k, tc) = timecode("edge"); put(&k, tc.into()); },
            0x001 | 0x0ae => { let (k, tc) = timecode("absolute"); put(&k, tc.into()); },
            // White balance and the legacy look: exposure compensation, saturation (around 1), contrast, brightness and the
            // RGB gains
            0x00f => if let Some(x) = f(0) { put("saturation", ((x + 1.0) as f64).into()) },
            0x010 => if let Some(x) = f(0) { put("exposure_compensation", ((x - 0.4) as f64).into()) },
            0x014 => if let Some(x) = f(0) { derived.gain = Some(x); put("digital_gain", (x as f64).into()) },
            0x015 => if let [r, g, b] = floats(3)[..] { derived.gains = Some([r, g, b]) },
            // The luma curve: x and y of its black, low, mid, high and white points
            0x035 => {
                const KEYS: [&str; 10] = ["user_curve_black_x", "user_curve_black_y", "user_curve_low_x", "user_curve_low_y", "user_curve_mid_x",
                                          "user_curve_mid_y", "user_curve_high_x", "user_curve_high_y", "user_curve_white_x", "user_curve_white_y"];
                for (key, x) in KEYS.iter().zip(floats(10)) {
                    put(key, (x as f64).into());
                }
            },
            // The frame guide, action guide and title guide: their rectangles in pixels (x, y, width, height), and which are enabled
            0x037 | 0x04e | 0x04f => if let (Some(x), Some(y), Some(w), Some(h)) = (u16_(0), u16_(1), u16_(2), u16_(3)) {
                derived.guides[match item { 0x037 => 0, 0x04e => 1, _ => 2 }] = Some([x, y, w, h]);
            },
            0x059 | 0x05a => derived.guides_enabled = u8_(0),
            // The lens responses, as the camera received them from a Cooke /i lens, as far as they can be decoded: a field
            // none of which can be decoded is left out. The problems are logged once per clip, they usually repeat in every
            // frame
            0x039 | 0x03a => {
                let key = if item == 0x039 { "lens_cooke_i_static" } else { "lens_cooke_i_dynamic" };
                let responses = crate::cooke::bin::parse(v);
                for e in &responses.errors {
                    if !self.cooke_error_logged {
                        log::warn!("RED: {key}: {e}");
                        self.cooke_error_logged = true;
                    } else {
                        log::debug!("RED: {key}: {e}");
                    }
                }
                if !responses.records.is_empty() || responses.errors.is_empty() {
                    put(key, responses.records.into());
                }
            },
            // ISO 1250 is 1280
            0x03b => if let Some(iso) = u16_(0) { put("iso", (if iso == 1250 { 1280 } else { iso }).into()) },
            0x03c => if let Some(mode) = u8_(0) { derived.anamorphic_mode = Some(mode); put("anamorphic_mode", mode.into()) },
            // The sensor type, which gives the sensor's name and ID when the clip has neither. A second byte below 25 is the
            // type, else the sensor ID
            0x043 => if let Some(t) = u8_(0) {
                let second = u8_(1);
                let t = second.filter(|&b| b < 25).unwrap_or(if t == 0xff { 4 } else { t });
                derived.sensor_type = Some(t);
                put("sensor_type", t.into());
                if let Some(id) = second.filter(|&b| b >= 25) {
                    put("sensor_id", id.into());
                }
            },
            // The REDCODE data rate of RED ONE clips (MB/s at 24 fps)
            0x044 => if let Some(rate) = u16_(0) { derived.data_rate = Some(rate); put("redcode_data_rate", rate.into()) },
            // The color science version the clip was recorded with and the current one (3: legacy, 4: IPP2, 5: broadcast),
            // which give the clip's default color version
            0x046 => if let Some(original) = u8_(0) { derived.color_version = Some((original, u8_(1))) },
            // A preset of the legacy look's color space and gamma curve, for clips without these
            0x02b => if let Some(preset) = u8_(0) { derived.color_preset = Some(preset); put("color_gamma_preset", preset.into()) },
            // IPP2's exposure adjust, or the legacy look's FLUT
            0x047 => derived.exposure_adjust = f(0),
            // The sensor's OLPF, by the list of OLPF names
            0x048 => if let Some(id) = u32_(0) { derived.olpf_id = Some(id); put("sensor_olpf_id", id.into()) },
            0x049 => if let Some(flip) = u8_(0) {
                put("flip_horizontal", (flip & 1).into());
                put("flip_vertical", (flip >> 1).into());
            },
            // The legacy look's red, green and blue curves, as the luma curve
            0x04b => {
                let x = floats(30);
                for (key, curve) in ["user_curve_red", "user_curve_green", "user_curve_blue"].iter().zip(x.chunks(10)) {
                    put(key, float_values(curve));
                }
            },
            // The legacy look's lift, gamma and gain, red, green and blue each
            0x04c => {
                let x = floats(9);
                for (key, rgb) in ["lgg_lift", "lgg_gamma", "lgg_gain"].iter().zip(x.chunks(3)) {
                    put(key, float_values(rgb));
                }
            },
            0x055 => if let Some(mount) = u8_(0) {
                put("lens_mount_id", mount.into());
                let name = match mount { 4 => "P/L", 8 => "Nikon", 9 => "Canon", 10 => "Leica M", 11 => "P/L Motion Mount", 12 => "Canon Motion Mount", _ => "" };
                if !name.is_empty() {
                    put("lens_mount", name.into());
                }
            },
            // Without its top bit, which clips of RED2 version 6 and later set
            0x065 => if let Some(rev) = u32_(0) { put("camera_firmware_revision", (rev & 0x7fffffff).into()) },
            // The record frame rate as the camera's float, by which the frames are timed, and which gives the record frame
            // rate when the clip has no numerator of it (item 0xbf)
            0x066 => if let Some(fps) = f(0) {
                self.record_framerate = Some(fps as f64);
                derived.record_rate = Some(fps);
            },
            // ND in stops, the built-in or the Motion Mount's
            0x069 => if let Some(nd) = u16_(0) {
                let stops = (nd as f32 / 100.0) as f64;
                put("motion_mount_nd_stops", stops.into());
                put("nd_stops", stops.into());
            },
            // The aperture × 10, 0xffff fully closed
            0x06a => if let Some(aperture) = u16_(0) { derived.aperture = Some(aperture); put("lens_aperture", aperture.into()) },
            // In mm, 0xffffffff infinite (and so is any value above 0xffffff00)
            0x06c => if let Some(distance) = u32_(0) { put("lens_focus_distance", (if distance > 0xffffff00 { 0xffffffff } else { distance }).into()) },
            0x06d => if let Some(units) = u8_(0) {
                derived.aperture_units = Some(units >> 1 & 1);
                put("lens_distance_units", (units & 1).into());
                put("lens_aperture_units", (units >> 1 & 1).into());
            },
            // The color space and the gamma curve of the legacy look and of IPP2
            0x072 => derived.color_space = Some(v.to_vec()),
            0x073 => derived.gamma_curve = Some(v.to_vec()),
            // The camera's motion at the frame: acceleration in g and rotation in °/s, x, y and z
            0x07f | 0x080 => put(if item == 0x07f { "camera_acceleration" } else { "camera_rotation" }, float_values(&floats(3))),
            0x082 => if let (Some(head), Some(tail)) = (u8_(0), u8_(1)) {
                put("auto_slate_head_frames", head.into());
                put("auto_slate_tail_frames", tail.into());
            },
            // The REDCODE compression ratio × 100, which gives the REDCODE when the clip has no name of it
            0x087 => if let Some(ratio) = u16_(0) { derived.compression = Some(ratio); put("redcode_compression_ratio", ((ratio as f32 / 100.0) as f64).into()) },
            // The sensor's color calibration, and its version
            0x08b => put("color_calibration", float_values(&floats(v.len() / 4))),
            0x08f => if let Some(version) = u16_(0) { put("color_calibration_version", version.into()) },
            0x095 => if let (Some(installed), Some(interchangeable)) = (u8_(0), u8_(1)) {
                put("sensor_olpf_interchangeable", u8::from(interchangeable != 0xff && installed == 1).into());
            },
            0x096 => { let name = text(); put("sensor_olpf_name", (if name == "H&redsub2;O" { "H2O".to_owned() } else { name }).into()) },
            // Pairs of a key and a value the user set, after a flag word and their number: each of the two a byte of its length
            // and its text
            0x09f => if let Some(count) = u16_(1) {
                let mut pairs = serde_json::Map::new();
                let mut rest = &v[4..];
                for _ in 0..count {
                    let Some((&key_len, after)) = rest.split_first().filter(|(len, _)| **len > 0) else { break };
                    let Some(key) = after.get(..key_len as usize) else { break };
                    let Some((&value_len, after)) = after[key_len as usize..].split_first() else { break };
                    let Some(value) = after.get(..value_len as usize) else { break };
                    pairs.insert(Self::entry_text(key), Self::entry_text(value).into());
                    rest = &after[value_len as usize..];
                }
                if !pairs.is_empty() {
                    put("user_external", Value::Object(pairs));
                }
            },
            0x0a1 => {
                let name = text();
                self.sensor_name = Some(name.clone());
                put("sensor_name", name.into());
            },
            0x0a0 => {
                let model = text();
                self.model = Some(model.clone());
                put("camera_model", model.into());
            },
            // The ASC CDL: saturation, then the slope, offset and power of red, green and blue. It's enabled by default when
            // the clip has it in item 0xaa
            0x0aa | 0x0c0 => if let [saturation, sr, sg, sb, or, og, ob, pr, pg, pb] = floats(10)[..] {
                put("cdl_saturation", (saturation as f64).into());
                put("cdl_slope", float_values(&[sr, sg, sb]));
                put("cdl_offset", float_values(&[or, og, ob]));
                put("cdl_power", float_values(&[pr, pg, pb]));
                if item == 0x0aa {
                    put("cdl_default_enabled", 1.into());
                }
            },
            0x0ab | 0x0c1 => {
                put("3d_lut", text().into());
                if item == 0x0ab {
                    put("3d_lut_default_enabled", 1.into());
                }
            },
            0x0ac => if let Some(x) = f(0) { put("iso_calibration_version", (if x != 0.0 { 2 } else { 1 }).into()) },
            0x0af => if let Some(id) = u8_(0) {
                put("sensor_sensitivity_id", id.into());
                match id {
                    0 => put("sensor_sensitivity_name", "Standard".into()),
                    1 => put("sensor_sensitivity_name", "Low-light".into()),
                    _ => {}
                }
            },
            // The timebase × 1001 (inferred)
            0x0b0 => if let Some(rate) = u32_(0) { put("framerate", (rate as f64 / 1001.0).into()) },
            // The record frame rate × 1001
            0x0bf => if let Some(numerator) = u32_(0) {
                derived.record_numerator = Some(numerator);
                put("record_framerate_numerator", numerator.into());
                put("record_framerate_denominator", 1001.into());
            },
            // The lens distortion correction is available but off by default when the bit 0 is set
            0x0fb => if u8_(0).is_some_and(|x| x & 1 != 0) { put("lens_distortion_correction", 2.into()) },
            // Zeros padding a frame's metadata
            0x090 if typ >> 4 == 0 && v.iter().all(|&b| b == 0) => {},
            _ => match Self::item_format(item) {
                Some((key, format)) => {
                    let value = match format {
                        ItemFormat::Text => Some(text().into()),
                        ItemFormat::F32 => f(0).map(|x| (x as f64).into()),
                        ItemFormat::U8 => u8_(0).map(Into::into),
                        ItemFormat::U16 => u16_(0).map(Into::into),
                        ItemFormat::U16s => Some(Value::Array(v.chunks_exact(2).map(|b| BigEndian::read_u16(b).into()).collect())),
                        ItemFormat::U32 => u32_(0).map(Into::into),
                        ItemFormat::U64 => v.get(..8).map(|b| BigEndian::read_u64(b).into()),
                        ItemFormat::F32s => Some(float_values(&floats(v.len() / 4))),
                    };
                    put(key, value.unwrap_or_else(|| Self::entry_value(typ, v)));
                },
                None => put(&format!("0x{item:x}"), Self::entry_value(typ, v)),
            },
        }
    }

    /// The key and format of the metadata items whose value is read as it is, [`Self::insert_item`]
    fn item_format(item: u16) -> Option<(&'static str, ItemFormat)> {
        use ItemFormat::*;
        Some(match item {
            0x006 => ("camera_pin", Text),
            // In µs
            0x008 => ("exposure_time", F32),
            0x00d => ("white_balance_kelvin", F32),
            0x00e => ("white_balance_tint", F32),
            0x011 => ("brightness", F32),
            0x013 => ("contrast", F32),
            0x018 => ("exposure_compensation", F32),
            0x019 => ("camera_id", Text),
            0x01a => ("reel_id", Text),
            0x01b => ("clip_id", Text),
            0x023 => ("local_date", Text),
            0x024 => ("local_time", Text),
            0x025 => ("camera_firmware_version", Text),
            0x026 => ("user_timecode_preference", U8),
            0x028 => ("shutter_phase_offset", F32),
            // The model of the media recorded to, e.g. "RED PRO CFexpress" (inferred)
            0x02a => ("media_model", Text),
            0x02d => ("genlock_setting", U8),
            0x02e => ("jamsync_setting", U8),
            0x030 => ("gmt_date", Text),
            0x031 => ("gmt_time", Text),
            0x032 => ("media_serial_number", Text),
            // The firmware version of the media recorded to (inferred)
            0x033 => ("media_firmware_version", Text),
            0x036 => ("frame_guide_name", Text),
            0x04a => ("shadow", F32),
            0x04d => ("linked_camera_setup", U8),
            0x051 => ("camera_position", Text),
            // The two characters that end the clip's name (inferred)
            0x052 => ("clip_name_suffix", Text),
            0x056 => ("original_filename", Text),
            0x05d => ("stereo_setup", U8),
            // In mm
            0x06b => ("lens_focal_length", U16),
            0x06e => ("lens_brand", Text),
            // The lens' ID, as Canon EF lenses give it (inferred)
            0x06f => ("lens_id", Text),
            0x070 => ("lens_name", Text),
            0x071 => ("camera_network_name", Text),
            0x074 => ("lens_focus_distance_near", U32),
            0x075 => ("lens_focus_distance_far", U32),
            0x076 => ("user_production_name", Text),
            0x077 => ("user_director", Text),
            0x078 => ("user_director_of_photography", Text),
            0x079 => ("user_copyright", Text),
            0x07a => ("user_unit", Text),
            0x07b => ("user_location", Text),
            0x07c => ("user_camera_operator", Text),
            0x07d => ("user_scene", Text),
            0x07e => ("user_take", Text),
            0x085 => ("motion_mount_shutter_type", U8),
            0x086 => ("resolution_format_name", Text),
            0x092 => ("monitor_sharpness", U8),
            0x097 => ("user_shot", Text),
            0x098 => ("lens_focal_length_range", U16s), // The lens' shortest and longest focal length in mm (inferred)
            0x09a => ("drx", U8),
            // The sensor's size and the offset of the recorded image on it, in pixels, in DSMC2 clips (inferred)
            0x09b => ("sensor_size", U16s),
            0x09c => ("sensor_crop_offset", U16s),
            0x09d => ("lens_serial_number", Text),
            0x09e => ("lens_owner", Text),
            // IPP2's highlight roll-off (0 none, 1 hard, 2 medium, 3 soft, 4 very soft) and output tone map (0 low, 1 medium,
            // 2 high, 3 none)
            0x0a5 => ("highlight_roll_off", U8),
            0x0a8 => ("output_tone_map", U8),
            0x0b1 => ("output_transform", U8),
            // The lens' minimum focus distance in mm (inferred)
            0x0b6 => ("lens_minimum_focus_distance", U32),
            // 1 off, 2 mild, 3 medium, 4 strong
            0x0bb => ("flashing_pixel_adjustment", U8),
            0x0be => ("redcode", Text),
            0x0c2 => ("cdl_filename", Text),
            0x0c4 => ("wav_filename", Text),
            // The ND filter, e.g. "Clear" or "Electronic ND" (inferred)
            0x0c7 => ("nd_filter", Text),
            0x0cf => ("lens_hyperfocal_distance", U32),
            0x0d0 => ("lens_focal_length_micrometers", U32),
            // In ns
            0x0d2 => ("frame_ptp_timestamp", U64),
            0x0da => ("phantom_track_index", U8),
            0x0dc => ("phantom_track_frame_gap_ns", U32),
            0x0dd => ("phantom_track_count", U8),
            0x0de => ("user_ext_filter_1", Text),
            0x0df => ("user_ext_filter_2", Text),
            0x0e0 => ("user_ext_filter_3", Text),
            0x0e1 => ("user_ext_lut", Text),
            0x0e2 => ("user_ext_tlut", Text),
            0x0e3 => ("user_ext_proxy", Text),
            0x0e4 => ("user_ext_upload_service", Text),
            0x0e5 => ("user_ext_gps", Text),
            0x0e7 => ("external_tally", U8),
            // RGBA
            0x0ee => ("external_tally_color", U32),
            // The lens distortion model's 4 parameters
            0x0fc => ("lens_distortion_parameters", F32s),
            _ => return None,
        })
    }

    /// An item's value for one of the camera's monitor outputs, into `md`'s "monitor_outputs", by the output's number. The
    /// value is a byte that is 1 in every entry seen, the output's number and the value, of the type of the entry's high
    /// nibble: the output's name for item 0xc5, its output transform for item 0xb1 (as the clip's [`Self::insert_item`])
    fn monitor_output_entry(md: &mut serde_json::Map<String, serde_json::Value>, typ: u8, item: u8, v: &[u8]) {
        let (Some(&output), Some(value)) = (v.get(1), v.get(2..)) else { return };
        let key = match item { 0xc5 => "name".to_owned(), 0xb1 => "output_transform".to_owned(), _ => format!("0x{item:x}") };
        let outputs = md.entry("monitor_outputs").or_insert_with(|| serde_json::Value::Object(Default::default()));
        if let Some(output) = outputs.as_object_mut().and_then(|x| x.entry(output.to_string()).or_insert_with(|| serde_json::Value::Object(Default::default())).as_object_mut()) {
            output.insert(key, Self::entry_value(typ, value));
        }
    }

    /// The value of an entry by its type, the high nibble of its first byte: text (1), f32 (2), u16 (4), u32 (6), u64 (7), u8
    /// (3, 5 and a byte of 0) and binary data as hex (0, 8, 9). A value of more than one number is an array
    fn entry_value(typ: u8, v: &[u8]) -> serde_json::Value {
        use serde_json::Value;
        let list = |mut x: Vec<Value>| if x.len() == 1 { x.remove(0) } else { Value::Array(x) };
        match typ >> 4 {
            0x1 => Self::entry_text(v).into(),
            0x2 => list(v.chunks_exact(4).map(|b| (BigEndian::read_f32(b) as f64).into()).collect()),
            0x3 | 0x5 => list(v.iter().map(|&b| b.into()).collect()),
            0x4 => list(v.chunks_exact(2).map(|b| BigEndian::read_u16(b).into()).collect()),
            0x6 => list(v.chunks_exact(4).map(|b| BigEndian::read_u32(b).into()).collect()),
            0x7 => list(v.chunks_exact(8).map(|b| BigEndian::read_u64(b).into()).collect()),
            0x0 if v.len() == 1 => v[0].into(),
            _ => v.iter().map(|b| format!("{b:02x}")).collect::<String>().into(),
        }
    }

    /// The text of an entry: its bytes up to the first NUL, without the spaces around it. The entry size bounds the text, and
    /// some cameras also count a C terminator in it, e.g. the V-RAPTOR XE writes its model as `V-RAPTOR XE 8K VV\0`
    fn entry_text(v: &[u8]) -> String {
        let end = v.iter().position(|&c| c == 0).unwrap_or(v.len());
        String::from_utf8_lossy(&v[..end]).trim().to_owned()
    }

    /// The frame rate of a `RED2` header's first video stream descriptor (`rdi`, from 0x44 when the header has one): its
    /// numerator, a u32 at 20, over its denominator, a u16 at 18
    fn video_rate(atom: &[u8]) -> Option<(u64, u64)> {
        if atom.get(0x40).is_none_or(|count| *count == 0) || atom.get(0x44..0x47) != Some(b"rdi") { return None; }
        let d = atom.get(0x44..0x44 + 24)?;
        let (num, den) = (u32::from_be_bytes(d[20..24].try_into().ok()?) as u64, u16::from_be_bytes(d[18..20].try_into().ok()?) as u64);
        (num > 0 && den > 0).then_some((num, den))
    }

    /// The image size of a `RED2` header's first video stream descriptor (`rdi`, from 0x44 when the header has one): its
    /// width and height, u32s at 8 and 12
    fn image_size(atom: &[u8]) -> Option<(u32, u32)> {
        if atom.get(0x40).is_none_or(|count| *count == 0) || atom.get(0x44..0x47) != Some(b"rdi") { return None; }
        let d = atom.get(0x44..0x44 + 24)?;
        let (width, height) = (BigEndian::read_u32(&d[8..]), BigEndian::read_u32(&d[12..]));
        (width > 0 && height > 0).then_some((width, height))
    }

    /// The segment table of the clip, in its RMD sidecar ([`crate::segment_table::read_rmd`]), checked against the clip's
    /// frames once it's parsed. `None` when the clip has no RMD or the RMD no table
    pub(crate) fn segment_table(&self) -> Option<crate::segment_table::Found> {
        let rmd = self.all_parts.iter().find(|x| filesystem::get_extension(x) == "rmd")?;
        let bytes = match filesystem::open_file(rmd).and_then(|mut f| util::read_data(&mut f.file, f.size)) {
            Ok(bytes) => bytes,
            Err(e) => return Some(Err(format!("the RMD sidecar {rmd} can't be read: {e}"))),
        };
        crate::segment_table::read_rmd(&bytes, self.clip_frames, self.video_rate)
    }

    /// The clip metadata block of a `RED2` header atom, `atom` being the whole atom. At 0x40 the header has the number of its
    /// video, audio and data stream descriptors (then a byte that is 1 in every clip seen), and from 0x44 on the descriptors:
    /// an `rdi` of 24 bytes per video stream, an `rda` of 20 bytes per audio stream and an `rdx` of 16 bytes per kind of RDX
    /// atom, each starting with its tag. The metadata block follows them: its size, a big-endian u16, and its entries.
    /// Established on 124 clips of the RED RAVEN, DSMC2 DRAGON-X, KOMODO 6K, KOMODO-X, V-RAPTOR 8K VV, V-RAPTOR [X] and
    /// V-RAPTOR XE, the counts are 1, 0 to 1 and 0 to 2
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

    /// The frames and data atoms of a part through its index. The part's last 4 KB are its
    /// footer: REOB in a clip of one file, REOF in a part another part follows, REOC in the clip's last part. It has its
    /// file's size (u64 at 0x40), its numbers of video, audio and data streams (bytes at 0x4C) and from 0x50 a descriptor of
    /// each stream, `rdi` of 24 bytes, `rda` of 40 and `rdx` of 24: its tag, then u32s, the 4 KB block of the stream's index
    /// atom, the size of its largest atom, the number of its atoms in the part, 0 and their number in the clip up to the end
    /// of the part (an `rda` then has the audio samples of the part and of the clip, u64 each). An index atom (RII frames,
    /// RIA audio, RIX\x01 and RIX\x02 data) has at 0x40 how many u32s an entry has after its first, and from 0x44 an entry
    /// per atom: its 4 KB block, a value only RED RAVEN and DSMC2 frame indexes have, and its size. The camera numbers the
    /// frames from 1 through the clip's parts, in their atoms (u32 at 0x4C). Established on the 124 R3D files here, of the
    /// RED RAVEN, DSMC2 DRAGON-X, KOMODO 6K, KOMODO-X, V-RAPTOR 8K VV, V-RAPTOR [X] and V-RAPTOR XE, in all of which the
    /// index has exactly the atoms that following the atoms' sizes finds. The frames are where the index has them: a frame's
    /// own atom size isn't used. `Err` with what's wrong when the part has no footer or its
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

    /// The frames and data atoms of a part by following its atoms by their sizes from its header on, for a part without a
    /// usable index, such as one cut short: the atoms not known are skipped, and it stops at the end of
    /// the file or at an atom smaller than its header or going past the end of the file. A frame atom is a frame when the
    /// file goes on after it with an atom header, and a data atom gives the complete records the file has of it.
    /// Established on clips cut short in their frames, data atoms, index and footer, and damaged in an
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
        log::warn!("RED: {path} has no usable index, {why}: it was cut short or is damaged. Its atoms are followed from its header on: {} frames, up to {end}", atoms.frames.len());
        Ok(atoms)
    }

    fn read_at<S: Read + Seek + ?Sized>(stream: &mut S, offset: u64, buf: &mut [u8]) -> Result<()> {
        stream.seek(SeekFrom::Start(offset))?;
        stream.read_exact(buf)
    }

    /// The pixel pitch of the sensor in nanometres (square pixels), by the sensor name the camera records or, in clips of
    /// firmware that records none (KOMODO 6K before 1.7.3, V-RAPTOR 8K VV 1.3.4), by the camera model. The names of the
    /// sensors without a clip here are those given clips that record only their sensor type ([`DerivedItems::SENSORS`]),
    /// as the RED RAVEN and V-RAPTOR 8K VV clips here record theirs, "DRAGON 4.5K S35" and "V-RAPTOR VV".
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

/// How [`RedR3d::insert_item`] reads the value of an item it takes as it is: from the value's start, the floats and u16s of
/// the whole value
enum ItemFormat { Text, F32, F32s, U8, U16, U16s, U32, U64 }

/// The values of the metadata items that give keys together with other items, once a block's items are read
/// ([`RedR3d::parse_meta`])
#[derive(Default)]
struct DerivedItems {
    /// The digital gain (item 0x14) and the red, green and blue gains (0x15) it multiplies
    gain: Option<f32>,
    gains: Option<[f32; 3]>,
    /// The frame, action and title guides (x, y, width and height in pixels), and the bits of the enabled ones
    guides: [Option<[u16; 4]>; 3],
    guides_enabled: Option<u8>,
    sensor_type: Option<u8>,
    olpf_id: Option<u32>,
    anamorphic_mode: Option<u8>,
    /// The aperture × 10 and its units (0 f-stop, 1 T-stop)
    aperture: Option<u16>,
    aperture_units: Option<u8>,
    /// The record frame rate, as a float and as a numerator over 1001
    record_rate: Option<f32>,
    record_numerator: Option<u32>,
    /// The camera model's ID, which tells the DRAGON 6K from the DRAGON 5K
    camera_model_id: Option<u8>,
    /// The color science version the clip was recorded with and the current one
    color_version: Option<(u8, Option<u8>)>,
    exposure_adjust: Option<f32>,
    /// The color space and gamma curve: the legacy look's, then IPP2's, and the preset of both
    color_space: Option<Vec<u8>>,
    gamma_curve: Option<Vec<u8>>,
    color_preset: Option<u8>,
    /// The REDCODE compression ratio × 100, and the REDCODE data rate of RED ONE clips
    compression: Option<u16>,
    data_rate: Option<u16>,
}

impl DerivedItems {
    /// The sensor names and IDs by sensor type, the names not known left out. Type 6 is the DRAGON 5K S35 (ID 9) in camera
    /// model 12 and the DRAGON 6K S35 (ID 4) otherwise, and type 8 the DRAGON 4K S35 with OLPF 22 and the DRAGON 4.5K S35
    /// otherwise, both ID 8
    const SENSORS: [(u8, &'static str, u32); 23] = [
        (0, "MYSTERIUM 4.5K S35", 0), (1, "MYSTERIUM-X 4.5K S35", 1), (2, "MYSTERIUM-X 4.5K S35", 1), (3, "MYSTERIUM-X 3K 2/3\"", 2),
        (4, "MYSTERIUM-X 5K S35", 1), (5, "MYSTERIUM-X 5K S35 MONOCHROME", 3), (7, "DRAGON 6K S35 MONOCHROME", 5), (9, "DRAGON 8K VV", 7),
        (10, "HELIUM 8K S35", 6), (11, "DRAGON 8K VV", 7), (12, "GEMINI 5K S35", 13), (13, "HELIUM 8K S35 MONOCHROME", 10),
        (14, "MONSTRO 8K VV", 11), (15, "MONSTRO 8K VV MONOCHROME", 12), (16, "KOMODO", 14), (17, "V-RAPTOR VV", 15),
        (18, "V-RAPTOR S35", 16), (19, "", 17), (20, "", 18), (21, "", 19), (22, "", 20), (23, "", 21), (24, "", 22),
    ];

    /// The OLPF names by the OLPF's ID, for a clip that has no name of it
    const OLPF_NAMES: [&'static str; 23] = [
        "", "", "", "MONOCHROME", "MONOCHROME", "MONOCHROME", "SKIN TONE-HIGHLIGHT", "LOW LIGHT OPTIMIZED", "SKIN TONE-HIGHLIGHT", "",
        "LOW LIGHT OPTIMIZED", "LOW LIGHT OPTIMIZED", "LOW LIGHT OPTIMIZED", "SKIN TONE-HIGHLIGHT", "MONOCHROME", "LOW LIGHT OPTIMIZED",
        "H2O", "DRAGON COLOR", "IR PASS COLOR", "IR PASS MONOCHROME", "NONE", "STANDARD", "",
    ];

    /// The legacy look's color space and gamma curve of the presets (item 0x2b)
    const COLOR_PRESETS: [(u8, u8); 7] = [(1, 1), (2, 2), (2, 2), (3, 7), (6, 10), (6, 9), (8, 11)];

    /// The names of the color spaces
    fn color_space_name(x: u8) -> Option<&'static str> {
        Some(match x {
            0 => "CameraRGB", 1 => "Rec709", 2 => "REDspace", 3 => "REDcolor", 4 => "sRGB", 5 => "Adobe1998", 6 => "REDcolor2", 8 => "REDcolor3",
            9 => "DRAGONcolor", 11 => "REDcolor4", 12 => "DRAGONcolor2", 13 => "Rec2020", 15 => "REDWideGamutRGB", 16 => "DCI-P3",
            17 => "ProPhotoRGB", 18 => "DCI-P3 D65",
            _ => return None,
        })
    }

    /// The names of the gamma curves
    fn gamma_curve_name(x: u8) -> Option<&'static str> {
        Some(match x {
            0 => "Linear", 1 => "Rec709", 2 => "REDspace", 3 => "REDlog", 4 => "PDlog685", 5 => "PDlog985", 6 => "CustomPDlog", 7 => "REDgamma",
            8 => "sRGB", 9 => "REDlogFilm", 10 => "REDgamma2", 11 => "REDgamma3", 12 => "REDgamma4", 14 => "HDR2084", 15 => "BT1886",
            16 => "Log3G12", 18 => "Log3G10", 19 => "HybridLogGamma", 20 => "Gamma2.2", 21 => "Gamma2.6",
            _ => return None,
        })
    }

    /// Inserts into `md` the keys these values give, the image being `image_size` (width, height) when known
    fn insert(self, md: &mut serde_json::Map<String, serde_json::Value>, image_size: Option<(u32, u32)>) {
        use serde_json::Value;
        let float = |x: f32| Value::from(x as f64);
        // Gains of 0 are 1
        if let Some(gains) = self.gains {
            let gains = if gains == [0.0; 3] { [1.0; 3] } else { gains };
            for (key, g) in ["digital_gain_red", "digital_gain_green", "digital_gain_blue"].into_iter().zip(gains) {
                md.insert(key.into(), float(g * self.gain.unwrap_or(1.0)));
            }
        }
        // A guide of an empty rectangle is left out, and the enabled ones are those of a rectangle when the clip doesn't say.
        // A guide is in fractions of the image (its top left corner of the image's last pixel column and row), and the frame
        // guide is the whole image when it doesn't fit in it. Without the image's size it's in pixels
        for (i, (name, rect)) in ["frame_guide", "user_action_guide", "user_title_guide"].into_iter().zip(self.guides).enumerate() {
            let Some([x, y, w, h]) = rect.filter(|r| r[2] != 0 && r[3] != 0).map(|r| r.map(u32::from)) else { continue };
            md.insert(format!("{name}_enabled"), u8::from(self.guides_enabled.is_none_or(|bits| bits & (1 << i) != 0)).into());
            let Some((width, height)) = image_size else {
                md.insert(name.into(), serde_json::json!([x, y, w, h]));
                continue;
            };
            let (x, y, w, h) = if i == 0 && (x + w > width || y + h > height) {
                md.insert("frame_guide_name".into(), "FULL".into());
                (0, 0, width, height)
            } else {
                (x, y, w, h)
            };
            md.insert(format!("{name}_topleft_x"), float(x as f32 / width.saturating_sub(1).max(1) as f32));
            md.insert(format!("{name}_topleft_y"), float(y as f32 / height.saturating_sub(1).max(1) as f32));
            md.insert(format!("{name}_width"), float(w as f32 / width as f32));
            md.insert(format!("{name}_height"), float(h as f32 / height as f32));
        }
        // The sensor's ID and name, for a clip that doesn't have them
        let sensor = match self.sensor_type {
            Some(6) if self.camera_model_id == Some(12) => Some(("DRAGON 5K S35", 9)),
            Some(6) => Some(("DRAGON 6K S35", 4)),
            Some(8) => Some((if self.olpf_id == Some(22) { "DRAGON 4K S35" } else { "DRAGON 4.5K S35" }, 8)),
            Some(t) => Self::SENSORS.iter().find(|s| s.0 == t).map(|s| (s.1, s.2)),
            None => None,
        };
        if let Some((name, id)) = sensor {
            md.entry("sensor_id").or_insert(id.into());
            if !name.is_empty() {
                md.entry("sensor_name").or_insert(name.into());
            }
        }
        if let Some(name) = self.olpf_id.and_then(|id| Self::OLPF_NAMES.get(id as usize)).filter(|x| !x.is_empty()) {
            md.entry("sensor_olpf_name").or_insert((*name).into());
        }
        if let Some(mode) = self.anamorphic_mode {
            let squeeze: f32 = match mode { 1 => 2.0, 2 => 1.3, 3 => 1.25, 4 => 1.6, 5 => 1.8, 6 => 1.5, _ => 1.0 };
            md.insert("pixel_aspect_ratio".into(), float(squeeze));
        }
        if let Some(aperture) = self.aperture.filter(|&x| x != 0) {
            let prefix = if self.aperture_units == Some(1) { "T" } else { "f/" };
            let label = match (aperture, aperture % 10) {
                (0xffff, _) => "closed".to_owned(),
                (_, 0) => format!("{prefix}{}", aperture / 10),
                (_, tenth) => format!("{prefix}{}.{tenth}", aperture / 10),
            };
            md.insert("lens_aperture_label".into(), label.into());
        }
        // The REDCODE by its compression ratio, rounded up, or the data rate of a RED ONE, for a clip that has no name of it
        if !md.contains_key("redcode") {
            let name = match (self.compression.map(|x| x.div_ceil(100)).filter(|x| (1..100).contains(x)), self.data_rate) {
                (Some(ratio), _) => format!("REDcode {ratio}:1"),
                (None, Some(28)) => "REDcode 10:1 (REDcode 28)".into(),
                (None, Some(36)) => "REDcode 9:1 (REDcode 36)".into(),
                (None, Some(42)) => "REDcode 7.5:1 (REDcode 42)".into(),
                _ => String::new(),
            };
            if !name.is_empty() {
                md.insert("redcode".into(), name.into());
            }
        }
        // The record frame rate: its numerator over 1001 when the clip has it, else its float, rounded to whole frames, and
        // then its fraction. The NTSC rates of 24, 30, 48 and 60 fps are 23.976, 29.97, 47.95 and 59.94
        let ntsc = |fps: f32| match fps {
            x if x > 23.0 && x < 23.99 => Some((23.976, 24000)),
            x if x > 29.0 && x < 29.99 => Some((29.97, 30000)),
            x if x > 47.0 && x < 47.99 => Some((47.95, 48000)),
            x if x > 59.0 && x < 59.99 => Some((59.94, 60000)),
            _ => None,
        };
        if let Some(numerator) = self.record_numerator {
            let fps = numerator as f32 / 1001.0;
            md.insert("record_framerate".into(), float(ntsc(fps).map_or(fps, |x| x.0)));
        } else if let Some(fps) = self.record_rate {
            let (fps, numerator, denominator) = match ntsc(fps) {
                Some((fps, numerator)) => (fps, numerator, 1001),
                None => ((fps + 0.5).floor(), (fps + 0.5).floor() as u32, 1),
            };
            md.insert("record_framerate".into(), float(fps));
            md.insert("record_framerate_numerator".into(), numerator.into());
            md.insert("record_framerate_denominator".into(), denominator.into());
        }
        // The clip's default color version (2 legacy, 3 IPP2, 4 broadcast) by its current color science, the recorded one
        // when it has none: 4 is IPP2 and 5 broadcast. Its exposure adjust is IPP2's, or the legacy look's FLUT
        let ipp2 = match self.color_version {
            Some((original, current)) => {
                let original = original.min(5);
                let current = current.map_or(0, |c| c.min(5).max(original));
                let current = if current == 0 { original } else { current };
                md.insert("clip_default_color_version".into(), (match current { 4 => 3, 5 => 4, _ => 2 }).into());
                current >= 4
            },
            None => true,
        };
        if let Some(x) = self.exposure_adjust {
            md.insert((if ipp2 { "exposure_adjust" } else { "flut_control" }).into(), float(x));
        }
        // The color space and gamma curve: IPP2's unless 0xff, the legacy look's then, each within its range. The clip's
        // gamma curves 14, 15 and 16 are Log3G12, HDR2084 and BT1886. Without either, the preset
        let pick = |pair: &[u8], max: u8| -> Option<u8> {
            let legacy = *pair.first().filter(|&&x| x != 0xff)?;
            let x = pair.get(1).copied().filter(|&x| x != 0xff).unwrap_or(legacy);
            Some(if x > max { legacy.min(max) } else { x })
        };
        let color_space = self.color_space.as_deref().and_then(|pair| pick(pair, 18));
        let gamma_curve = self.gamma_curve.as_deref().and_then(|pair| {
            pick(&pair.iter().map(|&x| match x { 14 => 16, 15 => 14, 16 => 15, x => x }).collect::<Vec<_>>(), 21)
        });
        let preset = self.color_preset.map(|x| Self::COLOR_PRESETS.get(x as usize).copied().unwrap_or((2, 2)));
        let (color_space, gamma_curve) = match (color_space, gamma_curve, preset) {
            (Some(c), Some(g), _) => (Some(c), Some(g)),
            (_, _, Some((c, g))) => (Some(c), Some(g)),
            (c, g, None) => (c, g),
        };
        for (key, x, name) in [("color_space", color_space, color_space.and_then(Self::color_space_name)),
                               ("gamma_curve", gamma_curve, gamma_curve.and_then(Self::gamma_curve_name))] {
            if let Some(x) = x {
                md.insert(key.into(), x.into());
            }
            if let Some(name) = name {
                md.insert(format!("{key}_name"), name.into());
            }
        }
    }
}
