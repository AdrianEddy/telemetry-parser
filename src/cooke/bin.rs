// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2023 Adrian <adrian.eddy at gmail>

//! Cooke /i lens responses as cameras store them in their metadata: the `lens_cooke_i_static` and `lens_cooke_i_dynamic`
//! entries of R3D clips (RED) and the Cooke binary metadata tags of the RTMD (Sony).
//!
//! The responses are described in Cooke Optics, "Cooke /i Protocol, Part I: Lens Hardware, CORE Commands", version 5.0
//! (2021), section 5, and the responses of earlier lens firmware in Part II, appendix A.1. The decoded records follow the
//! record types of the Cooke /i Lens Metadata File specification.

use std::io::{ Error, ErrorKind, Result };
use serde_json::{ json, Value };

/// Every lens response ends with [l/f][c/r] (Part I, 4.1)
const END: &[u8] = b"\n\r";

/// The ZEISS eXtended Data responses (as a ZEISS CP.3 sends them besides the Cooke /i responses) have a fixed size, their
/// tag included: their shading and distortion values are binary, so [l/f][c/r] can't delimit them. `z` answers the Zd
/// command and `Z` the Zd1 command, which adds the focal length, the depth of field and 7 image heights (as ZEISS Lens
/// Data Viewer 0.10.0 reads them)
const ZEISS_XD_LEN: usize = 35;
const ZEISS_XD1_LEN: usize = 68;

/// The Kdi response (5.1.28): the tag, the sequence number, the length, the Kd lens data (38 bytes), the timestamp and the
/// magnetometer sample, then the inertial data packets
const KDI_HEADER_LEN: usize = 50;
/// An inertial data packet: its type, the timestamp of its last sample and 8 samples of 3 axes
const KDI_PACKET_LEN: usize = 51;

fn b1(v: u8) -> u32 { (v & 0b1) as u32 }
fn b4(v: u8) -> u32 { (v & 0b1111) as u32 }
fn b6(v: u8) -> u32 { (v & 0b111111) as u32 }
fn b7(v: u8) -> u32 { (v & 0b1111111) as u32 }
/// A 24-bit value packed into the low 6 bits of 4 bytes (Kd focus and depth of field distances)
fn b24(d: &[u8]) -> u32 { (b6(d[0]) << 18) | (b6(d[1]) << 12) | (b6(d[2]) << 6) | b6(d[3]) }
fn be_i16(d: &[u8]) -> i16 { i16::from_be_bytes([d[0], d[1]]) }
fn be_u16(d: &[u8]) -> u16 { u16::from_be_bytes([d[0], d[1]]) }

/// An error about the data `d`, which shows its start: enough to recognize it, where `d` can run to the end of the field
fn invalid(what: &str, d: &[u8]) -> Error {
    const SHOWN: usize = 16;
    let more = if d.len() > SHOWN { format!(" … ({} bytes)", d.len()) } else { String::new() };
    Error::new(ErrorKind::InvalidData, format!("Cooke /i data: {what}: {}{more}", pretty_hex::simple_hex(&&d[..d.len().min(SHOWN)])))
}

/// Text of a field. The protocol is ASCII, anything else is kept as replacement characters rather than lost
fn text(d: &[u8]) -> String {
    String::from_utf8_lossy(d).trim().to_string()
}

/// The lens responses of a field, decoded
#[derive(Debug, Default)]
pub struct Responses {
    /// The records of the responses, in order. A response gives all its records, or none when any part of it can't be
    /// decoded
    pub records: Vec<Value>,
    /// Why responses were left out, in order
    pub errors: Vec<Error>,
}

/// Decodes the lens responses in `d`, in order. R3D clips store the responses in fields of a fixed size, padded with NULs,
/// and a field without a lens has nothing but the padding. Responses of the types this module doesn't decode are skipped,
/// as are empty ones, a bare [l/f][c/r]: that delimits itself, and has nothing to decode.
///
/// A response that can't be decoded is left out with an `ErrorKind::InvalidData` error, and one of a type the protocol
/// doesn't have with an `ErrorKind::Unsupported` error when it's ASCII text, which its [l/f][c/r] delimits. Where a
/// response can't be delimited (one of an unknown type that isn't text, or one without [l/f][c/r] where its type puts
/// it), the rest of the field is left out with an `ErrorKind::InvalidData` error
pub fn parse(d: &[u8]) -> Responses {
    responses(d, true)
}

/// The errors of [`parse`], the same and in the same order, from a check of the responses that doesn't build their records:
/// about 200 times cheaper on the Kd and Kdi responses of VENICE 2 clips
pub fn errors(d: &[u8]) -> Vec<Error> {
    responses(d, false).errors
}

/// The responses in `d`, as [`parse`] decodes them, their records built only if `build`
fn responses(mut d: &[u8], build: bool) -> Responses {
    let mut responses = Responses::default();
    while let Some(&tag) = d.first() {
        if tag == 0 {
            if d.iter().any(|&b| b != 0) {
                responses.errors.push(invalid("data after the NUL padding", d));
            }
            break;
        }
        if let Some(rest) = d.strip_prefix(END) {
            d = rest;
            continue;
        }
        let len = match response_len(d) {
            Ok(len) => len,
            Err(e) => { responses.errors.push(e); break; }
        };
        match decode(&d[..len], build) {
            Ok(mut records) => responses.records.append(&mut records),
            Err(e) => responses.errors.push(e),
        }
        d = &d[len + END.len()..];
    }
    responses
}

/// How a response type is delimited
#[derive(Clone, Copy)]
enum Framing {
    /// By the first [l/f][c/r]: the ASCII and the packed binary responses, none of whose bytes is a control character (5.1.3)
    Terminated,
    /// By its length, the big-endian u16 at this offset: the pre-defined binary data packets, whose data can hold any byte.
    /// The length counts from the tag, without [l/f][c/r]
    Length(usize),
    /// By its fixed length, its tag included
    Fixed(usize),
}

/// The name and the framing of the response type that `r` starts with, by its tag: the responses of Part I (5.1) to the
/// CORE commands and the basic responses (4.4), and the ZEISS eXtended Data. `None` for a type the protocol doesn't have
fn response_type(r: &[u8]) -> Option<(&'static str, Framing)> {
    use Framing::*;
    Some(match r {
        [b'N', b'N', ..]                    => ("NN", Length(2)),                // 5.1.35
        [b'N', ..]                          => ("N", Terminated),                // 5.1.1
        [b'D', ..]                          => ("D", Terminated),                // 5.1.2, 5.1.9
        [b'd', ..]                          => ("Kd", Terminated),               // 5.1.3, 5.1.10
        [b'i', ..]                          => ("Kdi", Length(2)),               // 5.1.28
        [b'K', b'3', ..]                    => ("K3", Terminated),               // 5.1.4
        [b'K', b'4', ..]                    => ("K4", Terminated),               // 5.1.5
        [b'K', b'6', b'1', ..]              => ("K61", Length(3)),               // 5.1.29
        [b'K', b'8', ..]                    => ("K8", Terminated),               // 5.1.30
        [b'K', b'9', b'1', ..]              => ("K91", Terminated),              // 5.1.31
        [b'K', b'b', ..]                    => ("Kbn", Terminated),              // 5.1.8
        // KKi, KKd and KKid answer "KKi?" etc. when the lens has no such data
        [b'K', b'K', b'i', b'd', b'?', ..]  => ("KKid", Terminated),
        [b'K', b'K', b'i', b'd', ..]        => ("KKid", Length(4)),              // 5.1.34
        [b'K', b'K', b'i', b'?', ..]        => ("KKi", Terminated),
        [b'K', b'K', b'i', ..]              => ("KKi", Length(3)),               // 5.1.32
        [b'K', b'K', b'd', b'?', ..]        => ("KKd", Terminated),
        [b'K', b'K', b'd', ..]              => ("KKd", Length(3)),               // 5.1.33
        [b'P', ..]                          => ("P", Terminated),                // 5.1.6
        [b'B', ..]                          => ("B", Terminated),                // 5.1.7
        [b'X' | b'Y', ..]                   => ("X/Y", Terminated),              // 5.1.13, 5.1.14
        [b'V' | b'W', ..]                   => ("V/W/Wnn", Terminated),          // 5.1.15 to 5.1.17
        [b'O', b'r', ..]                    => ("OS", Terminated),               // 5.1.23
        [b'O', b't', ..]                    => ("OT", Terminated),               // 5.1.24
        [b'z', ..]                          => ("ZEISS Zd", Fixed(ZEISS_XD_LEN)),
        [b'Z', ..]                          => ("ZEISS Zd1", Fixed(ZEISS_XD1_LEN)),
        // Power-up, acknowledgement, invalid command, channel locked out, loss of program (4.4)
        [b'<' | b'!' | b'?' | b'^' | b'@', ..] | [b'+', b'+', b'+', b'<', ..] => ("status", Terminated),
        _ => return None,
    })
}

/// The length of the response that `d` starts with, without its [l/f][c/r]
fn response_len(d: &[u8]) -> Result<usize> {
    let terminated = || memchr::memmem::find(d, END).ok_or_else(|| invalid("response without [l/f][c/r]", d));
    let len = match response_type(d) {
        Some((_, Framing::Terminated)) => terminated()?,
        Some((_, Framing::Length(at))) => d.get(at..at + 2).map(|v| be_u16(v) as usize).ok_or_else(|| invalid("truncated response", d))?,
        Some((_, Framing::Fixed(len))) => len,
        // A response of a type the protocol doesn't have ends at the first [l/f][c/r] only when it's text: binary data could
        // hold that pair
        None => {
            let len = terminated()?;
            if !d[..len].iter().all(|c| (b' '..=b'~').contains(c)) {
                return Err(invalid("response of an unknown type that isn't text, it can't be delimited", d));
            }
            len
        },
    };
    if d.get(len..len + END.len()) != Some(END) {
        return Err(invalid(&format!("no [l/f][c/r] after the {len} bytes of the response"), d));
    }
    Ok(len)
}

/// Decodes one response `r`, without its [l/f][c/r]. Without `build`, it only checks that `r` can be decoded, and gives no
/// records
fn decode(r: &[u8], build: bool) -> Result<Vec<Value>> {
    let record = match r {
        [b'N', b'N', ..]  => None,
        [b'N', data @ ..] => Some(fixed_data(data)?),
        [b'd', data @ ..] => Some(lens_data(data, build)?),
        [b'i', ..]        => return inertial_data(r, build),
        [b'z', data @ ..] => Some(zeiss_extended_data(data, build)?),
        _                 => None,
    };
    if let Some(record) = record {
        return Ok(if build { vec![record] } else { Vec::new() });
    }
    match response_type(r) {
        Some((name, _)) => if build { log::debug!("Cooke /i data: skipping a {name} response, it isn't decoded") },
        None => return Err(Error::new(ErrorKind::Unsupported, format!("Cooke /i data: skipping a response of an unknown type: {:?}",
            String::from_utf8_lossy(&r[..r.len().min(32)])))),
    }
    Ok(Vec::new())
}

/// 5.1.1 N: fixed data in ASCII format, and the variations of earlier S4/i firmware (Part II, A.1.2)
fn fixed_data(mut d: &[u8]) -> Result<Value> {
    let mut json = serde_json::Map::new();
    json.insert("RecordType".into(), "rt.header.lens.info".into());
    let field = |d: &[u8], len: usize| d.get(1..1 + len).map(|v| v.to_vec()).ok_or_else(|| invalid("truncated N response field", d));
    let number = |d: &[u8], len: usize| -> Result<u32> {
        let v = field(d, len)?;
        if !v.iter().all(u8::is_ascii_digit) { return Err(invalid("N response field isn't a number", d)); }
        Ok(v.iter().fold(0, |n, c| n * 10 + (c - b'0') as u32))
    };
    // With the focal length tagged `f` (S4/i primes), `N` and `n` are the infinity nodal distance, positive and negative
    let mut s4i_prime = false;
    while let Some(&tag) = d.first() {
        let len = match tag {
            b'S' => { json.insert("SerialNumber".into(), text(&field(d, 9)?).into()); 9 },
            b'O' => { json.insert("Owner".into(),        text(&field(d, 31)?).into()); 31 },
            b'L' => { json.insert("LensType".into(),     if field(d, 1)? == b"Z" { "zoom" } else { "prime" }.into()); 1 },
            b'N' | b'n' if s4i_prime => {
                let v = number(d, 3)? as i32;
                json.insert("InfinityNodalDistance".into(), (if tag == b'n' { -v } else { v }).into());
                3
            },
            b'N' | b'f' => { s4i_prime = tag == b'f'; json.insert("MinFocalLength".into(), number(d, 3)?.into()); 3 },
            b'M' => { json.insert("MaxFocalLength".into(), number(d, 3)?.into()); 3 },
            b'U' => { json.insert("Units".into(), if matches!(field(d, 1)?[0], b'I' | b'B') { "imperial" } else { "metric" }.into()); 1 },
            b'T' => { json.insert("TransmissionFactor".into(), (number(d, 2)? as f32 / 100.0).into()); 2 },
            b'E' => {
                let sign = field(d, 1)?[0];
                if sign != b'+' && sign != b'-' { return Err(invalid("N response entrance pupil position without a sign", d)); }
                let v = number(&d[1..], 3)? as i32;
                json.insert("EntrancePupilPosition".into(), (if sign == b'-' { -v } else { v }).into());
                4
            },
            b'B' => { json.insert("FirmwareVersion".into(), text(&field(d, 4)?).into()); 4 },
            // The space characters after the transmission factor and the entrance pupil position
            b' ' => 0,
            _ => return Err(invalid("unknown N response field", d)),
        };
        d = &d[1 + len..];
    }
    Ok(Value::Object(json))
}

/// 5.1.3 Kd: one set of packed binary lens data, after its tag, and the variations of earlier firmware (Part II, A.1.3).
/// Also the lens data of the Kdi response. Without `build`, it only checks the layout, and gives null
fn lens_data(d: &[u8], build: bool) -> Result<Value> {
    if d.len() < 26 { return Err(invalid("truncated Kd response", d)); }
    let (v, rest) = d.split_at(26);
    // The normalized zoom value is 10 bits in 2 bytes (0.000 to 1.000), 7 bits in 1 byte in S4/i zooms up to 1.21 and 1.30
    // (0.00 to 1.00), absent in early S4/i primes; the serial number is absent before 0.21, 0.34, 0.22 and 1.31. The first byte
    // of a 2-byte zoom value is 0x40 to 0x4F and the 1-byte one is 0x80 or more, so neither is the `S` of the serial number
    let (zoom, serial) = match rest {
        []                            => (None, None),
        [z]                           => (Some(b7(*z) as f32 / 100.0), None),
        [z1, z2]                      => (Some(((b4(*z1) << 6) | b6(*z2)) as f32 / 1000.0), None),
        [b'S', s @ ..] if s.len() == 9 => (None, Some(s)),
        [z1, z2, b'S', s @ ..] if s.len() == 9 => (Some(((b4(*z1) << 6) | b6(*z2)) as f32 / 1000.0), Some(s)),
        _ => return Err(invalid("unknown Kd response layout", d)),
    };
    if !build { return Ok(Value::Null); }
    let mut json = json!({
        "RecordType": "rt.temporal.lens.general",
        "FocusDistance": b24(&v[0..4]),
        "ApertureValue": ((b6(v[4]) << 6) | b6(v[5])) as f32 / 100.0,
        "ApertureRingTPosition": {
            "TNumber": ((b1(v[7] >> 6) << 7) | b7(v[6])) as f32 / 10.0,
            "Tenths": b4(v[7])
        },
        "FocalLength": ((b4(v[8]) << 6) | b6(v[9])) as f32, // mm, only for zoom lenses
        "HyperfocalDistance":     b24(&v[10..14]),
        "NearFocusDistance":      b24(&v[14..18]),
        "FarFocusDistance":       b24(&v[18..22]),
        "HorizontalFov":          ((b6(v[22]) << 6) | b6(v[23])) as f32 / 10.0,
        "EntrancePupilPosition":  ((b4(v[24]) << 6) | b6(v[25])) as i32 * if ((v[24] >> 5) & 1) == 1 { -1 } else { 1 },
    });
    let json_map = json.as_object_mut().unwrap();
    if let Some(zoom) = zoom { json_map.insert("NormalizedZoomPosition".into(), zoom.into()); }
    if let Some(serial) = serial { json_map.insert("SerialNumber".into(), text(serial).into()); }
    Ok(json)
}

/// 5.1.28 Kdi: lens plus inertial tracking data, the whole response `r` without its [l/f][c/r]
///
/// The 16-bit inertial samples (the magnetometer, gyro and accelerometer ADC values) are little endian, unlike the
/// big-endian timestamps and length. The specification's tables put the high byte first for all of them, but the lens's own
/// factory calibration turns only the little-endian reading into physical values: in VENICE 2 clips with an S7/i (firmware
/// 7.63), and the LockitPlus recordings of them, the calibrated accelerometer magnitude is 9.83 m/s² (gravity) with a
/// spread of 0.5, and the magnetic field 29 µT, against 19 ± 5 m/s² and 490 µT with the high byte first. The timestamps
/// advance 6000 ticks of the 150 kHz clock per frame at 25 fps only as big-endian values. (LockitPlus writes the values
/// with their bytes swapped, see `RecorderQuirks` in the parent module.)
///
/// The values are kept raw, in records of the `.raw` types, without a unit: the calibration that turns them into physical
/// units is the lens's K61 response, which isn't decoded. Without `build`, it only checks the response, and gives no records
fn inertial_data(r: &[u8], build: bool) -> Result<Vec<Value>> {
    if r.len() < KDI_HEADER_LEN || !(r.len() - KDI_HEADER_LEN).is_multiple_of(KDI_PACKET_LEN) {
        return Err(invalid("Kdi response length isn't 50 + 51 per inertial data packet", r));
    }
    let lens = lens_data(&r[4..42], build)?;
    let packets = r[KDI_HEADER_LEN..].as_chunks::<KDI_PACKET_LEN>().0;
    let record_type = |packet: &[u8; KDI_PACKET_LEN]| match packet[0] {
        1 => Ok("rt.temporal.lens.gyro.raw"),
        2 => Ok("rt.temporal.lens.accelerometer.raw"),
        _ => Err(invalid("Kdi inertial data packet of an unknown type", packet)),
    };
    if !build {
        for packet in packets {
            record_type(packet)?;
        }
        return Ok(Vec::new());
    }
    let xyz = |s: &[u8]| json!({ "X": i16::from_le_bytes([s[0], s[1]]), "Y": i16::from_le_bytes([s[2], s[3]]), "Z": i16::from_le_bytes([s[4], s[5]]) });

    let mut records = vec![
        lens,
        json!({
            "RecordType": "rt.temporal.lens.magnetometer.raw",
            "Timestamp": be_u16(&r[42..44]),
            "Datavals": [xyz(&r[44..50])]
        }),
    ];
    for packet in packets {
        let record_type = record_type(packet)?;
        records.push(json!({
            "RecordType": record_type,
            "Timestamp": be_u16(&packet[1..3]),
            "Datavals": packet[3..].as_chunks::<6>().0.iter().map(|s| xyz(s)).collect::<Vec<_>>()
        }));
    }
    Ok(records)
}

/// ZEISS eXtended Data, after its tag: the Kd focus distance, T-stop ring position, horizontal field of view and entrance
/// pupil position, then 6 shading and 6 distortion values. Without `build`, it only checks the size, and gives null
fn zeiss_extended_data(d: &[u8], build: bool) -> Result<Value> {
    if d.len() != ZEISS_XD_LEN - 1 { return Err(invalid("ZEISS eXtended Data of the wrong size", d)); }
    if !build { return Ok(Value::Null); }
    let mut json = json!({
        "RecordType": "rt.temporal.zeiss.extended.data",
        "FocusDistance": b24(&d[0..4]),
        "ApertureRingTPosition": {
            "TNumber": ((b1(d[5] >> 6) << 7) | b7(d[4])) as f32 / 10.0,
            "Tenths": b4(d[5])
        },
        "HorizontalFov":         ((b6(d[6]) << 6) | b6(d[7])) as f32 / 10.0,
        "EntrancePupilPosition": ((b4(d[8]) << 6) | b6(d[9])) as i32 * if ((d[8] >> 5) & 1) == 1 { -1 } else { 1 }
    });
    let json_map = json.as_object_mut().unwrap();
    json_map.insert("ShadingData".into(),    d[10..22].as_chunks::<2>().0.iter().map(|v| be_i16(v)).collect::<Vec<_>>().into());
    json_map.insert("DistortionData".into(), d[22..34].as_chunks::<2>().0.iter().map(|v| be_i16(v)).collect::<Vec<_>>().into());
    Ok(json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_responses_are_skipped() {
        let kd = [&b"d"[..], &[b'@'; 26], END].concat();
        for field in [[&kd[..], END, &[0; 8]].concat(), [END, &kd[..]].concat(), [&kd[..], END, END, &kd[..]].concat()] {
            let responses = parse(&field);
            assert!(responses.errors.is_empty() && errors(&field).is_empty(), "{:?}", responses.errors);
            assert!(!responses.records.is_empty() && responses.records.iter().all(|x| x["RecordType"] == "rt.temporal.lens.general"));
        }
    }
}
