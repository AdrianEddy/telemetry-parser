// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2021 Adrian <adrian.eddy at gmail>

use std::io::*;
use std::sync::{ Arc, atomic::AtomicBool };

mod bin;
mod csv;

use crate::*;
use memchr::memmem;

#[derive(Default)]
pub struct ArduPilot {
    pub model: Option<String>
}

// .bin can be converted to .log using mission planner or https://github.com/ArduPilot/pymavlink/blob/master/tools/mavlogdump.py

impl ArduPilot {
    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], filepath: P) -> Option<Self> {
        if buffer.len() > 4 && buffer[..4] == [0xA3, 0x95, 0x80, 0x80] &&
           memmem::find(&buffer[..256], b"BBnNZ").is_some() &&
           memmem::find(&buffer[..256], b"Type,Length,Name,Format,Columns").is_some() {
            return Some(Self { model: Some(".bin".to_owned()) });
        }

        if !filepath.as_ref().to_str().unwrap_or_default().ends_with(".log") { return None }

        if memmem::find(buffer, b"FMT,").is_some() &&
           memmem::find(buffer, b"PARM,").is_some() &&
           memmem::find(buffer, b"VSTB,").is_some() {
            return Some(Self { model: Some(".log".to_owned()) });
        }
        None
    }

    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, size: usize, progress_cb: F, cancel_flag: Arc<AtomicBool>) -> Result<Vec<SampleInfo>> {
        match self.model.as_deref() {
            Some(".bin") => bin::parse(stream, size, progress_cb, cancel_flag),
            Some(".log") => csv::parse(stream, size, progress_cb, cancel_flag),
            _ => Err(ErrorKind::InvalidData.into())
        }
    }

    pub fn normalize_imu_orientation(v: String) -> String {
        v
    }

    pub fn camera_type(&self) -> String {
        "ArduPilot".to_owned()
    }

    pub fn frame_readout_time(&self) -> Option<f64> {
        None
    }
}
