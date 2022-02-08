
use std::io::*;

use crate::tags_impl::*;
use crate::*;

pub fn parse<T: Read + Seek>(stream: &mut T, _size: usize) -> Result<Vec<SampleInfo>> {
    let mut headers = None;

    let mut gyro = Vec::new();
    let mut accl = Vec::new();
    let mut angl = Vec::new();
    let mut magn = Vec::new();
    let mut quat = Vec::new();
    
    let mut last_timestamp = 0.0;
    let mut first_timestamp = 0.0;

    let mut csv = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .trim(csv::Trim::All)
        .delimiter(b'\t')
        .from_reader(stream);
    
    for row in csv.records() {
        let row = row?;
        if let Some(ref h) = headers {
            let map = util::create_csv_map(&row, &h);

            let ts = chrono::NaiveDateTime::parse_from_str(*map.get("ChipTime").unwrap(), "%Y-%m-%d %H:%M:%S%.3f").unwrap().timestamp_millis() as f64 / 1000.0;
            if first_timestamp == 0.0 {
                first_timestamp = ts;
            }
            last_timestamp = ts;

            crate::try_block!({
                accl.push(TimeVector3 {
                    t: ts as f64,
                    x: map.get("ax(g)")?.replace(',', ".").parse::<f64>().ok()?,
                    y: map.get("ay(g)")?.replace(',', ".").parse::<f64>().ok()?,
                    z: map.get("az(g)")?.replace(',', ".").parse::<f64>().ok()?
                });
            });
            crate::try_block!({
                gyro.push(TimeVector3 {
                    t: ts as f64,
                    x: map.get("wx(deg/s)")?.replace(',', ".").parse::<f64>().ok()?,
                    y: map.get("wy(deg/s)")?.replace(',', ".").parse::<f64>().ok()?,
                    z: map.get("wz(deg/s)")?.replace(',', ".").parse::<f64>().ok()?
                });
            });
            crate::try_block!({
                angl.push(TimeVector3 {
                    t: ts as f64,
                    x: map.get("AngleX(deg)")?.replace(',', ".").parse::<f64>().ok()?, // Roll
                    y: map.get("AngleY(deg)")?.replace(',', ".").parse::<f64>().ok()?, // Pitch
                    z: map.get("AngleZ(deg)")?.replace(',', ".").parse::<f64>().ok()?  // Yaw
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
        } else if row.len() > 3 {
            headers = Some(row.iter().map(|x| x.trim().into()).collect::<Vec<String>>());
        }
    }

    let mut map = GroupedTagMap::new();

    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Data, "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), accl, vec![]));
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Data, "Gyroscope data",     Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]));

    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Unit, "Accelerometer unit", String, |v| v.to_string(), "m/s²".into(),  Vec::new()));
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
        SampleInfo { index: 0, timestamp_ms: first_timestamp as f64, duration_ms: (last_timestamp - first_timestamp) as f64, tag_map: Some(map) }
    ])
}
