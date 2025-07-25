// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2021 Adrian <adrian.eddy at gmail>

use std::io::*;

use crate::tags_impl::*;
use crate::*;
use byteorder::{ReadBytesExt, BigEndian, LittleEndian};

pub fn parse<T: Read + Seek>(stream: &mut T, _size: usize, options: crate::InputOptions) -> Result<Vec<SampleInfo>> {
    let mut stream = std::io::BufReader::new(stream);

    let mut gyro = Vec::new();
    let mut accl = Vec::new();
    let mut angl = Vec::new();
    let mut magn = Vec::new();
    let mut quat = Vec::new();

    let mut has_any_time = false;

    let default_step = 1.0 / 200.0; // 200 Hz

    let mut last_timestamp = -default_step;
    let mut first_timestamp = 0.0;
    while let Ok(tag) = stream.read_u16::<BigEndian>() {
        match tag {
            0x5550 => { // Time Output
                if let Ok(mut d) = checksum(tag, &mut stream, 8) {
                    has_any_time = true;
                    let yy = d.read_u8()? as i32 + 2000;
                    let mm = d.read_u8()? as u32;
                    let dd = d.read_u8()? as u32;
                    let h  = d.read_u8()? as u32;
                    let m  = d.read_u8()? as u32;
                    let s  = d.read_u8()? as u32;
                    let ms = d.read_u16::<LittleEndian>()? as u32;

                    last_timestamp = chrono::NaiveDate::from_ymd_opt(yy, mm, dd).and_then(|x| x.and_hms_milli_opt(h, m, s, ms)).unwrap_or_default().and_utc().timestamp_millis() as f64 / 1000.0;

                    if first_timestamp == 0.0 {
                        first_timestamp = last_timestamp;
                    }
                    last_timestamp = last_timestamp - first_timestamp;
                }
            }
            0x5551 => { // Acceleration Output
                if let Ok(mut d) = checksum(tag, &mut stream, 8) {
                    if !has_any_time {
                        last_timestamp += default_step;
                    }
                    accl.push(TimeVector3 {
                        t: last_timestamp as f64,
                        x: d.read_i16::<LittleEndian>()? as f64 / 32768.0 * 16.0,
                        y: d.read_i16::<LittleEndian>()? as f64 / 32768.0 * 16.0,
                        z: d.read_i16::<LittleEndian>()? as f64 / 32768.0 * 16.0
                    });
                    let _t = d.read_u16::<LittleEndian>()? / 100; // Temperature (°C)
                }
            }
            0x5552 => { // Angular Velocity Output (gyro)
                if let Ok(mut d) = checksum(tag, &mut stream, 8) {
                    gyro.push(TimeVector3 {
                        t: last_timestamp as f64,
                        x: d.read_i16::<LittleEndian>()? as f64 / 32768.0 * 2000.0,
                        y: d.read_i16::<LittleEndian>()? as f64 / 32768.0 * 2000.0,
                        z: d.read_i16::<LittleEndian>()? as f64 / 32768.0 * 2000.0
                    });
                    let _t = d.read_u16::<LittleEndian>()? / 100; // Temperature (°C)
                }
            }
            0x5553 => { // Angle Output
                if let Ok(mut d) = checksum(tag, &mut stream, 8) {
                    angl.push(TimeVector3 {
                        t: last_timestamp as f64,
                        x: d.read_i16::<LittleEndian>()? as f64 / 32768.0 * 180.0, // Roll
                        y: d.read_i16::<LittleEndian>()? as f64 / 32768.0 * 180.0, // Pitch
                        z: d.read_i16::<LittleEndian>()? as f64 / 32768.0 * 180.0  // Yaw
                    });
                    let _v = d.read_u16::<LittleEndian>()?; // version
                }
            }
            0x5554 => { // Magnetic Output
                if let Ok(mut d) = checksum(tag, &mut stream, 8) {
                    magn.push(TimeVector3 {
                        t: last_timestamp as f64,
                        x: d.read_i16::<LittleEndian>()? as i64,
                        y: d.read_i16::<LittleEndian>()? as i64,
                        z: d.read_i16::<LittleEndian>()? as i64
                    });
                    let _t = d.read_u16::<LittleEndian>()? / 100; // Temperature (°C)
                }
            }
            0x5559 => { // Quaternion
                if let Ok(mut d) = checksum(tag, &mut stream, 8) {
                    quat.push(TimeQuaternion {
                        t: last_timestamp as f64 * 1000.0,
                        v: Quaternion {
                            w: d.read_i16::<LittleEndian>()? as f64 / 32768.0,
                            x: d.read_i16::<LittleEndian>()? as f64 / 32768.0,
                            y: d.read_i16::<LittleEndian>()? as f64 / 32768.0,
                            z: d.read_i16::<LittleEndian>()? as f64 / 32768.0
                        }
                    });
                }
            }
            _ => {
                log::warn!("Unknown tag! 0x{:02x}", tag);
            }
        }
    }

    let mut map = GroupedTagMap::new();

    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Data, "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), accl, vec![]), &options);
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Data, "Gyroscope data",     Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]), &options);

    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Unit, "Accelerometer unit", String, |v| v.to_string(), "g".into(), Vec::new()), &options);
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Unit, "Gyroscope unit",     String, |v| v.to_string(), "deg/s".into(), Vec::new()), &options);

    let imu_orientation = "ZYx";
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()), &options);
    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()), &options);

    util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer,  TagId::Data, "Magnetometer data", Vec_TimeVector3_i64f64, |v| format!("{:?}", v), magn, vec![]), &options);
    util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer,  TagId::Unit, "Magnetometer unit", String, |v| v.to_string(), "μT".into(), Vec::new()), &options);

    util::insert_tag(&mut map, tag!(parsed GroupId::Custom("Angle".into()),        TagId::Data, "Angle data", Vec_TimeVector3_f64, |v| format!("{:?}", v), angl, vec![]), &options);
    util::insert_tag(&mut map, tag!(parsed GroupId::Custom("Angle".into()),        TagId::Unit, "Angle unit", String, |v| v.to_string(), "deg".into(),  Vec::new()), &options);

    util::insert_tag(&mut map, tag!(parsed GroupId::Quaternion,   TagId::Data, "Quaternion data",   Vec_TimeQuaternion_f64,  |v| format!("{:?}", v), quat, vec![]), &options);

    Ok(vec![
        SampleInfo { timestamp_ms: first_timestamp as f64, duration_ms: last_timestamp as f64, tag_map: Some(map), ..Default::default() }
    ])
}

fn checksum<T: Read + Seek>(tag: u16, stream: &mut T, item_size: u64) -> Result<Cursor<Vec<u8>>> {
    let mut buf = vec![0u8; item_size as usize];
    stream.read_exact(&mut buf)?;
    let sum  = stream.read_u8()?;

    let init: u8 = ((tag & 0xff) as u8) + ((tag >> 8) & 0xff) as u8;
    let calculated_sum = buf.iter().fold(init, |sum, &x| sum.wrapping_add(x));

    if calculated_sum == sum {
        Ok(Cursor::new(buf))
    } else {
        log::error!("Invalid checksum! {} != {} | {:04x} {}", calculated_sum, sum, tag, crate::util::to_hex(&buf));
        Err(Error::from(ErrorKind::InvalidData))
    }
}
