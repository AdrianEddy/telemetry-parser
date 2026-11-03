// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2021 Adrian <adrian.eddy at gmail>

//! Walking the KLVs of an MXF file (SMPTE ST 336, ST 377-1), for the parsers of formats in MXF

use std::io::*;
use std::sync::atomic::{ AtomicBool, Ordering };
use byteorder::ReadBytesExt;

use crate::util;

/// The first bytes of a key of an MXF file, a SMPTE UL's (ST 336)
pub(crate) const KEY_START: u32 = 0x060e2b34;
/// The partition packs' keys, ahead of their kind (2 the header partition's, 3 a body partition's, 4 the footer
/// partition's), their status and 0 (ST 377-1 §7.1)
pub(crate) const PARTITION_PREFIX: [u8; 13] = [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x05, 0x01, 0x01, 0x0d, 0x01, 0x02, 0x01, 0x01];
/// The primer pack (ST 377-1 §9.3)
pub(crate) const PRIMER: [u8; 16] = [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x05, 0x01, 0x01, 0x0d, 0x01, 0x02, 0x01, 0x01, 0x05, 0x01, 0x00];

/// Two MXF keys, without byte 8 (the registry version, ST 336)
pub(crate) fn same_ul(a: &[u8], b: &[u8]) -> bool {
    a.len() == 16 && b.len() == 16 && a[..7] == b[..7] && a[8..] == b[8..]
}

/// From the stream's position, the next KLV that lies before `end`: its key and the length of its value, with the stream
/// at the value. Bytes that don't start a key, and a key whose length can't be one (a BER length of more than 8 bytes,
/// which SMPTE 379M §5.3.4 and FFmpeg's demuxer don't allow, or one running past `end`, which can't be stepped over) are
/// passed by the next key ([`next_key`]). `None` at `end`, at the end of the stream or when cancelled
pub(crate) fn next_klv<R: Read + Seek>(stream: &mut BufReader<R>, end: u64, cancel: &AtomicBool, progress: &impl Fn(u64)) -> Result<Option<([u8; 16], u64)>> {
    let mut id = [0u8; 16];
    loop {
        if cancel.load(Ordering::Relaxed) || end.saturating_sub(stream.stream_position()?) < 17 || stream.read_exact(&mut id).is_err() {
            return Ok(None);
        }
        if id[..4] != KEY_START.to_be_bytes() {
            log::warn!("Unknown ID {} at 0x{:08x}", util::to_hex(&id), stream.stream_position()? - 16);
            // The next key may start in the bytes just read
            stream.seek_relative(-15)?;
            next_key(stream, end, cancel, progress)?;
            continue;
        }
        let Ok(length) = read_ber(stream) else { return Ok(None) };

        let position = stream.stream_position()?;
        progress(position);
        match length.filter(|length| *length <= end.saturating_sub(position)) {
            Some(length) => return Ok(Some((id, length))),
            None => {
                log::warn!("The length of {} at 0x{:08x} can't be one or runs past its end", util::to_hex(&id), position);
                next_key(stream, end, cancel, progress)?;
            }
        }
    }
}

/// From the stream's position, on to the next bytes that start a key (`06.0e.2b.34`, as FFmpeg's demuxer finds one),
/// `end`, or the end of the stream, unless cancelled: every byte is read once
pub(crate) fn next_key<R: Read + Seek>(stream: &mut BufReader<R>, end: u64, cancel: &AtomicBool, progress: &impl Fn(u64)) -> Result<()> {
    let mut position = stream.stream_position()?;
    let mut last = 0u32;
    while position < end && !cancel.load(Ordering::Relaxed) {
        let buffer = stream.fill_buf()?;
        if buffer.is_empty() { break; }
        let buffer = &buffer[..buffer.len().min(usize::try_from(end - position).unwrap_or(usize::MAX))];
        let found = buffer.iter().position(|byte| {
            last = last << 8 | *byte as u32;
            last == KEY_START
        });
        if let Some(i) = found {
            stream.consume(i + 1);
            stream.seek_relative(-4)?;
            break;
        }
        let n = buffer.len();
        stream.consume(n);
        position += n as u64;
        progress(position);
    }
    Ok(())
}

/// A BER length (ST 336 §3.3), `None` for one of more than 8 bytes
pub(crate) fn read_ber<T: Read>(stream: &mut T) -> Result<Option<u64>> {
    let first = stream.read_u8()?;
    if first & 0x80 == 0 { return Ok(Some(first as u64)); }

    let bytes = first & 0x7f;
    if bytes > 8 { return Ok(None); }
    let mut size = 0u64;
    for _ in 0..bytes {
        size = size << 8 | stream.read_u8()? as u64;
    }
    Ok(Some(size))
}

/// The KLVs of `data`, a value of KLVs (a set's or an element's), in order: each one's key and value, up to one that
/// isn't whole in it
pub(crate) fn klvs(mut data: &[u8]) -> impl Iterator<Item = (&[u8; 16], &[u8])> {
    std::iter::from_fn(move || {
        let (key, mut rest) = data.split_first_chunk::<16>()?;
        let length = usize::try_from(read_ber(&mut rest).ok()??).ok()?;
        let value = rest.get(..length)?;
        data = &rest[length..];
        Some((key, value))
    })
}
