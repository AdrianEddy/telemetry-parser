// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2021 Adrian <adrian.eddy at gmail>

use std::collections::BTreeMap;
use std::io::*;
use std::sync::{ Arc, atomic::AtomicBool };
use byteorder::{ ReadBytesExt, BigEndian, LittleEndian };

use crate::tags_impl::*;
use crate::*;

struct Format {
    typ: u8,
    _length: u8,
    name: String,
    format: String,
    multipliers: Option<String>,
    units: Option<String>,
    labels: Vec<String>
}

#[allow(non_camel_case_types)]
#[derive(Debug, Clone, ::serde::Serialize, ::serde::Deserialize)]
pub enum FieldType {
    u8(u8), i8(i8),
    u16(u16), i16(i16),
    u32(u32), i32(i32),
    u64(u64), i64(i64),
    f32(f32), f64(f64),
    String(String),
    Vec_i16(Vec<i16>), Vec_u16(Vec<u16>),
    Vec_i32(Vec<i32>), Vec_u32(Vec<u32>),
}

#[derive(Debug, Clone, ::serde::Serialize, ::serde::Deserialize)]
pub struct Field {
    value: FieldType,
    unit: Option<String>,
    multiplier: Option<f64>
}

#[derive(Debug, Clone, ::serde::Serialize, ::serde::Deserialize)]
pub struct LogItem {
    typ: u8,
    name: String,
    data: BTreeMap<String, Field>
}

pub fn parse_full<T: Read + Seek, F: Fn(f64)>(stream: &mut T, size: usize, progress_cb: F, cancel_flag: Arc<AtomicBool>) -> Result<Vec<LogItem>> {
    let mut units = BTreeMap::from([
        ( '-', ""             .to_owned() ), // no units e.g. Pi, or a string
        ( '?', "UNKNOWN"      .to_owned() ), // Units which haven't been worked out yet....
        ( 'A', "A"            .to_owned() ), // Ampere
        ( 'a', "Ah"           .to_owned() ), // Ampere hours
        ( 'd', "deg"          .to_owned() ), // of the angular variety, -180 to 180
        ( 'b', "B"            .to_owned() ), // bytes
        ( 'k', "deg/s"        .to_owned() ), // degrees per second. Degrees are NOT SI, but is some situations more user-friendly than radians
        ( 'D', "deglatitude"  .to_owned() ), // degrees of latitude
        ( 'e', "deg/s/s"      .to_owned() ), // degrees per second per second. Degrees are NOT SI, but is some situations more user-friendly than radians
        ( 'E', "rad/s"        .to_owned() ), // radians per second
        ( 'G', "Gauss"        .to_owned() ), // Gauss is not an SI unit, but 1 tesla = 10000 gauss so a simple replacement is not possible here
        ( 'h', "degheading"   .to_owned() ), // 0.? to 359.?
        ( 'i', "A.s"          .to_owned() ), // Ampere second
        ( 'J', "W.s"          .to_owned() ), // Joule (Watt second)
        // ( 'l', "l"         .to_owned() ), // litres
        ( 'L', "rad/s/s"      .to_owned() ), // radians per second per second
        ( 'm', "m"            .to_owned() ), // metres
        ( 'n', "m/s"          .to_owned() ), // metres per second
        // ( 'N', "N"         .to_owned() ), // Newton
        ( 'o', "m/s/s"        .to_owned() ), // metres per second per second
        ( 'O', "degC"         .to_owned() ), // degrees Celsius. Not SI, but Kelvin is too cumbersome for most users
        ( '%', "%"            .to_owned() ), // percent
        ( 'S', "satellites"   .to_owned() ), // number of satellites
        ( 's', "s"            .to_owned() ), // seconds
        ( 'q', "rpm"          .to_owned() ), // rounds per minute. Not SI, but sometimes more intuitive than Hertz
        ( 'r', "rad"          .to_owned() ), // radians
        ( 'U', "deglongitude" .to_owned() ), // degrees of longitude
        ( 'u', "ppm"          .to_owned() ), // pulses per minute
        ( 'v', "V"            .to_owned() ), // Volt
        ( 'P', "Pa"           .to_owned() ), // Pascal
        ( 'w', "Ohm"          .to_owned() ), // Ohm
        ( 'W', "Watt"         .to_owned() ), // Watt
        ( 'X', "W.h"          .to_owned() ), // Watt hour
        ( 'Y', "us"           .to_owned() ), // pulse width modulation in microseconds
        ( 'z', "Hz"           .to_owned() ), // Hertz
        ( '#', "instance"     .to_owned() )  // (e.g.)Sensor instance number
    ]);
    let mut multipliers = BTreeMap::from([
        ( '-', 0.0 ),       // no multiplier e.g. a string
        ( '?', 1.0 ),       // multipliers which haven't been worked out yet....
    // <leave a gap here, just in case....>
        ( '2', 1e2 ),
        ( '1', 1e1 ),
        ( '0', 1e0 ),
        ( 'A', 1e-1 ),
        ( 'B', 1e-2 ),
        ( 'C', 1e-3 ),
        ( 'D', 1e-4 ),
        ( 'E', 1e-5 ),
        ( 'F', 1e-6 ),
        ( 'G', 1e-7 ),
        ( 'I', 1e-9 ),
    // <leave a gap here, just in case....>
        ( '!', 3.6 ), // (ampere*second => milliampere*hour) and (km/h => m/s)
        ( '/', 3600.0 ), // (ampere*second => ampere*hour)
    ]);

    let mut stream = std::io::BufReader::with_capacity(16*1024*1024, stream);

    let mut formats = BTreeMap::new();
    let mut log = Vec::<LogItem>::new();

    while (size as i64 - stream.stream_position()? as i64) >= 3 {
        let mut update_format = BTreeMap::new();

        if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) { break; }
        if size > 0 {
            progress_cb(stream.stream_position()? as f64 / size as f64);
        }

        if stream.read_u16::<BigEndian>()? == 0xA395 {
            let id = stream.read_u8()?;
            if id == 0x80 { // Format message
                let mut name = vec![0u8; 4];
                let mut format = vec![0u8; 16];
                let mut labels = vec![0u8; 64];
                let typ = stream.read_u8()?;
                let _length = stream.read_u8()?;
                stream.read_exact(&mut name)?;
                stream.read_exact(&mut format)?;
                stream.read_exact(&mut labels)?;
                formats.insert(typ, Format {
                    typ,
                    _length,
                    units: None,
                    multipliers: None,
                    name: String::from_utf8_lossy(&name).trim_matches('\0').to_string(),
                    format: String::from_utf8_lossy(&format).trim_matches('\0').to_string(),
                    labels: String::from_utf8_lossy(&labels).trim_matches('\0').split(',').map(str::to_string).collect(),
                });
            } else if let Some(desc) = formats.get(&id) {
                if desc.format.len() > 0 && desc.format.len() == desc.labels.len() {
                    let mut msg = BTreeMap::new();
                    for (i, (f, label)) in desc.format.chars().zip(&desc.labels).enumerate() {
                        if let Err(e) = (|| -> Result<()> {
                            let unit = desc.units.as_ref().and_then(|v| v.chars().nth(i));
                            let unit = unit.map(|v| units.get(&v).cloned().unwrap_or_else(|| format!("{}", v)));
                            let mult = desc.multipliers.as_ref().and_then(|v| v.chars().nth(i));
                            let mult = mult.and_then(|v| multipliers.get(&v).copied());

                            let value = match f {
                                'a' => Some(FieldType::Vec_i16((0..32).filter_map(|_| stream.read_i16::<LittleEndian>().ok()).collect())),
                                'b' => Some(FieldType::i8(stream.read_i8()?)),
                                'B' => Some(FieldType::u8(stream.read_u8()?)),
                                'h' => Some(FieldType::i16(stream.read_i16::<LittleEndian>()?)),
                                'H' => Some(FieldType::u16(stream.read_u16::<LittleEndian>()?)),
                                'i' => Some(FieldType::i32(stream.read_i32::<LittleEndian>()?)),
                                'I' => Some(FieldType::u32(stream.read_u32::<LittleEndian>()?)),
                                'f' => Some(FieldType::f32(stream.read_f32::<LittleEndian>()?)),
                                'd' => Some(FieldType::f64(stream.read_f64::<LittleEndian>()?)),
                                'n' | 'N' | 'Z' => {
                                    let s = match f { 'n' => 4, 'N' => 16, 'Z' => 64, _ => 0 };
                                    let mut data = vec![0u8; s];
                                    stream.read_exact(&mut data)?;
                                    Some(FieldType::String(String::from_utf8_lossy(&data).trim_matches('\0').to_string()))
                                }
                                'c' => Some(FieldType::Vec_i16((0..100).filter_map(|_| stream.read_i16::<LittleEndian>().ok()).collect())),
                                'C' => Some(FieldType::Vec_u16((0..100).filter_map(|_| stream.read_u16::<LittleEndian>().ok()).collect())),
                                'e' => Some(FieldType::Vec_i32((0..100).filter_map(|_| stream.read_i32::<LittleEndian>().ok()).collect())),
                                'E' => Some(FieldType::Vec_u32((0..100).filter_map(|_| stream.read_u32::<LittleEndian>().ok()).collect())),
                                'L' => Some(FieldType::i32(stream.read_i32::<LittleEndian>()?)), // latitude/longitude
                                'M' => Some(FieldType::u8(stream.read_u8()?)), // flight mode
                                'q' => Some(FieldType::i64(stream.read_i64::<LittleEndian>()?)),
                                'Q' => Some(FieldType::u64(stream.read_u64::<LittleEndian>()?)),
                                _ => {
                                    log::error!("Invalid format {}", f);
                                    None
                                }
                            };
                            if let Some(value) = value {
                                msg.insert(label.clone(), Field { value, unit, multiplier: mult });
                            }
                            Ok(())
                        })() {
                            log::error!("error parsing data: {e:?}")
                        }
                    }
                    match desc.name.as_ref() {
                        "UNIT" => match (msg.get("Id").map(|v| &v.value), msg.get("Label").map(|v| &v.value)) {
                            (Some(FieldType::i8(id)), Some(FieldType::String(label))) => {
                                units.insert(char::from(*id as u8), label.clone());
                            },
                            _ => { }
                        },
                        "MULT" => match (msg.get("Id").map(|v| &v.value), msg.get("Mult").map(|v| &v.value)) {
                            (Some(FieldType::i8(id)), Some(FieldType::f64(mult))) => {
                                multipliers.insert(char::from(*id as u8), *mult);
                            },
                            _ => { }
                        },
                        "FMTU" => match (msg.get("FmtType").map(|v| &v.value), msg.get("MultIds").map(|v| &v.value), msg.get("UnitIds").map(|v| &v.value)) {
                            (Some(FieldType::u8(id)), Some(FieldType::String(mult)), Some(FieldType::String(unit))) => {
                                update_format.insert(*id, (mult.clone(), unit.clone()));
                            },
                            _ => { }
                        },
                        _ => { }
                    }
                    // log::debug!("{}: {:?}", desc.name, msg);
                    log.push(LogItem {
                        typ: desc.typ,
                        name: desc.name.clone(),
                        data: msg
                    });
                }
            } else {
                log::warn!("Unknown msg: {}", id);
            }
        }
        if !update_format.is_empty() {
            for (id, (mult, unit)) in update_format {
                if let Some(desc) = formats.get_mut(&id) {
                    desc.units = Some(unit);
                    desc.multipliers = Some(mult);
                }
            }
        }
    }
    Ok(log)
}

pub fn parse<T: Read + Seek, F: Fn(f64)>(stream: &mut T, size: usize, progress_cb: F, cancel_flag: Arc<AtomicBool>) -> Result<Vec<SampleInfo>> {
    let log = parse_full(stream, size, progress_cb, cancel_flag)?;

    let mut gyro = BTreeMap::from([ ("VSTB", vec![]), ("IMU", vec![]), ("GYR", vec![]) ]);
    let mut accl = BTreeMap::from([ ("VSTB", vec![]), ("IMU", vec![]), ("ACC", vec![]) ]);
    let mut quats = Vec::new();

    let mut first_quat_ts = None;

    for l in &log {
        if let Some(FieldType::u64(time)) = l.data.get("SampleUS").or_else(|| l.data.get("TimeUS")).map(|v| &v.value) {
            match l.name.as_ref() {
                "IMU" | "GYR" | "ACC" | "VSTB" => {
                    match (l.data.get("AccX").map(|v| &v.value), l.data.get("AccY").map(|v| &v.value), l.data.get("AccZ").map(|v| &v.value)) {
                        (Some(FieldType::f32(x)), Some(FieldType::f32(y)), Some(FieldType::f32(z))) => {
                            accl.get_mut(l.name.as_str()).unwrap().push(TimeVector3 { t: *time as f64 / 1000000.0,
                                x: *x as f64,
                                y: *y as f64,
                                z: *z as f64
                            });
                        },
                        _ => { }
                    }
                    match (l.data.get("GyrX").map(|v| &v.value), l.data.get("GyrY").map(|v| &v.value), l.data.get("GyrZ").map(|v| &v.value)) {
                        (Some(FieldType::f32(x)), Some(FieldType::f32(y)), Some(FieldType::f32(z))) => {
                            gyro.get_mut(l.name.as_str()).unwrap().push(TimeVector3 { t: *time as f64 / 1000000.0,
                                x: *x as f64,
                                y: *y as f64,
                                z: *z as f64
                            });
                        },
                        _ => { }
                    }
                    match (l.data.get("Q1").map(|v| &v.value), l.data.get("Q2").map(|v| &v.value), l.data.get("Q3").map(|v| &v.value), l.data.get("Q4").map(|v| &v.value)) {
                        (Some(FieldType::f32(w)), Some(FieldType::f32(x)), Some(FieldType::f32(y)), Some(FieldType::f32(z))) => {
                            if first_quat_ts.is_none() {
                                first_quat_ts = Some(*time as i64);
                            }
                            quats.push(TimeQuaternion {
                                t: (*time as i64 - first_quat_ts.unwrap()) as f64 / 1000.0,
                                v: util::multiply_quats(
                                    (*w as f64,
                                    *x as f64,
                                    *y as f64,
                                    *z as f64),
                                    (0.5, -0.5, -0.5, 0.5),
                                ),
                            });
                        },
                        _ => { }
                    }
                },
                _ => { }
            }
        }
    }

    // Prefer VSTB, then IMU, and then GYR/ACC. Don't add all of them because the data is duplicated then
    let gyro = [&gyro["VSTB"], &gyro["IMU"], &gyro["GYR"]].iter().find(|v| !v.is_empty()).map(|v| v.to_vec()).unwrap_or_default();
    let accl = [&accl["VSTB"], &accl["IMU"], &accl["ACC"]].iter().find(|v| !v.is_empty()).map(|v| v.to_vec()).unwrap_or_default();

    let mut map = GroupedTagMap::new();

    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Data, "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), accl, vec![]));
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Data, "Gyroscope data",     Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]));
    util::insert_tag(&mut map, tag!(parsed GroupId::Quaternion,    TagId::Data, "Quaternion data",    Vec_TimeQuaternion_f64, |v| format!("{:?}", v), quats, vec![]));

    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Unit, "Accelerometer unit", String, |v| v.to_string(), "m/s²".into(),  Vec::new()));
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Unit, "Gyroscope unit",     String, |v| v.to_string(), "rad/s".into(), Vec::new()));

    let imu_orientation = "zyx";
    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()));
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()));

    Ok(vec![
        SampleInfo { tag_map: Some(map), ..Default::default() }
    ])
}
