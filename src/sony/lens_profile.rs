// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2026 Adrian <adrian.eddy at gmail>

// Lens breathing tables, stored as the "Lens profile" item of the MP4 `meta` box: an 'LPIF' KLV local set
// with one profile per lens. Every array starts with a key element and the rest is XOR-masked with it.
// Sony's reader byte-swaps the values twice and keeps them that way, the RTMD focus positions refer to that form.

use std::io::*;
use memchr::memmem;

const UL: [u8; 16] = [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x53, 0x01, 0x01, 0x0e, 0x06, 0x0c, 0x02, 0x01, 0x02, 0x02, 0x41];
const MASK: [u32; 16] = [
    0xabababab, 0xbcbcbcbc, 0xcdcdcdcd, 0xdededede, 0xeaeaeaea, 0xacacacac, 0xbdbdbdbd, 0xcececece,
    0xdadadada, 0xebebebeb, 0xadadadad, 0xbebebebe, 0xcacacaca, 0xdbdbdbdb, 0xecececec, 0xaeaeaeae,
];

pub fn read<T: Read + Seek>(stream: &mut T, size: usize) -> Option<serde_json::Value> {
    let read_size = size.min(2 * 1024 * 1024);
    stream.seek(SeekFrom::End(-(read_size as i64))).ok()?;
    let mut buf = vec![0u8; read_size];
    stream.read_exact(&mut buf).ok()?;

    let mut end = buf.len();
    while let Some(pos) = memmem::rfind(&buf[..end], b"idat") {
        if buf.get(pos + 8..pos + 12) == Some(b"LPIF") {
            return parse(&buf[pos + 4..]);
        }
        end = pos;
    }
    None
}

fn u16_at(d: &[u8], p: usize) -> Option<u16> { Some(u16::from_be_bytes(d.get(p..p + 2)?.try_into().ok()?)) }
fn u32_at(d: &[u8], p: usize) -> Option<u32> { Some(u32::from_be_bytes(d.get(p..p + 4)?.try_into().ok()?)) }

fn parse(d: &[u8]) -> Option<serde_json::Value> {
    let d = d.get(..u32_at(d, 0)? as usize)?;
    if d.get(12..28)? != UL { return None; }
    let (mut p, len) = match d.get(28)? {
        b if b & 0x80 == 0 => (29, *b as usize),
        b => {
            let n = (b & 0x7f) as usize;
            (29 + n, d.get(29..29 + n)?.iter().fold(0usize, |acc, &x| acc << 8 | x as usize))
        }
    };
    let end = (p + len).min(d.len());
    let mut lenses = Vec::new();
    while p + 4 <= end {
        let (tag, len) = (u16_at(d, p)?, u16_at(d, p + 2)? as usize);
        let item = d.get(p + 4..p + 4 + len)?;
        p += 4 + len;
        if (0xff01..=0xff07).contains(&tag) {
            lenses.push(parse_lens(item)?);
        }
    }
    if lenses.is_empty() { None } else { Some(serde_json::Value::Array(lenses)) }
}

fn parse_lens(v: &[u8]) -> Option<serde_json::Value> {
    let (mut lens_id, mut zoom_positions, mut entries) = (0u16, 0u16, 0u16);
    let (mut focus_scale, mut focus_curve, mut magnification) = (Vec::new(), Vec::new(), Vec::new());
    let mut q = 0;
    while q + 4 <= v.len() {
        let (tag, len) = (u16_at(v, q)?, u16_at(v, q + 2)? as usize);
        let item = v.get(q + 4..q + 4 + len)?;
        q += 4 + len;
        match tag {
            0 => lens_id        = u16_at(item, 0)?,
            1 => zoom_positions = u16_at(item, 0)?,
            2 => entries        = u16_at(item, 0)?,
            3 => focus_scale    = unmask32(&array(item, 4)?.map(|x| u32::from_be_bytes(x.try_into().unwrap())).collect::<Vec<_>>()),
            4 => focus_curve    = unmask16(&array(item, 2)?.map(|x| u16::from_be_bytes(x.try_into().unwrap())).collect::<Vec<_>>()),
            5 => magnification  = unmask16(&array(item, 2)?.map(|x| u16::from_be_bytes(x.try_into().unwrap())).collect::<Vec<_>>()),
            _ => { }
        }
    }
    Some(serde_json::json!({
        "lens_id": lens_id,
        "zoom_positions": zoom_positions,
        "entries": entries,
        "focus_scale": focus_scale,
        "focus_curve": focus_curve,
        "magnification": magnification
    }))
}

// base_Array: count, element bits, then the elements
fn array(item: &[u8], elem_size: usize) -> Option<std::slice::ChunksExact<'_, u8>> {
    let count = u32_at(item, 0)? as usize;
    Some(item.get(8..8 + count * elem_size)?.chunks_exact(elem_size))
}

fn unmask16(raw: &[u16]) -> Vec<u16> {
    let Some(key) = raw.first().map(|x| x.swap_bytes()) else { return Vec::new() };
    raw[1..].iter().enumerate().map(|(j, e)| (MASK[((key >> ((j & 3) * 4)) & 15) as usize] as u16 & !key) ^ e.swap_bytes()).collect()
}

fn unmask32(raw: &[u32]) -> Vec<u32> {
    let Some(key) = raw.first().map(|x| x.swap_bytes()) else { return Vec::new() };
    raw[1..].iter().enumerate().map(|(j, e)| (MASK[((key >> ((j & 7) * 4)) & 15) as usize] & !key) ^ e.swap_bytes()).collect()
}
