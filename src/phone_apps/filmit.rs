// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2023 Adrian <adrian.eddy at gmail>

use std::io::*;
use byteorder::{ ReadBytesExt, BigEndian };

use crate::tags_impl::*;
use crate::*;
use memchr::memmem;

pub fn detect(buffer: &[u8]) -> bool {
    memmem::find(buffer, b"mettapplication/gyro").is_some()
}

pub fn parse<T: Read + Seek, F: Fn(f64)>(stream: &mut T, size: usize, progress_cb: F, cancel_flag: Arc<AtomicBool>) -> Result<Vec<SampleInfo>> {
    // Determine which track is which
    let mut gyro_track = 2;
    let mut accel_track = 3;
    {
        let mut current_track: i32 = -1;
        while let Ok((typ, _offs, size, header_size)) = util::read_box(stream) {
            let org_pos = stream.stream_position()?;
            if size == 0 || typ == 0 { break; }
            if typ == fourcc("moov") || typ == fourcc("trak") || typ == fourcc("mdia") || typ == fourcc("minf") || typ == fourcc("stbl") || typ == fourcc("stsd") {
                if typ == fourcc("trak") { current_track += 1; }
                if typ == fourcc("stsd") { stream.seek(SeekFrom::Current(8))?; }
                continue; // go inside these boxes
            } else {
                if typ == fourcc("mett") {
                    let mut buf = [0u8; 32];
                    if stream.read(&mut buf).is_ok() {
                        if memmem::find(&buf, b"application/gyro") .is_some() { gyro_track  = current_track as usize; }
                        if memmem::find(&buf, b"application/accel").is_some() { accel_track = current_track as usize; }
                    }
                }
                stream.seek(SeekFrom::Start(org_pos + size - header_size as u64))?;
            }
        }
        stream.seek(SeekFrom::Start(0))?;
    }

    let mut gyro = Vec::new();
    let mut accel = Vec::new();

    let mut map = GroupedTagMap::new();

    let mut samples = Vec::new();

    util::get_metadata_track_samples(stream, size, false, |info: SampleInfo, data: &[u8], file_position: u64, _video_md: Option<&VideoMetadata>| {
        if size > 0 {
            progress_cb(((info.track_index as f64 - 1.0) + (file_position as f64 / size as f64)) / 3.0);
        }

        if data.len() >= 3*4 {
            let mut d = Cursor::new(data);

            crate::try_block!({
                let v = TimeVector3 {
                    t: info.timestamp_ms / 1000.0,
                    x: d.read_f32::<BigEndian>().ok()? as f64,
                    y: d.read_f32::<BigEndian>().ok()? as f64,
                    z: d.read_f32::<BigEndian>().ok()? as f64
                };
                if info.track_index == gyro_track {
                    gyro.push(v);
                } else if info.track_index == accel_track {
                    accel.push(v);
                }
            });
        }
    }, cancel_flag)?;

    let imu_orientation = "XYZ";
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope, TagId::Data, "Gyroscope data",         Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]));
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope, TagId::Unit, "Gyroscope unit",         String, |v| v.to_string(), "rad/s".into(), Vec::new()));
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()));

    if !accel.is_empty() {
        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Data, "Accelerometer data",         Vec_TimeVector3_f64, |v| format!("{:?}", v), accel, vec![]));
        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Unit, "Accelerometer unit",         String, |v| v.to_string(), "m/s²".into(), Vec::new()));
        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation",     String, |v| v.to_string(), imu_orientation.into(), Vec::new()));
    }

    samples.insert(0, SampleInfo { tag_map: Some(map), ..Default::default() });

    Ok(samples)
}
