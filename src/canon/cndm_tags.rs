// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2025 Adrian <adrian.eddy at gmail>

use std::io::*;
use byteorder::{ ReadBytesExt, BigEndian };
use crate::tags_impl::*;
use crate::tag;
use crate::tags_impl::TagId::*;
use crate::tags_impl::GroupId::*;

pub fn get_tag(tag: u16, tag_data: &[u8]) -> TagDescription {
    match tag {
        // -------------- UserDefinedAcquisitionMetadata --------------
        0xe100 => tag!(Lens,      TagId::Custom("ApertureDisplayMode".into()),        "Aperture Display Mode",                   String, |v| v.to_string(),               |d| Ok(match d.read_u8()? { 0 => "TNumber", 1 => "FNumber", 0xff => "", _ => "Unknown" }.into()), tag_data),
        0xe108 => tag!(Lens,      SerialNumber,                                       "Lens Serial Number",                      String, |v| v.to_string(),               |d| read_utf8(d), tag_data),
        0xe109 => tag!(Lens,      DisplayName,                                        "Lens Type",                               String, |v| v.to_string(),               |d| read_utf8(d), tag_data),
        0xe10d => tag!(Lens,      LensZoomNative,                                     "LensZoom (Actual Focal Length)",          f32,    "{:.2} mm",                      |d| Ok(d.read_u32::<BigEndian>()? as f32 / 100.0), tag_data),
        0xe117 => tag!(Lens,      TagId::Custom("FocalLengthNative".into()),          "Focal Length (native)",                   f32,    "{:.2} mm",                      |d| Ok(d.read_u32::<BigEndian>()? as f32), tag_data),
        0xe118 => tag!(Lens,      LensZoom35mm,                                       "LensZoom (35mm Still Camera Equivalent)", f32,    "{:.2} mm",                      |d| Ok(d.read_u32::<BigEndian>()? as f32/ 10.0), tag_data),
        0xe119 => tag!(Lens,      Data2,                                              "Lens Make",                               String, |v| v.to_string(),               |d| read_utf8(d), tag_data),
        0xe11e => tag!(Lens,      FocalLength,                                        "Focal length",                            f32,    "{:.2} mm",                      |d| Ok(d.read_f32::<BigEndian>()?), tag_data),
        0xe219 => tag!(Lens,      DistortionPixelCenter,                              "OpenCV Distortion Center Pixel",          u32x2,  "{:?}",                          |d| { let num = d.read_u16::<BigEndian>()?; let den = d.read_u16::<BigEndian>()?; Ok((num as u32, den as u32)) }, tag_data),
        0xe21a => tag!(Lens,      TagId::Custom("EnabledCorrections".into()),         "Optical Correction (Vignetting, Chroma Aberration, Distortion, Breathing)", Vec_u8, "{:?}", |d| { Ok(vec![d.read_u8()?, d.read_u8()?, d.read_u8()?, d.read_u8()?]) }, tag_data),
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
        0xe21b => tag!(Default,   ImageStabilizer,                                    "Optical Image Stabilizer",                bool,   "{}",                            |d| Ok(d.read_u8()? == 0), tag_data),
        0xe222 => tag!(Default,   RollingShutterCorrection,                           "RS Distortion Flag",                      bool,   "{}",                            |d| Ok(d.read_u8()? == 1), tag_data),
        0xe224 => tag!(Default,   TagId::Custom("LensStabilizerMode".into()),         "Lens Image Stabilizer Mode",              u8,     "{}",                            |d| d.read_u8(), tag_data),
        0xe225 => tag!(Default,   TagId::Custom("IBISStabilizerMode".into()),         "In-Body Image Stabilizer Mode",           u8,     "{}",                            |d| d.read_u8(), tag_data),
        0xe226 => tag!(Default,   TagId::Custom("EISStabilizerMode".into()),          "Electronic Image Stabilizer Mode",        u8,     "{}",                            |d| d.read_u8(), tag_data),
        0xe228 => tag!(Default,   Name,                                               "Model name",                              String, |v| v.to_string(),               |d| read_utf8(d), tag_data),
        0xe229 => tag!(Default,   SerialNumber,                                       "Camera Serial Number",                    String, |v| v.to_string(),               |d| read_utf8(d), tag_data),
        0xe22A => tag!(Default,   TagId::Custom("Firmware".into()),                   "Camera firmware",                         String, |v| v.to_string(),               |d| read_utf8(d), tag_data),
        0xe22B => tag!(Default,   TagId::Custom("AnamorphicSqueezeRatio".into()),     "Anamorphic Squeeze Ratio",                f32,    "{:.2}",                         |d| Ok(d.read_u16::<BigEndian>()? as f32 / 100.0), tag_data),
        0xe231 => tag!(Exposure,  TagId::Custom("ShutterSpeed2".into()),              "Shutter Speed",                           u32x2,  |v| format!("{}/{}s", v.0, v.1), |d| { let num = d.read_u32::<BigEndian>()?; let den = d.read_u32::<BigEndian>()?; Ok((den, num)) }, tag_data),
        0xe21f => tag!(Gyroscope, Data, "Gyroscope data", Vec_Vector3_f32, "{:?}", |d| {
            let mut ret = Vec::with_capacity(21);
            while d.position() < d.get_ref().len() as u64 {
                let x = d.read_u16::<BigEndian>()?;
                let y = d.read_u16::<BigEndian>()?;
                let z = d.read_u16::<BigEndian>()?;
                if x == 0 && y == 0 && z == 0 {
                    continue;
                }
                ret.push(Vector3 {
                    x: half::f16::from_bits(x).to_f32(),
                    y: half::f16::from_bits(y).to_f32(),
                    z: half::f16::from_bits(z).to_f32(),
                });
            }
            Ok(ret)
        }, tag_data),
        0xe220 => tag!(Accelerometer, Data, "Accelerometer data", Vec_Vector3_f32, "{:?}", |d| {
            let mut ret = Vec::with_capacity(21);
            while d.position() < d.get_ref().len() as u64 {
                let x = d.read_u16::<BigEndian>()?;
                let y = d.read_u16::<BigEndian>()?;
                let z = d.read_u16::<BigEndian>()?;
                if x == 0 && y == 0 && z == 0 {
                    continue;
                }
                ret.push(Vector3 {
                    x: half::f16::from_bits(x).to_f32(),
                    y: half::f16::from_bits(y).to_f32(),
                    z: half::f16::from_bits(z).to_f32(),
                });
            }
            Ok(ret)
        }, tag_data),
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

            let date = chrono::NaiveDate::from_ymd_opt(year as _, month as _, day as _).unwrap();
            let datetime = date.and_hms_milli_opt(hour as _, minute as _, second as _, millisecond as _).unwrap();

            Ok(datetime.and_utc().timestamp_millis() as f64)
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

        _ => tag!(UnknownGroup(0), Unknown(tag as u32), "Unknown", tag_data),
    }
}

// Helper functions

fn read_utf8(d: &mut Cursor::<&[u8]>) -> Result<String> {
    let v = d.get_ref().to_vec();
    let nul = v.iter().position(|&c| c == b'\0').unwrap_or(v.len());
    std::str::from_utf8(&v[..nul]).map_err(|_| Error::new(ErrorKind::Other, "Invalid UTF-8")).map(|s| s.to_string())
}

fn read_uuid(d: &mut Cursor::<&[u8]>) -> Result<(u32,u32,u32,u32)> {
    Ok((d.read_u32::<BigEndian>()?, d.read_u32::<BigEndian>()?, d.read_u32::<BigEndian>()?, d.read_u32::<BigEndian>()?))
}
