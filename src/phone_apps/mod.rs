// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2021 Adrian <adrian.eddy at gmail>

mod sensor_logger;
mod gyro;
mod gf_recorder;
mod sensor_logger_android;
mod sensor_record;
mod opencamera_sensors;
mod filmit;

use std::io::*;
use std::sync::{ Arc, atomic::AtomicBool };

use crate::*;

#[derive(Default)]
pub struct PhoneApps {
    pub model: Option<String>,
    path: String
}

impl PhoneApps {
    pub fn camera_type(&self) -> String {
        "Mobile app".to_owned()
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

    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], filepath: P, options: &crate::InputOptions) -> Option<Self> {
        let path = filepath.as_ref().to_str().unwrap_or_default().to_owned();
        // let filename = filesystem::get_filename(&filepath);

        if sensor_logger        ::detect(&buffer)        { return Some(Self { model: Some("Sensor Logger"        .to_owned()), path }); }
        if gf_recorder          ::detect(&buffer)        { return Some(Self { model: Some("GF Recorder"          .to_owned()), path }); }
        if gyro                 ::detect(&buffer)        { return Some(Self { model: Some("Gyro"                 .to_owned()), path }); }
        if sensor_logger_android::detect(&buffer)        { return Some(Self { model: Some("Sensor Logger Android".to_owned()), path }); }
        if sensor_record        ::detect(&buffer)        { return Some(Self { model: Some("Sensor Record"        .to_owned()), path }); }
        if opencamera_sensors   ::detect(&buffer, &path, options) { return Some(Self { model: Some("OpenCamera Sensors"   .to_owned()), path }); }
        if filmit               ::detect(&buffer)        { return Some(Self { model: Some("Film it"              .to_owned()), path }); }

        None
    }

    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, size: usize, progress_cb: F, cancel_flag: Arc<AtomicBool>, options: crate::InputOptions) -> Result<Vec<SampleInfo>> {
        match self.model.as_deref() {
            Some("Sensor Logger")           => sensor_logger        ::parse(stream, size, options),
            Some("GF Recorder")             => gf_recorder          ::parse(stream, size, options),
            Some("Gyro")                    => gyro                 ::parse(stream, size, options),
            Some("Sensor Logger Android")   => sensor_logger_android::parse(stream, size, &self.path, options),
            Some("Sensor Record")           => sensor_record        ::parse(stream, size, options),
            Some("OpenCamera Sensors")      => opencamera_sensors   ::parse(stream, size, &self.path, options),
            Some("Film it")                 => filmit               ::parse(stream, size, progress_cb, cancel_flag, options),
            _ => {
                Err(ErrorKind::InvalidInput.into())
            }
        }
    }
}
