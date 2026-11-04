// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2025 Adrian <adrian.eddy at gmail>

use std::io::*;
use byteorder::{ ReadBytesExt, BigEndian };
use crate::tags_impl::*;
use crate::tag;
use crate::tags_impl::TagId::*;
use crate::tags_impl::GroupId::*;

/// The group of what a frame states beyond the tags that have a use: Gyroflow leaves it out of the samples it keeps, as
/// it did these items when they were unknown, so a frame's many items cost it no memory
pub const FRAME_METADATA: GroupId = GroupId::Custom(std::borrow::Cow::Borrowed("FrameMetadata"));

pub fn get_tag(tag: u16, tag_data: &[u8]) -> TagDescription {
    match tag {
        // -------------- UserDefinedAcquisitionMetadata --------------
        0xe100 => tag!(Lens,      TagId::Custom("ApertureDisplayMode".into()),        "Aperture Display Mode",                   String, |v| v.to_string(),               |d| Ok(match d.read_u8()? { 0 => "TNumber", 1 => "FNumber", 0xff => "", _ => "Unknown" }.into()), tag_data),
        0xe108 => tag!(Lens,      SerialNumber,                                       "Lens Serial Number",                      String, |v| v.to_string(),               |d| read_utf8(d), tag_data),
        0xe109 => tag!(Lens,      DisplayName,                                        "Lens Type",                               String, |v| v.to_string(),               |d| read_utf8(d), tag_data),
        0xe10d => tag!(Lens,      LensZoomNative,                                     "LensZoom (Actual Focal Length)",          f32,    "{:.2} mm",                      |d| Ok(d.read_u32::<BigEndian>()? as f32 / 100.0), tag_data),
        // In meters, as the C50, R6 V and R6 Mark III write it. The C80 (firmware 1.0.2.1) writes millimeters, 1000 times its RDD 18
        // focus position and its packs
        0xe115 => tag!(Lens,      TagId::Custom("FocusPosition".into()),              "Focus Position (meters)",                 f32,    "{:.3} m",                       |d| Ok(d.read_f32::<BigEndian>()?), tag_data),
        0xe117 => tag!(Lens,      TagId::Custom("FocalLengthNative".into()),          "Focal Length (native)",                   f32,    "{:.2} mm",                      |d| Ok(d.read_u32::<BigEndian>()? as f32 / 10.0), tag_data),
        0xe118 => tag!(Lens,      LensZoom35mm,                                       "LensZoom (35mm Still Camera Equivalent)", f32,    "{:.2} mm",                      |d| Ok(d.read_u32::<BigEndian>()? as f32/ 10.0), tag_data),
        0xe119 => tag!(Lens,      Data2,                                              "Lens Make",                               String, |v| v.to_string(),               |d| read_utf8(d), tag_data),
        0xe11e => tag!(Lens,      FocalLength,                                        "Focal length",                            f32,    "{:.2} mm",                      |d| Ok(d.read_f32::<BigEndian>()?), tag_data),
        0xe219 => tag!(Lens,      DistortionPixelCenter,                              "OpenCV Distortion Center Pixel",          u32x2,  "{:?}",                          |d| { let num = d.read_u16::<BigEndian>()?; let den = d.read_u16::<BigEndian>()?; Ok((num as u32, den as u32)) }, tag_data),
        0xe21a => tag!(Lens,      TagId::Custom("EnabledCorrections".into()),         "Optical Correction (Vignetting, Chroma Aberration, Distortion, Breathing)", Vec_u8, "{:?}", |d| { Ok(vec![d.read_u8()?, d.read_u8()?, d.read_u8()?, d.read_u8()?]) }, tag_data),
        0xe11a => tag!(Lens,      TagId::Custom("Firmware".into()),                   "Lens Firmware Version",                   String, |v| v.to_string(),               |d| read_utf8(d), tag_data),
        0xe204 => tag!(Imager,    PixelHeight,                                        "Image Sensor Pixel Effective Height",     u32,    "{:?} px",                       |d| Ok(d.read_u16::<BigEndian>()? as u32), tag_data),
        0xe205 => tag!(Imager,    PixelWidth,                                         "Image Sensor Pixel Effective Width",      u32,    "{:?} px",                       |d| Ok(d.read_u16::<BigEndian>()? as u32), tag_data),
        0xe206 => tag!(Imager,    TagId::Custom("EffectiveMarkerAspectRatio".into()), "Effective Marker Aspect Ratio",           u32x2,  |v| format!("{}x{}", v.0, v.1),  |d| Ok((d.read_u16::<BigEndian>()? as u32, d.read_u16::<BigEndian>()? as u32)), tag_data),
        0xe207 => tag!(Imager,    TagId::Custom("ActiveAreaAspectRatio".into()),      "Active Area Aspect Ratio",                u32x2,  |v| format!("{}x{}", v.0, v.1),  |d| Ok((d.read_u16::<BigEndian>()? as u32, d.read_u16::<BigEndian>()? as u32)), tag_data),
        0xe209 => tag!(Imager,    FrameReadoutTime,                                   "Sensor readout time",                     f64,    "{:.4} ms",                      |d| d.read_u32::<BigEndian>().map(|x| x as f64 / 1000000.0), tag_data),
        0xe20a => tag!(Imager,    ExposureTime,                                       "Exposure time",                           f64,    "{:.4} ms",                      |d| d.read_u32::<BigEndian>().map(|x| x as f64 / 1000000.0), tag_data),
        0xe11f => tag!(Default,   SensorWidth,                                        "Imager Dimension (Effective Width)",      f32,    "{:.2} mm",                      |d| Ok(d.read_f32::<BigEndian>()? as f32), tag_data),
        0xe120 => tag!(Default,   SensorHeight,                                       "Imager Dimension (Effective Height)",     f32,    "{:.2} mm",                      |d| Ok(d.read_f32::<BigEndian>()? as f32), tag_data),
        0xe203 => tag!(Default,   PixelAspectRatio,                                   "Image Sensor Pixel Aspect Ratio",         u32x2,  |v| format!("{}:{}", v.0, v.1),  |d| { let num = d.read_u16::<BigEndian>()? as u32; let den = d.read_u16::<BigEndian>()? as u32; Ok((num, den)) }, tag_data),
        0xe210 => tag!(Default,   TagId::Custom("LookFileName".into()),               "Look File Name",                          String, |v| v.to_string(),               |d| read_utf8(d), tag_data),
        0xe21b => tag!(Default,   ImageStabilizer,                                    "Lens Image Stabilizer",                   u8,     |v| match v { 0 => "Off", 1 => "On", 3 => "No information", _ => "Reserved" }.to_string(), |d| d.read_u8(), tag_data),
        0xe222 => tag!(Default,   RollingShutterCorrection,                           "RS Distortion Flag",                      bool,   "{}",                            |d| Ok(d.read_u8()? == 1), tag_data),
        0xe224 => tag!(Default,   TagId::Custom("LensStabilizerMode".into()),         "Lens Image Stabilizer Mode",              u8,     |v| code_name(*v, &["Mode 1", "Mode 2", "Mode 3"]), |d| d.read_u8(), tag_data),
        0xe225 => tag!(Default,   TagId::Custom("IBISStabilizerMode".into()),         "In-Body Image Stabilizer Mode",           u8,     |v| code_name(*v, &["Off", "On"]), |d| d.read_u8(), tag_data),
        0xe226 => tag!(Default,   TagId::Custom("EISStabilizerMode".into()),          "Electronic Image Stabilizer Mode",        u8,     |v| code_name(*v, &["Off", "On/Standard", "High"]), |d| d.read_u8(), tag_data),
        0xe228 => tag!(Default,   Name,                                               "Model name",                              String, |v| v.to_string(),               |d| read_utf8(d), tag_data),
        0xe229 => tag!(Default,   SerialNumber,                                       "Camera Serial Number",                    String, |v| v.to_string(),               |d| read_utf8(d), tag_data),
        0xe22A => tag!(Default,   TagId::Custom("Firmware".into()),                   "Camera firmware",                         String, |v| v.to_string(),               |d| read_utf8(d), tag_data),
        0xe22B => tag!(Default,   TagId::Custom("AnamorphicSqueezeRatio".into()),     "Anamorphic Squeeze Ratio",                f32,    "{:.2}",                         |d| Ok(d.read_u16::<BigEndian>()? as f32 / 100.0), tag_data),
        0xe231 => tag!(Exposure,  TagId::Custom("ShutterSpeed2".into()),              "Shutter Speed",                           u32x2,  |v| format!("{}/{}s", v.0, v.1), |d| { let num = d.read_u32::<BigEndian>()?; let den = d.read_u32::<BigEndian>()?; Ok((den, num)) }, tag_data),
        0xe21f => tag!(Gyroscope, Data, "Gyroscope data", Vec_Vector3_f32, "{:?}", |d| read_imu(d), tag_data),
        0xe220 => tag!(Accelerometer, Data, "Accelerometer data", Vec_Vector3_f32, "{:?}", |d| read_imu(d), tag_data),
        0xe221 => tag!(Magnetometer, Data, "Magnetometer data", Vec_Vector3_f32, "{:?}", |d| read_imu(d), tag_data),
        0xe227 => tag!(Default, TimestampMs, "Timestamp per frame", f64, "{:?}", |d| {
            let _tz_dst = d.read_u8()?; // 0 - standard time, 1 - Summertime
            let _tz_hour_sign = d.read_u8()?; // 0: Positive number (Local time is faster than UTC); 1: Negative number (Local time is behind UTC)
            let _tz_hour      = d.read_u8()?; // Absolute difference in time with the UTC (Hour). Note: FF is invalid value.
            let _tz_minute    = d.read_u8()?; // Absolute difference in time with the UTC (Minute). Note: FF is invalid value.
            let year         = d.read_u16::<BigEndian>()?; // 0 to 9999: AD
            let month        = d.read_u8()?; // 1 to 12: Month
            let day          = d.read_u8()?; // 1 to 31: Day
            let hour         = d.read_u8()?; // 0 to 23: Hour
            let minute       = d.read_u8()?; // 0 to 59: Minute
            let second       = d.read_u8()?; // 0 to 59: Second
            let mut millisecond  = d.read_u16::<BigEndian>()?; // 0 to 999: Millisecond; Note: FFFF is invalid value.
            if millisecond == 0xFFFF {
                millisecond = 0;
            }

            //dbg!(year, month, day, hour, minute, second, millisecond);

            timestamp_ms(year, month, day, hour, minute, second, millisecond)
        }, tag_data),
        0xe121 => tag!(Lens, Distortion, "OpenCV distortion param", Vec_f32, "{:?}", |d| {
            let mut data = Vec::with_capacity(8);
            for _ in 0..8 {
                data.push(d.read_f32::<BigEndian>()?);
            }
            Ok(data)
        }, tag_data),
        0xe11d => tag!(Lens, PixelFocalLength, "Focal Length Expressed in Pixel Unit", Vec_f32, "{:?}", |d| {
            let mut data = Vec::with_capacity(8);
            for _ in 0..2 {
                data.push(d.read_f32::<BigEndian>()?);
            }
            Ok(data)
        }, tag_data),
        0xe211 => tag!(Colors, CaptureGammaEquation, "Capture Gamma Equation", Uuid, |v| { match (v.2, v.3) {
            (0x04010101, 0x01020000) => "BT.709"                         .into(),
            (0x04010101, 0x010a0000) => "BT.2100 Perceptual Quantization".into(),
            (0x04010101, 0x010b0000) => "BT.2100 Hybrid Log-Gamma"       .into(),
            (0x04010101, 0x01100000) => "Camera Log C2"                  .into(),
            (0x04010101, 0x01110000) => "Camera Log C3"                  .into(),
            (0x0e150001, 0x01000000) => "Canon Log"                      .into(),
            (0x0e150001, 0x02000000) => "Canon Log 2"                    .into(),
            (0x0e150001, 0x06000000) => "DCI"                            .into(),
            (0x0e150001, 0x07000000) => "Canon Log 3"                    .into(),
            _ => format!("{{{:08x}-{:08x}-{:08x}-{:08x}}}", v.0, v.1, v.2, v.3)
        } }, |d| read_uuid(d), tag_data),
        0xe212 => tag!(Colors, ColorPrimaries, "Color Primaries", Uuid, |v| { match(v.2, v.3) {
            (0x04010101, 0x03030000) => "BT.709"        .into(),
            (0x04010101, 0x03040000) => "BT.2020"       .into(),
            (0x04010101, 0x030E0000) => "Camera Gamut C".into(),
            (0x0e150002, 0x02000000) => "Cinema Gamut"  .into(),
            (0x0e150002, 0x04000000) => "DCI-P3"        .into(),
            _ => format!("{{{:08x}-{:08x}-{:08x}-{:08x}}}", v.0, v.1, v.2, v.3)
        } }, |d| read_uuid(d), tag_data),

        // -------------- Canon Lens Metadata, what the frame only states --------------
        0xe101 => tag!(FRAME_METADATA, TagId::Custom("ApertureDisplayFNumber".into()),        "Aperture Display F-Number Value",           f32,     |v| stated(*v, format!("f/{v:.1}")),      |d| read_rational(d), tag_data),
        0xe102 => tag!(FRAME_METADATA, TagId::Custom("ApertureDisplayTNumber".into()),        "Aperture Display T-Number Value",           f32,     |v| stated(*v, format!("T{v:.1}")),       |d| read_rational(d), tag_data),
        0xe103 => tag!(FRAME_METADATA, TagId::Custom("ApertureFNumberValue".into()),          "Aperture F-Number Value (APEX)",            f32,     |v| stated(*v, format!("{v:.2} EV")),     |d| read_aperture_ev(d), tag_data),
        0xe104 => tag!(FRAME_METADATA, TagId::Custom("ApertureTNumberValue".into()),          "Aperture T-Number Value (APEX)",            f32,     |v| stated(*v, format!("{v:.2} EV")),     |d| read_aperture_ev(d), tag_data),
        0xe105 => tag!(FRAME_METADATA, TagId::Custom("IrisEffectiveFNumber".into()),          "Iris Effective F-Number",                   f32,     |v| stated(*v, format!("f/{v:.2}")),      |d| read_iris(d), tag_data),
        0xe106 => tag!(FRAME_METADATA, TagId::Custom("IrisEffectiveTNumber".into()),          "Iris Effective T-Number",                   f32,     |v| stated(*v, format!("T{v:.2}")),       |d| read_iris(d), tag_data),
        0xe107 => tag!(FRAME_METADATA, TagId::Custom("ApertureShape".into()),                 "Aperture Shape",                            u32,     |v| match *v {
            0..=0xFFFFFFF0 => format!("{v} blades"),
            0xFFFFFFFE     => "Circular".into(),
            0xFFFFFFFF     => "No information".into(),
            _              => "Reserved".into()
        }, |d| d.read_u32::<BigEndian>(), tag_data),
        0xe10a => tag!(FRAME_METADATA, TagId::Custom("TransmissionFactor".into()),            "Transmission Factor",                       String,  |v| format!("{v}%"),                      |d| read_utf8(d), tag_data),
        0xe10b => tag!(FRAME_METADATA, TagId::Custom("EntrancePupilPosition".into()),         "Entrance Pupil Position",                   f32,     "{:.2} mm",                               |d| d.read_f32::<BigEndian>(), tag_data),
        0xe10c => tag!(FRAME_METADATA, TagId::Custom("NormalizedAperturePosition".into()),    "Normalized Aperture Position (0: close end, 65535: open end)", u16, "{}",                         |d| d.read_u16::<BigEndian>(), tag_data),
        0xe10e => tag!(FRAME_METADATA, TagId::Custom("MinimumFocalLength".into()),            "Minimum Focal Length",                      f32,     "{:.2} mm",                               |d| Ok(d.read_u32::<BigEndian>()? as f32 / 100.0), tag_data),
        0xe10f => tag!(FRAME_METADATA, TagId::Custom("MaximumFocalLength".into()),            "Maximum Focal Length",                      f32,     "{:.2} mm",                               |d| Ok(d.read_u32::<BigEndian>()? as f32 / 100.0), tag_data),
        0xe110 => tag!(FRAME_METADATA, TagId::Custom("HyperfocalLength".into()),              "Hyperfocal Length",                         u32,     "{} mm",                                  |d| d.read_u32::<BigEndian>(), tag_data),
        0xe111 => tag!(FRAME_METADATA, TagId::Custom("NormalizedZoomPosition".into()),        "Normalized Zoom Position (0: wide end, 65535: tele end)", u16, "{}",                              |d| d.read_u16::<BigEndian>(), tag_data),
        0xe112 => tag!(FRAME_METADATA, TagId::Custom("NormalizedZoomSpeed".into()),           "Normalized Zoom Speed (0: fastest to wide, 32768: stop, 65535: fastest to tele)", u16, "{}",      |d| d.read_u16::<BigEndian>(), tag_data),
        0xe113 => tag!(FRAME_METADATA, TagId::Custom("ExtenderMagnificationSetting".into()),  "Extender Magnification Setting",            u8,      |v| code_name(*v, &["Unattached", "Attached"]), |d| d.read_u8(), tag_data),
        0xe114 => tag!(FRAME_METADATA, TagId::Custom("NormalizedFocusPosition".into()),       "Normalized Focus Position (0: nearest, 4294967295: infinity)", u32, "{}",                         |d| d.read_u32::<BigEndian>(), tag_data),
        0xe116 => tag!(FRAME_METADATA, TagId::Custom("NormalizedFocusSpeed".into()),          "Normalized Focus Speed (0: fastest to near, 32768: stop, 65535: fastest to far)", u16, "{}",      |d| d.read_u16::<BigEndian>(), tag_data),
        0xe11b => tag!(FRAME_METADATA, TagId::Custom("VariableAberrationLensSetting".into()), "Variable Aberration Lens Setting",          bool,    "{}",                                     |d| Ok(d.read_u8()? != 0), tag_data),
        0xe11c => tag!(FRAME_METADATA, TagId::Custom("VariableAberrationValue".into()),       "Variable Aberration Value",                 f32,     |v| stated(*v, format!("{v:.1}")),        |d| read_offset_tenths(d), tag_data),

        // -------------- Canon Camera Metadata, what the frame only states --------------
        0xe200 => tag!(FRAME_METADATA, TagId::Custom("IRStatus".into()),                      "IR Status",                                 u8,      |v| code_name(*v, &["Off", "On"]),        |d| d.read_u8(), tag_data),
        0xe201 => tag!(FRAME_METADATA, TagId::Custom("ShutterValueAngle".into()),             "Shutter Value (Angle)",                     f32,     |v| stated(*v, format!("{v:.2}°")),       |d| read_rational(d), tag_data),
        0xe202 => tag!(FRAME_METADATA, TagId::Custom("MeteringExposureValue".into()),         "Metering Exposure Value (APEX)",            f32,     |v| stated(*v, format!("{v:.2} EV")),     |d| read_ev(d), tag_data),
        0xe208 => tag!(FRAME_METADATA, TagId::Custom("ElectricZoom".into()),                  "Electric Zoom",                             f32,     |v| stated(*v, format!("x{v:.2}")),       |d| read_rational(d), tag_data),
        0xe20b => tag!(FRAME_METADATA, TagId::Custom("SensorReadoutFlag".into()),             "Sensor Readout Flag",                       u8,      |v| code_name(*v, &["Copied frame", "Frame with sensor readout"]), |d| d.read_u8(), tag_data),
        0xe20c => tag!(FRAME_METADATA, TagId::Custom("WhiteBalanceMode".into()),              "White Balance Mode",                        u8,      |v| code_name(*v, &["Auto (Ambience priority)", "Auto (White priority)", "Daylight", "Shade", "Cloudy", "Tungsten light", "White fluorescent light", "When using Flash", "Color Temperature", "Manual Gain", "Custom"]), |d| d.read_u8(), tag_data),
        0xe20d => tag!(FRAME_METADATA, TagId::Custom("WhiteBalanceAccurateFlag".into()),      "White Balance Accurate Flag",               bool,    "{}",                                     |d| Ok(d.read_u8()? != 0), tag_data),
        0xe20e => tag!(FRAME_METADATA, TagId::Custom("GammaForLook".into()),                  "Gamma for Look",                            u8,      |v| code_name(*v, &["BT.709", "PQ", "HLG", "Canon 709", "Wide DR", "Canon Log 2", "Canon Log 3", "CMT 709", "BT.709 (Picture Style)", "PQ (Picture Style)", "HLG (Picture Style)"]), |d| d.read_u8(), tag_data),
        0xe20f => tag!(FRAME_METADATA, TagId::Custom("ColorPrimariesForLook".into()),         "Color Primaries for Look",                  u8,      |v| code_name(*v, &["BT.709", "BT.2020", "Cinema Gamut"]), |d| d.read_u8(), tag_data),
        0xe213 => tag!(FRAME_METADATA, TagId::Custom("SDIOutViewing".into()),                 "SDI OUT Viewing",                           u8,      |v| viewing_name(*v),                     |d| d.read_u8(), tag_data),
        0xe214 => tag!(FRAME_METADATA, TagId::Custom("MonViewing".into()),                    "MON. Viewing",                              u8,      |v| viewing_name(*v),                     |d| d.read_u8(), tag_data),
        0xe215 => tag!(FRAME_METADATA, TagId::Custom("HDMIViewing".into()),                   "HDMI Viewing",                              u8,      |v| viewing_name(*v),                     |d| d.read_u8(), tag_data),
        0xe216 => tag!(FRAME_METADATA, TagId::Custom("BaseISOSensitivity".into()),            "Base ISO Sensitivity",                      u32,     "{}",                                     |d| d.read_u32::<BigEndian>(), tag_data),
        0xe217 => tag!(FRAME_METADATA, TagId::Custom("BaseISOSensitivityMode".into()),        "Base ISO Sensitivity Mode",                 u8,      |v| code_name(*v, &["Auto", "Manual"]),   |d| d.read_u8(), tag_data),
        0xe218 => tag!(FRAME_METADATA, TagId::Custom("HighSensitivityMode".into()),           "High Sensitivity Mode",                     u8,      |v| code_name(*v, &["Off", "On"]),        |d| d.read_u8(), tag_data),
        0xe21e => tag!(FRAME_METADATA, TagId::Custom("NumberOfStoredData".into()),            "Number of Stored Data (Gyroscope, Accelerometer, Magnetometer)", Vec_u8, "{:?}",                  |d| Ok(d.get_ref().to_vec()), tag_data),
        0xe223 => tag!(FRAME_METADATA, TagId::Custom("MatchMoveData".into()),                 "Match Move Data (X, Y, Z in mm, Pitch, Yaw, Roll in degrees)", Vec_f32, "{:?}",                   |d| (0..6).map(|_| d.read_f32::<BigEndian>()).collect(), tag_data),
        0xe22c => tag!(FRAME_METADATA, TagId::Custom("NDFilterDisplayMode".into()),           "Neutral Density Filter Display Mode",       u8,      |v| code_name(*v, &["Stop", "Transmittance", "Optical Density"]), |d| d.read_u8(), tag_data),
        0xe22d => tag!(FRAME_METADATA, TagId::Custom("NDFilterStop".into()),                  "Neutral Density Filter Stop",               f32,     |v| stated(*v, format!("{v:.2}")),        |d| read_rational(d), tag_data),
        0xe22e => tag!(FRAME_METADATA, TagId::Custom("NDFilterTransmittance".into()),         "Neutral Density Filter Transmittance",      f32,     |v| stated(*v, format!("{v:.4}")),        |d| read_rational(d), tag_data),
        0xe22f => tag!(FRAME_METADATA, TagId::Custom("NDFilterOpticalDensity".into()),        "Neutral Density Filter Optical Density",    f32,     |v| stated(*v, format!("{v:.2}")),        |d| read_rational(d), tag_data),
        0xe230 => tag!(FRAME_METADATA, TagId::Custom("ShutterSpeedDisplayMode".into()),       "Shutter Speed Value Display Mode",          u8,      |v| code_name(*v, &["Second", "Angle", "Frequency (Hz)"]), |d| d.read_u8(), tag_data),
        0xe232 => tag!(FRAME_METADATA, TagId::Custom("ExposureTimeValue".into()),             "Exposure Time Value (APEX)",                f32,     |v| stated(*v, format!("{v:.2} EV")),     |d| read_ev(d), tag_data),
        0xe233 => tag!(FRAME_METADATA, TagId::Custom("SensitivityValue".into()),              "Sensitivity Value (APEX)",                  f32,     |v| stated(*v, format!("{v:.2} EV")),     |d| read_ev(d), tag_data),
        0xe234 => tag!(FRAME_METADATA, TagId::Custom("NDFilterValue".into()),                 "Neutral Density Filter Value (APEX)",       f32,     |v| stated(*v, format!("{v:.2} EV")),     |d| read_ev(d), tag_data),
        0xe235 => tag!(FRAME_METADATA, TagId::Custom("LookFileIntensity".into()),             "Look File Intensity",                       u8,      "{}%",                                    |d| d.read_u8(), tag_data),

        // -------------- Cooke /i Lens Metadata --------------
        0xe400 => tag!(Cooke, SerialNumber,                                    "Cooke Lens Serial Number",     String, |v| v.to_string(),                  |d| read_utf8(d), tag_data),
        0xe401 => tag!(Cooke, DisplayName,                                     "Cooke Lens Type",              String, |v| v.to_string(),                  |d| read_utf8(d), tag_data),
        0xe402 => tag!(Cooke, TagId::Custom("MinimumFocalLength".into()),      "Cooke Minimum Focal Length",   u16,    "{} mm",                            |d| d.read_u16::<BigEndian>(), tag_data),
        0xe403 => tag!(Cooke, TagId::Custom("MaximumFocalLength".into()),      "Cooke Maximum Focal Length",   u16,    "{} mm",                            |d| d.read_u16::<BigEndian>(), tag_data),
        0xe404 => tag!(Cooke, TagId::Custom("TransmissionFactor".into()),      "Cooke Transmission Factor",    u8,     "{}%",                              |d| d.read_u8(), tag_data),
        0xe405 => tag!(Cooke, FocusDistance,                                   "Cooke Actual Focus Distance",  u32,    |v| cooke_distance(*v),             |d| d.read_u32::<BigEndian>(), tag_data),
        0xe406 => tag!(Cooke, IrisTStop,                                       "Cooke Ring T Number",          f32,    "T{:.2}",                           |d| Ok(d.read_u16::<BigEndian>()? as f32 / 100.0), tag_data),
        0xe407 => tag!(Cooke, FocalLength,                                     "Cooke Effective Focal Length", u16,    "{} mm",                            |d| d.read_u16::<BigEndian>(), tag_data),
        0xe408 => tag!(Cooke, TagId::Custom("HyperfocalDistance".into()),      "Cooke Hyperfocal Distance",    u32,    |v| cooke_distance(*v),             |d| d.read_u32::<BigEndian>(), tag_data),
        0xe409 => tag!(Cooke, TagId::Custom("NearFocusDistance".into()),       "Cooke Near Focus Distance",    u32,    |v| cooke_distance(*v),             |d| d.read_u32::<BigEndian>(), tag_data),
        0xe40a => tag!(Cooke, TagId::Custom("FarFocusDistance".into()),        "Cooke Far Focus Distance",     u32,    |v| cooke_distance(*v),             |d| d.read_u32::<BigEndian>(), tag_data),
        0xe40b => tag!(Cooke, TagId::Custom("HorizontalFieldOfView".into()),   "Cooke Horizontal Field of View", f32,  "{:.1}°",                           |d| Ok(d.read_u16::<BigEndian>()? as f32 / 10.0), tag_data),
        0xe40c => tag!(Cooke, TagId::Custom("EntrancePupilPosition".into()),   "Cooke Entrance Pupil Position", i16,   "{} mm",                            |d| d.read_i16::<BigEndian>(), tag_data),
        0xe40d => tag!(Cooke, TagId::Custom("NormalizedZoomValue".into()),     "Cooke Normalized Zoom Value (0: wide end, 1000: tele end)", u16, "{}",     |d| d.read_u16::<BigEndian>(), tag_data),
        0xe40e => tag!(Cooke, TagId::Custom("Make".into()),                    "Cooke Lens Make",              String, |v| v.to_string(),                  |d| read_utf8(d), tag_data),
        0xe40f => tag!(Cooke, TagId::Custom("Firmware".into()),                "Cooke Lens Firmware Version",  String, |v| v.to_string(),                  |d| read_utf8(d), tag_data),

        _ => tag!(UnknownGroup(0), Unknown(tag as u32), "Unknown", tag_data),
    }
}

// Helper functions

/// The time zone that E2.27 and the CRM's time pack state, `None` when they have no hour of it
pub fn time_zone_tag(summer_time: bool, behind_utc: bool, hour: u8, minute: Option<u8>) -> Option<TagDescription> {
    if hour == 0xff { return None; }
    let zone = format!("UTC{}{hour:02}{}{}", if behind_utc { '-' } else { '+' }, minute.map_or(String::new(), |x| format!(":{x:02}")), if summer_time { " (summer time)" } else { "" });
    Some(tag!(parsed FRAME_METADATA, TagId::Custom("TimeZone".into()), "Time Zone", String, |v| v.to_string(), zone, Vec::new()))
}

/// The time that E2.27 and the CRM's time pack state, in the camera's time zone, as ms from 1970
pub fn timestamp_ms(year: u16, month: u8, day: u8, hour: u8, minute: u8, second: u8, millisecond: u16) -> Result<f64> {
    let date = chrono::NaiveDate::from_ymd_opt(year as _, month as _, day as _);
    let datetime = date.and_then(|date| date.and_hms_milli_opt(hour as _, minute as _, second as _, millisecond as _));
    let datetime = datetime.ok_or_else(|| Error::new(ErrorKind::InvalidData, "Invalid date or time"))?;

    Ok(datetime.and_utc().timestamp_millis() as f64)
}

/// The readings of an IMU item (E2.1F to E2.21), which has room for 21 whatever the frame has: the first `stored`, the
/// number in E2.1E, or in a set without it those up to the zeros that follow them
pub fn imu_readings(data: &[u8], stored: Option<u8>) -> &[u8] {
    let readings = stored.map_or_else(|| data.chunks_exact(6).rposition(|x| x != [0; 6]).map_or(0, |last| last + 1), |x| x as usize);
    &data[..(readings * 6).min(data.len())]
}

fn read_imu(d: &mut Cursor::<&[u8]>) -> Result<Vec<Vector3<f32>>> {
    let mut ret = Vec::with_capacity(21);
    while d.position() < d.get_ref().len() as u64 {
        ret.push(Vector3 {
            x: half::f16::from_bits(d.read_u16::<BigEndian>()?).to_f32(),
            y: half::f16::from_bits(d.read_u16::<BigEndian>()?).to_f32(),
            z: half::f16::from_bits(d.read_u16::<BigEndian>()?).to_f32(),
        });
    }
    Ok(ret)
}

/// How a value read below is shown: `NAN` stands for no information and `INFINITY` for a closed iris, which aren't numbers
fn stated(v: f32, number: String) -> String {
    if v.is_nan() { "No information".into() } else if v.is_infinite() { "Closed".into() } else { number }
}

/// The name of an enumerated code. In all of the tables FFh is "No information" or "Undefined", and the rest is reserved
pub fn code_name(code: u8, names: &[&str]) -> String {
    if code == 0xff { return "No information".into(); }
    names.get(code as usize).map_or("Reserved".into(), |x| x.to_string())
}

/// The characteristics of the signal of a terminal (E2.13 to E2.15)
fn viewing_name(code: u8) -> String {
    if code == 0xfe { return "Other".into(); }
    code_name(code, &["BT.709", "DCI", "PQ", "HLG", "Canon 709", "Wide DR", "Canon Log 2", "Canon Log 3", "CMT 709", "CMT DCI", "CMT PQ", "CMT HLG", "BT.709 (Picture Style)", "PQ (Picture Style)", "HLG (Picture Style)", "ACESproxy", "HDR Assist", "User LUT"])
}

fn cooke_distance(mm: u32) -> String {
    if mm == 0xFFFFFFFF { "Infinity".into() } else { format!("{mm} mm") }
}

/// A rational of two signed 32-bit integers. FFFFFFFFh is no information and FFFEFFFEh a closed iris
fn read_rational(d: &mut Cursor::<&[u8]>) -> Result<f32> {
    let (num, den) = (d.read_u32::<BigEndian>()?, d.read_i32::<BigEndian>()?);
    Ok(match num {
        0xFFFFFFFF => f32::NAN,
        0xFFFEFFFE => f32::INFINITY,
        _ if den == 0 => f32::NAN,
        _ => num as i32 as f32 / den as f32
    })
}

/// An APEX value in [EV]/768, 7FFFh for no information
fn read_ev(d: &mut Cursor::<&[u8]>) -> Result<f32> {
    Ok(match d.read_i16::<BigEndian>()? { 0x7fff => f32::NAN, v => v as f32 / 768.0 })
}

/// An aperture's APEX value, and 7FFEh for a closed iris
fn read_aperture_ev(d: &mut Cursor::<&[u8]>) -> Result<f32> {
    Ok(match d.read_i16::<BigEndian>()? { 0x7fff => f32::NAN, 0x7ffe => f32::INFINITY, v => v as f32 / 768.0 })
}

/// The F or T number of E1.05 and E1.06: a stop in every 1000h, F16 at 6000h. 0 is no information and 1 a closed iris
fn read_iris(d: &mut Cursor::<&[u8]>) -> Result<f32> {
    Ok(match d.read_u16::<BigEndian>()? {
        0 => f32::NAN,
        1 => f32::INFINITY,
        v => 2f32.powf(8.0 * (1.0 - (v as f32 + 8192.0) / 65536.0))
    })
}

/// A value in tenths around 8000h, FFFFh for no information
fn read_offset_tenths(d: &mut Cursor::<&[u8]>) -> Result<f32> {
    Ok(match d.read_u16::<BigEndian>()? { 0xffff => f32::NAN, v => (v as i32 - 0x8000) as f32 / 10.0 })
}

fn read_utf8(d: &mut Cursor::<&[u8]>) -> Result<String> {
    let v = d.get_ref().to_vec();
    let nul = v.iter().position(|&c| c == b'\0').unwrap_or(v.len());
    std::str::from_utf8(&v[..nul]).map_err(|_| Error::new(ErrorKind::Other, "Invalid UTF-8")).map(|s| s.to_string())
}

fn read_uuid(d: &mut Cursor::<&[u8]>) -> Result<(u32,u32,u32,u32)> {
    Ok((d.read_u32::<BigEndian>()?, d.read_u32::<BigEndian>()?, d.read_u32::<BigEndian>()?, d.read_u32::<BigEndian>()?))
}
