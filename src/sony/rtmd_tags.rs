// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2021 Adrian <adrian.eddy at gmail>

use std::io::*;

use byteorder::{ReadBytesExt, BigEndian};

use crate::tags_impl::*;
use crate::tag;
use crate::tags_impl::TagId::*;
use crate::tags_impl::GroupId::*;

// https://github.com/MediaArea/MediaInfoLib/blob/master/Source/MediaInfo/Multiple/File_Mxf.cpp
// https://github.com/exiftool/exiftool/blob/master/lib/Image/ExifTool/MXF.pm
// https://github.com/exiftool/exiftool/blob/master/lib/Image/ExifTool/Sony.pm
// Also these tags are in SMDK-VC140-x64-4_19_0.dll and SVMUlib.dll included in Catalyst Browse
pub fn get_tag(tag: u16, tag_data: &[u8]) -> TagDescription {
    match tag {
        // -------------- LensUnitMetadata --------------
        0x8000 => tag!(Lens, IrisFStop,          "Iris",                                   f32,  "f/{:.1}", |d| Ok(2f32.powf(8.0 * (1.0 - (d.read_u16::<BigEndian>()? as f32 / 65536.0)))), tag_data),
        0x8008 => tag!(Lens, IrisTStop,          "Iris",                                   f32,  "T/{:.1}", |d| Ok(2f32.powf(8.0 * (1.0 - (d.read_u16::<BigEndian>()? as f32 / 65536.0)))), tag_data),
        0x8001 => tag!(Lens, FocusDistance,      "Focus Position (Image Plane)",           f32,  "{:.2}m",  |d| read_f16(d), tag_data),
        0x8002 => tag!(Lens, FocusDistance,      "Focus Position (Front Lens Vertex)",     f32,  "{:.2}m",  |d| read_f16(d), tag_data),
        0x8003 => tag!(Lens, MacroEnabled,       "Macro Setting",                          bool, "{:?}",    |d| Ok(d.read_i8()? == 1), tag_data),
        0x8004 => tag!(Lens, LensZoom35mm,       "LensZoom (35mm Still Camera Equivalent)",f32,  "{:.2} mm",|d| Ok(read_f16(d)? * 1000.0), tag_data),
        0x8005 => tag!(Lens, FocalLength,        "LensZoom (Actual Focal Length)",         f32,  "{:.2} mm",|d| Ok(read_f16(d)? * 1000.0), tag_data),
        0x8006 => tag!(Lens, OpticalZoomPercent, "Optical Extender Magnification",         u16,  "{:.2}%",  |d| d.read_u16::<BigEndian>(), tag_data),
        0x8007 => tag!(Lens, LensAttributes,     "Lens Attributes",                        String, |v| v.to_string(),   |d| read_utf8(d), tag_data),
        0x8009 => tag!(Lens, IrisRingPosition,   "Iris Ring Position",                     f32,  "{:.2}%",  |d| Ok(d.read_u16::<BigEndian>()? as f32 / 65536.0 * 100.0), tag_data),
        0x800A => tag!(Lens, FocusRingPosition,  "Focus Ring Position",                    f32,  "{:.2}%",  |d| Ok(d.read_u16::<BigEndian>()? as f32 / 65536.0 * 100.0), tag_data),
        0x800B => tag!(Lens, ZoomRingPosition,   "Zoom Ring Position",                     f32,  "{:.2}%",  |d| Ok(d.read_u16::<BigEndian>()? as f32 / 65536.0 * 100.0), tag_data),

        // -------------- CameraUnitMetadata --------------
        0x3219 => tag!(Colors, ColorPrimaries, "Color Primaries", Uuid, |v| {
            let types = ["Unknown", "BT.601 NTSC", "BT.601 PAL", "BT.709", "BT.2020", "XYZ", "Display P3", "ACES" /*SMPTE ST 2065-1*/, "XYZ" /*SMPTE ST 2067-40 / ISO 11664-3*/];
            let t = ((v.3 >> 16) & 0xFF) as usize;
            if t > 0 && t < types.len() {
                types[t].to_owned()
            } else {
                format!("{{{:08x}-{:08x}-{:08x}-{:08x}}}", v.0, v.1, v.2, v.3)
            }
        }, |d| read_uuid(d), tag_data),
        0x321A => tag!(Colors, CodingEquation, "Coding Equations", Uuid, |v| {
            let types = ["Unknown", "BT.601", "BT.709", "SMPTE 240M", "YCgCo", "Identity", "BT.2020 non-constant"];
            let t = ((v.3 >> 16) & 0xFF) as usize;
            // 04010101 03030000: rec709
            // 04010101 03040000: rec2020
            // 0e060401 01030103: S-Gamut
            // 0e060401 01030104: S-Gamut3
            // 0e060401 01030105: S-Gamut3.Cine
            if t > 0 && t < types.len() {
                types[t].to_owned()
            } else {
                format!("{{{:08x}-{:08x}-{:08x}-{:08x}}}", v.0, v.1, v.2, v.3)
            }
        }, |d| read_uuid(d), tag_data),
        0x3210 => tag!(Colors, CaptureGammaEquation, "Capture Gamma Equation", Uuid, |v| { match v.3 {
            0x01010000 => "BT.470"                    .into(),
            0x01020000 => "BT.709"                    .into(),
            0x01030000 => "SMPTE ST 240"              .into(),
            0x01040000 => "SMPTE ST 274"              .into(),
            0x01050000 => "BT.1361"                   .into(),
            0x01060000 => "SceneLinear"               .into(),
            0x01080000 => "Rec709-xvYCC"              .into(),
            0x010b0000 => "Rec2100-HLG"               .into(),
            0x01010101 => "DVW-709 Like"              .into(),
            0x01010102 => "E10/E30STD for J EK"       .into(),
            0x01010103 => "E10/E30STD for UC"         .into(),
            0x01010106 => "BBC Initial50"             .into(),
            0x01010107 => "SD CamCorder STD"          .into(),
            0x01010108 => "BVW-400 Like"              .into(),
            0x01010109 => "Ikegami"                   .into(),
            0x0101017F => "reproduced unknown label"  .into(),
            0x01010201 => "HG3250G36"                 .into(),
            0x01010202 => "HG4600G30"                 .into(),
            0x01010203 => "HG3259G40"                 .into(),
            0x01010204 => "HG4609G33"                 .into(),
            0x01010205 => "HG8000G36"                 .into(),
            0x01010206 => "HG8000G30"                 .into(),
            0x01010207 => "HG8009G40"                 .into(),
            0x01010208 => "HG8009G33"                 .into(),
            0x01010301 => "CINE1 of EX1/EX3"          .into(),
            0x01010302 => "CINE2 of EX1/EX3"          .into(),
            0x01010303 => "CINE3 of EX1/EX3"          .into(),
            0x01010304 => "CINE4 of EX1/EX3"          .into(),
            0x01010305 => "Kodak 5248 film like"      .into(),
            0x01010306 => "Kodak 5245 film like"      .into(),
            0x01010307 => "Kodak 5293 film like"      .into(),
            0x01010308 => "Kodak 5296 film like"      .into(),
            0x01010309 => "Average of Film of MSW-900".into(),
            0x01010401 => "User defined curve1"       .into(),
            0x01010402 => "User defined curve2"       .into(),
            0x01010403 => "User defined curve3"       .into(),
            0x01010404 => "User defined curve4"       .into(),
            0x01010405 => "User defined curve5"       .into(),
            0x01010406 => "User defined curve6"       .into(),
            0x01010407 => "User defined curve7"       .into(),
            0x01010408 => "User defined curve8"       .into(),
            0x01010409 => "User defined curve9"       .into(),
            0x01010501 => "S-Log"                     .into(),
            0x01010502 => "FS-Log"                    .into(),
            0x01010503 => "R709 180%"                 .into(),
            0x01010504 => "R709 800%"                 .into(),
            0x01010506 => "Cine-Log"                  .into(),
            0x01010507 => "ASC-CDL"                   .into(),
            0x01010508 => "S-Log2"                    .into(),
            0x01010509 => "ACESproxy"                 .into(),
            0x01010601 => "NXCAM Standard"            .into(),
            0x01010602 => "NXCAM Still Camera Like"   .into(),
            0x01010603 => "xvYCC"                     .into(),
            0x01010604 => "S-Gamut3/S-Log3"           .into(),
            0x01010605 => "S-Gamut3.Cine/S-Log3"      .into(),
            0x01010606 => "ITU-R Rec. BT.2020"                         .into(),
            0x01010607 => "SMPTE ST 2084 Inverse EOTF"                 .into(),
            0x01010608 => "ITU-R Rec. BT.2100 Hybrid Log-Gamma"        .into(),
            0x01010701 => "NXCAM Cinematone 1 (negative film)"         .into(),
            0x01010702 => "NXCAM Cinematone 2 (printed film)"          .into(),
            0x01010704 => "Hybrid Log-Gamma with S-Log3 OOTF Live Look".into(),
            0x01010705 => "S-Cinetone"                                 .into(),
            0x01010706 => "ACES cct"                                   .into(),
            0x01010707 => "Hybrid Log-Gamma with S-Log3 OOTF Mild Look".into(),
            _ => format!("{{{:08x}-{:08x}-{:08x}-{:08x}}}", v.0, v.1, v.2, v.3)
        } }, |d| read_uuid(d), tag_data),

        0x8100 => tag!(Exposure, AutoExposureMode, "AutoExposure Mode", Uuid, |v| { match v.3 {
            0x01010000 => "Manual"               .into(),
            0x01020000 => "Full Auto"            .into(),
            0x01030000 => "Gain Priority Auto"   .into(),
            0x01040000 => "Iris Priority Auto"   .into(),
            0x01050000 => "Shutter Priority Auto".into(),
            _ => format!("{{{:08x}-{:08x}-{:08x}-{:08x}}}", v.0, v.1, v.2, v.3)
        } }, |d| read_uuid(d), tag_data),

        0x8101 => tag!(Autofocus, AutoFocusMode, "Auto Focus Sensing Area Setting", u8, |v| { match v {
            0 => "Manual"                  .into(),
            1 => "Center Sensitive Auto"   .into(),
            2 => "Full Screen Sensing Auto".into(),
            3 => "Multi Spot Sensing Auto" .into(),
            4 => "Single Spot Sensing Auto".into(),
            _ => format!("{}", v)
        } }, |d| d.read_u8(), tag_data),

        0x8102 => tag!(Colors, ColorCorrectionSetting, "Color Correction Filter Wheel Setting", u8, |v| { match v {
            0 => "Cross effect"             .into(),
            1 => "Color Compensation 3200 K".into(),
            2 => "Color Compensation 4300 K".into(),
            3 => "Color Compensation 6300 K".into(),
            4 => "Color Compensation 5600 K".into(),
            _ => format!("{}", v)
        } }, |d| d.read_u8(), tag_data),

        0x8103 => tag!(Default, NDFilterSetting, "Neutral Density Filter Wheel Setting", u16, |v| { match v {
            1 => "Clear".into(),
            _ => format!("1/{}", v)
        } }, |d| d.read_u16::<BigEndian>(), tag_data),

        0x8104 => tag!(Default, SensorWidth,  "Imager Dimension (Effective Width)",              f32, "{:.2} mm", |d| Ok(d.read_u16::<BigEndian>()? as f32 / 1000.0), tag_data),
        0x8105 => tag!(Default, SensorHeight, "Imager Dimension (Effective Height)",             f32, "{:.2} mm", |d| Ok(d.read_u16::<BigEndian>()? as f32 / 1000.0), tag_data),
        0x8106 => tag!(Default, FrameRate,    "Capture Frame Rate",                              f64, "{:.3}fps", |d| read_rational(d), tag_data),

        0x8107 => tag!(Default, SensorReadoutMode, "Image Sensor Readout Mode", u8, |v| { match v {
            0 => "Interlaced field" .into(),
            1 => "Interlaced frame" .into(),
            2 => "Progressive frame".into(),
            0xFF => "Undefined"     .into(),
            _ => format!("{}", v)
        } }, |d| d.read_u8(), tag_data),

        0x8108 => tag!(Exposure, ShutterAngle, "Shutter Angle",                                   f32,  "{:.1}°",  |d| Ok(d.read_i32::<BigEndian>()? as f32 / 60.0), tag_data),
        0x8109 => tag!(Exposure, ShutterSpeed, "Shutter Speed",                                   u32x2, |v| format!("{}/{}s", v.0, v.1), |d| Ok((d.read_u32::<BigEndian>()?, d.read_u32::<BigEndian>()?)), tag_data),
        0x810A => tag!(Exposure, TagId::Custom("MasterGainAdjustment".into()), "Camera Master Gain Adjustment",           f32,  "{:.2}%", |d| Ok(d.read_u16::<BigEndian>()? as f32 / 100.0), tag_data),
        0x810B => tag!(Exposure, ISOValue, "ISO Sensitivity",                                     u16, "{}",     |d| d.read_u16::<BigEndian>(), tag_data),
        0x810C => tag!(Default, TagId::Custom("ElectricalExtenderMagnification".into()), "Electrical Extender Magnification",               u16, "{}%",    |d| d.read_u16::<BigEndian>(), tag_data),

        0x810D => tag!(Colors, AutoWBMode, "Auto White Balance Mode", u8, |v| { match v {
            0 => "Preset"   .into(),
            1 => "Automatic".into(),
            2 => "Hold"     .into(),
            3 => "One Push" .into(),
            _ => format!("{}", v)
        } }, |d| d.read_u8(), tag_data),

        0x810E => tag!(Colors, WhiteBalance, "White Balance",                                     u16,  "{}K",    |d| d.read_u16::<BigEndian>(), tag_data),
        0x810F => tag!(Colors, MasterBlackLevel, "Camera Master BlackLevel",                      f32,  "{:.2}",  |d| Ok(d.read_u16::<BigEndian>()? as f32 / 10.0), tag_data),
        0x8110 => tag!(Colors, KneePoint, "Camera Knee Point",                                    f32,  "{:.2}",  |d| Ok(d.read_u16::<BigEndian>()? as f32 / 10.0), tag_data),
        0x8111 => tag!(Colors, KneeSlope, "Camera Knee Slope",                                    f64,  "{:.2}",  |d| read_rational(d), tag_data),
        0x8112 => tag!(Colors, LuminanceDynamicRange, "Camera Luminance Dynamic Range",           f32,  "{:.2}",  |d| Ok(d.read_u16::<BigEndian>()? as f32 / 10.0), tag_data),
        0x8113 => tag!(Default, TagId::Custom("SettingFileURI".into()), "Camera Setting File URI",String, |v| v.to_string(), |d| read_utf8(d), tag_data),
        0x8114 => tag!(Default, CameraAttributes, "Camera Attributes",                            String, |v| v.to_string(), |d| read_utf8(d), tag_data),
        0x8115 => tag!(Exposure, TagId::Custom("ISOValue2".into()), "Exposure Index of Photo Meter", u16, "{}",   |d| d.read_u16::<BigEndian>(), tag_data),

        0x8116 => tag!(Colors, TagId::Custom("GammaforCDL".into()), "Gamma for CDL", u8, |v| { match v {
            0 => "Same as Capture Gamma".into(),
            1 => "Scene Linear"         .into(),
            2 => "S-Log"                .into(),
            3 => "Cine-Log"             .into(),
            0xFF => "Undefined"         .into(),
            _ => format!("{}", v)
        } }, |d| d.read_u8(), tag_data),

        0x8117 => tag!(Colors, TagId::Custom("ASCCDLValue".into()), "ASC CDL V1.2", Json, |v| v.to_string(), |d| {
            let count = d.read_u32::<BigEndian>()?;
            let length = d.read_u32::<BigEndian>()?;
            if count != 10 || length != 2 { return Err(Error::new(ErrorKind::Other, "Invalid")); }
            let sr = read_f16_corrected(d)?; let sg = read_f16_corrected(d)?; let sb = read_f16_corrected(d)?;
            let or = read_f16_corrected(d)?; let og = read_f16_corrected(d)?; let ob = read_f16_corrected(d)?;
            let pr = read_f16_corrected(d)?; let pg = read_f16_corrected(d)?; let pb = read_f16_corrected(d)?;

            let sat = read_f16_corrected(d)?;
            Ok(serde_json::json!({
                "slope":  { "r": sr, "g": sg, "b": sb},
                "offset": { "r": or, "g": og, "b": ob},
                "power":  { "r": pr, "g": pg, "b": pb},
                "saturation": sat
            }))
        }, tag_data),

        0x8118 => tag!(Colors, ColorMatrix, "Color matrix", Json, |v| v.to_string(), |d| {
            let count  = d.read_u32::<BigEndian>()?;
            let length = d.read_u32::<BigEndian>()?;
            if count != 9 || length != 8 {
                return Err(Error::new(ErrorKind::Other, "Invalid"));
            }

            let rr = d.read_u32::<BigEndian>()? as f32 / d.read_u32::<BigEndian>()? as f32;
            let gr = d.read_u32::<BigEndian>()? as f32 / d.read_u32::<BigEndian>()? as f32;
            let br = d.read_u32::<BigEndian>()? as f32 / d.read_u32::<BigEndian>()? as f32;

            let rg = d.read_u32::<BigEndian>()? as f32 / d.read_u32::<BigEndian>()? as f32;
            let gg = d.read_u32::<BigEndian>()? as f32 / d.read_u32::<BigEndian>()? as f32;
            let bg = d.read_u32::<BigEndian>()? as f32 / d.read_u32::<BigEndian>()? as f32;

            let rb = d.read_u32::<BigEndian>()? as f32 / d.read_u32::<BigEndian>()? as f32;
            let gb = d.read_u32::<BigEndian>()? as f32 / d.read_u32::<BigEndian>()? as f32;
            let bb = d.read_u32::<BigEndian>()? as f32 / d.read_u32::<BigEndian>()? as f32;
            Ok(serde_json::json!({
                "RR": rr, "GR": gr, "BR": br,
                "RG": rg, "GG": gg, "BG": bg,
                "RB": rb, "GB": gb, "BB": bb
            }))
        }, tag_data),

        // -------------- UserDefinedAcquisitionMetadata --------------
        0xe000 => tag!(Default, GroupIdentifier, "UDAM Set Identifier", Uuid, |v| format!("{{{:08x}-{:08x}-{:08x}-{:08x}}}", v.0, v.1, v.2, v.3), |d| read_uuid(d), tag_data),

        0xe101 => tag!(Default, TagId::Custom("EffectiveMarkerCoverage".into()),         "Effective marker coverage",       u32x2, |v| format!("{} x {}", v.0, v.1), |d| Ok((d.read_u32::<BigEndian>()?, d.read_u32::<BigEndian>()?)), tag_data),
        0xe102 => tag!(Default, TagId::Custom("EffectiveMarkerAspectRatio".into()),      "Effective marker aspect ratio",   u32x2, |v| format!("{} x {}", v.0, v.1), |d| Ok((d.read_u32::<BigEndian>()?, d.read_u32::<BigEndian>()?)), tag_data),
        0xe103 => tag!(Default, TagId::Custom("CameraProcessDiscriminationCode".into()), "Camera process discrimination code", u16,|v| { match v {
            0x0101 => "F65 RAW Mode released in December 2011".into(),
            0x0102 => "F65 HD Mode released in April 2012".into(),
            0x0103 => "F65 RAW High Frame Rate Mode released in July 2012".into(),
            _ => format!("0x{:04x}", v)
        } }, |d| d.read_u16::<BigEndian>(), tag_data),
        0xe104 => tag!(Default, TagId::Custom("RotaryShutterMode".into()),               "Rotary shutter mode",             bool,   "{}", |d| Ok(d.read_u8()? != 0), tag_data),
        0xe105 => tag!(Default, TagId::Custom("RawBlackCodeValue".into()),               "RawBlack code value",             u16,    "{}", |d| d.read_u16::<BigEndian>(), tag_data),
        0xe106 => tag!(Default, TagId::Custom("RawGrayCodeValue".into()),                "RawGray code value",              u16,    "{}", |d| d.read_u16::<BigEndian>(), tag_data),
        0xe107 => tag!(Default, TagId::Custom("RawWhiteCodeValue".into()),               "RawWhite code value",             u16,    "{}", |d| d.read_u16::<BigEndian>(), tag_data),
        0xe109 => tag!(Default, TagId::Custom("MonitoringDescriptions".into()),          "Monitoring descriptions",         String, "{}", |d| read_utf8(d), tag_data),
        0xe10B => tag!(Default, TagId::Custom("MonitoringBaseCurve".into()),             "Monitoring base curve",           Uuid, |v| { match v.3 {
            0x01010000 => "BT.470"                    .into(),
            0x01020000 => "BT.709"                    .into(),
            0x01030000 => "SMPTE ST 240"              .into(),
            0x01040000 => "SMPTE ST 274"              .into(),
            0x01050000 => "BT.1361"                   .into(),
            0x01060000 => "SceneLinear"               .into(),
            0x01080000 => "Rec709-xvYCC"              .into(),
            0x010b0000 => "Rec2100-HLG"               .into(),
            0x01010101 => "DVW-709 Like"              .into(),
            0x01010102 => "E10/E30STD for J EK"       .into(),
            0x01010103 => "E10/E30STD for UC"         .into(),
            0x01010106 => "BBC Initial50"             .into(),
            0x01010107 => "SD CamCorder STD"          .into(),
            0x01010108 => "BVW-400 Like"              .into(),
            0x01010109 => "Ikegami"                   .into(),
            0x0101017F => "reproduced unknown label"  .into(),
            0x01010201 => "HG3250G36"                 .into(),
            0x01010202 => "HG4600G30"                 .into(),
            0x01010203 => "HG3259G40"                 .into(),
            0x01010204 => "HG4609G33"                 .into(),
            0x01010205 => "HG8000G36"                 .into(),
            0x01010206 => "HG8000G30"                 .into(),
            0x01010207 => "HG8009G40"                 .into(),
            0x01010208 => "HG8009G33"                 .into(),
            0x01010301 => "CINE1 of EX1/EX3"          .into(),
            0x01010302 => "CINE2 of EX1/EX3"          .into(),
            0x01010303 => "CINE3 of EX1/EX3"          .into(),
            0x01010304 => "CINE4 of EX1/EX3"          .into(),
            0x01010305 => "Kodak 5248 film like"      .into(),
            0x01010306 => "Kodak 5245 film like"      .into(),
            0x01010307 => "Kodak 5293 film like"      .into(),
            0x01010308 => "Kodak 5296 film like"      .into(),
            0x01010309 => "Average of Film of MSW-900".into(),
            0x01010401 => "User defined curve1"       .into(),
            0x01010402 => "User defined curve2"       .into(),
            0x01010403 => "User defined curve3"       .into(),
            0x01010404 => "User defined curve4"       .into(),
            0x01010405 => "User defined curve5"       .into(),
            0x01010406 => "User defined curve6"       .into(),
            0x01010407 => "User defined curve7"       .into(),
            0x01010408 => "User defined curve8"       .into(),
            0x01010409 => "User defined curve9"       .into(),
            0x01010501 => "S-Log"                     .into(),
            0x01010502 => "FS-Log"                    .into(),
            0x01010503 => "R709 180%"                 .into(),
            0x01010504 => "R709 800%"                 .into(),
            0x01010506 => "Cine-Log"                  .into(),
            0x01010507 => "ASC-CDL"                   .into(),
            0x01010508 => "S-Log2"                    .into(),
            0x01010509 => "ACESproxy"                 .into(),
            0x01010601 => "NXCAM Standard"            .into(),
            0x01010602 => "NXCAM Still Camera Like"   .into(),
            0x01010603 => "xvYCC"                     .into(),
            0x01010604 => "S-Gamut3/S-Log3"           .into(),
            0x01010605 => "S-Gamut3.Cine/S-Log3"      .into(),
            0x01010606 => "ITU-R Rec. BT.2020"                         .into(),
            0x01010607 => "SMPTE ST 2084 Inverse EOTF"                 .into(),
            0x01010608 => "ITU-R Rec. BT.2100 Hybrid Log-Gamma"        .into(),
            0x01010701 => "NXCAM Cinematone 1 (negative film)"         .into(),
            0x01010702 => "NXCAM Cinematone 2 (printed film)"          .into(),
            0x01010704 => "Hybrid Log-Gamma with S-Log3 OOTF Live Look".into(),
            0x01010705 => "S-Cinetone"                                 .into(),
            0x01010706 => "ACES cct"                                   .into(),
            0x01010707 => "Hybrid Log-Gamma with S-Log3 OOTF Mild Look".into(),
            _ => format!("{{{:08x}-{:08x}-{:08x}-{:08x}}}", v.0, v.1, v.2, v.3)
        } }, |d| read_uuid(d), tag_data),
        0xe114 => tag!(Default, TagId::Custom("PostCDLTransform".into()), "Post-CDL transform", String, "{}", |d| read_utf8(d), tag_data),
        0xe201 => tag!(Cooke, TagId::Data,    "BinaryMetadata",    Json, "{:?}", |d| {
            Ok(serde_json::Value::Array(crate::cooke::bin::parse(d.get_ref()).unwrap())) // TODO: unwrap
        }, tag_data),
        0xe202 => tag!(Cooke, TagId::Custom("UserMetadata".into()),    "UserMetadata",      String, "{}", |d| read_utf8(d), tag_data),
        0xe203 => tag!(Cooke, TagId::Custom("CalibrationType".into()), "CalibrationType",   u8, |v| { match v {
            0 => "mm".into(),
            1 => "in".into(),
            _ => format!("{}", v)
        } }, |d| d.read_u8(), tag_data),
        0xe208 => tag!(Cooke, Unknown(tag as u32), "", tag_data), // will be parsed in process_map
        0xe209 => tag!(Cooke, Unknown(tag as u32), "", tag_data), // continuation of 0xe208, will be parsed in process_map
        0xe108 => tag!(Default, Unknown(0xe108), "Unknown_e108", Uuid, |v| format!("{{{:08x}-{:08x}-{:08x}-{:08x}}}", v.0, v.1, v.2, v.3), |d| read_uuid(d), tag_data),
        0xe10d => tag!(Default, Unknown(0xe10d), "Unknown_e10d", Uuid, |v| format!("{{{:08x}-{:08x}-{:08x}-{:08x}}}", v.0, v.1, v.2, v.3), |d| read_uuid(d), tag_data),
        0xe10e => tag!(Default, Unknown(0xe10e), "Unknown_e10e", Uuid, |v| format!("{{{:08x}-{:08x}-{:08x}-{:08x}}}", v.0, v.1, v.2, v.3), |d| read_uuid(d), tag_data),
        0xe10f => tag!(Default, Unknown(0xe10f), "Unknown_e10f", u32x2, |v| format!("{} x {}", v.0, v.1), |d| Ok((d.read_u32::<BigEndian>()?, d.read_u32::<BigEndian>()?)), tag_data),

        0xe111 => tag!(Default, Unknown(0xe111), "Unknown_e111", Uuid, |v| format!("{{{:08x}-{:08x}-{:08x}-{:08x}}}", v.0, v.1, v.2, v.3), |d| read_uuid(d), tag_data),
        0xe112 => tag!(Default, Unknown(0xe112), "Unknown_e112", Uuid, |v| format!("{{{:08x}-{:08x}-{:08x}-{:08x}}}", v.0, v.1, v.2, v.3), |d| read_uuid(d), tag_data),
        0xe113 => tag!(Default, Unknown(0xe113), "Unknown_e113", String, "{}", |d| read_utf8(d), tag_data),

        // -------------- Sony's proprietary --------------
        0xe300 => tag!(Default, StabilizationEnabled, "Stabilization", u8, "{}", |d| d.read_u8(), tag_data),
        0xe301 => tag!(Exposure, TagId::Custom("ISOValue3".into()), "ISO value", u32, "{}", |d| d.read_u32::<BigEndian>(), tag_data),
        0x8119 => tag!(Exposure, TagId::Custom("ISOValue4".into()), "ISO value", u32, "{}", |d| d.read_u32::<BigEndian>(), tag_data),
        0x811e => tag!(Exposure, TagId::Custom("ISOValue5".into()), "ISO value", u32, "{}", |d| d.read_u32::<BigEndian>(), tag_data),
        0xe304 => tag!(Default, CaptureTimestamp, "Capture timestamp", u64, |&v| chrono::TimeZone::timestamp_opt(&chrono::Utc, v as i64, 0).single().map(|x| x.to_string()).unwrap_or_default(), |x| {
            let _tz = x.read_u8()?; // TODO: timezone, unknown format, 0 for UTC, 68 for GMT+2, 42 for GMT-5, 2 for GMT+1

            fn read_as_dec(x: &mut Cursor::<&[u8]>) -> Result<u32> {
                let v = x.read_u8()?;
                Ok((((v >> 4) & 0xF) * 10 + (v & 0xF)) as u32)
            }
            let yy1 = read_as_dec(x)? as f32;
            let yy2 = read_as_dec(x)? as f32;
            let mm  = read_as_dec(x)?;
            let dd  = read_as_dec(x)?;
            let h   = read_as_dec(x)?;
            let m   = read_as_dec(x)?;
            let s   = read_as_dec(x)?;

            Ok(chrono::NaiveDate::from_ymd_opt((yy1 * 100.0 + yy2) as i32, mm, dd).and_then(|x| x.and_hms_opt(h, m, s)).unwrap_or_default().and_utc().timestamp() as u64)
        }, tag_data),

        // Possible values: zFar, zNear, aspect, temporal_position, temporal_rotation
        ////////////////////////////////////////// ImagerControlInformation (IBIS) //////////////////////////////////////////
        0xe400 => tag!(IBIS, Unknown(tag as u32), "IBIS position/rotation 3xi32", Vector3_i32, "{:?}", |d| {
            let x = d.read_i32::<BigEndian>()?;
            let y = d.read_i32::<BigEndian>()?;
            let z = d.read_i32::<BigEndian>()?;
            Ok(Vector3 { x, y, z })
        }, tag_data),
        0xe401 => tag!(IBIS, Unknown(tag as u32), "IBIS position/rotation u8", u8, "{}", |d| d.read_u8(), tag_data),
        0xe402 => tag!(IBIS, Unknown(tag as u32), "IBIS position/rotation i32", i32, "{}", |d| d.read_i32::<BigEndian>(), tag_data),
        0xe403 => tag!(IBIS, Unknown(tag as u32), "IBIS position/rotation u8", u8, "{}", |d| d.read_u8(), tag_data),
        0xe404 => tag!(IBIS, Unknown(tag as u32), "IBIS position/rotation 3xi16", Vector3_i16, "{:?}", |d| {
            let x = d.read_i16::<BigEndian>()?;
            let y = d.read_i16::<BigEndian>()?;
            let z = d.read_i16::<BigEndian>()?;
            Ok(Vector3 { x, y, z })
        }, tag_data),
        0xe405 => tag!(Imager, SensorSizePixels, "Sensor pixel size", u32x2, "{:?}", |d| {
            let width = d.read_u16::<BigEndian>()? as u32;
            let height = d.read_u16::<BigEndian>()? as u32;
            Ok((width, height))
        }, tag_data),
        0xe406 => tag!(Imager, Unknown(tag as u32), "Imager i32", i32, "{}", |d| d.read_i32::<BigEndian>(), tag_data),
        0xe407 => tag!(Imager, PixelPitch, "Pixel pitch", u32x2, "{:?}", |d| {
            let x = d.read_i16::<BigEndian>()? as u32;
            let y = d.read_i16::<BigEndian>()? as u32;
            Ok((x, y))
        }, tag_data),
        0xe408 => tag!(Imager, Unknown(tag as u32), "Crop scaler", i32, "{}", |d| d.read_i32::<BigEndian>(), tag_data),
        0xe409 => tag!(Imager, CaptureAreaOrigin, "Sensor crop origin", f32x2, "{:?}", |d| {
            let x = d.read_u32::<BigEndian>()? as f32;
            let y = d.read_u32::<BigEndian>()? as f32;
            Ok((x, y))
        }, tag_data),
        0xe40a => tag!(Imager, CaptureAreaSize, "Sensor crop size", f32x2, "{:?}", |d| {
            let width = d.read_u32::<BigEndian>()? as f32;
            let height = d.read_u32::<BigEndian>()? as f32;
            Ok((width, height))
        }, tag_data),
        0xe40b => tag!(Imager, Unknown(tag as u32), "Imager i32", i32, "{}", |d| d.read_i32::<BigEndian>(), tag_data),
        0xe40c => tag!(Imager, FirstFrameTimestamp, "First frame timestamp", f64, "{:.4} ms", |d| d.read_i32::<BigEndian>().map(|x| x as f64 / 1000.0), tag_data),
        0xe40d => tag!(Imager, ExposureTime,        "Exposure time", f64, "{:.4} ms", |d| d.read_i32::<BigEndian>().map(|x| x as f64 / 1000.0), tag_data),
        0xe40e => tag!(Imager, FrameReadoutTime,    "Frame readout time", f64, "{:.4} ms", |d| d.read_i32::<BigEndian>().map(|x| x as f64 / 1000.0), tag_data),
        0xe40f => tag!(IBIS, Data, "IBIS TimeOffset table 1", Vec_TimeVector3_i32, "{:?}", |d| {
            let count  = d.read_i32::<BigEndian>()?;
            let length = d.read_i32::<BigEndian>()?;
            if length != 16 {
                return Err(Error::new(ErrorKind::Other, "Invalid OSS table"));
            }
            if count > 0 {
                let mut ret = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    // XAVC::base_2D_TimeOffset<XAVC::base_3D<int>>
                    ret.push(TimeVector3 {
                        t: d.read_i32::<BigEndian>()?, // time offset
                        x: d.read_i32::<BigEndian>()?, // x, confirmed i32
                        y: d.read_i32::<BigEndian>()?, // y, confirmed i32
                        z: d.read_i32::<BigEndian>()?  // z. confirmed i32
                    });
                }
                Ok(ret)
            } else {
                Ok(vec![])
            }
        }, tag_data),
        0xe450 => tag!(IBIS, Data2, "IBIS TimeOffset table 2", Vec_TimeVector3_i32, "{:?}", |d| {
            let count  = d.read_i32::<BigEndian>()?;
            let length = d.read_i32::<BigEndian>()?;
            if length != 10 {
                return Err(Error::new(ErrorKind::Other, "Invalid table"));
            }
            if count > 0 {
                let mut ret = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    // XAVC::base_2D_TimeOffset<XAVC::base_3D<short>>
                    ret.push(TimeVector3 {
                        t: d.read_i32::<BigEndian>()?, // time offset
                        x: d.read_i16::<BigEndian>()? as i32, // x, confirmed i16
                        y: d.read_i16::<BigEndian>()? as i32, // y, confirmed i16
                        z: d.read_i16::<BigEndian>()? as i32  // z, confirmed i16
                    });
                }
                Ok(ret)
            } else {
                Ok(vec![])
            }
        }, tag_data),
        ////////////////////////////////////////// ImagerControlInformation (IBIS) //////////////////////////////////////////

        ////////////////////////////////////////// LensControlInformation (Lens OSS) //////////////////////////////////////////
        0xe410 => tag!(LensOSS, Unknown(tag as u32), "Lens OSS position/rotation 3xi32", String, |v| v.to_string(), |d| {
            let x = d.read_i32::<BigEndian>()?;
            let y = d.read_i32::<BigEndian>()?;
            let z = d.read_i32::<BigEndian>()?;
            Ok(format!("{} {} {}", x, y, z))
        }, tag_data),
        0xe411 => tag!(LensOSS, Unknown(tag as u32), "Lens OSS position/rotation u8", u8, "{}", |d| d.read_u8(), tag_data),
        0xe412 => tag!(LensOSS, Unknown(tag as u32), "Lens OSS position/rotation i32", i32, "{}", |d| d.read_i32::<BigEndian>(), tag_data),
        0xe413 => tag!(LensOSS, Unknown(tag as u32), "Lens OSS position/rotation u8", u8, "{}", |d| d.read_u8(), tag_data),
        0xe414 => tag!(LensOSS, Unknown(tag as u32), "Lens OSS position/rotation 3xi16", String, |v| v.to_string(), |d| {
            let x = d.read_i16::<BigEndian>()?;
            let y = d.read_i16::<BigEndian>()?;
            let z = d.read_i16::<BigEndian>()?;
            Ok(format!("{} {} {}", x, y, z))
        }, tag_data),
        0xe415 => tag!(LensOSS, Unknown(tag as u32), "Lens OSS i32", i32, "{}", |d| d.read_i32::<BigEndian>(), tag_data),
        0xe416 => tag!(LensOSS, Data, "Lens OSS TimeOffset table", Vec_TimeVector3_i32, "{:?}", |d| {
            // same format as 0xe40f
            let count  = d.read_i32::<BigEndian>()?;
            let length = d.read_i32::<BigEndian>()?;
            if length != 16 {
                return Err(Error::new(ErrorKind::Other, "Invalid table"));
            }
            if count > 0 {
                let mut ret = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    // XAVC::base_2D_TimeOffset<XAVC::base_3D<int>>
                    ret.push(TimeVector3 {
                        t: d.read_i32::<BigEndian>()?, // time offset
                        x: d.read_i32::<BigEndian>()?, // x
                        y: d.read_i32::<BigEndian>()?, // y
                        z: d.read_i32::<BigEndian>()?  // z
                    });
                }
                Ok(ret)
            } else {
                Ok(vec![])
            }
        }, tag_data),
        ////////////////////////////////////////// LensControlInformation (Lens OSS) //////////////////////////////////////////

        ////////////////////////////////////////// DistortionCorrection //////////////////////////////////////////
        0xe420 => tag!(GroupId::Custom("LensDistortion".into()), Enabled, "LensDistortion bool", bool, "{}", |d| Ok(d.read_u8()? != 0), tag_data),
        0xe421 => tag!(GroupId::Custom("LensDistortion".into()), Data,    "LensDistortion Table", Json, |v| v.to_string(), |d| {
            let focal_length_nm = d.read_u32::<BigEndian>()?;
            let effective_sensor_height_nm = d.read_u32::<BigEndian>()?;

            let unk1 = d.read_u8()?; // confirmed u8
            let coeff_scale = d.read_f32::<BigEndian>()?; // confirmed f32
            let mut elem_count = d.read_u32::<BigEndian>()?;
            let _elem_size = d.read_u32::<BigEndian>()?;
            if elem_count == 0xffffffff {
                elem_count = 0;
            }
            let mut ret = Vec::with_capacity(elem_count as usize); // &XAVC::base_Array<unsigned short>
            for _ in 0..elem_count {
                ret.push(d.read_u16::<BigEndian>()?); // confirmed u16
            }
            Ok(serde_json::json!({
                "focal_length_nm": focal_length_nm,
                "effective_sensor_height_nm": effective_sensor_height_nm,
                "unk1": unk1,
                "coeff_scale": coeff_scale,
                "coeffs": ret
            }))
        }, tag_data),
        0xe422 => tag!(GroupId::Custom("FocalPlaneDistortion".into()), Enabled, "FocalPlaneDistortion bool", bool, "{}", |d| Ok(d.read_u8()? != 0), tag_data),
        0xe423 => tag!(GroupId::Custom("FocalPlaneDistortion".into()), Data,    "FocalPlaneDistortion Table", Json, |v| v.to_string(), |d| {
            let aa = d.read_i32::<BigEndian>()?;
            let bb = d.read_i16::<BigEndian>()?;
            let cc = d.read_i16::<BigEndian>()?;
            let elem_count = d.read_i32::<BigEndian>()?;
            let _elem_size = d.read_i32::<BigEndian>()?;
            let mut ret = Vec::with_capacity(elem_count as usize); // XAVC::base_Array<XAVC::base_2D<short>>:
            for _ in 0..elem_count {
                ret.push((
                    d.read_i16::<BigEndian>()?, // x
                    d.read_i16::<BigEndian>()?, // y
                ));
            }
            let scale = if cc != 0 { 32768.0 / cc as f64 } else { 1.0 };

            Ok(serde_json::json!({
                "unk1": aa,
                "unk2": bb,
                "scale": scale,
                "unk4": ret
            }))
        }, tag_data),
        0xe424 => tag!(GroupId::Custom("MeshCorrection".into()), Enabled, "MeshCorrection::Mesh bool", bool, "{}", |d| Ok(d.read_u8()? != 0), tag_data),
        0xe425 => tag!(GroupId::Custom("MeshCorrection".into()), TagId::Unknown(tag as u32), "MeshCorrection::Mesh i16", i16, "{}", |d| d.read_i16::<BigEndian>(), tag_data),
        0xe42f => tag!(GroupId::Custom("MeshCorrection".into()), Data,    "MeshCorrection::Mesh", Json, |v| v.to_string(), |x| {
            let unk1 = x.read_i16::<BigEndian>()?;

            let offset_x = x.read_i32::<BigEndian>()?;
            let offset_y = x.read_i32::<BigEndian>()?;

            let size_x = x.read_u16::<BigEndian>()?;
            let size_y = x.read_u16::<BigEndian>()?;

            let mut xs = Vec::with_capacity(81);
            let mut ys = Vec::with_capacity(81);
            for _ in 0..81 { xs.push(x.read_i16::<BigEndian>()?); }
            for _ in 0..81 { ys.push(x.read_i16::<BigEndian>()?); }

            let divisions_x_2d = 2.0_f64.powi(x.read_u8()? as i32);
            let divisions_y_2d = 2.0_f64.powi(x.read_u8()? as i32);
            let divisions_x = x.read_u8()? as usize;
            let divisions_y = x.read_u8()? as usize;

            let total = divisions_x * divisions_y;
            let mut mesh_2d = Vec::with_capacity(total);
            for y in 0..divisions_y as usize {
                for x in 0..divisions_x as usize {
                    let idx = total - 1 - (y * divisions_x + x);
                    mesh_2d.push((
                        (size_x as f64 / 8.0) * x as f64 + (xs[idx] as f64 / divisions_x_2d),
                        (size_y as f64 / 8.0) * y as f64 + (ys[idx] as f64 / divisions_y_2d)
                    ));
                }
            }

            Ok(serde_json::json!({
                "unk1": unk1,
                "offset": [offset_x, offset_y],
                "size": [size_x, size_y],
                "mesh": mesh_2d,
                "raw_mesh": xs.into_iter().zip(ys.into_iter()).collect::<Vec<_>>(),
                "divisions_2d": [divisions_x_2d as i32, divisions_y_2d as i32],
                "divisions": [divisions_x, divisions_y]
            }))
        }, tag_data),
        ////////////////////////////////////////// DistortionCorrection //////////////////////////////////////////

        ////////////////////////////////////////// Gyroscope //////////////////////////////////////////
        // Position/rotation tags
        0xe430 => tag!(Gyroscope, Unknown(tag as u32), "Gyro position/rotation 3xi32", Vector3_i32, |v| format!("{} {} {}", v.x, v.y, v.z), |d| {
            Ok(Vector3 {
                x: d.read_i32::<BigEndian>()?,
                y: d.read_i32::<BigEndian>()?,
                z: d.read_i32::<BigEndian>()?
            })
        }, tag_data),
        0xe431 => tag!(Gyroscope, Unknown(tag as u32), "Gyro position/rotation u8", u8, "{}", |d| d.read_u8(), tag_data),
        0xe432 => tag!(Gyroscope, Unknown(tag as u32), "Gyro position/rotation i32", i32, "{}", |d| d.read_i32::<BigEndian>(), tag_data),
        0xe433 => tag!(Gyroscope, Unknown(tag as u32), "Gyro position/rotation u8", u8, "{}", |d| d.read_u8(), tag_data),
        0xe434 => tag!(Gyroscope, Unknown(tag as u32), "Gyro position/rotation 3xi16", Vector3_i16, |v| format!("{} {} {}", v.x, v.y, v.z), |d| {
            Ok(Vector3 {
                x: d.read_i16::<BigEndian>()?,
                y: d.read_i16::<BigEndian>()?,
                z: d.read_i16::<BigEndian>()?
            })
        }, tag_data),
        // IMU tags
        0xe435 => tag!(Gyroscope, Frequency,       "Gyroscope frequency", i32, "{} Hz", |d| d.read_i32::<BigEndian>(), tag_data),
        0xe436 => tag!(Gyroscope, Unknown(0xe436), "Sampling scaler (1000000)", i32, "{}", |d| d.read_i32::<BigEndian>(), tag_data),
        0xe437 => tag!(Gyroscope, TimeOffset,      "Gyroscope offset", f64, "{:.4} ms", |d| d.read_i32::<BigEndian>().map(|x| x as f64 / 1000.0), tag_data),
        0xe438 => tag!(Gyroscope, Unknown(0xe438), "Gyroscope is radians", bool, "{}", |d| d.read_u8().map(|x| x != 0), tag_data),
        0xe439 => tag!(Gyroscope, Scale,           "Gyroscope scale", f32, "{}", |d| d.read_f32::<BigEndian>(), tag_data),
        0xe43a => tag!(Gyroscope, Orientation,     "Gyroscope orientation", String, "{}", read_orientation, tag_data),
        0xe43b => tag!(Gyroscope, Data,            "Gyroscope data", Vec_Vector3_i16, "{:?}", |d| {
            let count = d.read_i32::<BigEndian>()?;
            let length = d.read_i32::<BigEndian>()?;
            if length != 6 {
                return Err(Error::new(ErrorKind::Other, "Invalid gyro data format"));
            }
            if count > 0 {
                let mut ret = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    ret.push(Vector3 {
                        x: d.read_i16::<BigEndian>()?, // pitch
                        y: d.read_i16::<BigEndian>()?, // roll
                        z: d.read_i16::<BigEndian>()?, // yaw
                    });
                }
                Ok(ret)
            } else {
                Ok(vec![])
            }
        }, tag_data),
        ////////////////////////////////////////// Gyroscope //////////////////////////////////////////
        ////////////////////////////////////////// Accelerometer //////////////////////////////////////////
        // Position/rotation tags
        0xe440 => tag!(Accelerometer, Unknown(0xe440), "Accelerometer position/rotation 3xi32", Vector3_i32, |v| format!("{} {} {}", v.x, v.y, v.z), |d| {
            Ok(Vector3 {
                x: d.read_i32::<BigEndian>()?,
                y: d.read_i32::<BigEndian>()?,
                z: d.read_i32::<BigEndian>()?
            })
        }, tag_data),
        0xe441 => tag!(Accelerometer, Unknown(0xe441), "Accelerometer position/rotation u8",    u8, "{}", |d| d.read_u8(), tag_data),
        0xe442 => tag!(Accelerometer, Unknown(0xe442), "Accelerometer position/rotation i32",   i32, "{}", |d| d.read_i32::<BigEndian>(), tag_data),
        0xe443 => tag!(Accelerometer, Unknown(0xe443), "Accelerometer position/rotation u8",    u8, "{}", |d| d.read_u8(), tag_data),
        0xe444 => tag!(Accelerometer, Unknown(0xe444), "Accelerometer position/rotation 3xi16", Vector3_i16, |v| format!("{} {} {}", v.x, v.y, v.z), |d| {
            Ok(Vector3 {
                x: d.read_i16::<BigEndian>()?,
                y: d.read_i16::<BigEndian>()?,
                z: d.read_i16::<BigEndian>()?
            })
        }, tag_data),

        // IMU tags
        0xe445 => tag!(Accelerometer, Frequency,       "Accelerometer frequency", i32, "{} Hz", |d| d.read_i32::<BigEndian>(), tag_data),
        0xe446 => tag!(Accelerometer, Unknown(0xe446), "Sampling scaler (1000000)", i32, "{}", |d| d.read_i32::<BigEndian>(), tag_data),
        0xe447 => tag!(Accelerometer, TimeOffset,      "Accelerometer offset", f64, "{:.4} ms", |d| d.read_i32::<BigEndian>().map(|x| x as f64 / 1000.0), tag_data),
        0xe448 => tag!(Accelerometer, Unknown(0xe448), "Accelerometer is m/s²", bool, "{}", |d| d.read_u8().map(|x| x != 0), tag_data),
        0xe449 => tag!(Accelerometer, Scale,           "Accelerometer scale", f32, "{}", |d| d.read_f32::<BigEndian>(), tag_data),
        0xe44a => tag!(Accelerometer, Orientation,     "Accelerometer orientation", String, "{}", read_orientation, tag_data),
        0xe44b => tag!(Accelerometer, Data,            "Accelerometer data", Vec_Vector3_i16, "{:?}", |d| {
            let count  = d.read_i32::<BigEndian>()?;
            let length = d.read_i32::<BigEndian>()?;
            if length != 6 {
                return Err(Error::new(ErrorKind::Other, "Invalid accel data format"));
            }
            if count > 0 {
                let mut ret = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    ret.push(Vector3 {
                        x: d.read_i16::<BigEndian>()?, // X
                        y: d.read_i16::<BigEndian>()?, // Y
                        z: d.read_i16::<BigEndian>()?, // Z
                    });
                }
                Ok(ret)
            } else {
                Ok(vec![])
            }
        }, tag_data),
        ////////////////////////////////////////// Accelerometer //////////////////////////////////////////

        0xf010 => tag!(UnknownGroup(0xf000), Unknown(tag as u32), "Large unknown", tag_data),
        0xf020 => tag!(UnknownGroup(0xf000), Unknown(tag as u32), "Large unknown", tag_data),

        ////////////////////////////////////////// GPS //////////////////////////////////////////
        0x8500 => tag!(GPS, TagId::Custom("GPSVersionID".into()), "GPS version ID", u32, |v| format!("{}.{}.{}.{}", (v << 24) & 0xff, (v << 16) & 0xff, (v << 8) & 0xff, v & 0xff), |d| d.read_u32::<BigEndian>(), tag_data),
        0x8501 => tag!(GPS, TagId::Custom("GPSLatitudeRef".into()), "GPS latitude ref", u8, |v| match v {
            b'N' => "North".into(),
            b'S' => "South".into(),
            _ => format!("{}", *v as char)
        }, |d| d.read_u8(), tag_data),
        0x8502 => tag!(GPS, TagId::Custom("GPSLatitude".into()), "GPS latitude", f64x3, |v| format!("{}:{}:{}", v.0, v.1, v.2), |d| Ok((read_rational(d)?, read_rational(d)?, read_rational(d)?)), tag_data), // TODO: ToDegrees, ToDMS
        0x8503 => tag!(GPS, TagId::Custom("GPSLongitudeRef".into()), "GPS longitude ref", u8, |v| match v {
            b'E' => "East".into(),
            b'W' => "West".into(),
            _ => format!("{}", *v as char)
        }, |d| d.read_u8(), tag_data),
        0x8504 => tag!(GPS, TagId::Custom("GPSLongitude".into()), "GPS longitude", f64x3, |v| format!("{}:{}:{}", v.0, v.1, v.2), |d| Ok((read_rational(d)?, read_rational(d)?, read_rational(d)?)), tag_data), // TODO: ToDegrees, ToDMS
        0x8505 => tag!(GPS, TagId::Custom("GPSAltitudeRef".into()), "GPS altitude ref", u8, "{}", |d| d.read_u8(), tag_data),
        0x8506 => tag!(GPS, TagId::Custom("GPSAltitude".into()), "GPS altitude", f64, "{}", |d| read_rational(d), tag_data),

        0x8507 => tag!(GPS, TagId::Custom("GPSTimeStamp".into()), "GPS timestamp", f64x3, |v| format!("{}:{}:{}", v.0, v.1, v.2), |d| Ok((read_rational(d)?, read_rational(d)?, read_rational(d)?)), tag_data), // TODO: ConvertTimeStamp, PrintTimeStamp
        0x8509 => tag!(GPS, TagId::Custom("GPSStatus".into()), "GPS status", u8, |v| match v {
            b'A' => "Active".into(),
            b'V' => "Void".into(),
            _ => format!("{}", *v as char)
        }, |d| d.read_u8(), tag_data),
        0x850a => tag!(GPS, TagId::Custom("GPSMeasureMode".into()), "GPS measure mode", u8, |v| match v {
            2 => "2D".into(),
            3 => "3D".into(),
            _ => format!("{}", v)
        }, |d| d.read_u8(), tag_data),

        0x850b => tag!(GPS, TagId::Custom("DOP".into()), "DOP", f64, "{}", |d| read_rational(d), tag_data),
        0x850c => tag!(GPS, TagId::Custom("GPSSpeedRef".into()), "GPS speed ref", u8, |v| match v {
            b'K' => "km/h".into(),
            b'M' => "mph".into(),
            b'N' => "knots".into(),
            _ => format!("{}", *v as char)
        }, |d| d.read_u8(), tag_data),
        0x850d => tag!(GPS, TagId::Custom("GPSSpeed".into()), "GPS speed", f64, "{}", |d| read_rational(d), tag_data),
        0x850e => tag!(GPS, TagId::Custom("GPSTrackRef".into()), "GPS track ref", u8, |v| match v {
            b'T' => "True direction".into(),
            b'M' => "Magnetic direction".into(),
            _ => format!("{}", *v as char)
        }, |d| d.read_u8(), tag_data),

        0x8512 => tag!(GPS, TagId::Custom("GPSMapDatum".into()),  "GPS map datum",  String, |v| v.to_string(), |d| read_utf8(d), tag_data),
        0x851d => tag!(GPS, TagId::Custom("GPSDateStamp".into()), "GPS date stamp", String, |v| v.to_string(), |d| read_utf8(d), tag_data), // TODO: Exif::ExifDate

        ////////////////////////////////////////// GPS //////////////////////////////////////////
        _ => tag!(UnknownGroup(0), Unknown(tag as u32), "Unknown", tag_data),
    }
}

// Helper functions

fn read_f16(d: &mut Cursor::<&[u8]>) -> Result<f32> {
    let num = d.read_i16::<BigEndian>()? as i32;
    let mut exp = (num >> 12) & 0x0F;
    if exp >= 8 {
        exp = -(((!exp) & 0x7) + 1);
    }
    Ok(((num & 0x0FFF) as f64 * 10f64.powf(exp as f64)) as f32)
}

fn read_f16_corrected(d: &mut Cursor::<&[u8]>) -> Result<f32> {
    let num = d.read_i16::<BigEndian>()? as i32;
    let sign = (num & 0x8000) != 0;
    let mut exp = (num >> 10) & 0xFF;
    let mant = (num & 0x03FF) as f64;

    if exp == 0 || exp == 0xFF {
        return Err(Error::new(ErrorKind::Other, "Invalid f16"));
    }
    exp -= 0x0F; // bias
    let ret = ((mant / 8388608.0 + 1.0) * 2f64.powf(exp as f64)) as f32; // (1 + mantissa) * 2^exponent

    Ok(if sign { -ret } else { ret })
}

fn read_utf8(d: &mut Cursor::<&[u8]>) -> Result<String> {
    String::from_utf8(d.get_ref().to_vec()).map_err(|_| Error::new(ErrorKind::Other, "Invalid UTF-8"))
}

fn read_uuid(d: &mut Cursor::<&[u8]>) -> Result<(u32,u32,u32,u32)> {
    Ok((d.read_u32::<BigEndian>()?, d.read_u32::<BigEndian>()?, d.read_u32::<BigEndian>()?, d.read_u32::<BigEndian>()?))
}

fn read_orientation(d: &mut Cursor::<&[u8]>) -> Result<String> {
    let num = d.read_u16::<BigEndian>()?;
    // RX0 II:    0x241 ; 0010 0100 0001 ; xZY
    // A7s III:   0x420 ; 0100 0010 0000 ; XYZ
    // RX100 VII: 0x152 ; 0001 0101 0010 ; Yzx
    // ZV1:       0x351 ; 0011 0101 0001 ; xzy
    // A7C:       0x420 ; 0100 0010 0000 ; XYZ

    fn from_num(n: i8) -> Result<char> {
        Ok(match n { // lowercase is negative
            0 => 'X', 1 => 'x',
            2 => 'Y', 3 => 'y',
            4 => 'Z', 5 => 'z',
            _ => { return Err(Error::new(ErrorKind::Other, format!("Invalid orientation data! {} {:#x} {:#b}", n, n, n))); }
        })
    }

    Ok(vec![
        from_num((num & 0x0f) as i8)?,
        from_num(((num >> 4) & 0x0f) as i8)?,
        from_num(((num >> 8) & 0x0f) as i8)?
    ].iter().collect())
}

fn read_rational(d: &mut Cursor::<&[u8]>) -> Result<f64> {
    let n = d.read_i32::<BigEndian>()? as f64;
    let d = d.read_i32::<BigEndian>()? as f64;
    if d > 0.0 {
        Ok(n / d)
    } else {
        Err(Error::new(ErrorKind::Other, "Invalid rational"))
    }
}
/*
pub trait ReadRTMDExt: io::Read {
    #[inline]
    fn read_i8(&mut self) -> Result<i8> {
        let mut buf = [0; 1];
        self.read_exact(&mut buf)?;
        Ok(buf[0] as i8)
    }
}*/