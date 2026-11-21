// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2021 Adrian <adrian.eddy at gmail>

use std::collections::BTreeMap;
use std::io::*;
use std::sync::{ Arc, atomic::AtomicBool };

use crate::tags_impl::*;
use crate::*;

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
    name: String,
    data: BTreeMap<String, Field>
}

pub fn parse_full<T: Read + Seek, F: Fn(f64)>(stream: &mut T, size: u64, progress_cb: F, cancel_flag: Arc<AtomicBool>) -> Result<Vec<LogItem>> {
    let mut bytes = Vec::with_capacity(size as usize);
    stream.read_to_end(&mut bytes)?;
    if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) { return Ok(Vec::new()); }

    let opts = flightlog_core::ReadOptions { keep_order: true, ..Default::default() };
    let parsed = flightlog_core::Log::read(&bytes, &opts).map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;
    if !parsed.integrity.is_clean() {
        log::warn!("DataFlash log damaged: {:?}", parsed.integrity);
    }

    let value = |data: &flightlog_core::ColumnData, row: usize| -> Option<FieldType> {
        use flightlog_core::ColumnData as C;
        Some(match data {
            C::Int8(v)    => FieldType::i8(v[row]),
            C::UInt8(v)   => FieldType::u8(v[row]),
            C::Int16(v)   => FieldType::i16(v[row]),
            C::UInt16(v)  => FieldType::u16(v[row]),
            C::Int32(v)   => FieldType::i32(v[row]),
            C::UInt32(v)  => FieldType::u32(v[row]),
            C::Int64(v)   => FieldType::i64(v[row]),
            C::UInt64(v)  => FieldType::u64(v[row]),
            C::Float32(v) => FieldType::f32(v[row]),
            C::Float64(v) => FieldType::f64(v[row]),
            C::Utf8(v) | C::Binary(v) => FieldType::String(String::from_utf8_lossy(v.get(row)).into_owned()),
            C::FixedList { size, values } => match values.as_ref() {
                C::Int16(v) => FieldType::Vec_i16(v[row * size..(row + 1) * size].to_vec()),
                _ => return None,
            },
        })
    };

    let tables = parsed.tables();
    let log = parsed.order.as_deref().unwrap_or_default().iter().map(|&(t, row)| {
        let table = &tables[t as usize];
        LogItem {
            name: table.name.clone(),
            data: table.columns.iter().filter_map(|c| Some((c.name.clone(), Field {
                value: value(&c.data, row as usize)?,
                unit: c.unit.clone(),
                multiplier: c.multiplier,
            }))).collect(),
        }
    }).collect();
    progress_cb(1.0);
    Ok(log)
}

pub fn parse<T: Read + Seek, F: Fn(f64)>(stream: &mut T, size: u64, progress_cb: F, cancel_flag: Arc<AtomicBool>, options: crate::InputOptions) -> Result<Vec<SampleInfo>> {
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

    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Data, "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), accl, vec![]), &options);
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Data, "Gyroscope data",     Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]), &options);
    util::insert_tag(&mut map, tag!(parsed GroupId::Quaternion,    TagId::Data, "Quaternion data",    Vec_TimeQuaternion_f64, |v| format!("{:?}", v), quats, vec![]), &options);

    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Unit, "Accelerometer unit", String, |v| v.to_string(), "m/s²".into(),  Vec::new()), &options);
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Unit, "Gyroscope unit",     String, |v| v.to_string(), "rad/s".into(), Vec::new()), &options);

    let imu_orientation = "zyx";
    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()), &options);
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()), &options);

    Ok(vec![
        SampleInfo { tag_map: Some(map), ..Default::default() }
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(typ: u8, len: u8, name: &str, format: &str, labels: &str) -> Vec<u8> {
        let mut m = vec![0xA3, 0x95, 0x80, typ, len];
        for (s, n) in [(name, 4), (format, 16), (labels, 64)] {
            let mut b = s.as_bytes().to_vec();
            b.resize(n, 0);
            m.extend(b);
        }
        m
    }

    fn tst(time: u64, alt: i16) -> Vec<u8> {
        let mut m = vec![0xA3, 0x95, 0x81];
        m.extend(time.to_le_bytes());
        m.extend(alt.to_le_bytes());
        m
    }

    #[test]
    fn scales_centi_fields_and_resyncs_after_odd_garbage() {
        let mut bytes = fmt(0x80, 89, "FMT", "BBnNZ", "Type,Length,Name,Format,Columns");
        bytes.extend(fmt(0x81, 13, "TST", "Qc", "TimeUS,Alt"));
        bytes.extend(tst(1, 1234));
        bytes.extend([0x00, 0xA3, 0x00]); // 3 corrupt bytes
        bytes.extend(tst(2, -500));

        let size = bytes.len() as u64;
        let log = parse_full(&mut Cursor::new(bytes), size, |_| {}, Arc::new(AtomicBool::new(false))).unwrap();
        let alts: Vec<f64> = log.iter().filter(|l| l.name == "TST").map(|l| match l.data["Alt"].value {
            FieldType::f64(v) => v,
            ref v => panic!("{v:?}"),
        }).collect();
        assert_eq!(alts, vec![12.34, -5.0]);
    }
}
