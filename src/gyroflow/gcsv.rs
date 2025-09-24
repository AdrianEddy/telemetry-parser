// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2021-2023 Elvin, Adrian

use std::collections::BTreeMap;
use std::io::*;
use std::sync::{ Arc, atomic::AtomicBool };
use std::path::Path;

use crate::tags_impl::*;
use crate::*;

#[derive(Default)]
pub struct GyroflowGcsv {
    pub model: Option<String>,
    vendor: String,
    frame_readout_time: Option<f64>
}

// .gcsv format as described here: https://docs.gyroflow.xyz/app/technical-details/gcsv-format

impl GyroflowGcsv {
    pub fn camera_type(&self) -> String {
        self.vendor.to_owned()
    }
    pub fn has_accurate_timestamps(&self) -> bool {
        false
    }
    pub fn possible_extensions() -> Vec<&'static str> {
        vec!["mp4", "mov", "mkv", "gcsv", "csv", "txt", "bin"]
    }
    pub fn frame_readout_time(&self) -> Option<f64> {
        self.frame_readout_time
    }
    pub fn normalize_imu_orientation(v: String) -> String {
        v
    }

    pub fn detect<P: AsRef<Path>>(buffer: &[u8], _filepath: P, _options: &crate::InputOptions) -> Option<Self> {
        let match_hdr = |line: &[u8]| -> bool {
            &buffer[0..line.len().min(buffer.len())] == line
        };
        if match_hdr(b"GYROFLOW IMU LOG") || match_hdr(b"CAMERA IMU LOG") {
            let mut header = BTreeMap::new();

            // get header block
            let header_block = &buffer[0..buffer.len().min(500)];

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
                    "0" | "TopToBottom" => Some(readout_time), // top -> bottom
                    "1" | "180" | "BottomToTop" => Some(-readout_time), // bottom -> top
                    "2" | "270" | "LeftToRight" => Some(readout_time + 10000.0), // left -> right
                    "3" | "90"  | "RightToLeft" => Some(-(readout_time + 10000.0)), // right -> left
                    _ => None
                }
            } else { None };

            let model = Some(id);
            return Some(Self { model, vendor, frame_readout_time });
        }
        None
    }

    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, _size: usize, _progress_cb: F, _cancel_flag: Arc<AtomicBool>, options: crate::InputOptions) -> Result<Vec<SampleInfo>> {

        let mut header = BTreeMap::new();

        let mut gyro = Vec::new();
        let mut accl = Vec::new();
        let mut magn = Vec::new();

        let mut csv = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .trim(csv::Trim::All)
            .from_reader(stream);

        let mut passed_header = false;

        let mut time_scale = 0.001; // default to millisecond

        for row in csv.records() {
            let row = match row {
                Ok(row) => row,
                Err(_) => { continue; }
            };

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

            let time = match row[0].parse::<f64>() {
                Ok(time) => time * time_scale,
                Err(e) => { log::error!("Failed to parse time: {row:?} - {e:?}"); continue; }
            };
            println!("csv ts: {time}");
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
            util::insert_tag(&mut map, tag!(parsed GroupId::Lens, TagId::Name, "Lens profile", String, |v| v.to_string(), lensprofile, Vec::new()), &options);
        }

        util::insert_tag(&mut map,
            tag!(parsed GroupId::Default, TagId::Metadata, "Extra metadata", Json, |v| format!("{:?}", v), serde_json::to_value(header).map_err(|_| Error::new(ErrorKind::Other, "Serialize error"))?, vec![]),
            &options
        );

        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Data, "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), accl, vec![]), &options);
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Data, "Gyroscope data",     Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]), &options);
        util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer,  TagId::Data, "Magnetometer data",  Vec_TimeVector3_f64, |v| format!("{:?}", v), magn, vec![]), &options);

        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Unit, "Accelerometer unit", String, |v| v.to_string(), "g".into(),  Vec::new()), &options);
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Unit, "Gyroscope unit",     String, |v| v.to_string(), "deg/s".into(), Vec::new()), &options);
        util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer,  TagId::Unit, "Magnetometer unit",  String, |v| v.to_string(), "μT".into(), Vec::new()), &options);

        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Scale, "Gyroscope scale",     f64, |v| format!("{:?}", v), gyro_scale, vec![]), &options);
        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Scale, "Accelerometer scale", f64, |v| format!("{:?}", v), accl_scale, vec![]), &options);
        util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer,  TagId::Scale, "Magnetometer scale",  f64, |v| format!("{:?}", v), mag_scale, vec![]), &options);

        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.to_string(), Vec::new()), &options);
        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.to_string(), Vec::new()), &options);
        util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer,  TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.to_string(), Vec::new()), &options);

        Ok(vec![
            SampleInfo { tag_map: Some(map), ..Default::default() }
        ])
    }
}
