mod sensor_logger;
mod gyro;
mod gf_recorder;
mod sensor_logger_android;
mod sensor_record;

use std::io::*;

use crate::*;

#[derive(Default)]
pub struct PhoneApps {
    pub model: Option<String>,
}

impl PhoneApps {
    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], filepath: P) -> Option<Self> {
        let filename = filepath.as_ref().file_name().map(|x| x.to_string_lossy()).unwrap_or_default();

        if sensor_logger        ::detect(&buffer, &filename) { return Some(Self { model: Some("Sensor Logger"        .to_owned()) }); }
        if gf_recorder          ::detect(&buffer, &filename) { return Some(Self { model: Some("GF Recorder"          .to_owned()) }); }
        if gyro                 ::detect(&buffer, &filename) { return Some(Self { model: Some("Gyro"                 .to_owned()) }); }
        if sensor_logger_android::detect(&buffer, &filename) { return Some(Self { model: Some("Sensor Logger Android".to_owned()) }); }
        if sensor_record        ::detect(&buffer, &filename) { return Some(Self { model: Some("Sensor Record"        .to_owned()) }); }

        None
    }

    pub fn parse<T: Read + Seek>(&mut self, stream: &mut T, size: usize) -> Result<Vec<SampleInfo>> {
        match self.model.as_deref() {
            Some("Sensor Logger")           => sensor_logger        ::parse(stream, size),
            Some("GF Recorder")             => gf_recorder          ::parse(stream, size),
            Some("Gyro")                    => gyro                 ::parse(stream, size),
            Some("Sensor Logger Android")   => sensor_logger_android::parse(stream, size),
            Some("Sensor Record")           => sensor_record        ::parse(stream, size),
            _ => {
                Err(ErrorKind::InvalidInput.into())
            }
        }
    }

    pub fn normalize_imu_orientation(v: String) -> String {
        v
    }

    pub fn camera_type(&self) -> String {
        "Mobile app".to_owned()
    }
}
