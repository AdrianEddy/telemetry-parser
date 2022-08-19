use std::io::*;
use std::sync::{ Arc, atomic::AtomicBool };

use crate::tags_impl::*;
use crate::*;

pub fn parse<T: Read + Seek, F: Fn(f64)>(stream: &mut T, _size: usize, _progress_cb: F, _cancel_flag: Arc<AtomicBool>) -> Result<Vec<SampleInfo>> {
    let e = |_| -> Error { ErrorKind::InvalidData.into() };

    let mut gyro = Vec::new();
    let mut accl = Vec::new();

    let mut csv = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(stream);

    let time_scale = 1.0e-6;
    for row in csv.records() {
        let row = row?;
        if &row[0] != "VSTB" || row.len() < 8 {
            continue;
        }
        let time = row[1].parse::<f64>().map_err(e)? * time_scale;
        gyro.push(TimeVector3 {
            t: time,
            x: row[2].parse::<f64>().map_err(e)?,
            y: row[3].parse::<f64>().map_err(e)?,
            z: row[4].parse::<f64>().map_err(e)?
        });
        accl.push(TimeVector3 {
            t: time,
            x: row[5].parse::<f64>().map_err(e)?,
            y: row[6].parse::<f64>().map_err(e)?,
            z: row[7].parse::<f64>().map_err(e)?
        });
    }

    let mut map = GroupedTagMap::new();

    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Data, "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), accl, vec![]));
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Data, "Gyroscope data",     Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]));

    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Unit, "Accelerometer unit", String, |v| v.to_string(), "m/s²".into(),  Vec::new()));
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Unit, "Gyroscope unit",     String, |v| v.to_string(), "rad/s".into(), Vec::new()));

    let imu_orientation = "zyx";
    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()));
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()));

    Ok(vec![
        SampleInfo { index: 0, timestamp_ms: 0.0, duration_ms: 0.0, tag_map: Some(map) }
    ])
}
