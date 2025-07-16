// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2024 Adrian <adrian.eddy at gmail>

use std::io::*;

use crate::tags_impl::*;
use crate::*;

pub fn parse<T: Read + Seek>(stream: &mut T, size: usize, options: crate::InputOptions) -> Result<Vec<SampleInfo>> {
    let mut headers: Option<Vec<String>> = None;

    let mut gyro = Vec::new();
    let mut accl = Vec::new();
    let mut angl = Vec::new();
    let mut magn = Vec::new();
    let mut quat = Vec::new();

    let mut last_timestamp = 0.0;
    let mut first_timestamp = 0.0;

    let mut buffer = String::with_capacity(size);
    stream.read_to_string(&mut buffer)?;

    for line in buffer.lines() {
        let row = line.split('\t');

        if let Some(ref h) = headers {
            let map: std::collections::BTreeMap<&str, &str> = h.iter().zip(row).map(|(a, b)| (&a[..], b.trim())).collect();

            if let Ok(ts) = chrono::NaiveDateTime::parse_from_str(map.get("time").unwrap_or(&""), "%Y-%m-%d %-H:%-M:%-S:%3f") {
                let ts = ts.and_utc().timestamp_millis() as f64 / 1000.0;
                if first_timestamp == 0.0 {
                    first_timestamp = ts;
                }
                last_timestamp = ts;

                crate::try_block!({
                    accl.push(TimeVector3 {
                        t: ts as f64,
                        x: map.get("AccX(g)")?.replace(',', ".").parse::<f64>().ok()?,
                        y: map.get("AccY(g)")?.replace(',', ".").parse::<f64>().ok()?,
                        z: map.get("AccZ(g)")?.replace(',', ".").parse::<f64>().ok()?
                    });
                });
                crate::try_block!({
                    gyro.push(TimeVector3 {
                        t: ts as f64,
                        x: map.get("AsX(°/s)")?.replace(',', ".").parse::<f64>().ok()?,
                        y: map.get("AsY(°/s)")?.replace(',', ".").parse::<f64>().ok()?,
                        z: map.get("AsZ(°/s)")?.replace(',', ".").parse::<f64>().ok()?
                    });
                });
                crate::try_block!({
                    angl.push(TimeVector3 {
                        t: ts as f64,
                        x: map.get("AngleX(°)")?.replace(',', ".").parse::<f64>().ok()?, // Roll
                        y: map.get("AngleY(°)")?.replace(',', ".").parse::<f64>().ok()?, // Pitch
                        z: map.get("AngleZ(°)")?.replace(',', ".").parse::<f64>().ok()?  // Yaw
                    });
                });
                crate::try_block!({
                    magn.push(TimeVector3 {
                        t: ts as f64,
                        x: map.get("HX(uT)")?.parse::<i64>().ok()?,
                        y: map.get("HY(uT)")?.parse::<i64>().ok()?,
                        z: map.get("HZ(uT)")?.parse::<i64>().ok()?
                    });
                });
                crate::try_block!({
                    quat.push(TimeArray4 {
                        t: ts as f64,
                        v: [
                            map.get("Q0()")?.replace(',', ".").parse::<f64>().ok()?,
                            map.get("Q1()")?.replace(',', ".").parse::<f64>().ok()?,
                            map.get("Q2()")?.replace(',', ".").parse::<f64>().ok()?,
                            map.get("Q3()")?.replace(',', ".").parse::<f64>().ok()?
                        ]
                    });
                });
            }
        } else if line.len() > 40 {
            if line.starts_with("Start time") { continue; }
            headers = Some(row.map(|x| x.trim().into()).collect());
        }
    }

    let mut map = GroupedTagMap::new();

    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Data, "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), accl, vec![]), &options);
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Data, "Gyroscope data",     Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]), &options);

    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Unit, "Accelerometer unit", String, |v| v.to_string(), "g".into(), Vec::new()), &options);
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Unit, "Gyroscope unit",     String, |v| v.to_string(), "deg/s".into(), Vec::new()), &options);

    let imu_orientation = "ZYx";
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()), &options);
    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()), &options);

    util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer,  TagId::Data, "Magnetometer data", Vec_TimeVector3_i64f64, |v| format!("{:?}", v), magn, vec![]), &options);
    util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer,  TagId::Unit, "Magnetometer unit", String, |v| v.to_string(), "μT".into(), Vec::new()), &options);

    util::insert_tag(&mut map, tag!(parsed GroupId::Custom("Angle".into()),        TagId::Data, "Angle data", Vec_TimeVector3_f64, |v| format!("{:?}", v), angl, vec![]), &options);
    util::insert_tag(&mut map, tag!(parsed GroupId::Custom("Angle".into()),        TagId::Unit, "Angle unit", String, |v| v.to_string(), "deg".into(),  Vec::new()), &options);

    util::insert_tag(&mut map, tag!(parsed GroupId::Quaternion,                    TagId::Data, "Quaternion data",   Vec_TimeArray4_f64,  |v| format!("{:?}", v), quat, vec![]), &options);

    Ok(vec![
        SampleInfo { timestamp_ms: first_timestamp as f64, duration_ms: (last_timestamp - first_timestamp) as f64, tag_map: Some(map), ..Default::default() }
    ])
}
