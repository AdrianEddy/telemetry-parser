// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2025 Adrian <adrian.eddy at gmail>

//! What a clip states once in the `moov` of an MP4 or CRM file: in the public boxes of Canon's `uuid` box, `CNOP`'s `CNLG`, `CNLC`,
//! `CNFB`, `CNCM` and `CNID` (XF-AVC S / XF-HEVC S Content Specification, 6.3.1; CRM Specification, 6.), and the camera's make and
//! model, in `udta`'s `manu` and `modl` of an MP4 and the TIFF of a CRM's `CMT1`

use std::io::*;
use byteorder::{ BigEndian, ReadBytesExt };
use serde_json::{ Map, Value };

use super::cndm_tags::code_name;
use crate::gyroflow::cdng::tiff::{ Tiff, find };
use crate::tags_impl::GroupId;

/// The group of what a clip states once, in its first sample. No `Metadata` of `Default`, which Gyroflow takes for the clip's data
pub const CLIP_METADATA: GroupId = GroupId::Custom(std::borrow::Cow::Borrowed("ClipMetadata"));

const CANON_UUID: [u8; 16] = hex_literal::hex!("85C0B687 820F11E0 8111F4CE 462B6A48");

/// No box read here is longer, but the TIFF in `CMT1`
const MAX_BOX: u64 = 64;
const MAX_TIFF: u64 = 64 * 1024;

/// A box: its type and where its content starts and ends
struct Box { kind: [u8; 4], start: u64, end: u64 }

/// The boxes from `start` up to `end`, up to one that isn't whole in there
fn boxes<T: Read + Seek>(stream: &mut T, start: u64, end: u64) -> Result<Vec<Box>> {
    let mut ret = Vec::new();
    let mut position = start;
    while end.saturating_sub(position) >= 8 {
        stream.seek(SeekFrom::Start(position))?;
        let size = stream.read_u32::<BigEndian>()? as u64;
        let mut kind = [0u8; 4];
        stream.read_exact(&mut kind)?;
        let (size, header) = match size {
            0 => (end - position, 8),
            1 => (stream.read_u64::<BigEndian>()?, 16),
            _ => (size, 8)
        };
        let Some(box_end) = position.checked_add(size).filter(|x| size >= header && *x <= end) else { break };
        ret.push(Box { kind, start: position + header, end: box_end });
        position = box_end;
    }
    Ok(ret)
}

fn read_box<T: Read + Seek>(stream: &mut T, item: &Box, max: u64) -> Result<Vec<u8>> {
    let mut data = vec![0; (item.end - item.start).min(max) as usize];
    stream.seek(SeekFrom::Start(item.start))?;
    stream.read_exact(&mut data)?;
    Ok(data)
}

/// An ASCII string of `CNCM`, padded with zeros
fn string(data: &[u8]) -> Value {
    String::from_utf8_lossy(data.split(|x| *x == 0).next().unwrap_or_default()).into_owned().into()
}

fn hex(data: &[u8]) -> Value {
    data.iter().map(|x| format!("{x:02x}")).collect::<String>().into()
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

/// Adds what a box of `CNOP` states to `out`
fn read_public_box(kind: &[u8; 4], data: &[u8], out: &mut Map<String, Value>) {
    match kind {
        b"CNLG" => {
            if let (Some(flag), Some(setting)) = (u32_at(data, 0), u32_at(data, 4)) {
                out.insert("LogGammaFlag".into(), (flag == 1).into());
                out.insert("GammaSetting".into(), match setting { 0 | 1 => "Canon Log", 2 => "Canon Log 2", 3 => "Canon Log 3", _ => "Reserved" }.into());
            }
        }
        b"CNLC" => {
            if let (Some(flag), Some(setting)) = (u32_at(data, 0), u32_at(data, 4)) {
                out.insert("LogColourPrimariesFlag".into(), (flag == 1).into());
                out.insert("ColourPrimariesSetting".into(), match setting { 0 => "Refer to colr", 1 => "Cinema Gamut", _ => "Reserved" }.into());
            }
        }
        b"CNFB" if data.len() >= 52 => {
            out.insert("FileBreakVersion".into(), u32_at(data, 0).into());
            out.insert("FileBreakUID".into(), hex(&data[4..20]));
            out.insert("FileBreakNextID".into(), hex(&data[20..36]));
            out.insert("FileBreakPreID".into(), hex(&data[36..52]));
        }
        b"CNCM" if data.len() >= 43 => {
            out.insert("SceneNumber".into(), string(&data[0..16]));
            out.insert("TakeNumber".into(), string(&data[16..24]));
            out.insert("ShotNumber".into(), string(&data[24..40]));
            out.insert("ReelNumber".into(), u16::from_be_bytes([data[40], data[41]]).into());
            out.insert("LensSqueezeFactor".into(), code_name(data[42], &["Not used", "x1.33", "x2.00", "x1.80", "x1.50"]).into());
            // What a CRM has here is reserved, and zero
            if let Some(file) = data.get(43..47).filter(|x| x[0] != 0) {
                out.insert("ClipMetadataFileType".into(), code_name(file[0], &["Not recorded", "User Memo", "News Metadata"]).into());
                if file[0] == 2 {
                    out.insert("NewsMetadataFileVersion".into(), format!("{}.{}.{}", file[1], file[2], file[3]).into());
                }
            }
        }
        b"CNID" if data.len() >= 32 => {
            out.insert("UMID".into(), hex(&data[..32]));
            // The MSB of the Instance Number is the only difference between a main clip and its proxy
            out.insert("ClipType".into(), if data[13] & 0x80 != 0 { "Proxy" } else { "Main" }.into());
        }
        _ => { }
    }
}

/// Adds the string of `manu` or `modl` to `out`: it follows a full box's version and flags and a language
fn read_user_data(kind: &[u8; 4], data: &[u8], out: &mut Map<String, Value>) {
    let key = if kind == b"manu" { "Make" } else { "Model" };
    if let Some(text) = data.get(6..).filter(|x| x.first().is_some_and(|x| *x != 0)) {
        out.insert(key.into(), string(text));
    }
}

/// Adds the make and model in the first IFD of the TIFF of `CMT1` to `out`
fn read_tiff(data: Vec<u8>, out: &mut Map<String, Value>) -> Option<()> {
    let size = data.len() as u64;
    let mut stream = Cursor::new(data);
    let mut tiff = Tiff::new(&mut stream, size).ok()?;
    let first = tiff.first_ifd().ok()?;
    let ifd = tiff.ifd(first).ok()?;
    for (tag, key) in [(0x010F, "Make"), (0x0110, "Model")] {
        if let Some(text) = find(&ifd, tag).and_then(|x| tiff.text(x)) {
            out.insert(key.into(), text.into());
        }
    }
    Some(())
}

/// What the boxes of a file of `size` bytes state, `None` when there's no box of them
pub fn parse<T: Read + Seek>(stream: &mut T, size: u64) -> Result<Option<Value>> {
    let mut out = Map::new();
    for moov in boxes(stream, 0, size)?.iter().filter(|x| &x.kind == b"moov") {
        let items = boxes(stream, moov.start, moov.end)?;
        for udta in items.iter().filter(|x| &x.kind == b"udta") {
            for item in boxes(stream, udta.start, udta.end)?.iter().filter(|x| matches!(&x.kind, b"manu" | b"modl")) {
                read_user_data(&item.kind, &read_box(stream, item, MAX_BOX)?, &mut out);
            }
        }
        for uuid in items.iter().filter(|x| &x.kind == b"uuid" && x.end - x.start >= 16) {
            let mut id = [0u8; 16];
            stream.seek(SeekFrom::Start(uuid.start))?;
            stream.read_exact(&mut id)?;
            if id != CANON_UUID { continue; }

            for canon in boxes(stream, uuid.start + 16, uuid.end)? {
                match &canon.kind {
                    b"CNOP" => {
                        for item in boxes(stream, canon.start, canon.end)? {
                            read_public_box(&item.kind, &read_box(stream, &item, MAX_BOX)?, &mut out);
                        }
                    }
                    b"CMT1" => { read_tiff(read_box(stream, &canon, MAX_TIFF)?, &mut out); }
                    _ => { }
                }
            }
        }
    }
    Ok((!out.is_empty()).then(|| out.into()))
}
