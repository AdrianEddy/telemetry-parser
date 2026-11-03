// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2026 Adrian <adrian.eddy at gmail>

//! ARRI's MXF files, OP1a and frame-wrapped, one element of frame metadata per frame (all files verified here, of SUP 1 to
//! SUP 7 and of ARRIRAW, HDE, ARRICORE and ProRes):
//! - the ALEXA Mini and Mini LF: the frame's dynamic metadata, 256 bytes big-endian (`Frame::of_dynamic`), in the picture
//!   metadata of its system item, and in the header metadata a whole frame header (RDD 30) the camera writes as it closes
//!   the file, whose clip values and camera settings the frames don't have (exposure index, white balance, ND filter) are
//!   the clip's (`header_metadata`);
//! - the ALEXA 35 (RDD 55): the frame's sets of ARRI's items in a data element (`Frame::of_alexa35`), and in the header
//!   metadata JSON documents of the camera, its lens and the clip's slate (`Clip::of_documents`).
//!
//! The frames are timed by the edit rate of the video's track. Only the header metadata and the frames' metadata elements
//! are read, the essence is stepped over

use std::collections::HashMap;
use memchr::memmem;

use super::*;
use crate::mxf::{ klvs, next_key, next_klv, read_ber, same_ul, KEY_START, PARTITION_PREFIX, PRIMER };

/// ALEXA Mini and Mini LF: the picture metadata of a frame's system item, its items a tag (u8), a length (u16) and a value:
/// the item 0x88 is an RDD 18 lens unit set (`LENS_UNIT_SET`) whose item 0x8200 is the frame's dynamic metadata
const PICTURE_METADATA: [u8; 16] = [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x43, 0x01, 0x01, 0x0d, 0x01, 0x03, 0x01, 0x04, 0x01, 0x03, 0x01];
const LENS_UNIT_SET: [u8; 16] = [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x53, 0x01, 0x01, 0x0c, 0x02, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00];
/// ALEXA Mini and Mini LF: the header metadata set (BER lengths) of which an item (`HEADER`) is a frame header of the clip
const HEADER_SET: [u8; 16] = [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x13, 0x01, 0x0d, 0x0f, 0x01, 0x04, 0x02, 0x01, 0x01, 0x01, 0x00];
const HEADER: [u8; 16] = [0x06, 0x0e, 0x2b, 0x34, 0x01, 0x01, 0x01, 0x0d, 0x0f, 0x01, 0x04, 0x01, 0x01, 0x01, 0x01, 0x00];
/// ALEXA 35: a frame's data element of frame metadata, a generic container element (ST 379-1) of a data item (byte 12,
/// 0x17) of ARRI's element type 0x0f (byte 14), of any count of elements in its item (byte 13) and any number (byte 15)
/// (`is_frame_data`): the count isn't the elements' in these files, whose sound item writes 2 for its 6 elements
const FRAME_DATA: [u8; 16] = [0x06, 0x0e, 0x2b, 0x34, 0x01, 0x02, 0x01, 0x01, 0x0d, 0x01, 0x03, 0x01, 0x17, 0x01, 0x0f, 0x00];
/// ALEXA 35: a header metadata set of a JSON document, its items its text (UTF-8) and its schema's URI (UTF-16)
const DOCUMENT_SET: [u8; 16] = [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x53, 0x01, 0x01, 0x0e, 0x17, 0x01, 0x02, 0x10, 0x00, 0x00, 0x00];
const DOCUMENT: [u8; 16] = [0x06, 0x0e, 0x2b, 0x34, 0x01, 0x01, 0x01, 0x0e, 0x0e, 0x17, 0x01, 0x02, 0x10, 0x03, 0x00, 0x00];
const DOCUMENT_SCHEMA: [u8; 16] = [0x06, 0x0e, 0x2b, 0x34, 0x01, 0x01, 0x01, 0x0e, 0x0e, 0x17, 0x01, 0x02, 0x10, 0x04, 0x00, 0x00];
/// The longest frame metadata element read, more than ten times an ALEXA 35's (700 bytes)
const MAX_ELEMENT: u64 = 1 << 13;
/// Reads around the KLVs' headers: the frame metadata of the ALEXA Mini's system item is 400 bytes long, the ALEXA 35's
/// element 700, and the essence between them is stepped over
const BUFFER_SIZE: usize = 32 * 1024;

/// Whether a file starting with `buffer` is one of ARRI's MXF files: it starts with a header partition pack, and the
/// header metadata after it has a frame header (ALEXA Mini and Mini LF) or ARRI's own sets (`0e.17`, ALEXA 35)
pub(super) fn detect(buffer: &[u8]) -> bool {
    header_partition_metadata(buffer).is_some_and(|header| {
        memmem::find(header, b"ARRI\x12\x34\x56\x78").is_some() || memmem::find(header, &DOCUMENT_SET[..10]).is_some()
    })
}

/// The header metadata of the header partition pack `buffer` starts with, as far as `buffer` has it: the
/// `HeaderByteCount` bytes after the pack (ST 377-1 §7.1), and as many more as the pack's KAG, which the fill before
/// them can take (counted from the primer pack, as ARRI's cameras count them, after a fill to their KAG of 512). Only
/// they are searched, not the rest of a file's start and end, which can be a GB
fn header_partition_metadata(buffer: &[u8]) -> Option<&[u8]> {
    let (key, mut rest) = buffer.split_first_chunk::<16>()?;
    if key[..13] != PARTITION_PREFIX || key[13] != 0x02 { return None; }
    let length = usize::try_from(read_ber(&mut rest).ok()??).ok()?;
    let pack = rest.get(..length)?;
    let kag = u32::from_be_bytes(pack.get(4..8)?.try_into().ok()?) as u64;
    let header_bytes = u64::from_be_bytes(pack.get(32..40)?.try_into().ok()?);
    let after = &rest[length..];
    Some(&after[..usize::try_from(header_bytes.saturating_add(kag)).map_or(after.len(), |n| n.min(after.len()))])
}

/// Whether a key is an ALEXA 35's data element of frame metadata (`FRAME_DATA`)
fn is_frame_data(key: &[u8; 16]) -> bool {
    let mut kind = *key;
    (kind[13], kind[15]) = (FRAME_DATA[13], FRAME_DATA[15]);
    same_ul(&kind, &FRAME_DATA)
}

impl ArriRaw {
    /// A sample of the clip's tags (`header_metadata`), then one per frame metadata element: frame k of them is frame k of
    /// the video, at k over the edit rate (known also where the frames aren't, `segment_table::mxf_video_rate`), else the
    /// clip's frame rate. A frame whose element can't be read keeps its place, and the file read up to an error that
    /// stops it is kept, as Sony's MXF files are
    pub(super) fn parse_mxf<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, size: u64, progress_cb: F, cancel_flag: &AtomicBool, options: &crate::InputOptions) -> Result<Vec<SampleInfo>> {
        let rate = crate::segment_table::mxf_video_rate(stream, size).map(|(num, den)| num as f64 / den as f64);
        let (clip, settings) = header_metadata(stream, size, cancel_flag);
        let fps = rate.or(clip.fps);
        let mut samples = vec![self.clip_sample(clip, settings, fps, options)];
        let fps = timing_fps(fps);

        let (mut index, mut without_values) = (0u64, 0u64);
        let mut read = || -> Result<()> {
            stream.seek(SeekFrom::Start(0))?;
            let mut stream = BufReader::with_capacity(BUFFER_SIZE, &mut *stream);
            let progress = |position: u64| progress_cb(position as f64 / size as f64);
            let mut value = Vec::new();
            while let Some((key, length)) = next_klv(&mut stream, size, cancel_flag, &progress)? {
                let alexa35 = is_frame_data(&key);
                if !alexa35 && !same_ul(&key, &PICTURE_METADATA) {
                    stream.seek_relative(length as i64)?;
                    continue;
                }
                let frame = index;
                index += 1;
                if length > MAX_ELEMENT {
                    // A length no element has isn't stepped over, which would lose the frames it runs over
                    log::warn!("ARRI: a frame metadata element of {length} bytes at 0x{:08x}", stream.stream_position()?);
                    next_key(&mut stream, size, cancel_flag, &progress)?;
                    continue;
                }
                value.resize(length as usize, 0);
                stream.read_exact(&mut value)?;
                let values = if alexa35 { Some(Frame::of_alexa35(&value)) } else { dynamic_metadata(&value).map(Frame::of_dynamic) };
                without_values += values.is_none() as u64;
                samples.push(frame_sample(frame, values, fps, options));
                if options.probe_only { break; }
            }
            Ok(())
        };
        if let Err(e) = read() {
            log::warn!("ARRI: the file can't be read after {} frames, which are kept: {e}", samples.len() - 1);
        }
        if without_values > 0 {
            log::warn!("ARRI: {without_values} of {index} frames have no dynamic metadata");
        }
        Ok(samples)
    }
}

/// The clip's metadata in the file's header metadata, from the first of its copies that has it, in their order
/// (`segment_table::mxf_header_metadata`): an ALEXA Mini's or Mini LF's frame header, or an ALEXA 35's JSON documents.
/// The camera writes the frame header into every copy as it closes the file: its frame values are the last frame's (its
/// focus distance is), and its timecode and frame counter are unset in the files here. The clip takes its clip values
/// and the camera's settings (`Frame::settings_of_header`), and of the rest only the inclinometer's reading, which the
/// frames don't have, by a name that says when it was read, `tilt_roll_at_close` (an F003C002 clip's header and footer
/// partitions have tilts of -3.2° and -2.7°, read a moment apart). Only the primer pack and those sets are read
fn header_metadata<T: Read + Seek>(stream: &mut T, size: u64, cancel: &AtomicBool) -> (Clip, Option<Frame>) {
    for copy in crate::segment_table::mxf_header_metadata(stream, size) {
        let mut stream = BufReader::with_capacity(BUFFER_SIZE, &mut *stream);
        if stream.seek(SeekFrom::Start(copy.start)).is_err() { continue; }
        let mut primer = HashMap::new();
        let mut documents = serde_json::Map::new();
        while let Ok(Some((key, length))) = next_klv(&mut stream, copy.end, cancel, &|_| ()) {
            if ![PRIMER, HEADER_SET, DOCUMENT_SET].iter().any(|k| same_ul(&key, k)) {
                if stream.seek_relative(length as i64).is_err() { break; }
                continue;
            }
            let mut value = vec![0; length as usize];
            if stream.read_exact(&mut value).is_err() { break; }
            if same_ul(&key, &PRIMER) {
                primer = primer_of(&value);
                continue;
            }
            let (mut text, mut schema) = (None, None);
            // A set's local tags are the primer's (ST 377-1 §9.3): ARRI's are dynamic, from 0x8000 on
            for (tag, item) in local_items(&value, key[5] == 0x13) {
                match primer.get(&tag) {
                    Some(ul) if same_ul(ul, &HEADER) && ArriRaw::is_header(item) => {
                        let header = Header(item[..HEADER_SIZE].try_into().unwrap());
                        let mut clip = Clip::of_header(&header);
                        if let Some((tilt, roll)) = Frame::inclinometer(&header) {
                            // Tenths of a degree, not their f32's digits
                            clip.md.insert("tilt_roll_at_close".into(), [tilt, roll].map(|v| (v as f64 * 10.0).round() / 10.0).to_vec().into());
                        }
                        return (clip, Some(Frame::settings_of_header(&header)));
                    }
                    Some(ul) if same_ul(ul, &DOCUMENT) => text = Some(item),
                    Some(ul) if same_ul(ul, &DOCUMENT_SCHEMA) => schema = Some(item),
                    _ => {}
                }
            }
            if let Some((name, document)) = text.zip(schema).and_then(|(text, schema)| document(text, schema)) {
                documents.insert(name, document);
            }
        }
        if !documents.is_empty() {
            return (Clip::of_documents(&documents), None);
        }
    }
    (Clip::default(), None)
}

/// The local tags of a primer pack (ST 377-1 §9.3): a count and the size of an entry, 18 bytes, then the entries, each a
/// local tag and its UL
fn primer_of(value: &[u8]) -> HashMap<u16, [u8; 16]> {
    let entries = value.get(8..).filter(|_| value.get(4..8) == Some(&18u32.to_be_bytes()[..])).unwrap_or_default();
    entries.as_chunks::<18>().0.iter().map(|e| (u16::from_be_bytes([e[0], e[1]]), e[2..].try_into().unwrap())).collect()
}

/// The items of a local set (ST 377-1 §9.6): each one's local tag and value, of a length of 2 bytes, or BER-coded in a set
/// whose key says so, up to one that isn't whole in it
fn local_items(mut set: &[u8], ber: bool) -> impl Iterator<Item = (u16, &[u8])> {
    std::iter::from_fn(move || {
        let (tag, mut rest) = set.split_first_chunk::<2>()?;
        let length = if ber {
            usize::try_from(read_ber(&mut rest).ok()??).ok()?
        } else {
            let (length, after) = rest.split_first_chunk::<2>()?;
            rest = after;
            u16::from_be_bytes(*length) as usize
        };
        let value = rest.get(..length)?;
        set = &rest[length..];
        Some((u16::from_be_bytes(*tag), value))
    })
}

/// A JSON document, by the name its schema's URI gives it: the path after `https://www.arri.com/schema/json/camera/`,
/// without the version (`camera_device`, `lens_device`, `slate_info`)
fn document(text: &[u8], schema: &[u8]) -> Option<(String, serde_json::Value)> {
    let uri = char::decode_utf16(schema.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c))).collect::<std::result::Result<String, _>>().ok()?;
    let name = uri.trim_end_matches('\0').strip_prefix("https://www.arri.com/schema/json/camera/")?.rsplit_once('/')?.0.to_owned();
    let end = text.iter().rposition(|&c| c != 0).map_or(0, |i| i + 1);
    Some((name, serde_json::from_slice(&text[..end]).ok()?))
}

/// The dynamic metadata of an ALEXA Mini's or Mini LF's picture metadata element (`PICTURE_METADATA`): of its items 0x88,
/// of the KLVs of each, the first lens unit set's item 0x8200 of 256 bytes or more. As ART's reader
/// (`arrimxf_dyn_metadata`), an item without it is passed by for the next one; unlike it, a longer item 0x8200 isn't
/// refused, its first 256 bytes read, their fields where they are (its version at 0xFC)
fn dynamic_metadata(element: &[u8]) -> Option<&[u8; 256]> {
    let mut items = element;
    while let [tag, a, b, rest @ ..] = items {
        let value = rest.get(..u16::from_be_bytes([*a, *b]) as usize)?;
        items = &rest[value.len()..];
        if *tag != 0x88 { continue; }
        let dynamic = klvs(value).filter(|(key, _)| same_ul(*key, &LENS_UNIT_SET))
            .find_map(|(_, set)| local_items(set, false).find(|(tag, _)| *tag == 0x8200)?.1.first_chunk::<256>());
        if dynamic.is_some() { return dynamic; }
    }
    None
}

impl Frame {
    /// An ALEXA Mini's or Mini LF's dynamic metadata of a frame (`dynamic_metadata`), big-endian, a field with all bits set
    /// unset, its fields where ART (ARRI Reference Tool) reads them into RDD 55's (`arrimxf_dynmeta_legacy_to_dynmeta`):
    /// - its version at 0xFC: 1 and 2 in the MXF files here, written 0x10000 and 0x20000, and 3 in an ALEXA LF's MOV,
    ///   written 3;
    /// - the exposure time in µs at 0x50, and the shutter angle in thousandths of a degree at 0x54, an f32 before version 3;
    /// - the focus distance in mm at 0xA4, whatever unit the camera shows (ART takes it as RDD 55's metric one, never its
    ///   imperial one), 0 unknown and all bits set infinity (RDD 55's -1, as ART keeps it) where the lens has data, a focal
    ///   length or a T-stop, else unset;
    /// - the focal length and the effective focal length in µm at 0xA8 and 0xC0, 0 unknown;
    /// - the T-stop in thousandths at 0xAC, not on the linear scale (`t_stop`) though ART passes it on as RDD 55's linear
    ///   iris: the frame header the camera writes into the same file has 3655 for 2510, 5638 for 4991, 5280 for 4408 and
    ///   5636 for 4988, the same T-stops to 3 digits;
    /// - the lens encoders' raw values, u16, the LDS's and the motor's of focus at 0xB0 and 0xB2, zoom at 0xB4 and 0xB6 and
    ///   iris at 0xB8 and 0xBA, a frame header's order (ART's RDD 55 items 0d.07 to 0d.0c), the Mini LF writing 0 for the
    ///   LDS's;
    /// - the tilt and roll at 0xE0 and 0xE4 as a frame header has them, i16 tenths of a degree in the low 16 bits (as ART
    ///   reads them; all bits set in these files' frames);
    /// - the white balance factors on red and blue at 0xE8 and 0xEC, green's being 1.
    fn of_dynamic(d: &[u8; 256]) -> Self {
        let raw = |at: usize| u32::from_be_bytes(d[at..at + 4].try_into().unwrap());
        let field = |at: usize| Some(raw(at)).filter(|&v| v != u32::MAX);
        let float = |at| field(at).map(f32::from_bits).filter(|v| v.is_finite());
        let version = field(0xFC).map_or(0, |v| if v >= 0x10000 { v >> 16 } else { v });
        let shutter_angle = if version < 3 { float(0x54) } else { field(0x54).map(|v| v as f32) };
        let angle = |at| field(at).map(|v| v as u16 as i16 as f32 / 10.0);
        let focal_length = field(0xA8).filter(|&v| v > 0);
        // RDD 55's -2 and -3 are closed and nearly closed
        let t_stop = field(0xAC).filter(|&v| (v as i32) > 0);
        let mut md = serde_json::Map::new();
        if let (Some(red), Some(blue)) = (float(0xE8), float(0xEC)) {
            md.insert("white_balance_factors".into(), vec![red as f64, 1.0, blue as f64].into());
        }
        if let Some(v) = field(0xC0).filter(|&v| v > 0) {
            md.insert("effective_focal_length".into(), (v as f64 / 1000.0).into());
        }
        let mut encoders = serde_json::Map::new();
        for (i, value) in ["focus", "zoom", "iris"].iter().enumerate() {
            for (j, source) in ["lds", "motor"].iter().enumerate() {
                let at = 0xB0 + i * 4 + j * 2;
                let v = u16::from_be_bytes([d[at], d[at + 1]]);
                if v != u16::MAX { encoders.insert(format!("{value}_{source}"), v.into()); }
            }
        }
        if !encoders.is_empty() { md.insert("lens_encoders".into(), encoders.into()); }
        Self {
            focal_length: focal_length.map(|v| v as f32 / 1000.0),
            focus_distance: match raw(0xA4) {
                0 => None,
                u32::MAX => (focal_length.is_some() || t_stop.is_some()).then_some(f32::INFINITY),
                v => Some(v as f32 / 1000.0),
            },
            t_stop: t_stop.map(|v| v as f32 / 1000.0),
            exposure_time: field(0x50).map(|v| v as f64 / 1000.0),
            shutter_angle: shutter_angle.map(|v| v / 1000.0),
            tilt_roll: angle(0xE0).zip(angle(0xE4)).and_then(|(tilt, roll)| tilt_roll(tilt, roll)),
            md,
            ..Default::default()
        }
    }

    /// An ALEXA 35's frame metadata (RDD 55): sets of ARRI's items (06.0e.2b.34.02.01.01.01.0e.17.01.02.SS), each a KLV of
    /// the UL 06.0e.2b.34.01.01.01.0e.0e.17.01.02.SS.II, big-endian, an item the camera has no value of left out. The
    /// camera's position and orientation (SS 0c: the tilt 03 and the roll 04, f32 degrees), the lens configuration (0d, i32,
    /// as ART's table of RDD 55's items names them: the focus distance in mm 01 or thousandths of an inch 02, 0 unknown and
    /// -1 infinity, the linear iris 03, -1 unknown and -2 and -3 the iris closed or nearly, the axial nominal 05 and
    /// effective 06 focal length in µm, and the encoders' raw values, the lens's (an LDS's) and the motor's of focus 07 and
    /// 08, iris 09 and 0a and focal length 0b and 0c) and the frame's capture (0e: the exposure time 01, a rational in
    /// seconds, the exposure index 03, the white balance 05 in K and its tint 06, f32, the sensor's rate 07, a rational,
    /// and the ND filter's density 08, f32)
    fn of_alexa35(element: &[u8]) -> Self {
        const ENCODERS: [&str; 6] = ["focus_lds", "focus_motor", "iris_lds", "iris_motor", "zoom_lds", "zoom_motor"];
        let mut frame = Self::default();
        let (mut tilt, mut roll, mut focus_imperial) = (None, None, None);
        let mut encoders = serde_json::Map::new();
        for (_, set) in klvs(element) {
            for (key, v) in klvs(set) {
                if key[..4] != KEY_START.to_be_bytes() || key[8..12] != [0x0e, 0x17, 0x01, 0x02] { continue; }
                let uint = (v.len() <= 8).then(|| v.iter().fold(0u64, |n, &b| n << 8 | b as u64));
                let int = <[u8; 4]>::try_from(v).ok().map(i32::from_be_bytes);
                let distance = |per_unit: f64| match int? { -1 => Some(f32::INFINITY), v if v > 0 => Some((v as f64 * per_unit) as f32), _ => None };
                let float = <[u8; 4]>::try_from(v).ok().map(f32::from_be_bytes).filter(|v| v.is_finite());
                let rational = <[u8; 8]>::try_from(v).ok().map(|r| (u32::from_be_bytes([r[0], r[1], r[2], r[3]]), u32::from_be_bytes([r[4], r[5], r[6], r[7]])))
                    .filter(|r| r.1 > 0).map(|(num, den)| num as f64 / den as f64);
                match (key[12], key[13]) {
                    (0x0c, 0x03) => tilt = float,
                    (0x0c, 0x04) => roll = float,
                    (0x0d, 0x01) => frame.focus_distance = distance(0.001),
                    (0x0d, 0x02) => focus_imperial = distance(0.0000254),
                    (0x0d, 0x03) => frame.t_stop = int.filter(|&v| v > 0).map(|v| t_stop(v as u32)),
                    (0x0d, 0x05) => frame.focal_length = int.filter(|&v| v > 0).map(|v| v as f32 / 1000.0),
                    (0x0d, 0x06) => if let Some(v) = int.filter(|&v| v > 0) { frame.md.insert("effective_focal_length".into(), (v as f64 / 1000.0).into()); },
                    (0x0d, i @ 0x07..=0x0c) => if let Some(v) = uint { encoders.insert(ENCODERS[i as usize - 7].into(), v.into()); },
                    (0x0e, 0x01) => frame.exposure_time = rational.map(|s| s * 1000.0),
                    (0x0e, 0x03) => frame.exposure_index = uint.map(|v| v as u32),
                    (0x0e, 0x05) => frame.white_balance = uint.map(|v| v as u32),
                    (0x0e, 0x06) => if let Some(v) = float { frame.md.insert("white_balance_tint".into(), (v as f64).into()); },
                    (0x0e, 0x07) => if let Some(v) = rational { frame.md.insert("sensor_fps".into(), v.into()); },
                    // SUP 1 writes it in thousandths (1800 for ND 1.8), SUP 5 as it is: no filter is denser than ND 10
                    (0x0e, 0x08) => frame.nd_density = float.map(|d| if d > 10.0 { d as f64 / 1000.0 } else { d as f64 }),
                    _ => {}
                }
            }
        }
        frame.focus_distance = frame.focus_distance.or(focus_imperial);
        frame.tilt_roll = tilt.zip(roll).and_then(|(tilt, roll)| tilt_roll(tilt, roll));
        if !encoders.is_empty() { frame.md.insert("lens_encoders".into(), encoders.into()); }
        frame
    }
}

impl Clip {
    /// An ALEXA 35's JSON documents (`document`): of the camera, its model, serial number and software (`camera_device`),
    /// of its lens, its model, serial number and squeeze factor (`lens_device`, "2/1" for a 2x anamorphic lens in SUP 5, a
    /// numerator and a denominator in SUP 1, 2000 and 1000), and of the
    /// clip's slate, its reel and the clip's name (`slate_info`)
    fn of_documents(documents: &serde_json::Map<String, serde_json::Value>) -> Self {
        let text = |document: &str, key: &str| documents.get(document)?.get(key)?.as_str().map(str::trim).filter(|x| !x.is_empty()).map(str::to_owned);
        let mut md = serde_json::Map::new();
        for (key, v) in [("camera_firmware", text("camera_device", "cameraSoftwarePackageName")), ("reel", text("slate_info", "reelName")), ("clip_name", text("slate_info", "clipName"))] {
            if let Some(v) = v { md.insert(key.into(), v.into()); }
        }
        let squeeze = documents.get("lens_device").and_then(|x| x.get("lensSqueezeFactor")).and_then(|x| {
            let (num, den) = match x.as_str() {
                Some(text) => text.split_once('/').and_then(|(num, den)| Some((num.trim().parse::<f64>().ok()?, den.trim().parse::<f64>().ok()?)))?,
                None => (x.get("numerator")?.as_f64()?, x.get("denominator")?.as_f64()?),
            };
            Some((num / den) as f32)
        });
        Self {
            model: text("camera_device", "cameraModel"),
            serial: text("camera_device", "cameraSerialNumber"),
            lens: text("lens_device", "lensModel"),
            lens_serial: text("lens_device", "lensSerialNumber"),
            pixel_aspect_ratio: squeeze.filter(|v| v.is_finite() && *v > 0.0),
            fps: None,
            md,
        }
    }
}
