// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::tags_impl::*;
use crate::*;
use byteorder::{ReadBytesExt, BigEndian, LittleEndian};
use std::convert::TryInto;

pub trait FromBytes<T> {
    fn from_le_bytes(data: &[u8]) -> T;
    fn from_be_bytes(data: &[u8]) -> T;
}

macro_rules! impl_from_bytes {
    ($($t:ty),+) => {
        $(impl FromBytes<$t> for $t {
            fn from_le_bytes(data: &[u8]) -> $t {
                <$t>::from_le_bytes(data.try_into().unwrap())
            }
            fn from_be_bytes(data: &[u8]) -> $t {
                <$t>::from_be_bytes(data.try_into().unwrap())
            }
        })+
    }
}

impl_from_bytes!(u8, u16, i16, i32, u32, i64,f32,f64);
fn from_bytes<T: FromBytes<T>>(data: &[u8]) -> T {
    T::from_le_bytes(&data[..std::mem::size_of::<T>()])
}


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

    let brand =  unsafe { std::str::from_utf8_unchecked(&buf[0..12])};
    let version =  unsafe { std::str::from_utf8_unchecked(&buf[12..16])};
    let _product_id = &buf[16..36];
    let _product_sn = &buf[36..52];

    let imu_orientation =   std::str::from_utf8(&buf[60..64]).unwrap_or("XYZ");
    let yy = buf[64] as i32 + 2000;
    let mm = buf[65] as u32;
    let dd = buf[66] as u32;
    let h  = buf[67] as u32;
    let m  = buf[68] as u32;
    let s  = buf[69] as u32;
    let ms = 0u32;
    let create_at =  chrono::NaiveDate::from_ymd_opt(yy, mm, dd).and_then(|x| x.and_hms_milli_opt(h, m, s, ms)).unwrap_or_default();
    let first_timestamp = 0f64;//create_at.timestamp_millis() as f64 / 1000.0;

    let _init_quad = TimeQuaternion {
        t: (first_timestamp*1000.0) as f64,
        v: Quaternion{
            w: from_bytes::<f32>(&buf[76..80]) as f64,
            x: from_bytes::<f32>(&buf[80..84]) as f64,
            y: from_bytes::<f32>(&buf[84..88]) as f64,
            z: from_bytes::<f32>(&buf[88..92]) as f64,
        }
    };

    let log_freq = from_bytes::<u32>(&buf[92..96]);

    let acc_odr = from_bytes::<u16>(&buf[144..146]);
    let acc_max_bw = from_bytes::<u16>(&buf[146..148]);
    let acc_timeoffset = from_bytes::<i32>(&buf[148..152]);
    let acc_range = from_bytes::<u32>(&buf[152..156]) as f64;

    let gyro_odr = from_bytes::<u16>(&buf[156..158]);
    let gyro_max_bw = from_bytes::<u16>(&buf[158..160]);
    let gyro_timeoffset = from_bytes::<i32>(&buf[160..164]);
    let gyro_range = from_bytes::<u32>(&buf[164..168])as f64;

    let mag_odr = from_bytes::<u16>(&buf[168..170]);
    let mag_max_bw = from_bytes::<u16>(&buf[170..172]);
    let mag_timeoffset = from_bytes::<i32>(&buf[172..176]);
    let mag_range = from_bytes::<u32>(&buf[176..180])as f64 / 1000.0;

    log::info!("brand is: {}",brand);
    log::info!("version is: {}",version);
    log::info!("imu_orientation is: {}",imu_orientation);
    log::info!("create_at: {}",create_at);
    log::info!("first_timestamp: {}",first_timestamp);

    log::info!("log_freq: {}",log_freq);

    log::info!("acc_odr: {}",acc_odr);
    log::info!("acc_max_bandwidth: {}",acc_max_bw);
    log::info!("acc_timeoffset: {}",acc_timeoffset);
    log::info!("acc_range: {}",acc_range);

    log::info!("gyro_odr: {}",gyro_odr);
    log::info!("gyro_max_bandwidth: {}",gyro_max_bw);
    log::info!("gyro_timeoffset: {}",gyro_timeoffset);
    log::info!("gyro_range: {}",gyro_range);

    log::info!("mag_odr: {}",mag_odr);
    log::info!("mag_max_bandwidth: {}",mag_max_bw);
    log::info!("mag_timeoffset: {}",mag_timeoffset);
    log::info!("mag_range: {}",mag_range);

    log::info!("init quad is :qw:{}, qx{}, qy{}, qz{}",_init_quad.v.w,_init_quad.v.x,_init_quad.v.y,_init_quad.v.z);


    let timestamp_step = 1.0f64 /(log_freq as f64);
    last_timestamp = first_timestamp - timestamp_step;

    log::info!("timestamp_step: {}",timestamp_step);

    // acc gyro mag quad angle temp -- --
    let sensor_length = [6,6,6,8,12,2,0,0];
    let mut sensor_valid = [0u8;8];

    while let Ok(tag) = stream.read_u16::<BigEndian>() {
        if tag == 0xAA55{

            let mut data_valid = stream.read_u8()?;
            let mut data_length = 0;
            for n in 0..8
            {
                sensor_valid[n] = data_valid & 0b00000001;
                if sensor_valid[n] == 1 {
                    data_length+=sensor_length[n];
                }
                data_valid>>=1;
            }

            if let Ok(mut d) = checksum( &mut stream, data_length) {
                last_timestamp += timestamp_step;
                if sensor_valid[0] == 1 {   
                    accl.push(TimeVector3 {
                        t: last_timestamp as f64 + acc_timeoffset as f64/1000.0,
                        x: d.read_i16::<LittleEndian>()? as f64 / 32768.0 * acc_range,
                        y: d.read_i16::<LittleEndian>()? as f64 / 32768.0 * acc_range,
                        z: d.read_i16::<LittleEndian>()? as f64 / 32768.0 * acc_range
                    });
                }
                
                if sensor_valid[1] == 1 {   
                    gyro.push(TimeVector3 {
                        t: last_timestamp as f64 + gyro_timeoffset as f64/1000.0,
                        x: d.read_i16::<LittleEndian>()? as f64 / 32768.0 * gyro_range,
                        y: d.read_i16::<LittleEndian>()? as f64 / 32768.0 * gyro_range,
                        z: d.read_i16::<LittleEndian>()? as f64 / 32768.0 * gyro_range
                    });
                }

                if sensor_valid[2] == 1 { 
                    magn.push(TimeVector3 {
                        t: last_timestamp as f64+ mag_timeoffset as f64/1000.0,
                        x: d.read_i16::<LittleEndian>()? as i64,
                        y: d.read_i16::<LittleEndian>()? as i64,
                        z: d.read_i16::<LittleEndian>()? as i64
                    });
                }

                if sensor_valid[3] == 1 { 
                    quat.push(TimeQuaternion {
                        t: (last_timestamp*1000.0) as f64,
                        v: util::multiply_quats(
                            (d.read_i16::<LittleEndian>()? as f64 / 32768.0,
                            d.read_i16::<LittleEndian>()? as f64 / 32768.0,
                            d.read_i16::<LittleEndian>()? as f64 / 32768.0,
                            d.read_i16::<LittleEndian>()? as f64 / 32768.0),
                            ((2.0_f64).sqrt()*0.5, 0.0, 0.0, -(2.0_f64).sqrt()*0.5),
                        ),
                    });
                }

                if sensor_valid[4] == 1 { 
                    angl.push(TimeVector3 {
                        t: last_timestamp as f64,
                        x: d.read_i16::<LittleEndian>()? as f64 / 32768.0 * 180.0, // Roll
                        y: d.read_i16::<LittleEndian>()? as f64 / 32768.0 * 180.0, // Pitch
                        z: d.read_i16::<LittleEndian>()? as f64 / 32768.0 * 180.0  // Yaw
                    });
                }
            }
        }
    }

    let mut map = GroupedTagMap::new();

    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Data, "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}",  v), accl, vec![]));
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Data, "Gyroscope data",     Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]));

    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Unit, "Accelerometer unit", String, |v| v.to_string(), "g".into(), Vec::new()));
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Unit, "Gyroscope unit",     String, |v| v.to_string(), "deg/s".into(), Vec::new()));

    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()));
    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()));

    util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer,  TagId::Data, "Magnetometer data", Vec_TimeVector3_i64f64, |v| format!("{:?}", v), magn, vec![]));
    util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer,  TagId::Unit, "Magnetometer unit", String, |v| v.to_string(), "μT".into(), Vec::new()));

    util::insert_tag(&mut map, tag!(parsed GroupId::Custom("Angle".into()),        TagId::Data, "Angle data", Vec_TimeVector3_f64, |v| format!("{:?}", v), angl, vec![]));
    util::insert_tag(&mut map, tag!(parsed GroupId::Custom("Angle".into()),        TagId::Unit, "Angle unit", String, |v| v.to_string(), "deg".into(),  Vec::new()));
    
    util::insert_tag(&mut map, tag!(parsed GroupId::Quaternion,   TagId::Data, "Quaternion data",   Vec_TimeQuaternion_f64,  |v| format!("{:?}", v), quat, vec![]));
    
    Ok(vec![
        SampleInfo { timestamp_ms: first_timestamp as f64, duration_ms: (last_timestamp - first_timestamp) as f64, tag_map: Some(map), ..Default::default() }
    ])
}

fn checksum<T: Read + Seek>(stream: &mut T, item_size: u64) -> Result<Cursor<Vec<u8>>> {
    let mut buf = vec![0u8; item_size as usize];
    stream.read_exact(&mut buf)?;
    let sum  = stream.read_u8()?;
    let init: u8 = 0;
    let calculated_sum = buf.iter().fold(init, |sum, &x| sum.wrapping_add(x));

    if calculated_sum == sum {
        Ok(Cursor::new(buf))
    } else {
        log::error!("Invalid checksum! {} != {} | {}", calculated_sum, sum, crate::util::to_hex(&buf));
        Err(Error::from(ErrorKind::InvalidData))
    }
}
