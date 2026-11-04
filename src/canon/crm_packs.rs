// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2025 Adrian <adrian.eddy at gmail>

//! The timed metadata packs of a CRM frame (CRM Specification, 7.1 to 7.7), which all cameras write, the AcquisitionMetadataPack
//! only the ones since 2023. Their values are little-endian.
//!
//! What has a standard tag gets it, which the frame's acquisition metadata replaces when it has any. All of a pack is a tag of
//! `FRAME_METADATA`, read when it's asked for, without its items of no information

use std::io::*;
use byteorder::{ LittleEndian, ReadBytesExt };
use serde_json::{ json, Map, Value };

use crate::tags_impl::*;
use crate::{ tag, util };
use super::cndm_tags::{ self, FRAME_METADATA, code_name };

pub const RECORDER_TIME_AND_DATE: u32 = 0x00000001;
pub const BINARY_GROUP:           u32 = 0x00000002;
pub const FOCUS:                  u32 = 0x00000003;
pub const ZOOM:                   u32 = 0x00000004;
pub const EXPOSURE:               u32 = 0x00000005;
pub const WHITE_BALANCE:          u32 = 0x04000006;
pub const MENU_SETTING:           u32 = 0x04000007;

/// Size of a pack's Version and Reserved
const HEADER: u64 = 4;

/// Adds the tags of a pack, `data` being what follows its metadataID. Other packs than the ones above have no specification
pub fn parse_pack(id: u32, data: &[u8], options: &crate::InputOptions, map: &mut GroupedTagMap) -> Result<()> {
    let mut d = Cursor::new(data);
    d.set_position(HEADER);
    match id {
        RECORDER_TIME_AND_DATE => {
            let flags = d.read_u8()?;
            let zone_hour = d.read_u8()?;
            let (year, month, day) = (d.read_u16::<LittleEndian>()?, d.read_u8()?, d.read_u8()?);
            let (hour, minute, second) = (d.read_u8()?, d.read_u8()?, d.read_u8()?);
            let millisecond = d.read_u16::<LittleEndian>()?;
            let time = cndm_tags::timestamp_ms(year, month, day, hour, minute, second, if millisecond == 0xFFFF { 0 } else { millisecond })?;
            util::insert_tag(map, tag!(parsed GroupId::Default, TagId::TimestampMs, "Timestamp per frame", f64, |v| format!("{v:?}"), time, Vec::new()), options);

            let zone_minute = flags & 0x3f;
            if let Some(zone) = cndm_tags::time_zone_tag(flags & 0x80 != 0, flags & 0x40 != 0, zone_hour, (zone_minute != 0x3f).then_some(zone_minute)) {
                util::insert_tag(map, zone, options);
            }
        }
        BINARY_GROUP => {
            let groups = data.get(HEADER as usize..HEADER as usize + 4).ok_or(ErrorKind::UnexpectedEof)?;
            let groups = groups.iter().map(|x| format!("{x:02x}")).collect::<String>();
            util::insert_tag(map, tag!(parsed FRAME_METADATA, TagId::Custom("UserBits".into()), "User Bits (Binary Groups 1 to 8)", String, |v| v.to_string(), groups, Vec::new()), options);
        }
        FOCUS => {
            let bits = d.read_u32::<LittleEndian>()?;
            if !matches!(bits, 0 | 0xFFFFFFFE | 0xFFFFFFFF) {
                util::insert_tag(map, tag!(parsed GroupId::Lens, TagId::FocusDistance, "Focus Position", f32, |v| format!("{v:.2}m"), f32::from_bits(bits), Vec::new()), options);
            }
            util::insert_tag(map, tag!(FRAME_METADATA, TagId::Custom("FocusPack".into()), "Focus Pack", Json, |v| v.to_string(), |d| {
                d.set_position(HEADER);
                Ok(known(json!({ "FocusPosition": focus_position(d.read_u32::<LittleEndian>()?) })))
            }, data), options);
        }
        ZOOM => {
            let (focal_length, focal_length_35mm) = (ratio(d.read_u32::<LittleEndian>()?), ratio(d.read_u32::<LittleEndian>()?));
            if let Some(v) = focal_length {
                util::insert_tag(map, tag!(parsed GroupId::Lens, TagId::FocalLength, "Focal Length", f32, |v| format!("{v:.2} mm"), v as f32, Vec::new()), options);
            }
            if let Some(v) = focal_length_35mm {
                util::insert_tag(map, tag!(parsed GroupId::Lens, TagId::LensZoom35mm, "Focal Length (35mm Still Camera Equivalent)", f32, |v| format!("{v:.2} mm"), v as f32, Vec::new()), options);
            }
            util::insert_tag(map, tag!(FRAME_METADATA, TagId::Custom("ZoomPack".into()), "Zoom Pack", Json, |v| v.to_string(), |d| {
                d.set_position(HEADER);
                let (focal_length, focal_length_35mm, electric_zoom) = (d.read_u32::<LittleEndian>()?, d.read_u32::<LittleEndian>()?, d.read_u32::<LittleEndian>()?);
                Ok(known(json!({ "FocalLength": ratio(focal_length), "FocalLength35mm": ratio(focal_length_35mm), "ElectricZoom": ratio(electric_zoom) })))
            }, data), options);
        }
        EXPOSURE => {
            Exposure::read(&mut d)?.insert_tags(options, map);
            util::insert_tag(map, tag!(FRAME_METADATA, TagId::Custom("ExposurePack".into()), "Exposure Pack", Json, |v| v.to_string(), |d| {
                d.set_position(HEADER);
                Ok(Exposure::read(d)?.to_json())
            }, data), options);
        }
        WHITE_BALANCE => {
            d.set_position(HEADER + 4);
            let temperature = d.read_u16::<LittleEndian>()?;
            if temperature != 0xFFFF {
                util::insert_tag(map, tag!(parsed GroupId::Colors, TagId::WhiteBalance, "White Balance", u16, |v| format!("{v}K"), temperature, Vec::new()), options);
            }
            util::insert_tag(map, tag!(FRAME_METADATA, TagId::Custom("WhiteBalancePack".into()), "White Balance Pack", Json, |v| v.to_string(), |d| {
                d.set_position(HEADER);
                let (mode, preset, accurate, compensation, temperature) = (d.read_u8()?, d.read_u8()?, d.read_u8()?, d.read_u8()?, d.read_u16::<LittleEndian>()?);
                Ok(known(json!({
                    "WB_Mode": named(mode, &["Auto white balance", "Color temperature", "One-push", "Preset", "Manual Gain"]),
                    "WB_Preset": white_balance_preset(preset),
                    "AccurateFlag": named(accurate, &["Within measurable range", "Out of measurable range"]),
                    "ColorCompensation": (compensation != 0xFF).then_some(compensation),
                    "ColorTemperature": (temperature != 0xFFFF).then_some(temperature)
                })))
            }, data), options);
        }
        MENU_SETTING => {
            util::insert_tag(map, tag!(FRAME_METADATA, TagId::Custom("MenuSettingPack".into()), "Menu Setting Pack", Json, |v| v.to_string(), |d| menu_setting(d), data), options);
        }
        _ => { }
    }
    Ok(())
}

/// The name of a code in a pack's JSON, none for FFh, no information
pub fn named(code: u8, names: &[&str]) -> Value {
    if code == 0xff { Value::Null } else { code_name(code, names).into() }
}

/// A pack's JSON without what it has no information of
fn known(mut pack: Value) -> Value {
    if let Some(items) = pack.as_object_mut() {
        items.retain(|_, x| !x.is_null());
    }
    pack
}

/// A RATIONAL, two Uint16 of which the first is the numerator. 0 isn't used and FFFFFFFFh is no information
fn ratio(v: u32) -> Option<f64> {
    let (num, den) = (v & 0xFFFF, v >> 16);
    (v != 0xFFFFFFFF && den != 0).then(|| num as f64 / den as f64)
}

/// As meters, except for what's no distance
pub fn focus_position(bits: u32) -> Value {
    match bits {
        0          => "Beyond infinity".into(),
        0xFFFFFFFE => "Infinity".into(),
        0xFFFFFFFF => Value::Null,
        _          => json!(f32::from_bits(bits) as f64)
    }
}

fn white_balance_preset(code: u8) -> Value {
    if code == 254 { return "Other Mode".into(); }
    named(code, &["Candle", "Incandescent lamp", "Fluorescent lamp of low colour temperature", "Fluorescent lamp of high colour temperature", "Sunlight", "Cloudiness", "Shade"])
}

/// The ExposurePack (7.5)
struct Exposure { aperture: u32, exposure_time: u32, sensitivity: (u32, u32), nd_filter: u32, dynamic_range: u32, aperture_mode: u8, exposure_time_mode: u8, sensitivity_mode: u8, nd_filter_mode: u8 }

impl Exposure {
    fn read(d: &mut Cursor::<&[u8]>) -> Result<Self> {
        Ok(Self {
            aperture:           d.read_u32::<LittleEndian>()?,
            exposure_time:      d.read_u32::<LittleEndian>()?,
            sensitivity:       (d.read_u32::<LittleEndian>()?, d.read_u32::<LittleEndian>()?),
            nd_filter:          d.read_u32::<LittleEndian>()?,
            dynamic_range:      d.read_u32::<LittleEndian>()?,
            aperture_mode:      d.read_u8()?,
            exposure_time_mode: d.read_u8()?,
            sensitivity_mode:   d.read_u8()?,
            nd_filter_mode:     d.read_u8()?,
        })
    }

    /// The photographic sensitivity. Its numerator's 80000000h is 0, with a LSB of 1. 0 isn't used and FFFFFFFFh is no information
    fn sensitivity(&self) -> Option<f64> {
        let (num, den) = self.sensitivity;
        (!matches!(num, 0 | 0xFFFFFFFF) && !matches!(den, 0 | 0xFFFFFFFF)).then(|| (num as i64 - 0x80000000) as f64 / den as f64)
    }

    fn insert_tags(&self, options: &crate::InputOptions, map: &mut GroupedTagMap) {
        if let Some(v) = ratio(self.aperture).filter(|_| self.aperture != 0xFFFEFFFE) {
            match self.aperture_mode {
                1 => util::insert_tag(map, tag!(parsed GroupId::Lens, TagId::IrisFStop, "Iris", f32, |v| format!("f/{v:.1}"), v as f32, Vec::new()), options),
                2 => util::insert_tag(map, tag!(parsed GroupId::Lens, TagId::IrisTStop, "Iris", f32, |v| format!("T/{v:.1}"), v as f32, Vec::new()), options),
                _ => { }
            }
        }
        if let Some(v) = ratio(self.exposure_time) {
            let (num, den) = (self.exposure_time & 0xFFFF, self.exposure_time >> 16);
            match self.exposure_time_mode {
                1 => util::insert_tag(map, tag!(parsed GroupId::Exposure, TagId::ShutterSpeed, "Shutter Speed", u32x2, |v| format!("{}/{}s", v.0, v.1), (num, den), Vec::new()), options),
                2 => util::insert_tag(map, tag!(parsed GroupId::Exposure, TagId::ShutterAngle, "Shutter Angle", f32, |v| format!("{v:.1}°"), v as f32, Vec::new()), options),
                3 => util::insert_tag(map, tag!(parsed GroupId::Exposure, TagId::ShutterSpeed, "Shutter Speed", u32x2, |v| format!("{}/{}s", v.0, v.1), (den, num), Vec::new()), options),
                _ => { }
            }
        }
        if let Some(v) = self.sensitivity().filter(|_| self.sensitivity_mode == 1) {
            util::insert_tag(map, tag!(parsed GroupId::Exposure, TagId::ISOValue, "ISO Sensitivity", u16, |v| v.to_string(), v.round().clamp(0.0, u16::MAX as f64) as u16, Vec::new()), options);
        }
    }

    fn to_json(&self) -> Value {
        known(json!({
            "ApertureNumber": if self.aperture == 0xFFFEFFFE { "Close".into() } else { json!(ratio(self.aperture)) },
            "ExposureTime": ratio(self.exposure_time),
            "PhotographicSensitivity": self.sensitivity(),
            "NDFilter": ratio(self.nd_filter),
            "DynamicRange": (!matches!(self.dynamic_range, 0 | 0xFFFFFFFF)).then(|| self.dynamic_range as f64 / 256.0),
            "ApertureMode": named(self.aperture_mode, &["Not used", "F Number", "T Number"]),
            "ExposureTimeMode": named(self.exposure_time_mode, &["Not used", "Second", "Angle", "Frequency"]),
            "PhotographicSensitivityMode": named(self.sensitivity_mode, &["Not used", "ISO Speed", "Gain"]),
            "NDFilterMode": named(self.nd_filter_mode, &["Not used", "Stop notation", "Transmittance notation"]),
        }))
    }
}

/// How a byte of the MenuSettingPack reads. FFh is no information in all of them
#[derive(Clone, Copy)]
enum Menu {
    /// The code, plus this
    Number(i16),
    Names(&'static [&'static str]),
}
use Menu::*;

const OFF_ON: Menu = Names(&["Off", "On"]);
const LEVELS: Menu = Names(&["Off", "Low", "Middle", "High"]);

/// The bytes of the MenuSettingPack (7.7) from WB_ShockLess to ISOMode
const MENU_BYTES: [(&str, Menu); 84] = [
    ("WB_ShockLess", OFF_ON),
    ("AWB_Response", Names(&["High", "Normal", "Low"])),
    ("IR_Status", Names(&["IR Filter Off", "IR Filter On"])),
    ("Reverse", Names(&["Off", "Horizontal", "Vertical", "Horizontal and vertical"])),
    ("ChromaticAberration", OFF_ON),
    ("PeripheralIllumination", OFF_ON),
    ("Diffraction", OFF_ON),
    ("CP_White_Bal_GGain", Number(-50)),
    ("CP_Preset", Names(&["OFF", "BT.709(Neutral)", "BT.2020", "CanonLog", "CanonLog2(BT.709)", "CanonLog2(DCI-P3)", "CanonLog2(BT.2020)", "CanonLog2(C.Gamut)", "CanonLog3(BT.709)", "CanonLog3(DCI-P3)", "CanonLog3(BT.2020)", "CanonLog3(C.Gamut)", "BT.709(Video)"])),
    ("CP_Gamma", Names(&["Canon Log 2", "Canon Log", "Wide DR", "Wide DR (FLAT)", "EOS Std.", "Normal 1 (STANDARD)", "Normal 2 (x4.0)", "Normal 3 (BT.709)", "Normal 4 (x5.0)", "Canon Log 3", "PQ", "HLG", "Canon 709"])),
    ("CP_Color_Space", Names(&["BT.709 Gamut", "BT.2020 Gamut", "DCI-P3 Gamut", "Cinema Gamut"])),
    ("CP_ColorMatrix", Names(&["Off", "EOS Std.", "Video", "Cinema EOS Original", "Production Camera", "Neutral"])),
    ("CP_OtherSettingsActivate", OFF_ON),
    ("CP_Master_PED", Number(-50)),
    ("CP_Master_BLK_R", Number(-50)),
    ("CP_Master_BLK_G", Number(-50)),
    ("CP_Master_BLK_B", Number(-50)),
    ("CP_Black_Gamma_Level", Number(-50)),
    ("CP_Black_Gamma_Range", Number(-20)),
    ("CP_Black_Gamma_Point", Number(-20)),
    ("CP_Low_Key_SAT_Enable", OFF_ON),
    ("CP_Low_Key_SAT_Level", Number(-50)),
    ("CP_KneeEnable", OFF_ON),
    ("CP_KneeSlope", Number(-35)),
    ("CP_KneePoint", Number(50)),
    ("CP_Knee_SAT", Number(-10)),
    ("CP_SharpnessLevel", Number(-10)),
    ("CP_SharpnessH_Detail_FREQ", Number(-8)),
    ("CP_SharpnessCoringLevel", Number(-30)),
    ("CP_SharpnessCoringD_Offset", Number(0)),
    ("CP_SharpnessCoringD_Curve", Number(0)),
    ("CP_SharpnessCoringD_Depth", Number(-4)),
    ("CP_Sharpness_DTL_HV_BAL", Number(-8)),
    ("CP_SharpnessLimit", Number(-50)),
    ("CP_SharpnessSelect", Number(0)),
    ("CP_SharpnessKneeAPT_Gain", Number(0)),
    ("CP_SharpnessKneeAPT_Slope", Number(0)),
    ("CP_SharpnessD_SharpLevel", Number(0)),
    ("CP_SharpnessD_SharpSlope", Number(0)),
    ("CP_SharpnessD_SharpOffset", Number(0)),
    ("CP_NoiseReductionSet_Automatic", OFF_ON),
    ("CP_SkinDetailEffectLevel", LEVELS),
    ("CP_SkinDetailHue", Number(-16)),
    ("CP_SkinDetailChroma", Number(0)),
    ("CP_SkinDetailArea", Number(0)),
    ("CP_SkinDetailY_Level", Number(0)),
    ("CP_SelectiveNR_EffectLevel", LEVELS),
    ("CP_Selective_NR_Hue", Number(0)),
    ("CP_SelectiveNR_Chroma", Number(0)),
    ("CP_SelectiveNR_Area", Number(0)),
    ("CP_SelectiveNR_Y_Level", Number(0)),
    ("CP_ColorMat_Color_Gain", Number(-50)),
    ("CP_ColorMat_Color_Phase", Number(-18)),
    ("CP_ColorMat_R_GMatrix", Number(-50)),
    ("CP_ColorMat_R_BMatrix", Number(-50)),
    ("CP_ColorMat_G_RMatrix", Number(-50)),
    ("CP_ColorMat_G_BMatrix", Number(-50)),
    ("CP_ColorMat_B_RMatrix", Number(-50)),
    ("CP_ColorMat_B_GMatrix", Number(-50)),
    ("CP_White_Bal_RGain", Number(-50)),
    ("CP_White_Bal_BGain", Number(-50)),
    ("CP_Color_Corr_Correct", Names(&["Off", "Area A", "Area B", "Area A&B"])),
    ("CP_Color_Corr_A_Area_SEL_Color_Phase", Number(0)),
    ("CP_Color_Corr_A_Area_SEL_Chroma", Number(0)),
    ("CP_Color_Corr_A_Area_SEL_Area", Number(0)),
    ("CP_Color_Corr_A_Area_SEL_Y_Level", Number(0)),
    ("CP_Color_Corr_A_Area_REV_Color_Level", Number(-50)),
    ("CP_Color_Corr_A_Area_REV_Color_Phase", Number(-18)),
    ("CP_Color_Corr_B_Area_SEL_Color_Phase", Number(0)),
    ("CP_Color_Corr_B_Area_SEL_Chroma", Number(0)),
    ("CP_Color_Corr_B_Area_SEL_Area", Number(0)),
    ("CP_Color_Corr_B_Area_SEL_Y_Level", Number(0)),
    ("CP_Color_Corr_B_Area_REV_Color_Level", Number(-50)),
    ("CP_Color_Corr_B_Area_REV_Color_Phase", Number(-18)),
    ("CP_OthersSetupLVL_Level", Number(-50)),
    ("CP_OthersOver100Percent", Names(&["Through", "Clip", "Press"])),
    ("CP_HLG_Color", Names(&["BT.2100", "Vivid"])),
    ("CP_Sharpness_Detail_FREQ", Number(-8)),
    ("CP_NoiseReductionSet_SpatialFilter", Number(0)),
    ("CP_NoiseReductionSet_FrameCorrelation", Number(0)),
    ("FocusMode", Names(&["Manual focus mode", "Auto focus mode", "Reserved"])),
    ("ImageStabilizer", Names(&["Image Stabilizer OFF", "Standard IS", "Dynamic IS", "Powered IS"])),
    ("CP_LookFile", OFF_ON),
    ("ISOMode", Names(&["Auto", "Manual"])),
];

/// The MenuSettingPack, up to where a pack ends: a pack before version 1.5 stops at BaseISO. Bytes with no information are
/// left out
fn menu_setting(d: &mut Cursor::<&[u8]>) -> Result<Value> {
    d.set_position(0);
    let mut out = Map::new();
    let version = d.read_u16::<LittleEndian>()?;
    out.insert("Version".into(), format!("{}.{}", version >> 8, version & 0xff).into());
    d.set_position(HEADER);

    if let (Ok(width), Ok(height)) = (d.read_u16::<LittleEndian>(), d.read_u16::<LittleEndian>()) {
        if !matches!(width, 0 | 0xFFFF)  { out.insert("DevelopSizeH".into(), width.into()); }
        if !matches!(height, 0 | 0xFFFF) { out.insert("DevelopSizeV".into(), height.into()); }
    }
    d.set_position(HEADER + 8);

    let setting = |code: u8, kind: Menu| match kind {
        Number(offset) => Value::from(code as i16 + offset),
        Names(names)   => code_name(code, names).into(),
    };
    for (name, kind) in MENU_BYTES {
        let Ok(code) = d.read_u8() else { return Ok(out.into()) };
        if code != 0xFF { out.insert(name.into(), setting(code, kind)); }
    }

    if let Ok(iso) = d.read_u32::<LittleEndian>() {
        if iso != 0xFFFFFFFF { out.insert("BaseISO".into(), iso.into()); }
    }
    if let Ok(code) = d.read_u8() {
        if code != 0xFF { out.insert("CP_Knee_WhiteLevel100Percent".into(), setting(code, OFF_ON)); }
    }
    if let Ok(code) = d.read_u8() {
        if code != 0xFF { out.insert("CP_LookFile_Intensity".into(), setting(code, Number(0))); }
    }
    Ok(out.into())
}
