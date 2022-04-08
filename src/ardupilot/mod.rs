use std::io::*;

use crate::tags_impl::*;
use crate::*;
use memchr::memmem;

#[derive(Default)]
pub struct ArduPilot {
    pub model: Option<String>
}

// ArduPilot .log format, not native .bin yet, .bin can be converted to .log using mission planner or https://github.com/ArduPilot/pymavlink/blob/master/tools/mavlogdump.py

impl ArduPilot {
    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], filepath: P) -> Option<Self> {
        if !filepath.as_ref().to_str().unwrap_or_default().ends_with(".log") { return None }

        if memmem::find(buffer, b"FMT,").is_some() &&
           memmem::find(buffer, b"PARM,").is_some() &&
           memmem::find(buffer, b"VSTB,").is_some() {
            return Some(Self { model: Some(".log".to_owned()) });
        }
        None
    }

    pub fn parse<T: Read + Seek>(&mut self, stream: &mut T, _size: usize) -> Result<Vec<SampleInfo>> {
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

        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Scale, "Gyroscope scale",     f64, |v| format!("{:?}", v), 1.0, vec![]));
        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Scale, "Accelerometer scale", f64, |v| format!("{:?}", v), 1.0/9.81, vec![]));

        let imu_orientation = "zyx";
        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()));
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()));
    
        Ok(vec![
            SampleInfo { index: 0, timestamp_ms: 0.0, duration_ms: 0.0, tag_map: Some(map) }
        ])
    }

    pub fn normalize_imu_orientation(v: String) -> String {
        v
    }
    
    pub fn camera_type(&self) -> String {
        "ArduPilot".to_owned()
    }
}
