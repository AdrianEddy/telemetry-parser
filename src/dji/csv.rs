// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2022 Adrian <adrian.eddy at gmail>

use std::io::*;
use crate::tags_impl::*;
use crate::*;

// WARNING: Flight logs from DJI FPV are not usable in Gyroflow due to low sampling rate and lack of camera angle information.

pub fn parse<T: Read + Seek>(stream: &mut T, _size: usize, options: crate::InputOptions) -> Result<Vec<SampleInfo>> {
    let mut headers = None;

    let mut gyro = Vec::new();
    let mut accl = Vec::new();
    let mut magn = Vec::new();
    let mut quat = Vec::new();

    let mut last_timestamp = 0.0;
    let mut first_timestamp = 0.0;

    let mut csv = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .trim(csv::Trim::All)
        .delimiter(b',')
        .from_reader(stream);

    for row in csv.records() {
        let row = row?;
        if let Some(ref h) = headers {
            let map = util::create_csv_map(&row, &h);

            if !map.contains_key("Clock:offsetTime") {
                continue;
            }

            let ts = map.get("Clock:offsetTime").and_then(|x| x.parse::<f64>().ok()).unwrap_or_default();
            if first_timestamp == 0.0 {
                first_timestamp = ts;
            }
            last_timestamp = ts;
            // if ts > 20.0 {
            //     dbg!(&map);
            // }

            crate::try_block!({
                accl.push(TimeVector3 {
                    t: ts as f64,
                    x: map.get("IMU_ATTI(0):accelX")?.parse::<f64>().ok()?,
                    y: map.get("IMU_ATTI(0):accelY")?.parse::<f64>().ok()?,
                    z: map.get("IMU_ATTI(0):accelZ")?.parse::<f64>().ok()?
                });
            });
            crate::try_block!({
                gyro.push(TimeVector3 {
                    t: ts as f64,
                    x: map.get("IMU_ATTI(0):gyroX")?.parse::<f64>().ok()?,
                    y: map.get("IMU_ATTI(0):gyroY")?.parse::<f64>().ok()?,
                    z: map.get("IMU_ATTI(0):gyroZ")?.parse::<f64>().ok()?
                });
            });
            crate::try_block!({
                magn.push(TimeVector3 {
                    t: ts as f64,
                    x: map.get("IMU_ATTI(0):magX")?.parse::<f64>().ok()?,
                    y: map.get("IMU_ATTI(0):magY")?.parse::<f64>().ok()?,
                    z: map.get("IMU_ATTI(0):magZ")?.parse::<f64>().ok()?
                });
            });
            crate::try_block!({
                quat.push(TimeQuaternion {
                    t: ts as f64 * 1000.0,
                    v: util::multiply_quats(
                        (map.get("IMU_ATTI(0):quatW:D")?.parse::<f64>().ok()?,
                        map.get("IMU_ATTI(0):quatX:D")?.parse::<f64>().ok()?,
                        map.get("IMU_ATTI(0):quatY:D")?.parse::<f64>().ok()?,
                        map.get("IMU_ATTI(0):quatZ:D")?.parse::<f64>().ok()?),
                        (0.5, -0.5, -0.5, 0.5),
                    ),
                });
            });
        } else if row.len() > 3 {
            headers = Some(row.iter().map(|x| x.trim().into()).collect::<Vec<String>>());
        }
    }

    let mut map = GroupedTagMap::new();

    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Data, "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), accl, vec![]), &options);
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Data, "Gyroscope data",     Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]), &options);

    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Unit, "Accelerometer unit", String, |v| v.to_string(), "g".into(), Vec::new()), &options);
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Unit, "Gyroscope unit",     String, |v| v.to_string(), "deg/s".into(), Vec::new()), &options);

    let imu_orientation = "zyx";
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()), &options);
    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()), &options);

    util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer,  TagId::Data, "Magnetometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), magn, vec![]), &options);
    util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer,  TagId::Unit, "Magnetometer unit", String, |v| v.to_string(), "μT".into(), Vec::new()), &options);

    util::insert_tag(&mut map, tag!(parsed GroupId::Quaternion,    TagId::Data, "Quaternion data",   Vec_TimeQuaternion_f64,  |v| format!("{:?}", v), quat, vec![]), &options);

    Ok(vec![
        SampleInfo { timestamp_ms: first_timestamp as f64, duration_ms: (last_timestamp - first_timestamp) as f64, tag_map: Some(map), ..Default::default() }
    ])
}
