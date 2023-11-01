// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2022 Adrian <adrian.eddy at gmail>

use std::io::*;
use crate::tags_impl::*;
use crate::*;

fn is_numbers_only(buf: &[u8]) -> bool {
    if buf.len() < 10 { return false; }

    for c in buf {
        // OpenCamera Sensors produces csv files without any letters/headers, so numbers and comma are the only valid contents
        if !(c.is_ascii_digit() || *c == b'.' || *c == b'e' || *c == b'E' || *c == b',' || *c == b'-' || c.is_ascii_whitespace()) {
            return false;
        }
    }
    return true;
}

pub fn get_possible_paths<P: AsRef<std::path::Path>>(filepath: P) -> Vec<std::path::PathBuf> {
    let filename = filepath.as_ref().file_name().map(|x| x.to_string_lossy()).unwrap_or_default();

    let mut ret = Vec::new();
    let mut buf = vec![0u8; 200];

    if let Ok(mut f) = std::fs::File::open(&filepath) {
        if let Ok(_) = f.read_exact(&mut buf) {
            if filename.ends_with("gyro.csv") || filename.ends_with("accel.csv") || filename.ends_with("_imu_timestamps.csv") || filename.ends_with("magnetic.csv") {
                if is_numbers_only(&buf) {
                    let name_start = filename.replace("gyro.csv", "")
                                                   .replace("accel.csv", "")
                                                   .replace("_imu_timestamps.csv", "")
                                                   .replace("magnetic.csv", "");

                    { let p = filepath.as_ref().with_file_name(format!("{}gyro.csv",     name_start)); if p.exists() { ret.push(p); } }
                    { let p = filepath.as_ref().with_file_name(format!("{}accel.csv",    name_start)); if p.exists() { ret.push(p); } }
                    { let p = filepath.as_ref().with_file_name(format!("{}magnetic.csv", name_start)); if p.exists() { ret.push(p); } }
                }
            }
            if filename.ends_with(".mp4") {
                let part = filename.replace("VID_", "").replace(".mp4", "");
                let mut path2 = filepath.as_ref().with_file_name(part);
                path2.push(format!("{}gyro.csv", filename.replace(".mp4", "")));
                if path2.exists() {
                    return get_possible_paths(&path2);
                }

                let path3 = filepath.as_ref().with_file_name(format!("{}gyro.csv", filename.replace(".mp4", "")));
                if path3.exists() {
                    return get_possible_paths(&path3);
                }
            }
        }
    }

    ret
}

pub fn detect<P: AsRef<std::path::Path>>(_buffer: &[u8], filepath: P) -> bool {
    !get_possible_paths(filepath).is_empty()
}

pub fn parse<T: Read + Seek, P: AsRef<std::path::Path>>(_stream: &mut T, _size: usize, filepath: P) -> Result<Vec<SampleInfo>> {
    let paths = get_possible_paths(filepath);

    let mut gyro = Vec::new();
    let mut accl = Vec::new();
    let mut magn = Vec::new();

    let mut last_timestamp = 0.0;
    let mut first_timestamp = 0.0;

    for path in paths {
        let filename = path.file_name().map(|x| x.to_string_lossy()).unwrap_or_default();

        let mut csv = csv::ReaderBuilder::new()
            .has_headers(false)
            .trim(csv::Trim::All)
            .delimiter(b',')
            .from_path(&path)?;

        for row in csv.records() {
            let row = row?;

            if row.len() != 4 {
                continue;
            }
            let ts = row.get(3).unwrap().parse::<f64>().unwrap_or_default() / 1_000_000_000.0;

            if first_timestamp == 0.0 {
                first_timestamp = ts;
            }
            let ts = ts - first_timestamp;
            last_timestamp = ts;

            if filename.ends_with("gyro.csv") {
                crate::try_block!({
                    gyro.push(TimeVector3 {
                        t: ts as f64,
                        x: row.get(0)?.parse::<f64>().ok()?,
                        y: row.get(1)?.parse::<f64>().ok()?,
                        z: row.get(2)?.parse::<f64>().ok()?
                    });
                });
            } else if filename.ends_with("accel.csv") {
                crate::try_block!({
                    accl.push(TimeVector3 {
                        t: ts as f64,
                        x: row.get(0)?.parse::<f64>().ok()?,
                        y: row.get(1)?.parse::<f64>().ok()?,
                        z: row.get(2)?.parse::<f64>().ok()?
                    });
                });
            } else if filename.ends_with("magnetic.csv") {
                crate::try_block!({
                    magn.push(TimeVector3 {
                        t: ts as f64,
                        x: row.get(0)?.parse::<f64>().ok()?,
                        y: row.get(1)?.parse::<f64>().ok()?,
                        z: row.get(2)?.parse::<f64>().ok()?
                    });
                });
            }
        }
    }

    let mut map = GroupedTagMap::new();

    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Data, "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), accl, vec![]));
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Data, "Gyroscope data",     Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]));
    util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer,  TagId::Data, "Magnetometer data",  Vec_TimeVector3_f64, |v| format!("{:?}", v), magn, vec![]));

    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Unit, "Accelerometer unit", String, |v| v.to_string(), "m/s²" .into(), Vec::new()));
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Unit, "Gyroscope unit",     String, |v| v.to_string(), "rad/s".into(), Vec::new()));
    util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer,  TagId::Unit, "Magnetometer unit",  String, |v| v.to_string(), "μT"   .into(), Vec::new()));

    let imu_orientation = "XYZ";
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()));
    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()));
    util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer,  TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()));

    Ok(vec![
        SampleInfo { timestamp_ms: first_timestamp as f64, duration_ms: (last_timestamp - first_timestamp) as f64, tag_map: Some(map), ..Default::default() }
    ])
}
