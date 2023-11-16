// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2023 Adrian <adrian.eddy at gmail>

use std::io::*;
use byteorder::{ ReadBytesExt, BigEndian };

use crate::tags_impl::*;
use crate::*;
use memchr::memmem;

pub fn detect(buffer: &[u8], _filename: &str) -> bool {
    memmem::find(buffer, b"mettapplication/gyro").is_some()
}

pub fn parse<T: Read + Seek, F: Fn(f64)>(stream: &mut T, size: usize, progress_cb: F, cancel_flag: Arc<AtomicBool>) -> Result<Vec<SampleInfo>> {
    let mut gyro = Vec::new();

    let mut map = GroupedTagMap::new();

    let mut samples = Vec::new();

    util::get_metadata_track_samples(stream, size, false, |info: SampleInfo, data: &[u8], file_position: u64| {
        if size > 0 {
            progress_cb(((info.track_index as f64 - 1.0) + (file_position as f64 / size as f64)) / 3.0);
        }

        if data.len() >= 3*4 {
            let mut d = Cursor::new(data);

            crate::try_block!({
                gyro.push(TimeVector3 { t: info.timestamp_ms / 1000.0,
                    x: d.read_f32::<BigEndian>().ok()? as f64,
                    y: d.read_f32::<BigEndian>().ok()? as f64,
                    z: d.read_f32::<BigEndian>().ok()? as f64
                });
            });
        }
    }, cancel_flag)?;

    let imu_orientation = "XYZ";
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope, TagId::Data, "Gyroscope data",         Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]));
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope, TagId::Unit, "Gyroscope unit",         String, |v| v.to_string(), "rad/s".into(), Vec::new()));
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()));

    samples.insert(0, SampleInfo { tag_map: Some(map), ..Default::default() });

    Ok(samples)
}
