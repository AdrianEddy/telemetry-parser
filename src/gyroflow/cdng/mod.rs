// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2026 Adrian <adrian.eddy at gmail>

//! Gyroflow Protobuf telemetry in the frames of a CinemaDNG sequence, the carrier of the fpSup firmware mod of the SIGMA
//! fp (https://github.com/ijigen/gyroflow-plugins/blob/main/docs/cdng-protobuf-prototype.md). A local convention, not
//! part of DNG or CinemaDNG.
//!
//! Every frame holds one telemetry block, either in the zero-filled tail of its Exif MakerNote, framed as (little
//! endian, whatever the TIFF byte order):
//! ```text
//!    0  "FSG2"
//!    4  u8   version, 1
//!    5  u8   payload kind: 1 = a serialized gyroflow_proto::Main, 2 = the SIGMA fp's gyro record (see fsg2.rs)
//!    6  u16  reserved, 0
//!    8  u32  payload length n
//!   12  payload
//! 12+n  u32  CRC-32 (IEEE) of bytes 0 .. 12+n
//! ```
//! or, in the older prototype, as a serialized `Main` in IFD0 tag 65000 (BYTE or UNDEFINED), which wins when both are
//! there. The take's first frame has the header. A take of kind 2 records is turned into messages all at once: its
//! timing is fitted over all its frames, and the samples neighbouring blocks share are emitted once.
//!
//! Only the TIFF metadata is read, never the image.

mod tiff;
mod fsg2;
mod lensfit;

use std::collections::BTreeMap;
use std::io::*;
use std::sync::atomic::{ AtomicBool, Ordering::Relaxed };
use memchr::memmem;
use prost::Message;

use crate::filesystem;
use super::gyroflow_proto as pb;
use tiff::{ Tiff, DngInfo };
pub use tiff::Pictures;

const MAGIC: &[u8; 4] = b"FSG2";
const BLOCK_HEADER_SIZE: usize = 12;
const KIND_PROTOBUF: u8 = 1;
const KIND_SIGMA_FP: u8 = 2;
/// Of the older prototype, in IFD0
const PROTOBUF_TAG: u16 = 65000;
/// The longest payload: the fp's records are 2-11 KB
const MAX_PAYLOAD_SIZE: usize = 4 << 20;
/// The frames read to probe a file, enough for the timing of a kind 2 take
const PROBE_FRAMES: usize = 8;

fn invalid(msg: String) -> Error {
    Error::new(ErrorKind::InvalidData, msg)
}

/// CRC-32 with the IEEE polynomial, as zlib's
fn crc32(data: &[u8]) -> u32 {
    const TABLE: [u32; 256] = {
        let mut table = [0u32; 256];
        let mut i = 0;
        while i < 256 {
            let mut c = i as u32;
            let mut k = 0;
            while k < 8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
                k += 1;
            }
            table[i] = c;
            i += 1;
        }
        table
    };
    !data.iter().fold(!0u32, |c, x| TABLE[((c ^ *x as u32) & 0xFF) as usize] ^ (c >> 8))
}

/// The block at `at`, as payload kind and payload, if it is one
fn block_at(data: &[u8], at: usize) -> std::result::Result<(u8, &[u8]), String> {
    let header = data.get(at..at + BLOCK_HEADER_SIZE).ok_or("the block header is cut off")?;
    let (version, kind) = (header[4], header[5]);
    if version != 1 {
        return Err(format!("unsupported block version {version}"));
    }
    if header[6..8] != [0, 0] {
        return Err("the reserved bytes aren't zero".into());
    }
    let len = u32::from_le_bytes(header[8..12].try_into().unwrap()) as usize;
    if len == 0 || len > MAX_PAYLOAD_SIZE {
        return Err(format!("the payload length {len} is invalid"));
    }
    let end = at + BLOCK_HEADER_SIZE + len;
    let crc = data.get(end..end + 4).ok_or("the block runs past the MakerNote")?;
    if crc32(&data[at..end]) != u32::from_le_bytes(crc.try_into().unwrap()) {
        return Err("CRC mismatch".into());
    }
    if kind != KIND_PROTOBUF && kind != KIND_SIGMA_FP {
        return Err(format!("unknown payload kind {kind}"));
    }
    Ok((kind, &data[at + BLOCK_HEADER_SIZE..end]))
}

/// The one valid block in `data`: the magic may also be in the vendor's data, so every one is checked, and two valid
/// blocks are ambiguous
fn find_block(data: &[u8]) -> std::result::Result<(u8, usize, &[u8]), String> {
    let mut found = None;
    let mut rejected = None;
    for at in memmem::find_iter(data, MAGIC) {
        match block_at(data, at) {
            Ok((kind, payload)) if found.is_none() => found = Some((kind, at, payload)),
            Ok(_) => return Err("more than one telemetry block in the MakerNote".into()),
            Err(e) => rejected = Some(e),
        }
    }
    found.ok_or_else(|| match rejected {
        Some(e) => format!("invalid telemetry block in the MakerNote: {e}"),
        None => "no telemetry block in the MakerNote".into(),
    })
}

/// Whether a file starting with `buffer` is a DNG carrying a telemetry block
pub fn detect(buffer: &[u8]) -> bool {
    if !tiff::is_tiff(buffer) { return false; }
    if find_block(buffer).is_ok() { return true; }
    let mut cursor = Cursor::new(buffer);
    Tiff::new(&mut cursor, buffer.len() as u64)
        .and_then(|mut x| { let offset = x.first_ifd()?; x.ifd(offset) })
        .is_ok_and(|ifd0| tiff::find(&ifd0, PROTOBUF_TAG).is_some())
}

/// The frame rate of a CinemaDNG frame (its FrameRate tag)
pub fn frame_rate<T: Read + Seek>(stream: &mut T, size: u64) -> Option<f64> {
    let mut tiff = Tiff::new(stream, size).ok()?;
    let offset = tiff.first_ifd().ok()?;
    let ifd0 = tiff.ifd(offset).ok()?;
    let fps = tiff.numbers(tiff::find(&ifd0, tiff::FRAME_RATE)?)?;
    fps.first().copied().filter(|x| x.is_finite() && *x > 0.0)
}

enum Payload {
    Protobuf(Vec<u8>),
    SigmaFp(Vec<u8>),
}

struct FrameFile {
    payload: Payload,
    /// Where the payload was, for the raw metadata
    carrier: serde_json::Value,
    dng: DngInfo,
}

fn read_frame<T: Read + Seek>(stream: &mut T, size: u64) -> Result<FrameFile> {
    let mut tiff = Tiff::new(stream, size)?;
    let offset = tiff.first_ifd()?;
    let ifd0 = tiff.ifd(offset)?;
    let exif = match tiff::find(&ifd0, tiff::EXIF_IFD) {
        Some(x) => tiff.sub_ifd(x)?,
        None => Vec::new(),
    };
    let dng = DngInfo::read(&mut tiff, &ifd0, &exif);

    if let Some(entry) = tiff::find(&ifd0, PROTOBUF_TAG) {
        if !matches!(entry.kind, 1 | 7) {
            return Err(invalid("The telemetry tag 65000 isn't of type BYTE or UNDEFINED".into()));
        }
        let carrier = serde_json::json!({ "carrier": "ifd0_tag_65000", "offset": tiff.value_offset(entry), "size": entry.count });
        return Ok(FrameFile { payload: Payload::Protobuf(tiff.value(entry)?), carrier, dng });
    }
    let note = tiff::find(&exif, tiff::MAKER_NOTE).ok_or_else(|| invalid("No telemetry: the DNG has no MakerNote".into()))?;
    let data = tiff.value(note)?;
    let (kind, at, payload) = find_block(&data).map_err(invalid)?;
    let carrier = serde_json::json!({
        "carrier":     "makernote",
        "offset":      tiff.value_offset(note).map(|x| x + at as u64),
        "size":        payload.len(),
        "kind":        kind,
        "makernote_size": data.len(),
    });
    let payload = if kind == KIND_SIGMA_FP { Payload::SigmaFp(payload.to_vec()) } else { Payload::Protobuf(payload.to_vec()) };
    Ok(FrameFile { payload, carrier, dng })
}

fn read_frame_at(path: &str) -> Result<FrameFile> {
    let mut file = filesystem::open_file(path)?;
    let size = file.size as u64;
    read_frame(&mut file.file, size)
}

/// `(prefix, number, digits, extension)` of a numbered file name, `clip_000123.dng`
fn numbered(filename: &str) -> Option<(&str, u64, usize, &str)> {
    let (stem, ext) = filename.rsplit_once('.')?;
    let prefix = stem.trim_end_matches(|x: char| x.is_ascii_digit());
    let digits = &stem[prefix.len()..];
    Some((prefix, digits.parse().ok()?, digits.len(), ext))
}

/// The frame files of `path`'s folder that are numbered like it, by number. Of two names of a number (`1.dng` and
/// `0001.dng`) the one written like `path`'s.
fn sequence_files(path: &str) -> BTreeMap<u64, String> {
    let mut files = BTreeMap::new();
    let Some((prefix, _, digits, ext)) = numbered(&filesystem::get_filename(path)).map(|(p, n, d, e)| (p.to_owned(), n, d, e.to_owned())) else { return files };
    for (name, file) in filesystem::list_folder(&filesystem::get_folder(path)) {
        let Some((other_prefix, number, other_digits, other_ext)) = numbered(&name) else { continue };
        if other_prefix == prefix && other_ext.eq_ignore_ascii_case(&ext) && (other_digits == digits || !files.contains_key(&number)) {
            files.insert(number, file);
        }
    }
    files
}

/// The frames of the sequence from `path` on, of its folder's `files` (`sequence_files`): it, and the files numbered
/// after it up to the first missing number. That is the clip Gyroflow opens (FFmpeg reads an image sequence from the
/// frame the user picked up to a gap).
fn sequence(path: &str, files: &BTreeMap<u64, String>) -> Vec<String> {
    let mut out = vec![path.to_owned()];
    if let Some((_, first, _, _)) = numbered(&filesystem::get_filename(path)) {
        out.extend((first + 1..).map_while(|n| files.get(&n).cloned()));
    }
    out
}

/// The first frame of the take `path` is in, of its folder's `files` (`sequence_files`): the lowest of the numbers it
/// continues
fn take_start(path: &str, files: &BTreeMap<u64, String>) -> Option<String> {
    let (_, first, _, _) = numbered(&filesystem::get_filename(path))?;
    let lowest = (0..first).rev().map_while(|n| files.contains_key(&n).then_some(n)).last()?;
    files.get(&lowest).cloned()
}

/// What a frame's DNG adds to its messages: its raw metadata, for the `FrameMetadata` group, and its pictures
pub struct FrameExtras {
    pub metadata: serde_json::Value,
    pub pictures: Option<Pictures>,
}

impl FrameExtras {
    /// The frame's other picture, for a frame `frame` pixels in size: its size, and where the frame's top left corner is
    /// in it. The frame is either of the DNG's pictures, the default crop or the active area, and the decoder shows
    /// one or the other (`Pictures`)
    pub fn other_picture(&self, frame: (u32, u32)) -> Option<((u32, u32), (f32, f32))> {
        let p = self.pictures.filter(|p| p.crop_size != p.image)?;
        let (x, y) = (p.crop_origin.0 as f32, p.crop_origin.1 as f32);
        if frame == p.crop_size {
            Some((p.image, (x, y)))
        } else if frame == p.image {
            Some((p.crop_size, (-x, -y)))
        } else {
            None
        }
    }
}

/// The raw metadata of a frame, for the `FrameMetadata` group
fn frame_json(file: &FrameFile, path: &str, record: Option<&fsg2::Record>) -> serde_json::Value {
    let mut json = serde_json::json!({
        "file": filesystem::get_filename(path),
        "telemetry": file.carrier,
        "dng": file.dng,
    });
    if let Some(record) = record {
        json["sigma_fp"] = record.to_json();
    }
    json
}

/// The messages of the sequence from `path` (open in `stream`) on, in order, and what the frames' DNGs add to them, by
/// position in the sequence
pub fn read_sequence<T: Read + Seek, F: Fn(f64)>(stream: &mut T, size: usize, path: &str, progress_cb: F, cancel_flag: &AtomicBool, probe_only: bool) -> Result<(Vec<pb::Main>, BTreeMap<usize, FrameExtras>)> {
    // The folder is listed once, for the frames from `path` on and for the take's first frame: a take is tens of
    // thousands of files, and the folder can hold others
    let files = sequence_files(path);
    let mut paths = sequence(path, &files);
    if probe_only { paths.truncate(PROBE_FRAMES); }

    // The messages of the Protobuf frames, by position in the sequence
    let mut protobuf: Vec<(usize, pb::Main)> = Vec::new();
    let mut sigma_fp: Vec<fsg2::Frame> = Vec::new();
    let mut raw = BTreeMap::new();
    for (position, frame_path) in paths.iter().enumerate() {
        if cancel_flag.load(Relaxed) { break; }
        let file = if position == 0 {
            read_frame(stream, size as u64)?
        } else {
            match read_frame_at(frame_path) {
                Ok(x) => x,
                // A frame without telemetry is a gap in the timing, which the frame numbers tell
                Err(e) => { log::warn!("{frame_path}: {e}"); continue; }
            }
        };
        match &file.payload {
            Payload::Protobuf(data) => {
                let main = match pb::Main::decode(data.as_slice()) {
                    Ok(x) => x,
                    // A gap, as a frame that can't be read is, but for the frame picked, which the sequence starts at
                    Err(e) if position > 0 => { log::warn!("{frame_path}: invalid Gyroflow Protobuf: {e}"); continue; }
                    Err(e) => return Err(invalid(format!("{frame_path}: invalid Gyroflow Protobuf: {e}"))),
                };
                raw.insert(position, FrameExtras { metadata: frame_json(&file, frame_path, None), pictures: file.dng.pictures() });
                protobuf.push((position, main));
            }
            Payload::SigmaFp(data) => {
                let record = match fsg2::Record::parse(data) {
                    Ok(x) => x,
                    Err(e) if position > 0 => { log::warn!("{frame_path}: {e}"); continue; }
                    Err(e) => return Err(e),
                };
                if sigma_fp.first().is_some_and(|x| x.record.clip_id != record.clip_id) {
                    log::info!("{frame_path} is of another take, the sequence ends before it");
                    break;
                }
                raw.insert(position, FrameExtras { metadata: frame_json(&file, frame_path, Some(&record)), pictures: file.dng.pictures() });
                sigma_fp.push(fsg2::Frame { position, record, dng: file.dng });
            }
        }
        progress_cb(position as f64 / paths.len() as f64);
    }
    if !protobuf.is_empty() && !sigma_fp.is_empty() {
        return Err(invalid("The DNG sequence mixes Gyroflow Protobuf frames with SIGMA fp gyro records".into()));
    }

    if !sigma_fp.is_empty() {
        // The camera writes its lens table on the take's first frame only
        let mut take_table = None;
        if !sigma_fp.iter().any(|x| x.record.lens_table.is_some()) && sigma_fp[0].record.flags & fsg2::FLAG_FIRST_FRAME == 0 {
            if let Some(Payload::SigmaFp(data)) = take_start(path, &files).and_then(|x| read_frame_at(&x).ok()).map(|x| x.payload) {
                take_table = fsg2::Record::parse(&data).ok().filter(|x| x.clip_id == sigma_fp[0].record.clip_id).and_then(|x| x.lens_table);
            }
        }
        return Ok((fsg2::to_messages(&sigma_fp, take_table.as_ref())?, raw));
    }

    // The samples are numbered by their frames' numbers (`FrameIndex`), which tell the gaps of the frames left out above.
    // A take whose frames have none, as `FrameIndex` decides from the first one, would be numbered by a count instead, in
    // which every frame after a gap takes the place of the one before it: it's numbered by the frames' positions in the
    // sequence, as their raw metadata is, from 1 as the proto's frame numbers are
    let has_numbers = protobuf.iter().find_map(|(_, x)| x.frame.as_ref()).is_some_and(|x| x.frame_number > 0);
    let mut protobuf: Vec<pb::Main> = protobuf.into_iter().map(|(position, mut main)| {
        if let Some(frame) = main.frame.as_mut().filter(|_| !has_numbers) {
            frame.frame_number = u32::try_from(position + 1).unwrap_or(u32::MAX);
        }
        main
    }).collect();

    // Only the take's first frame has to have the header
    if protobuf.first().is_some_and(|x| x.header.is_none()) {
        let header = take_start(path, &files).and_then(|x| read_frame_at(&x).ok()).and_then(|x| match x.payload {
            Payload::Protobuf(data) => pb::Main::decode(data.as_slice()).ok()?.header,
            Payload::SigmaFp(_) => None,
        });
        match header {
            Some(header) => protobuf.insert(0, pb::Main { magic_string: "GyroflowProtobuf".into(), protocol_version: 1, header: Some(header), frame: None }),
            None => log::warn!("{path}: the DNG sequence has no Gyroflow Protobuf header"),
        }
    }
    Ok((protobuf, raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn block(payload: &[u8], kind: u8) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        out.extend_from_slice(&[1, kind, 0, 0]);
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(payload);
        let crc = crc32(&out);
        out.extend_from_slice(&crc.to_le_bytes());
        out
    }

    #[test]
    fn crc_is_zlibs() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn the_block_is_found_among_stray_magics_and_checked() {
        let real = block(b"a payload", KIND_PROTOBUF);
        let mut note = b"SIGMA\0\0\0 vendor data FSG2 that isn't a block ".to_vec();
        note.extend_from_slice(&[0; 100]);
        note.extend_from_slice(&real);
        note.extend_from_slice(&[0; 50]);
        assert_eq!(find_block(&note).unwrap(), (KIND_PROTOBUF, note.len() - 50 - real.len(), &b"a payload"[..]));

        let mut two = note.clone();
        two.extend_from_slice(&block(b"another", KIND_SIGMA_FP));
        assert!(find_block(&two).unwrap_err().contains("more than one"));

        let mut corrupt = real.clone();
        corrupt[14] ^= 0xFF;
        assert!(find_block(&corrupt).unwrap_err().contains("CRC"));
        assert!(find_block(&real[..real.len() - 2]).unwrap_err().contains("past the MakerNote"));
        assert!(find_block(&block(b"x", 3)).unwrap_err().contains("kind 3"));
        assert!(find_block(&[0; 64]).unwrap_err().contains("no telemetry block"));
    }

    #[test]
    fn sequence_names() {
        assert_eq!(numbered("A001_874_20261025_000037.DNG"), Some(("A001_874_20261025_", 37, 6, "DNG")));
        assert_eq!(numbered("clip.dng"), None);
    }

    /// A folder of frames for a test, removed after it
    struct Folder(std::path::PathBuf);
    impl Folder {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("telemetry-parser-cdng-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn frame(&self, n: u32) -> String { self.0.join(format!("A001_{n:06}.DNG")).to_string_lossy().to_string() }
    }
    impl Drop for Folder {
        fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
    }

    /// A little endian DNG with no image: IFD0 and an Exif IFD of (tag, type, count, value) entries
    fn dng(mut ifd0: Vec<(u16, u16, u32, Vec<u8>)>, exif: Vec<(u16, u16, u32, Vec<u8>)>) -> Vec<u8> {
        ifd0.push((tiff::EXIF_IFD, 4, 1, vec![0; 4]));
        ifd0.sort_by_key(|x| x.0);
        let ifd_size = |n: usize| 2 + n * 12 + 4;
        let exif_at = 8 + ifd_size(ifd0.len());
        let mut data_at = exif_at + ifd_size(exif.len());
        let (mut ifds, mut data) = (Vec::new(), Vec::new());
        for entries in [&ifd0, &exif] {
            ifds.extend_from_slice(&(entries.len() as u16).to_le_bytes());
            for (tag, kind, count, value) in entries.iter() {
                ifds.extend_from_slice(&tag.to_le_bytes());
                ifds.extend_from_slice(&kind.to_le_bytes());
                ifds.extend_from_slice(&count.to_le_bytes());
                if *tag == tiff::EXIF_IFD {
                    ifds.extend_from_slice(&(exif_at as u32).to_le_bytes());
                } else if value.len() <= 4 {
                    ifds.extend_from_slice(&value.iter().copied().chain([0; 4]).take(4).collect::<Vec<u8>>());
                } else {
                    ifds.extend_from_slice(&(data_at as u32).to_le_bytes());
                    data_at += value.len();
                    data.extend_from_slice(value);
                }
            }
            ifds.extend_from_slice(&0u32.to_le_bytes());
        }
        [&b"II\x2A\0\x08\0\0\0"[..], &ifds, &data].concat()
    }

    fn text(s: &str) -> (u16, u16, u32, Vec<u8>) {
        (0, 2, s.len() as u32 + 1, [s.as_bytes(), &[0]].concat())
    }
    fn tagged(tag: u16, x: (u16, u16, u32, Vec<u8>)) -> (u16, u16, u32, Vec<u8>) { (tag, x.1, x.2, x.3) }

    /// An fp frame (UHD at 25 fps, 1/100 s, a lens without contacts, the camera's default crop) with `payload` in its
    /// MakerNote's zero tail
    fn fp_frame(payload: &[u8], kind: u8) -> Vec<u8> {
        let mut note = b"SIGMA\0\0\0Ver.5.02".to_vec();
        note.extend((0..300u32).map(|x| (x * 31 + 7) as u8));
        note.extend_from_slice(&[0; 200]);
        note.extend_from_slice(&block(payload, kind));
        note.extend_from_slice(&[0; 100]);
        dng(vec![
            (tiff::IMAGE_WIDTH, 3, 1, 3856u16.to_le_bytes().to_vec()),
            (tiff::IMAGE_LENGTH, 3, 1, 2170u16.to_le_bytes().to_vec()),
            tagged(tiff::MAKE, text("SIGMA")),
            tagged(tiff::MODEL, text("SIGMA fp")),
            (tiff::FRAME_RATE, 10, 1, [25i32.to_le_bytes(), 1i32.to_le_bytes()].concat()),
            (tiff::DEFAULT_CROP_ORIGIN, 3, 2, [8u16.to_le_bytes(), 5u16.to_le_bytes()].concat()),
            (tiff::DEFAULT_CROP_SIZE, 3, 2, [3840u16.to_le_bytes(), 2160u16.to_le_bytes()].concat()),
        ], vec![
            (tiff::EXPOSURE_TIME, 5, 1, [1u32.to_le_bytes(), 100u32.to_le_bytes()].concat()),
            (tiff::ISO, 3, 1, 800u16.to_le_bytes().to_vec()),
            (tiff::MAKER_NOTE, 7, note.len() as u32, note),
        ])
    }

    /// The fp's record of frame `seq`: 100 samples from `100·seq`, its Vd 40 samples in
    fn fp_record(seq: u32) -> fsg2::Record {
        fsg2::Record {
            version: 1, orientation: 0, flags: if seq == 0 { fsg2::FLAG_FIRST_FRAME } else { 0 }, clip_id: 0xC0FFEE, frame_seq: seq,
            sample_period_ps: 400_085_400, gyro_scale: 0.000137923, accel_scale: 0.0009765625, readout_ns: 21_325_000, mode: 123,
            window: [0, 294, 6000, 3412], frame_mark: 100, base_index: seq * 100, samples: vec![[100, -20, 15]; 100],
            events: vec![
                fsg2::Event { position: 40, kind: 3, value: (seq * 40_000).to_le_bytes().to_vec() },
                fsg2::Event { position: 1, kind: 1, value: [5i16, -1022, -78].iter().flat_map(|x| x.to_le_bytes()).collect() },
            ],
            lens_table: None, trailing_bytes: 0,
        }
    }

    fn parse(path: &str) -> crate::Input {
        let mut file = std::fs::File::open(path).unwrap();
        let size = file.metadata().unwrap().len() as usize;
        crate::Input::from_stream(&mut file, size, path, |_| (), std::sync::Arc::new(AtomicBool::new(false))).unwrap()
    }

    /// The samples of an IMU group over all the frames
    fn count(input: &crate::Input, group: crate::tags_impl::GroupId) -> usize {
        use crate::tags_impl::*;
        input.samples.as_ref().unwrap().iter().filter_map(|x| x.tag_map.as_ref()?.get(&group)?.get_t(TagId::Data) as Option<&Vec<TimeVector3<f64>>>).map(|x| x.len()).sum()
    }

    #[test]
    fn a_sigma_fp_take_reads_from_the_picked_frame_on() {
        use crate::tags_impl::*;
        let folder = Folder::new("fp");
        for seq in 0..4 {
            std::fs::write(folder.frame(seq + 1), fp_frame(&fsg2::tests::encode(&fp_record(seq)), KIND_SIGMA_FP)).unwrap();
        }
        // Not part of the take: after a gap, and named differently
        std::fs::write(folder.frame(6), fp_frame(&fsg2::tests::encode(&fp_record(5)), KIND_SIGMA_FP)).unwrap();
        std::fs::write(folder.0.join("B001_000002.DNG"), b"II\x2A\0").unwrap();

        let input = parse(&folder.frame(1));
        assert_eq!((input.parser_name(), input.camera_type().as_str(), input.camera_model().map(|x| x.as_str())), ("GyroflowProtobuf", "SIGMA", Some("fp")));
        let samples = input.samples.as_ref().unwrap();
        assert_eq!(samples.iter().map(|x| x.sample_index).collect::<Vec<_>>(), [0, 1, 2, 3]);
        assert_eq!(count(&input, GroupId::Gyroscope), 400);
        assert_eq!(count(&input, GroupId::Accelerometer), 400);
        // The readout of the default crop's rows
        let readout = input.frame_readout_time().unwrap();
        assert!((readout - 21.325 * 2160.0 * 6000.0 / 3856.0 / 3412.0).abs() < 1e-6, "{readout}");
        for (i, x) in samples.iter().enumerate() {
            // The frame is the default crop, and a decoder may show the active area around it
            let imager = &x.tag_map.as_ref().unwrap()[&GroupId::Imager];
            assert_eq!(imager.get_t(TagId::Custom("FrameSize".into())) as Option<&(u32, u32)>, Some(&(3840, 2160)));
            assert_eq!(imager.get_t(TagId::Custom("AlternativeFrameSize".into())) as Option<&(u32, u32)>, Some(&(3856, 2170)));
            assert_eq!(imager.get_t(TagId::Custom("FrameOriginInAlternative".into())) as Option<&(f32, f32)>, Some(&(8.0, 5.0)));
            let raw = x.tag_map.as_ref().unwrap()[&GroupId::Custom("FrameMetadata".into())].get_t(TagId::Data) as Option<&serde_json::Value>;
            let raw = raw.unwrap();
            assert_eq!(raw["file"], format!("A001_{:06}.DNG", i + 1));
            assert_eq!(raw["sigma_fp"]["frame_seq"], i);
            assert_eq!(raw["dng"]["iso"], 800);
        }

        // Picked in the middle: that frame on, timed by its own Vd
        let from_third = parse(&folder.frame(3));
        assert_eq!(from_third.samples.as_ref().unwrap().len(), 2);
        assert_eq!(count(&from_third, GroupId::Gyroscope), 200);
    }

    #[test]
    fn a_protobuf_take_from_a_later_frame_gets_the_header_of_the_first() {
        let folder = Folder::new("protobuf");
        for n in 1..=3u32 {
            let main = pb::Main {
                magic_string: "GyroflowProtobuf".into(),
                protocol_version: 1,
                header: (n == 1).then(|| pb::Header {
                    camera: Some(pb::header::CameraMetadata { camera_brand: "Maker".into(), camera_model: "Cam".into(), imu_orientation: Some("XYZ".into()), ..Default::default() }),
                    clip: Some(pb::header::ClipMetadata { frame_width: 1920, frame_height: 1080, file_frame_rate: 25.0, duration_us: 120_000.0, ..Default::default() }),
                }),
                frame: Some(pb::FrameMetadata {
                    frame_number: n,
                    start_timestamp_us: 1000.0 + n as f64 * 40_000.0,
                    end_timestamp_us: 11_000.0 + n as f64 * 40_000.0,
                    imu: (0..8).map(|i| pb::ImuData { sample_timestamp_us: Some(n as f64 * 40_000.0 + i as f64 * 5000.0), gyroscope_x: 1.0, ..Default::default() }).collect(),
                    ..Default::default()
                }),
            };
            // The first frame in the MakerNote, the others in the prototype's IFD0 tag
            let frame = if n == 1 {
                fp_frame(&main.encode_to_vec(), KIND_PROTOBUF)
            } else {
                let data = main.encode_to_vec();
                dng(vec![(PROTOBUF_TAG, 7, data.len() as u32, data)], vec![])
            };
            std::fs::write(folder.frame(n), frame).unwrap();
        }
        let whole = parse(&folder.frame(1));
        assert_eq!((whole.camera_type().as_str(), whole.samples.as_ref().unwrap().len()), ("Maker", 3));
        let later = parse(&folder.frame(2));
        assert_eq!((later.camera_type().as_str(), later.samples.as_ref().unwrap().len()), ("Maker", 2));
        assert_eq!(later.samples.as_ref().unwrap().iter().map(|x| x.sample_index).collect::<Vec<_>>(), [0, 1]);
    }

    #[test]
    fn a_protobuf_frame_that_doesnt_decode_is_a_gap() {
        use crate::tags_impl::*;
        let folder = Folder::new("protobuf-gap");
        // Cut off: a field of 5 bytes with 1 of them
        let broken = vec![0x0A, 0x05, 0x01];
        for n in 1..=4u32 {
            let main = pb::Main {
                magic_string: "GyroflowProtobuf".into(),
                protocol_version: 1,
                header: (n == 1).then(|| pb::Header {
                    camera: Some(pb::header::CameraMetadata { camera_brand: "Maker".into(), camera_model: "Cam".into(), imu_orientation: Some("XYZ".into()), ..Default::default() }),
                    clip: Some(pb::header::ClipMetadata { frame_width: 1920, frame_height: 1080, file_frame_rate: 25.0, duration_us: 160_000.0, ..Default::default() }),
                }),
                // A producer that doesn't number its frames
                frame: Some(pb::FrameMetadata {
                    start_timestamp_us: 1000.0 + n as f64 * 40_000.0,
                    end_timestamp_us: 11_000.0 + n as f64 * 40_000.0,
                    imu: (0..8).map(|i| pb::ImuData { sample_timestamp_us: Some(n as f64 * 40_000.0 + i as f64 * 5000.0), gyroscope_x: 1.0, ..Default::default() }).collect(),
                    ..Default::default()
                }),
            };
            let data = if n == 3 { broken.clone() } else { main.encode_to_vec() };
            std::fs::write(folder.frame(n), dng(vec![(PROTOBUF_TAG, 7, data.len() as u32, data)], vec![])).unwrap();
        }
        let input = parse(&folder.frame(1));
        let samples = input.samples.as_ref().unwrap();
        assert_eq!(samples.iter().map(|x| x.sample_index).collect::<Vec<_>>(), [0, 1, 3]);
        // Each with its own frame's raw metadata
        for x in samples {
            let raw = x.tag_map.as_ref().unwrap()[&GroupId::Custom("FrameMetadata".into())].get_t(TagId::Data) as Option<&serde_json::Value>;
            assert_eq!(raw.unwrap()["file"], format!("A001_{:06}.DNG", x.sample_index + 1));
        }
        // The frame picked has to decode
        std::fs::write(folder.frame(5), dng(vec![(PROTOBUF_TAG, 7, broken.len() as u32, broken)], vec![])).unwrap();
        assert!(parse(&folder.frame(5)).samples.is_none());
    }

    #[test]
    fn the_other_picture_is_named_whichever_the_frame_is() {
        let extras = |crop: (u32, u32)| FrameExtras { metadata: serde_json::Value::Null, pictures: Some(Pictures { image: (3856, 2170), crop_origin: (8.0, 5.0), crop_size: crop }) };
        assert_eq!(extras((3840, 2160)).other_picture((3840, 2160)), Some(((3856, 2170), (8.0, 5.0))));
        assert_eq!(extras((3840, 2160)).other_picture((3856, 2170)), Some(((3840, 2160), (-8.0, -5.0))));
        // Neither picture, or only one
        assert_eq!(extras((3840, 2160)).other_picture((1920, 1080)), None);
        assert_eq!(extras((3856, 2170)).other_picture((3856, 2170)), None);
    }

    #[test]
    fn other_dngs_are_left_to_the_other_parsers() {
        let plain = dng(vec![tagged(tiff::MAKE, text("Blackmagic"))], vec![]);
        assert!(!detect(&plain));
        assert!(detect(&fp_frame(&fsg2::tests::encode(&fp_record(0)), KIND_SIGMA_FP)));
        let mut cursor = Cursor::new(fp_frame(&[1], KIND_PROTOBUF));
        let size = cursor.get_ref().len() as u64;
        assert_eq!(frame_rate(&mut cursor, size), Some(25.0));
    }

    /// The clip `FP_CLIP_DIR` points to (a folder of SIGMA fp CinemaDNG frames with FSG2 blocks), read through the
    /// whole parser: `FP_CLIP_DIR=F:/clips/fpSup_sample/A001_874 cargo test real_fp_clip -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_fp_clip() {
        use crate::tags_impl::*;
        let dir = std::env::var("FP_CLIP_DIR").expect("FP_CLIP_DIR");
        let mut names: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|x| x.ok()).map(|x| x.path().to_string_lossy().to_string())
            .filter(|x| x.to_ascii_lowercase().ends_with(".dng")).collect();
        names.sort();
        for (label, path) in [("first", &names[0]), ("middle", &names[names.len() / 2])] {
            let mut file = std::fs::File::open(path).unwrap();
            let size = file.metadata().unwrap().len() as usize;
            let started = std::time::Instant::now();
            let input = crate::Input::from_stream(&mut file, size, path, |_| (), std::sync::Arc::new(AtomicBool::new(false))).unwrap();
            let samples = input.samples.as_ref().unwrap();
            println!("{label}: {} {} {:?}, {} samples in {:.2} s, readout {:?} ms", input.parser_name(), input.camera_type(), input.camera_model(), samples.len(), started.elapsed().as_secs_f64(), input.frame_readout_time());
            assert_eq!(input.parser_name(), "GyroflowProtobuf");
            assert!(input.has_accurate_timestamps());
            let gyro: usize = samples.iter().filter_map(|x| x.tag_map.as_ref()?.get(&GroupId::Gyroscope)?.get_t(TagId::Data) as Option<&Vec<TimeVector3<f64>>>).map(|x| x.len()).sum();
            let raw = samples.iter().filter(|x| x.tag_map.as_ref().is_some_and(|x| x.contains_key(&GroupId::Custom("FrameMetadata".into())))).count();
            println!("  gyro samples {gyro}, frames with raw metadata {raw}, sample indexes {:?}..{:?}", samples.first().map(|x| x.sample_index), samples.last().map(|x| x.sample_index));
            let first = samples[0].tag_map.as_ref().unwrap();
            for (group, tags) in first {
                for (tag, v) in tags {
                    let s = v.value.to_string();
                    let raw = *group == GroupId::Custom("FrameMetadata".into());
                    println!("  {group:?} {tag:?}: {}", if s.len() > 300 && !raw { format!("{}...", &s[..300]) } else { s });
                }
            }
            assert_eq!(raw, samples.len());
        }
    }
}
