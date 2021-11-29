use std::io::*;

use crate::tags_impl::*;
use crate::*;

#[derive(Default)]
pub struct Runcam {
    pub model: Option<String>
}

impl Runcam {
    pub fn detect(buffer: &[u8], filename: &str) -> Option<Self> {
        let match_hdr = |line: &[u8]| -> bool {
            &buffer[0..line.len().min(buffer.len())] == line
        };
        if match_hdr(b"time,x,y,z,ax,ay,az") || match_hdr(b"time,rx,ry,rz,ax,ay,az") || match_hdr(b"time,x,y,z") {
            let model = if filename.starts_with("RC_") {
                Some("Runcam 5 Orange".to_owned())
            } else if filename.starts_with("gyroDat") {
                Some("iFlight GOCam GR".to_owned())
            } else {
                None
            };

            return Some(Self { model });
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
            .from_reader(stream);
        for row in csv.records() {
            let row = row?;
            if &row[0] == "time" { continue; }

            let time = row[0].parse::<f64>().map_err(e)? / 1_000.0;
            if row.len() >= 4 {
                gyro.push(TimeVector3 {
                    t: time,
                    x: row[1].parse::<f64>().map_err(e)?,
                    y: row[2].parse::<f64>().map_err(e)?,
                    z: row[3].parse::<f64>().map_err(e)?
                });
            }
            if row.len() >= 7 {
                accl.push(TimeVector3 {
                    t: time,
                    x: row[4].parse::<f64>().map_err(e)?,
                    y: row[5].parse::<f64>().map_err(e)?,
                    z: row[6].parse::<f64>().map_err(e)?
                });
            }
        }

        let mut map = GroupedTagMap::new();

        let accl_scale = 32768.0 / 2.0; // ± 2g
        let gyro_scale = 32768.0 / 500.0; // 500 dps
        
        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Data, "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), accl, vec![]));
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Data, "Gyroscope data",     Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]));

        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Unit, "Accelerometer unit", String, |v| v.to_string(), "m/s²".into(),  Vec::new()));
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Unit, "Gyroscope unit",     String, |v| v.to_string(), "deg/s".into(), Vec::new()));

        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Scale, "Gyroscope scale",     f64, |v| format!("{:?}", v), gyro_scale, vec![]));
        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Scale, "Accelerometer scale", f64, |v| format!("{:?}", v), accl_scale, vec![]));
        
        let imu_orientation = match self.model.as_deref() {
            Some("Runcam 5 Orange") => "xZy",
            Some("iFlight GOCam GR") => "xzY",
            _ => "xZy"
        };
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.to_string(), Vec::new()));
        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.to_string(), Vec::new()));

        Ok(vec![
            SampleInfo { index: 0, timestamp_ms: 0.0, duration_ms: 0.0, tag_map: Some(map) }
        ])
    }

    pub fn normalize_imu_orientation(v: String) -> String {
        v
    }
    
    pub fn camera_type(&self) -> String {
        match self.model.as_deref() {
            Some("iFlight GOCam GR") => "iFlight",
            _ => "Runcam"
        }.to_owned()
    }
}
