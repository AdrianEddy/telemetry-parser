// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::tags_impl::*;
use crate::*;
use byteorder::{BigEndian, LittleEndian, ReadBytesExt};
use std::io::*;

pub fn parse<T: Read + Seek>(stream: &mut T, _size: usize) -> Result<Vec<SampleInfo>> {
    let mut stream = std::io::BufReader::new(stream);

    let mut gyro = Vec::new();
    let mut accl = Vec::new();
    let mut angl = Vec::new();
    let mut magn = Vec::new();
    let mut quat = Vec::new();

    let mut last_timestamp;

    let mut buf = vec![0u8; 512];
    stream.read_exact(&mut buf[0..512])?;
    let mut d = std::io::Cursor::new(&buf);

    let brand = unsafe { std::str::from_utf8_unchecked(&buf[0..12]) };
    let version = unsafe { std::str::from_utf8_unchecked(&buf[12..16]) };
    let _product_id = &buf[16..36];
    let _product_sn = &buf[36..52];

    let imu_orientation = std::str::from_utf8(&buf[60..64]).unwrap_or("XYZ");

    let yy = (buf[64] as i32) + 2000;
    let mm = buf[65] as u32;
    let dd = buf[66] as u32;
    let h = buf[67] as u32;
    let m = buf[68] as u32;
    let s = buf[69] as u32;
    let ms = 0u32;
    let created_at = chrono::NaiveDate::from_ymd_opt(yy, mm, dd)
        .and_then(|x| x.and_hms_milli_opt(h, m, s, ms))
        .unwrap_or_default();

    let first_timestamp = 0f64;

    d.seek(SeekFrom::Start(76))?;
    let _init_quat = TimeQuaternion {
        t: (first_timestamp * 1000.0) as f64,
        v: Quaternion {
            w: d.read_f32::<LittleEndian>()? as f64,
            x: d.read_f32::<LittleEndian>()? as f64,
            y: d.read_f32::<LittleEndian>()? as f64,
            z: d.read_f32::<LittleEndian>()? as f64,
        },
    };
    let log_freq = d.read_u32::<LittleEndian>()?;

    d.seek(SeekFrom::Start(144))?;
    let accl_odr = d.read_u16::<LittleEndian>()?;
    let accl_max_bw = d.read_u16::<LittleEndian>()?;
    let accl_timeoffset = d.read_i32::<LittleEndian>()?;
    let accl_range = d.read_u32::<LittleEndian>()? as f64;

    let gyro_odr = d.read_u16::<LittleEndian>()?;
    let gyro_max_bw = d.read_u16::<LittleEndian>()?;
    let gyro_timeoffset = d.read_i32::<LittleEndian>()?;
    let gyro_range = d.read_u32::<LittleEndian>()? as f64;

    let magn_odr = d.read_u16::<LittleEndian>()?;
    let magn_max_bw = d.read_u16::<LittleEndian>()?;
    let magn_timeoffset = d.read_i32::<LittleEndian>()?;
    let magn_range = (d.read_u32::<LittleEndian>()? as f64) / 1000.0;

    let timestamp_step = 1.0f64 / (log_freq as f64);
    last_timestamp = first_timestamp - timestamp_step;

    let metadata = serde_json::json!({
       "brand": brand,
       "version": version,
       "created_at": created_at.to_string(),
       "log_freq": log_freq,
       "accl_odr": accl_odr,
       "accl_max_bandwidth": accl_max_bw,
       "accl_timeoffset": accl_timeoffset,
       "accl_range": accl_range,
       "gyro_odr": gyro_odr,
       "gyro_max_bandwidth": gyro_max_bw,
       "gyro_timeoffset": gyro_timeoffset,
       "gyro_range": gyro_range,
       "magn_odr": magn_odr,
       "magn_max_bandwidth": magn_max_bw,
       "magn_timeoffset": magn_timeoffset,
       "magn_range": magn_range,
       "timestamp_step": timestamp_step,
       "init quat": _init_quat,
    });

    // acc gyro mag quad angle temp -- --
    let sensor_length = [6, 6, 6, 8, 12, 2, 0, 0];
    let mut sensor_valid = [0u8; 8];

    while let Ok(tag) = stream.read_u16::<BigEndian>() {
        if tag == 0xaa55 {
            let mut data_valid = stream.read_u8()?;
            let mut data_length = 0;
            for n in 0..8 {
                sensor_valid[n] = data_valid & 0b00000001;
                if sensor_valid[n] == 1 {
                    data_length += sensor_length[n];
                }
                data_valid >>= 1;
            }

            if let Ok(mut d) = checksum(&mut stream, data_length) {
                last_timestamp += timestamp_step;
                if sensor_valid[0] == 1 {
                    accl.push(TimeVector3 {
                        t: (last_timestamp as f64) + (accl_timeoffset as f64) / 1000.0,
                        x: ((d.read_i16::<LittleEndian>()? as f64) / 32768.0) * accl_range,
                        y: ((d.read_i16::<LittleEndian>()? as f64) / 32768.0) * accl_range,
                        z: ((d.read_i16::<LittleEndian>()? as f64) / 32768.0) * accl_range,
                    });
                }

                if sensor_valid[1] == 1 {
                    gyro.push(TimeVector3 {
                        t: (last_timestamp as f64) + (gyro_timeoffset as f64) / 1000.0,
                        x: ((d.read_i16::<LittleEndian>()? as f64) / 32768.0) * gyro_range,
                        y: ((d.read_i16::<LittleEndian>()? as f64) / 32768.0) * gyro_range,
                        z: ((d.read_i16::<LittleEndian>()? as f64) / 32768.0) * gyro_range,
                    });
                }

                if sensor_valid[2] == 1 {
                    magn.push(TimeVector3 {
                        t: (last_timestamp as f64) + (magn_timeoffset as f64) / 1000.0,
                        x: d.read_i16::<LittleEndian>()? as i64,
                        y: d.read_i16::<LittleEndian>()? as i64,
                        z: d.read_i16::<LittleEndian>()? as i64,
                    });
                }

                if sensor_valid[3] == 1 {
                    quat.push(TimeQuaternion {
                        t: (last_timestamp * 1000.0) as f64,
                        v: util::multiply_quats(
                            (
                                (d.read_i16::<LittleEndian>()? as f64) / 32768.0,
                                (d.read_i16::<LittleEndian>()? as f64) / 32768.0,
                                (d.read_i16::<LittleEndian>()? as f64) / 32768.0,
                                (d.read_i16::<LittleEndian>()? as f64) / 32768.0,
                            ),
                            ((2.0_f64).sqrt() * 0.5, 0.0, 0.0, -(2.0_f64).sqrt() * 0.5),
                        ),
                    });
                }

                if sensor_valid[4] == 1 {
                    angl.push(TimeVector3 {
                        t: last_timestamp as f64,
                        x: ((d.read_i16::<LittleEndian>()? as f64) / 32768.0) * 180.0, // Roll
                        y: ((d.read_i16::<LittleEndian>()? as f64) / 32768.0) * 180.0, // Pitch
                        z: ((d.read_i16::<LittleEndian>()? as f64) / 32768.0) * 180.0, // Yaw
                    });
                }
            }
        }
    }

    let mut map = GroupedTagMap::new();

    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Data, "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}",  v), accl, vec![]),);
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Data, "Gyroscope data",     Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]),);

    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Unit, "Accelerometer unit", String, |v| v.to_string(), "g".into(), Vec::new()),);
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Unit, "Gyroscope unit",     String, |v| v.to_string(), "deg/s".into(), Vec::new()),);
    
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()),);
    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()),);
    
    util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer,  TagId::Data, "Magnetometer data", Vec_TimeVector3_i64f64, |v| format!("{:?}", v), magn, vec![]),);
    util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer,  TagId::Unit, "Magnetometer unit", String, |v| v.to_string(), "μT".into(), Vec::new()),);
    
    util::insert_tag(&mut map, tag!(parsed GroupId::Custom("Angle".into()),        TagId::Data, "Angle data", Vec_TimeVector3_f64, |v| format!("{:?}", v), angl, vec![]),);
    util::insert_tag(&mut map, tag!(parsed GroupId::Custom("Angle".into()),        TagId::Unit, "Angle unit", String, |v| v.to_string(), "deg".into(),  Vec::new()),);
    
    util::insert_tag(&mut map, tag!(parsed GroupId::Quaternion,   TagId::Data, "Quaternion data",   Vec_TimeQuaternion_f64,  |v| format!("{:?}", v), quat, vec![]),);
    util::insert_tag(&mut map, tag!(parsed GroupId::Default, TagId::Metadata, "Metadata", Json, |v| serde_json::to_string(v).unwrap(), metadata, vec![]),);
    
    Ok(vec![SampleInfo {
        timestamp_ms: first_timestamp as f64,
        duration_ms: (last_timestamp - first_timestamp) as f64,
        tag_map: Some(map),
        ..Default::default()
    }])
}

fn checksum<T: Read + Seek>(stream: &mut T, item_size: u64) -> Result<Cursor<Vec<u8>>> {
    let mut buf = vec![0u8; item_size as usize];
    stream.read_exact(&mut buf)?;
    let sum = stream.read_u8()?;
    let init: u8 = 0;
    let calculated_sum = buf.iter().fold(init, |sum, &x| sum.wrapping_add(x));

    if calculated_sum == sum {
        Ok(Cursor::new(buf))
    } else {
        log::error!("Invalid checksum! {} != {} | {}", calculated_sum, sum, crate::util::to_hex(&buf));
        Err(Error::from(ErrorKind::InvalidData))
    }
}
