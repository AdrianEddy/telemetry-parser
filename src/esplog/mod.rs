// SPDX-License-Identifier: MIT OR Apache-2.0

use std::io::*;
use std::str::from_utf8;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use byteorder::{LittleEndian, ReadBytesExt};

use crate::esplog::mini_decompressor::{decompress_block, State};
use crate::tags_impl::*;
use crate::*;

use self::mini_decompressor::FIX_MULT;

mod mini_decompressor;

#[derive(Default)]
pub struct EspLog {
    pub model: Option<String>,
}

impl EspLog {
    pub fn possible_extensions() -> Vec<&'static str> {
        vec!["bin"]
    }

    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], _filepath: P) -> Option<Self> {
        if buffer.len() > 7 && buffer[0..7] == [0x45, 0x73, 0x70, 0x4c, 0x6f, 0x67, 0x30] {
            return Some(Self { model: None });
        }
        None
    }

    pub fn parse<T: Read + Seek, F: Fn(f64)>(
        &mut self,
        stream: &mut T,
        size: usize,
        progress_cb: F,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<Vec<SampleInfo>> {
        let mut buf = vec![0u8; 8000];

        let mut gyro = Vec::new();
        let mut accel = Vec::new();

        // skip header
        stream.read_exact(&mut buf[..7])?;

        let mut accel_blk_size = 0;
        let mut accel_range = 0i32;
        let mut state = State::new();
        let mut tmp_quats = vec![];
        let mut rates: Vec<[i32; 3]> = vec![];
        let mut accels: Vec<[i16; 3]> = vec![];
        let mut cur_time = 0.0;
        let mut orientation = "xyz".to_string();
        let mut res = || {
            loop {
                if cancel_flag.load(Ordering::Relaxed) {
                    break;
                }
                match stream.read_u8()? {
                    0x01 => {
                        // gyro setup
                        if stream.read_u8()? != 0x01 {
                            return Err(Error::new(ErrorKind::Other, "Unsupported algo revision"));
                        }
                        let blk_size = stream.read_u16::<LittleEndian>()?;
                        tmp_quats.resize(blk_size as usize, [0, 0, 0]);
                    }
                    0x02 => {
                        // delta time
                        let dt = stream.read_u32::<LittleEndian>()?;

                        // generate timestamps for gyro
                        let scale = rates.len() as f64 / dt as f64 * 1e6 * 180.0
                            / std::f64::consts::PI
                            / ((1 << FIX_MULT) as f64);
                        for (i, &q) in rates.iter().enumerate() {
                            gyro.push(TimeVector3 {
                                t: cur_time + dt as f64 * 1e-6 * (i as f64 / rates.len() as f64),
                                x: q[0] as f64 * scale,
                                y: q[1] as f64 * scale,
                                z: q[2] as f64 * scale,
                            });
                        }
                        rates.clear();
                        // generate timestamps for accel
                        let scale = 16.0 / 32767.0;
                        for (i, &a) in accels.iter().enumerate() {
                            accel.push(TimeVector3 {
                                t: cur_time + dt as f64 * 1e-6 * (i as f64 / accels.len() as f64),
                                x: a[0] as f64 * scale,
                                y: a[1] as f64 * scale,
                                z: a[2] as f64 * scale,
                            });
                        }
                        accels.clear();
                        cur_time += dt as f64 * 1e-6;
                    }
                    0x03 => {
                        // gyro data
                        let nread = stream.read(&mut buf)?;
                        if let Some(res) = decompress_block(&state, &buf, &mut tmp_quats) {
                            rates.extend_from_slice(&tmp_quats);
                            state = res.new_state;
                            stream
                                .seek(SeekFrom::Current(res.bytes_eaten as i64 - nread as i64))?;
                        } else {
                            break;
                        }
                    }
                    0x04 => {
                        // accel setup
                        accel_blk_size = stream.read_u8()? as usize;
                        accel_range = 1 << stream.read_u8()?;
                    }
                    0x05 => {
                        // accel data
                        for _ in 0..accel_blk_size {
                            accels.push([
                                -stream.read_i16::<LittleEndian>()?,
                                -stream.read_i16::<LittleEndian>()?,
                                -stream.read_i16::<LittleEndian>()?,
                            ]);
                        }
                    }
                    0x06 => {
                        // time offset
                        let ofs = stream.read_i32::<LittleEndian>()?;
                        cur_time += ofs as f64 * 1e-6;
                    }
                    0x07 => {
                        // imu orientation
                        let mut buf = vec![0; 3];
                        stream.read_exact(&mut buf)?;
                        orientation = from_utf8(&buf).unwrap_or("xyz").to_string();
                    }
                    _ => {
                        break;
                    }
                }
                progress_cb(stream.stream_position()? as f64 / size as f64);
            }
            Ok::<(), Error>(())
        };

        // We want to ignore most errors as the log might theoreticaly
        // be truncated at any point or have random data in the end
        // as a result of power failure and this is completely OK
        let res = res();
        if let Err(e) = res {
            if e.kind() != ErrorKind::UnexpectedEof {
                log::warn!("Unknown error during decode: {}", e);
            }
        }

        // If we could not decode any samples, then this is definitely an error
        if gyro.len() == 0 {
            log::error!("No samples decoded");
            return Err(Error::new(
                ErrorKind::Other,
                "Could not decode any samples in this log file",
            ));
        }

        let first_ts = gyro.first().map(|x| x.t).unwrap_or(0.0);
        let last_ts = gyro.last().map(|x| x.t).unwrap_or(0.0);

        let mut map = GroupedTagMap::new();
        util::insert_tag(
            &mut map,
            tag!(parsed GroupId::Accelerometer, TagId::Data, "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), accel, vec![]),
        );
        util::insert_tag(
            &mut map,
            tag!(parsed GroupId::Gyroscope,     TagId::Data, "Gyroscope data",     Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]),
        );

        util::insert_tag(
            &mut map,
            tag!(parsed GroupId::Accelerometer, TagId::Unit, "Accelerometer unit", String, |v| v.to_string(), "g".into(), Vec::new()),
        );
        util::insert_tag(
            &mut map,
            tag!(parsed GroupId::Gyroscope,     TagId::Unit, "Gyroscope unit",     String, |v| v.to_string(), "deg/s".into(), Vec::new()),
        );

        util::insert_tag(
            &mut map,
            tag!(parsed GroupId::Gyroscope,     TagId::Orientation, "IMU orientation", String, |v| v.to_string(), orientation.clone(), Vec::new()),
        );
        util::insert_tag(
            &mut map,
            tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), orientation.clone(), Vec::new()),
        );

        Ok(vec![SampleInfo {
            timestamp_ms: first_ts,
            duration_ms: last_ts - first_ts,
            tag_map: Some(map),
            ..Default::default()
        }])
    }

    pub fn normalize_imu_orientation(v: String) -> String {
        v
    }

    pub fn camera_type(&self) -> String {
        "EspLog".to_owned()
    }

    pub fn frame_readout_time(&self) -> Option<f64> {
        None
    }
}
