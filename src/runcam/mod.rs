use std::io::*;
use std::path::{ Path, PathBuf };

use crate::tags_impl::*;
use crate::*;

#[derive(Default)]
pub struct Runcam {
    pub model: Option<String>,
    pub gyro_path: Option<PathBuf>
}

impl Runcam {
    pub fn detect<P: AsRef<Path>>(buffer: &[u8], filepath: P) -> Option<Self> {
        let filename = filepath.as_ref().file_name().map(|x| x.to_string_lossy()).unwrap_or_default();
        
        let mut gyro_path = None;
        if filename.to_ascii_lowercase().ends_with(".mp4") {
            gyro_path = Self::detect_gyro_path(filepath.as_ref(), &filename);
        }
        let gyro_buf = if let Some(gyro) = &gyro_path {
            std::fs::read(gyro).ok()?
        } else {
            buffer.to_vec()
        };

        let match_hdr = |line: &[u8]| -> bool {
            &gyro_buf[0..line.len().min(gyro_buf.len())] == line
        };
        if match_hdr(b"time,x,y,z,ax,ay,az") || match_hdr(b"time,rx,ry,rz,ax,ay,az") || match_hdr(b"time,x,y,z") {
            let model = if match_hdr(b"time,rx,ry,rz,ax,ay,az,temp") {
                // Mobius uses same log format as RunCam with an added temp field
                Some("Mobius Maxi 4K".to_owned())
            } else if filename.starts_with("RC_") {
                Some("Runcam 5 Orange".to_owned())
            } else if filename.starts_with("gyroDat") || filename.starts_with("IF-RC") {
                Some("iFlight GOCam GR".to_owned())
            } else if filename.starts_with("Thumb") {
                Some("Thumb".to_owned())
            } else {
                None
            };

            return Some(Self { model, gyro_path });
        }
        None
    }

    fn detect_gyro_path(path: &Path, filename: &str) -> Option<PathBuf> {
        if filename.starts_with("RC_") {
            let num = filename.split("_").collect::<Vec<&str>>().get(1).cloned().unwrap_or(&"");
            let gyropath = path.with_file_name(format!("RC_GyroData{}.csv", num));
            if gyropath.exists() {
                return Some(gyropath.into());
            }
        }
        if filename.starts_with("IF-RC") {
            let num = filename.split("_").collect::<Vec<&str>>().get(1).cloned().unwrap_or(&"");
            let num = num.to_ascii_lowercase().replace(".mp4", "");
            let gyropath = path.with_file_name(format!("gyroDate{}.csv", num));
            if gyropath.exists() {
                return Some(gyropath.into());
            }
            let gyropath = path.with_file_name(format!("gyroData{}.csv", num));
            if gyropath.exists() {
                return Some(gyropath.into());
            }
        }
        if filename.starts_with("Thumb") {
            let gyropath = path.with_extension("csv");
            if gyropath.exists() {
                return Some(gyropath.into());
            }
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

        let mut gyro = Vec::new();
        let mut accl = Vec::new();

        let mut csv = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .trim(csv::Trim::All)
            .from_reader(Cursor::new(gyro_buf));
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
                // Fix RC5 accelerometer orientation
                accl.push(if self.model.as_deref() == Some("Runcam 5 Orange") {
                    TimeVector3 {
                        t: time,
                        x: row[5].parse::<f64>().map_err(e)?,
                        y: row[6].parse::<f64>().map_err(e)?,
                        z: -row[4].parse::<f64>().map_err(e)?
                    }
                } else {
                    TimeVector3 {
                        t: time,
                        x: row[4].parse::<f64>().map_err(e)?,
                        y: row[5].parse::<f64>().map_err(e)?,
                        z: row[6].parse::<f64>().map_err(e)?
                    }
                });
            }
        }

        let mut map = GroupedTagMap::new();

        let accl_scale = 32768.0 / 2.0; // ± 2g
        let gyro_scale = 32768.0 / match self.model.as_deref() {
            Some("Thumb") => 1000.0, // 1000 dps
            _ => 500.0 // 500 dps default
        };
        
        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Data, "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), accl, vec![]));
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Data, "Gyroscope data",     Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]));

        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Unit, "Accelerometer unit", String, |v| v.to_string(), "m/s²".into(),  Vec::new()));
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Unit, "Gyroscope unit",     String, |v| v.to_string(), "deg/s".into(), Vec::new()));

        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Scale, "Gyroscope scale",     f64, |v| format!("{:?}", v), gyro_scale, vec![]));
        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Scale, "Accelerometer scale", f64, |v| format!("{:?}", v), accl_scale, vec![]));
        
        let imu_orientation = match self.model.as_deref() {
            Some("Runcam 5 Orange")  => "xzY",
            Some("iFlight GOCam GR") => "xZy",
            Some("Thumb")            => "Yxz",
            Some("Mobius Maxi 4K")   => "yxz",
            _ => "xzY"
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
            Some("Mobius Maxi 4K") => "Mobius",
            _ => "Runcam"
        }.to_owned()
    }
}
