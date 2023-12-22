// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2021 Adrian <adrian.eddy at gmail>

use std::io::*;
use std::sync::{ Arc, atomic::AtomicBool };
use std::path::Path;
use std::borrow::Cow;

use crate::tags_impl::*;
use crate::*;

#[derive(Default)]
pub struct Runcam {
    pub model: Option<String>,
    pub gyro_buf: Vec<u8>
}

impl Runcam {
    pub fn camera_type(&self) -> String {
        match self.model.as_deref() {
            Some("iFlight GOCam GR") => "iFlight",
            Some("Mobius Maxi 4K") => "Mobius",
            _ => "Runcam"
        }.to_owned()
    }
    pub fn has_accurate_timestamps(&self) -> bool {
        false
    }
    pub fn possible_extensions() -> Vec<&'static str> {
        vec![]
    }
    pub fn frame_readout_time(&self) -> Option<f64> {
        None
    }
    pub fn normalize_imu_orientation(v: String) -> String {
        v
    }

    pub fn detect<P: AsRef<Path>>(buffer: &[u8], filepath: P) -> Option<Self> {
        let path = filepath.as_ref().to_str().unwrap_or_default().to_owned();
        let filename = filesystem::get_filename(&path);

        let mut gyro_path = None;
        if filename.to_ascii_lowercase().ends_with(".mp4") {
            gyro_path = Self::detect_gyro_path(&path, &filename);
        }
        let gyro_buf = if let Some(gyro) = &gyro_path {
            filesystem::read_file(gyro).ok()?
        } else {
            buffer.to_vec()
        };

        let match_hdr = |line: &[u8]| -> bool {
            &gyro_buf[0..line.len().min(gyro_buf.len())] == line
        };
        if match_hdr(b"time,x,y,z,ax,ay,az") || match_hdr(b"time,rx,ry,rz,ax,ay,az") || match_hdr(b"time,x,y,z") || match_hdr(b"time(ms),x,y,z") {
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

            return Some(Self { model, gyro_buf });
        }
        None
    }

    fn detect_gyro_path(path: &str, filename: &str) -> Option<String> {
        let files = filesystem::list_folder(&filesystem::get_folder(path));
        if filename.starts_with("RC_") {
            let num = filename.split("_").collect::<Vec<&str>>().get(1).cloned().unwrap_or(&"");
            let new_name = format!("RC_GyroData{}.csv", num);
            if let Some(fpath) = files.iter().find_map(|(name, path)| if name == &new_name { Some(path) } else { None }) {
                return Some(fpath.into());
            }
        }
        if filename.starts_with("IF-RC") {
            let num = filename.split("_").collect::<Vec<&str>>().get(1).cloned().unwrap_or(&"");
            let num = num.to_ascii_lowercase().replace(".mp4", "");
            let new_name = format!("gyroDate{}.csv", num);
            if let Some(fpath) = files.iter().find_map(|(name, path)| if name == &new_name { Some(path) } else { None }) {
                return Some(fpath.into());
            }
            let new_name = format!("gyroData{}.csv", num);
            if let Some(fpath) = files.iter().find_map(|(name, path)| if name == &new_name { Some(path) } else { None }) {
                return Some(fpath.into());
            }
        }
        if filename.starts_with("Thumb") {
            let new_name = filename.replace(".mp4", ".csv");
            if let Some(fpath) = files.iter().find_map(|(name, path)| if name == &new_name { Some(path) } else { None }) {
                return Some(fpath.into());
            }
        }
        None
    }

    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, _size: usize, _progress_cb: F, _cancel_flag: Arc<AtomicBool>) -> Result<Vec<SampleInfo>> {
        let e = |_| -> Error { ErrorKind::InvalidData.into() };

        let gyro_buf = if !self.gyro_buf.is_empty() {
            Cow::Borrowed(&self.gyro_buf)
        } else {
            let mut vec = Vec::new();
            stream.read_to_end(&mut vec)?;
            Cow::Owned(vec)
        };

        let mut gyro = Vec::new();
        let mut accl = Vec::new();

        let mut csv = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .trim(csv::Trim::All)
            .from_reader(Cursor::new(gyro_buf.as_ref()));
        for row in csv.records() {
            let row = row?;
            if &row[0] == "time" || &row[0] == "time(ms)" { continue; }

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

        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Unit, "Accelerometer unit", String, |v| v.to_string(), "g".into(),  Vec::new()));
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
            SampleInfo { tag_map: Some(map), ..Default::default() }
        ])
    }
}
