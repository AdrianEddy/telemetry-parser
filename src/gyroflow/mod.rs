// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2021-2023 Elvin, Adrian

use std::collections::BTreeMap;
use std::io::*;
use std::sync::{ Arc, atomic::AtomicBool };
use std::path::Path;
use std::borrow::Cow;

use crate::tags_impl::*;
use crate::*;

#[derive(Default)]
pub struct Gyroflow {
    pub model: Option<String>,
    pub gyro_buffer: Vec<u8>,
    vendor: String,
    frame_readout_time: Option<f64>
}

// .gcsv format as described here: https://docs.gyroflow.xyz/app/technical-details/gcsv-format

impl Gyroflow {
    pub fn camera_type(&self) -> String {
        self.vendor.to_owned()
    }
    pub fn has_accurate_timestamps(&self) -> bool {
        false
    }
    pub fn possible_extensions() -> Vec<&'static str> {
        vec!["mp4", "mov", "gcsv", "csv", "txt", "bin"]
    }
    pub fn frame_readout_time(&self) -> Option<f64> {
        self.frame_readout_time
    }
    pub fn normalize_imu_orientation(v: String) -> String {
        v
    }

    pub fn detect<P: AsRef<Path>>(buffer: &[u8], filepath: P) -> Option<Self> {
        let filepath = filepath.as_ref().to_str().unwrap_or_default().to_owned();
        let ext = filesystem::get_extension(&filepath);

        let gyro_buffer = if ext != "gcsv" {
            if let Some(gyro_path) = filesystem::file_with_extension(&filepath, "gcsv") {
                Cow::Owned(crate::filesystem::read_file(&gyro_path).ok()?)
            } else {
                Cow::Borrowed(buffer)
            }
        } else {
            Cow::Borrowed(buffer)
        };

        let match_hdr = |line: &[u8]| -> bool {
            &gyro_buffer[0..line.len().min(gyro_buffer.len())] == line
        };
        if match_hdr(b"GYROFLOW IMU LOG") || match_hdr(b"CAMERA IMU LOG") {
            let mut header = BTreeMap::new();

            // get header block
            let header_block = &gyro_buffer[0..gyro_buffer.len().min(500)];

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

            // let version = header.remove("version").unwrap_or("1.0".to_owned());
            let id = header.remove("id").unwrap_or("NoID".to_owned()).replace("_", " ");
            let vendor = header.remove("vendor").unwrap_or("gcsv".to_owned());
            let frame_readout_time = if header.contains_key("frame_readout_time") {
                // assume top down if not given
                let readout_direction = header.remove("frame_readout_direction").unwrap_or("0".to_owned());
                let readout_time = header.remove("frame_readout_time").unwrap_or("0.0".to_owned()).parse::<f64>().unwrap_or_default();
                match readout_direction.as_str() {
                    "0" => Some(readout_time), // top -> bottom
                    "1" => Some(-readout_time), // bottom -> top
                    "2" => None, // left/right not supported
                    "3" => None,
                    _ => None
                }
            } else { None };

            let model = Some(id);
            return Some(Self { model, gyro_buffer: gyro_buffer.into(), vendor, frame_readout_time });
        }
        None
    }

    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, _size: usize, _progress_cb: F, _cancel_flag: Arc<AtomicBool>) -> Result<Vec<SampleInfo>> {
        let e = |_| -> Error { ErrorKind::InvalidData.into() };

        let gyro_buf = if !self.gyro_buffer.is_empty() {
            Cow::Borrowed(&self.gyro_buffer)
        } else {
            let mut vec = Vec::new();
            stream.read_to_end(&mut vec)?;
            Cow::Owned(vec)
        };

        let mut header = BTreeMap::new();

        let mut gyro = Vec::new();
        let mut accl = Vec::new();
        let mut magn = Vec::new();

        let mut csv = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .trim(csv::Trim::All)
            .from_reader(Cursor::new(gyro_buf.as_ref()));

        let mut passed_header = false;

        let mut time_scale = 0.001; // default to millisecond

        for row in csv.records() {
            let row = row?;

            if row.len() == 1 {
                continue; // first line
            } else if row.len() == 2 && !passed_header {
                header.insert(row[0].to_owned(), row[1].to_owned());
                continue;
            } else if &row[0] == "t" || &row[0] == "time" {
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

        if let Some(lensprofile) = header.remove("lensprofile") {
            util::insert_tag(&mut map, tag!(parsed GroupId::Lens, TagId::Name, "Lens profile", String, |v| v.to_string(), lensprofile, Vec::new()));
        }

        util::insert_tag(&mut map,
            tag!(parsed GroupId::Default, TagId::Metadata, "Extra metadata", Json, |v| format!("{:?}", v), serde_json::to_value(header).map_err(|_| Error::new(ErrorKind::Other, "Serialize error"))?, vec![])
        );

        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Data, "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), accl, vec![]));
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Data, "Gyroscope data",     Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]));
        util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer,  TagId::Data, "Magnetometer data",  Vec_TimeVector3_f64, |v| format!("{:?}", v), magn, vec![]));

        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Unit, "Accelerometer unit", String, |v| v.to_string(), "g".into(),  Vec::new()));
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Unit, "Gyroscope unit",     String, |v| v.to_string(), "deg/s".into(), Vec::new()));
        util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer,  TagId::Unit, "Magnetometer unit",  String, |v| v.to_string(), "μT".into(), Vec::new()));

        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Scale, "Gyroscope scale",     f64, |v| format!("{:?}", v), gyro_scale, vec![]));
        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Scale, "Accelerometer scale", f64, |v| format!("{:?}", v), accl_scale, vec![]));
        util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer,  TagId::Scale, "Magnetometer scale",  f64, |v| format!("{:?}", v), mag_scale, vec![]));

        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.to_string(), Vec::new()));
        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.to_string(), Vec::new()));
        util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer,  TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.to_string(), Vec::new()));

        Ok(vec![
            SampleInfo { tag_map: Some(map), ..Default::default() }
        ])
    }
}
