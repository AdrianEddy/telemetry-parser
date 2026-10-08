// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2026 Adrian <adrian.eddy at gmail>

//! The `moov/udta` `GPMF` box, read one `DEVC` at a time.
//!
//! The box is a KLV stream of `DEVC` containers, and on a 360 body several of
//! them carry the SAME keys for different things: a Fusion writes `POLY`,
//! `CTRX`, `CTRY` and `MFOV` once under `Front Lens` and again under
//! `Back Lens`. Flattening the whole box into one map — which is what the
//! detector does, and must keep doing, because that map is what a host reads
//! the model and the readout time out of — leaves only the last of each. So the
//! geometry is read from a second walk that keeps the devices apart.

use std::io::{Cursor, Seek, SeekFrom};

use crate::tags_impl::{GetWithType, GroupId, TagId, TagMap, TimeVector3, Vector3};

use super::klv::KLV;

/// One `DEVC` of the box: the name it gives itself, and everything under it.
pub(crate) struct Device {
    /// The `DVNM` string — `Front Lens`, `Geometry Calibrations`,
    /// `Global Settings`. Empty when the device did not name itself.
    pub name: String,
    pub tags: TagMap,
}

/// Every `DEVC` of `gpmf` (the box payload, i.e. past the eight-byte box
/// header), each parsed on its own.
pub(crate) fn devices(gpmf: &[u8], options: &crate::InputOptions) -> Vec<Device> {
    let mut out = Vec::new();
    let mut slice = Cursor::new(gpmf);
    while (slice.position() as usize) + 8 <= gpmf.len() {
        let Ok(klv) = KLV::parse_header(&mut slice) else { break };
        let start = slice.position() as usize;
        let len = klv.data_len();
        if len == 0 || start + len > gpmf.len() {
            // A zero-length key is the padding GoPro writes to a fixed box
            // size; anything longer than the box is not a key at all.
            if slice.seek(SeekFrom::Current(klv.aligned_data_len() as i64)).is_err() || len != 0 {
                break;
            }
            continue;
        }
        let payload = &gpmf[start..start + len];
        if slice.seek(SeekFrom::Current(klv.aligned_data_len() as i64)).is_err() {
            break;
        }
        if &klv.key != b"DEVC" || klv.data_type != 0 {
            continue;
        }
        let Ok(mut map) = super::GoPro::parse_metadata(payload, GroupId::Default, true, options) else { continue };
        let Some(tags) = map.remove(&GroupId::Default) else { continue };
        let name = (tags.get_t(TagId::Name) as Option<&String>).map(|s| s.trim().to_owned()).unwrap_or_default();
        out.push(Device { name, tags });
    }
    out
}

/// The device that named itself `name`, or `None`.
pub(crate) fn device<'a>(devices: &'a [Device], name: &str) -> Option<&'a TagMap> {
    devices.iter().find(|d| d.name == name).map(|d| &d.tags)
}

fn id(key: &[u8; 4]) -> TagId {
    TagId::Unknown(u32::from_be_bytes(*key))
}

/// A key's values as `f64`, whichever shape the KLV layer picked for it.
///
/// GPMF types a struct of N values by its element count, so the same
/// `d` array of four doubles arrives as a `TimeVector3` and of five as a
/// `Vec<Vec<_>>`. Every shape has to be accepted or a coefficient list silently
/// reads as absent — which for a lens block means no lens rather than a wrong
/// one, but for the projection means a `.360` that renders as flat video.
pub(crate) fn f64s(tags: &TagMap, key: &[u8; 4]) -> Option<Vec<f64>> {
    let key = id(key);
    if let Some(v) = tags.get_t(key.clone()) as Option<&Vec<Vec<f64>>> {
        return v.first().cloned();
    }
    if let Some(v) = tags.get_t(key.clone()) as Option<&Vec<f64>> {
        return Some(v.clone());
    }
    if let Some(v) = tags.get_t(key.clone()) as Option<&Vec<TimeVector3<f64>>> {
        return v.first().map(|p| vec![p.t, p.x, p.y, p.z]);
    }
    if let Some(v) = tags.get_t(key.clone()) as Option<&Vec<Vector3<f64>>> {
        return v.first().map(|p| vec![p.x, p.y, p.z]);
    }
    if let Some(v) = tags.get_t(key) as Option<&f64> {
        return Some(vec![*v]);
    }
    None
}

/// One `f64`, from a key that carries exactly one.
pub(crate) fn f64_of(tags: &TagMap, key: &[u8; 4]) -> Option<f64> {
    match f64s(tags, key)?.as_slice() {
        [v] => Some(*v),
        _ => None,
    }
}

/// A key's values as `u32`.
pub(crate) fn u32s(tags: &TagMap, key: &[u8; 4]) -> Option<Vec<u32>> {
    let key = id(key);
    if let Some(v) = tags.get_t(key.clone()) as Option<&Vec<Vec<u32>>> {
        return v.first().cloned();
    }
    if let Some(v) = tags.get_t(key.clone()) as Option<&Vec<u32>> {
        return Some(v.clone());
    }
    if let Some(v) = tags.get_t(key) as Option<&u32> {
        return Some(vec![*v]);
    }
    None
}

/// One `u32`, from a key that carries exactly one.
pub(crate) fn u32_of(tags: &TagMap, key: &[u8; 4]) -> Option<u32> {
    match u32s(tags, key)?.as_slice() {
        [v] => Some(*v),
        _ => None,
    }
}

/// A key's values as `i32`, from a `s`-typed (16-bit) list.
pub(crate) fn i16s(tags: &TagMap, key: &[u8; 4]) -> Option<Vec<i32>> {
    let key = id(key);
    if let Some(v) = tags.get_t(key.clone()) as Option<&Vec<Vec<i16>>> {
        return v.first().map(|r| r.iter().map(|x| i32::from(*x)).collect());
    }
    if let Some(v) = tags.get_t(key.clone()) as Option<&Vec<i16>> {
        return Some(v.iter().map(|x| i32::from(*x)).collect());
    }
    if let Some(v) = tags.get_t(key) as Option<&i16> {
        return Some(vec![i32::from(*v)]);
    }
    None
}

/// A key's text — a `c` string or an `F` fourcc.
pub(crate) fn text(tags: &TagMap, key: &[u8; 4]) -> Option<String> {
    (tags.get_t(id(key)) as Option<&String>).map(|s| s.trim_end_matches(['\0', ' ']).to_owned())
}

/// Assembling a `udta` `GPMF` stream, for the tests of the two modules that
/// read one. Written here rather than beside either of them so both drive the
/// same walker the camera's own bytes do, instead of a hand-built [`TagMap`]
/// that could not reproduce the KLV layer's shape-by-element-count typing.
#[cfg(test)]
pub(crate) mod build {
    /// One KLV: four-byte key, type, element size, element count, payload
    /// padded to a four-byte boundary.
    pub(crate) fn klv(key: &[u8; 4], data_type: u8, size: u8, repeat: u16, payload: &[u8]) -> Vec<u8> {
        assert_eq!(usize::from(size) * usize::from(repeat), payload.len(), "{}", String::from_utf8_lossy(key));
        let mut out = key.to_vec();
        out.extend([data_type, size]);
        out.extend(repeat.to_be_bytes());
        out.extend_from_slice(payload);
        while out.len() % 4 != 0 {
            out.push(0);
        }
        out
    }
    pub(crate) fn f64s(key: &[u8; 4], v: &[f64]) -> Vec<u8> {
        let payload: Vec<u8> = v.iter().flat_map(|x| x.to_be_bytes()).collect();
        klv(key, b'd', (v.len() * 8) as u8, 1, &payload)
    }
    pub(crate) fn u32s(key: &[u8; 4], v: &[u32]) -> Vec<u8> {
        let payload: Vec<u8> = v.iter().flat_map(|x| x.to_be_bytes()).collect();
        klv(key, b'L', 4, v.len() as u16, &payload)
    }
    pub(crate) fn i16s(key: &[u8; 4], v: &[i16]) -> Vec<u8> {
        let payload: Vec<u8> = v.iter().flat_map(|x| x.to_be_bytes()).collect();
        klv(key, b's', (v.len() * 2) as u8, 1, &payload)
    }
    pub(crate) fn text(key: &[u8; 4], v: &str) -> Vec<u8> {
        klv(key, b'c', 1, v.len() as u16, v.as_bytes())
    }
    pub(crate) fn fourcc(key: &[u8; 4], v: &[u8; 4]) -> Vec<u8> {
        klv(key, b'F', 4, 1, v)
    }
    /// A `DEVC` container holding `children`, named by a `DVNM` this helper adds.
    pub(crate) fn device(name: &str, children: &[Vec<u8>]) -> Vec<u8> {
        let mut body = text(b"DVNM", name);
        for c in children {
            body.extend_from_slice(c);
        }
        klv(b"DEVC", 0, 1, body.len() as u16, &body)
    }
    pub(crate) fn stream(devices: &[Vec<u8>]) -> Vec<u8> {
        devices.concat()
    }
}
