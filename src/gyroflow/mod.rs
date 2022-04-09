use std::collections::BTreeMap;
use std::io::*;
use std::path::{ Path, PathBuf };

use crate::tags_impl::*;
use crate::*;

#[derive(Default)]
pub struct Gyroflow {
    pub model: Option<String>,
    pub gyro_path: Option<PathBuf>
}

// .gcsv format as described here: https://docs.gyroflow.xyz/logging/gcsv/

impl Gyroflow {
    pub fn detect<P: AsRef<Path>>(buffer: &[u8], filepath: P) -> Option<Self> {
        let filename = filepath.as_ref().file_name().map(|x| x.to_string_lossy()).unwrap_or_default();

        let mut gyro_path = None;
        if !filename.to_ascii_lowercase().ends_with(".gcsv") && filepath.as_ref().with_extension("gcsv").exists() {
            gyro_path = Some(filepath.as_ref().with_extension("gcsv").into());
        }
        let gyro_buf = if let Some(gyro) = &gyro_path {
            std::fs::read(gyro).ok()?
        } else {
            buffer.to_vec()
        };

        let match_hdr = |line: &[u8]| -> bool {
            &gyro_buf[0..line.len().min(gyro_buf.len())] == line
        };
        if match_hdr(b"GYROFLOW IMU LOG") || match_hdr(b"CAMERA IMU LOG"){
            let mut header = BTreeMap::new();

            // get header block
            let header_block = &gyro_buf[0..gyro_buf.len().min(500)];

            let mut csv = csv::ReaderBuilder::new()
                .has_headers(false)
                .flexible(true)
                .trim(csv::Trim::All)
                .from_reader(Cursor::new(header_block));
                
            for row in csv.records() {
                let row = row.ok()?;
                if row.len() == 2 {
                    header.insert(row[0].to_owned(), row[1].to_owned());
                    continue;
                }
                if &row[0] == "t" { break; }
            }

            let version = header.remove("version").unwrap_or("1.0".to_owned());
            let id = header.remove("id").unwrap_or("NoID".to_owned()).replace("_", " ");

            let model = Some(format!("{} (gcsv v{})", id, version));
            return Some(Self { model, gyro_path });
        }
        None
    }

    pub fn parse<T: Read + Seek>(&mut self, stream: &mut T, _size: usize) -> Result<Vec<SampleInfo>> {
        let e = |_| -> Error { ErrorKind::InvalidData.into() };

        let gyro_buf = if let Some(gyro) = &self.gyro_path {
            std::fs::read(gyro)?
        } else {
            let mut vec = Vec::new();
            stream.read_to_end(&mut vec)?;
            vec
        };

        let mut header = BTreeMap::new();

        let mut gyro = Vec::new();
        let mut accl = Vec::new();
        let mut magn = Vec::new();

        let mut csv = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .trim(csv::Trim::All)
            .from_reader(Cursor::new(gyro_buf));

        let mut passed_header = false;

        let mut time_scale = 0.001; // default to millisecond

        for row in csv.records() {
            let row = row?;

            if row.len() == 1 {
                continue; // first line
            } else if row.len() == 2 && !passed_header {
                header.insert(row[0].to_owned(), row[1].to_owned());
                continue;
            } else if &row[0] == "t" {
                passed_header = true;
                time_scale =  header.remove("tscale").unwrap_or("0.001".to_owned()).parse::<f64>().unwrap();
                continue;
            }

            let time = row[0].parse::<f64>().map_err(e)? * time_scale;
            if row.len() >= 4 {
                gyro.push(TimeVector3 {
                    t: time,
                    x: row[1].parse::<f64>().unwrap_or_default(),
                    y: row[2].parse::<f64>().unwrap_or_default(),
                    z: row[3].parse::<f64>().unwrap_or_default(),
                });
            }
            if row.len() >= 7 {
                accl.push(TimeVector3 {
                    t: time,
                    x: row[4].parse::<f64>().unwrap_or_default(),
                    y: row[5].parse::<f64>().unwrap_or_default(),
                    z: row[6].parse::<f64>().unwrap_or_default()
                });
            }
            if row.len() >= 10 {
                magn.push(TimeVector3 {
                    t: time,
                    x: row[7].parse::<f64>().unwrap_or_default(),
                    y: row[8].parse::<f64>().unwrap_or_default(),
                    z: row[9].parse::<f64>().unwrap_or_default()
                });
            }
        }
        let accl_scale = 1.0 / header.remove("ascale").unwrap_or("1.0".to_owned()).parse::<f64>().unwrap();
        let gyro_scale = 1.0 / header.remove("gscale").unwrap_or("1.0".to_owned()).parse::<f64>().unwrap() * std::f64::consts::PI / 180.0;
        let mag_scale = 100.0 / header.remove("mscale").unwrap_or("1.0".to_owned()).parse::<f64>().unwrap(); // Gauss to microtesla
        let imu_orientation = header.remove("orientation").unwrap_or("xzY".to_owned()); // default

        let mut map = GroupedTagMap::new();

        util::insert_tag(&mut map, 
            tag!(parsed GroupId::Default, TagId::Metadata, "Extra metadata", Json, |v| format!("{:?}", v), serde_json::to_value(header).map_err(|_| Error::new(ErrorKind::Other, "Serialize error"))?, vec![])
        );

        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Data, "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), accl, vec![]));
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Data, "Gyroscope data",     Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]));
        util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer,  TagId::Data, "Magnetometer data",  Vec_TimeVector3_f64, |v| format!("{:?}", v), magn, vec![]));

        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Unit, "Accelerometer unit", String, |v| v.to_string(), "m/s²".into(),  Vec::new()));
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Unit, "Gyroscope unit",     String, |v| v.to_string(), "deg/s".into(), Vec::new()));
        util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer,  TagId::Unit, "Magnetometer unit",  String, |v| v.to_string(), "μT".into(), Vec::new()));

        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Scale, "Gyroscope scale",     f64, |v| format!("{:?}", v), gyro_scale, vec![]));
        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Scale, "Accelerometer scale", f64, |v| format!("{:?}", v), accl_scale, vec![]));
        util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer,  TagId::Scale, "Magnetometer scale",  f64, |v| format!("{:?}", v), mag_scale, vec![]));

        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.to_string(), Vec::new()));
        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.to_string(), Vec::new()));
        util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer,  TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.to_string(), Vec::new()));

        Ok(vec![
            SampleInfo { index: 0, timestamp_ms: 0.0, duration_ms: 0.0, tag_map: Some(map) }
        ])
    }

    pub fn normalize_imu_orientation(v: String) -> String {
        v
    }
    
    pub fn camera_type(&self) -> String {
        "gcsv".to_owned()
    }
}
