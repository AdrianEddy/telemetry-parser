// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2025 Adrian <adrian.eddy at gmail>

//! The private metadata of an MXF file (Content Specification for File Based Canon Video Camera, 6.3.1.1.2 and Annex C): the
//! camera metadata packs of each frame's system item, and the stream file's place in its clip

use serde_json::{ json, Map, Value };

use crate::mxf::{ klvs, same_ul };
use crate::tags_impl::*;
use crate::{ tag, util };
use super::clip_boxes::CLIP_METADATA;
use super::cndm_tags::FRAME_METADATA;
use super::crm_packs::{ focus_position, named };

/// A system item's package metadata set, a frame's first element
const PACKAGE_METADATA: [u8; 16] = hex_literal::hex!("060E2B34 02430101 0D010301 04010203");
/// The last set of the header metadata
const STREAM_FILE_SEQUENCE_INFO: [u8; 16] = hex_literal::hex!("060E2B34 0101010D 0E150000 00010000");
/// A KLV of the package metadata set
const PRIVATE_CAMERA_METADATA: [u8; 16] = hex_literal::hex!("060E2B34 0101010D 0E150001 00010000");

/// The tag of a KLV in a package metadata set
const KLV_METADATA: u8 = 0x88;

/// The KLVs that `parse_klv` reads
pub const KEYS: [[u8; 16]; 2] = [PACKAGE_METADATA, STREAM_FILE_SEQUENCE_INFO];

/// Adds the tags of a KLV of `KEYS`
pub fn parse_klv(key: &[u8; 16], data: &[u8], options: &crate::InputOptions, map: &mut GroupedTagMap) {
    if same_ul(key, &STREAM_FILE_SEQUENCE_INFO) {
        util::insert_tag(map, tag!(CLIP_METADATA, TagId::Metadata, "Stream File Sequence Info", Json, |v| v.to_string(), |d| Ok(stream_file_sequence_info(d.get_ref())), data), options);
        return;
    }

    // Its items are a tag of a byte and a length of 2 (SMPTE ST 385)
    let mut rest = data;
    while let [item, length_high, length_low, value @ ..] = rest {
        let Some((item_value, next)) = value.split_at_checked(u16::from_be_bytes([*length_high, *length_low]) as usize) else { break };
        rest = next;
        if *item == KLV_METADATA {
            for (_, packs) in klvs(item_value).filter(|(key, _)| same_ul(*key, &PRIVATE_CAMERA_METADATA)) {
                util::insert_tag(map, tag!(FRAME_METADATA, TagId::Custom("CameraMetadata".into()), "Private Camera Metadata", Json, |v| v.to_string(), |d| Ok(camera_metadata(d.get_ref())), packs), options);
            }
        }
    }
}

fn hex(data: &[u8]) -> String {
    data.iter().map(|x| format!("{x:02x}")).collect()
}

/// A local set (ST 377-1 §9.6): a tag and a length of 2 bytes each
fn stream_file_sequence_info(mut data: &[u8]) -> Value {
    let mut out = Map::new();
    while let [tag_high, tag_low, length_high, length_low, value @ ..] = data {
        let Some((item, next)) = value.split_at_checked(u16::from_be_bytes([*length_high, *length_low]) as usize) else { break };
        data = next;
        let position = <[u8; 8]>::try_from(item).map(i64::from_be_bytes);
        match (u16::from_be_bytes([*tag_high, *tag_low]), position) {
            (0x3c0a, _)       => { out.insert("InstanceUID".into(), hex(item).into()); }
            (0x4401, _)       => { out.insert("PackageUID".into(), hex(item).into()); }
            (0x1501, Ok(v))   => { out.insert("StartTimecode".into(), v.into()); }
            (0x1201, Ok(v))   => { out.insert("StartPosition".into(), v.into()); }
            _ => { }
        }
    }
    out.into()
}

/// F_Number and T_Number: 0 isn't used, 65534 is closed and 65535 no information
fn aperture(num: u16, den: u16) -> Value {
    match (num, den) {
        (65534, _) | (_, 65534) => "Close".into(),
        (0 | 65535, _) | (_, 0 | 65535) => Value::Null,
        _ => json!(num as f64 / den as f64)
    }
}

/// A value in tenths around 8000h, FFFFh for no information
fn offset_tenths(v: u16) -> Value {
    if v == 0xFFFF { Value::Null } else { json!((v as i32 - 0x8000) as f64 / 10.0) }
}

/// A value in tenths, its largest number for no information
fn tenths(v: u32, none: u32) -> Value {
    if v == none { Value::Null } else { json!(v as f64 / 10.0) }
}

/// The gain in dB of a Master_Gain, 0.5 dB a step from 0 dB at 32, and of a Master_Gain_Delta in 0.1 dB, when it has one
fn gain(master: u16, delta: Option<u8>) -> f64 {
    (master as f64 - 32.0) * 0.5 + delta.map_or(0.0, |x| x as f64 * 0.1)
}

/// The name of a code of 2 bits, whose 3 is no information
fn two_bits(code: u8, names: &[&str]) -> Value {
    named(if code == 3 { 0xff } else { code }, names)
}

/// The packs of a Private Camera Metadata: a count, and packs of an rm_id and 4 bytes (C.2.1)
pub(super) fn camera_metadata(data: &[u8]) -> Value {
    let mut out = Map::new();
    let Some((count, packs)) = data.split_first() else { return out.into() };
    for pack in packs.chunks_exact(5).take(*count as usize) {
        let b = [pack[1], pack[2], pack[3], pack[4]];
        let (first, last) = (u16::from_be_bytes([b[0], b[1]]), u16::from_be_bytes([b[2], b[3]]));
        let whole = u32::from_be_bytes(b);
        let mut put = |key: &str, value: Value| { if !value.is_null() { out.insert(key.into(), value); } };
        match pack[0] {
            0 => put("Focus_Position", focus_position(whole)),
            1 => {
                put("Focal_Length", tenths(first as u32, 0xFFFF));
                put("Electric_Zoom", if last == 0 { "Off".into() } else { tenths(last as u32, 0xFFFF) });
            }
            2 => {
                put("Focus_Mode", two_bits(b[0] >> 6, &["Manual focus mode", "Auto focus mode"]));
                put("Image_Stabilizer", two_bits((b[0] >> 4) & 3, &["Off", "On"]));
                put("Aperture_Mode", named((b[0] >> 3) & 1, &["T Number", "F Number"]));
            }
            3 if !matches!(first, 0 | 65535) && !matches!(last, 0 | 65535) => put("Shutter_Speed", json!(first as f64 / last as f64)),
            4 => {
                put("Set_Mode", two_bits(b[0] >> 6, &["Second", "Degree", "Hertz"]));
                let (num, den) = (u16::from_be_bytes([b[1], b[2]]), b[3]);
                if !matches!(num, 0 | 65535) && !matches!(den, 0 | 255) { put("Shutter_Value", json!(num as f64 / den as f64)); }
            }
            5 => put("F_Number", aperture(first, last)),
            6 => {
                let (mode, preset) = (b[0] >> 5, b[0] & 0x1f);
                put("WB_Mode", if mode == 7 { Value::Null } else { named(mode, &["Auto White Balance", "Color Temperature", "One-push", "Preset", "Manual Gain"]) });
                put("WB_Preset", match preset {
                    30 => "Others".into(),
                    31 => Value::Null,
                    _  => named(preset, &["Candle", "Incandescent lamp", "Fluorescent lamp of low colour temperature", "Fluorescent lamp of high colour temperature", "Sunlight", "Cloudiness", "Shade"])
                });
                put("Accurate_Flag", named(b[1] >> 7, &["Within measurable range", "Out of measurable range"]));
                let temperature = u16::from_be_bytes([b[1] & 0x7f, b[2]]);
                if temperature != 32767 { put("Color_Temperature", temperature.into()); }
                if b[3] != 255 { put("Isotemperature_Line_Index", (b[3] as i16 - 0x80).into()); }
            }
            7 => {
                let (master, delta) = ((b[0] & 0x7f) as u16, b[2] >> 4);
                put("Gain_Mode", named(b[0] >> 7, &["AGC Mode", "Manual mode"]));
                if master != 127 { put("Gain", json!(gain(master, (delta <= 4).then_some(delta)))); }
                if b[1] != 255 { put("ND_Filter", b[1].into()); }
            }
            8 => {
                put("IR_Status", named(b[0] >> 6, &["Off", "On"]));
                let pack_type = b[0] & 0x0f;
                if pack_type == 0 { put("HighSensitivityMode", named(b[1] & 1, &["Off", "On"])); }
            }
            9 if whole != 0xFFFFFFFF => put("ISO_Speed", whole.into()),
            10 => put("T_Number", aperture(first, last)),
            11 => {
                let (delta, master) = ((b[0] >> 4) & 7, first & 0x0fff);
                put("Gain_Mode", named(b[0] >> 7, &["AGC Mode", "Manual mode"]));
                if master != 4095 { put("Gain", json!(gain(master, (delta <= 4).then_some(delta)))); }
                if last != 65535 { put("ND_Filter", last.into()); }
            }
            12 => put("Camera_D_Range", match whole {
                0xFFFFFFFE => "Auto".into(),
                _ => tenths(whole, 0xFFFFFFFF)
            }),
            13 => put("Base_Gamma_D_Range", tenths(whole, 0xFFFFFFFF)),
            14 => put("VFX_Tilt", offset_tenths(first)),
            15 => put("VFX_Roll", offset_tenths(first)),
            16 => put("Focal_Length_No35Conv", tenths(whole, 0xFFFFFFFF)),
            17 => put("Focal_Length_35Conv", tenths(whole, 0xFFFFFFFF)),
            18 => {
                put("SetDistortion", (b[0] >> 7 != 0).into());
                put("AbstatLensValue", offset_tenths(last));
            }
            19 if whole != 0xFFFFFFFF => put("BaseISO", whole.into()),
            20 => put("ISOMode", match b[0] & 3 { 0 => "Auto".into(), 1 => "Manual".into(), 2 => "Reserved".into(), _ => Value::Null }),
            _ => { }
        }
    }
    out.into()
}
