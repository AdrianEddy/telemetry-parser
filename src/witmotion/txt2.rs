// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2022 Adrian <adrian.eddy at gmail>

use std::io::*;

use crate::tags_impl::*;
use crate::*;

pub fn parse<T: Read + Seek>(stream: &mut T, size: usize) -> Result<Vec<SampleInfo>> {
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
        let row = line.split_ascii_whitespace();

        if let Some(ref h) = headers {
            let map: std::collections::BTreeMap<&str, &str> = h.iter().zip(row).map(|(a, b)| (&a[..], b.trim())).collect();

            if let Ok(ts) = chrono::NaiveDateTime::parse_from_str(&format!("{} {}", map.get("Date").unwrap_or(&""), map.get("Time").unwrap_or(&"")), "%Y-%m-%d %H:%M:%S%.3f") {
                let ts = ts.timestamp_millis() as f64 / 1000.0;
                if first_timestamp == 0.0 {
                    first_timestamp = ts;
                }
                last_timestamp = ts;

                dbg!(&ts);
                dbg!(&map);
                crate::try_block!({
                    accl.push(TimeVector3 {
                        t: ts as f64,
                        x: map.get("ax")?.replace(',', ".").parse::<f64>().ok()?,
                        y: map.get("ay")?.replace(',', ".").parse::<f64>().ok()?,
                        z: map.get("az")?.replace(',', ".").parse::<f64>().ok()?
                    });
                });
                crate::try_block!({
                    gyro.push(TimeVector3 {
                        t: ts as f64,
                        x: map.get("wx")?.replace(',', ".").parse::<f64>().ok()?,
                        y: map.get("wy")?.replace(',', ".").parse::<f64>().ok()?,
                        z: map.get("wz")?.replace(',', ".").parse::<f64>().ok()?
                    });
                });
                crate::try_block!({
                    angl.push(TimeVector3 {
                        t: ts as f64,
                        x: map.get("AngleX")?.replace(',', ".").parse::<f64>().ok()?, // Roll
                        y: map.get("AngleY")?.replace(',', ".").parse::<f64>().ok()?, // Pitch
                        z: map.get("AngleZ")?.replace(',', ".").parse::<f64>().ok()?  // Yaw
                    });
                });
                crate::try_block!({
                    magn.push(TimeVector3 {
                        t: ts as f64,
                        x: map.get("hx")?.parse::<i64>().ok()?,
                        y: map.get("hy")?.parse::<i64>().ok()?,
                        z: map.get("hz")?.parse::<i64>().ok()?
                    });
                });
                crate::try_block!({
                    quat.push(TimeArray4 {
                        t: ts as f64,
                        v: [
                            map.get("q0")?.replace(',', ".").parse::<f64>().ok()?,
                            map.get("q1")?.replace(',', ".").parse::<f64>().ok()?,
                            map.get("q2")?.replace(',', ".").parse::<f64>().ok()?,
                            map.get("q3")?.replace(',', ".").parse::<f64>().ok()?
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

    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Data, "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), accl, vec![]));
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Data, "Gyroscope data",     Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]));

    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Unit, "Accelerometer unit", String, |v| v.to_string(), "g".into(), Vec::new()));
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Unit, "Gyroscope unit",     String, |v| v.to_string(), "deg/s".into(), Vec::new()));

    let imu_orientation = "ZYx";
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()));
    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()));

    util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer,  TagId::Data, "Magnetometer data", Vec_TimeVector3_i64f64, |v| format!("{:?}", v), magn, vec![]));
    util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer,  TagId::Unit, "Magnetometer unit", String, |v| v.to_string(), "μT".into(), Vec::new()));

    util::insert_tag(&mut map, tag!(parsed GroupId::Custom("Angle".into()),        TagId::Data, "Angle data", Vec_TimeVector3_f64, |v| format!("{:?}", v), angl, vec![]));
    util::insert_tag(&mut map, tag!(parsed GroupId::Custom("Angle".into()),        TagId::Unit, "Angle unit", String, |v| v.to_string(), "deg".into(),  Vec::new()));

    util::insert_tag(&mut map, tag!(parsed GroupId::Quaternion,                    TagId::Data, "Quaternion data",   Vec_TimeArray4_f64,  |v| format!("{:?}", v), quat, vec![]));

    Ok(vec![
        SampleInfo { timestamp_ms: first_timestamp as f64, duration_ms: (last_timestamp - first_timestamp) as f64, tag_map: Some(map), ..Default::default() }
    ])
}
