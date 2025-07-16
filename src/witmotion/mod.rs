// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2021 Adrian <adrian.eddy at gmail>

use std::io::*;
use std::sync::{ Arc, atomic::AtomicBool };

use crate::*;
use memchr::memmem;

mod binary;
mod txt;
mod txt2;
mod txt3;
mod txt4;

#[derive(Default)]
enum Format {
    #[default]
    Binary,
    Txt,
    Txt2,
    Txt3,
    Txt4,
}

#[derive(Default)]
pub struct WitMotion {
    pub model: Option<String>,
    format: Format
}

impl WitMotion {
    pub fn camera_type(&self) -> String {
        "WitMotion".to_owned()
    }
    pub fn has_accurate_timestamps(&self) -> bool {
        false
    }
    pub fn possible_extensions() -> Vec<&'static str> {
        vec!["txt", "bin"]
    }
    pub fn frame_readout_time(&self) -> Option<f64> {
        None
    }
    pub fn normalize_imu_orientation(v: String) -> String {
        v
    }

    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], _filepath: P) -> Option<Self> {
        if buffer.len() > 11 && (buffer[0..2] == [0x55, 0x50] || buffer[0..2] == [0x55, 0x51]) && buffer[11] == 0x55 {
            return Some(Self { format: Format::Binary, model: None });
        }
        if memmem::find(buffer, b"Time(s)").is_some() && memmem::find(buffer, b"AngleX(deg)").is_some() {
            return Some(Self { format: Format::Txt, model: None });
        }
        if memmem::find(buffer, b"Time").is_some() && memmem::find(buffer, b"AngleX").is_some() && memmem::find(buffer, b"AngleY").is_some() {
            return Some(Self { format: Format::Txt2, model: None });
        }
        if memmem::find(buffer, b"time").is_some() && memmem::find(buffer, b"AsX").is_some() && memmem::find(buffer, b"AsY").is_some() {
            return Some(Self { format: Format::Txt3, model: None });
        }
        if memmem::find(buffer, b"Time").is_some() && memmem::find(buffer, b"Angular velocity X").is_some() && memmem::find(buffer, b"Quaternions 0").is_some() {
            return Some(Self { format: Format::Txt4, model: None });
        }
        None
    }

    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, size: usize, _progress_cb: F, _cancel_flag: Arc<AtomicBool>, options: crate::InputOptions) -> Result<Vec<SampleInfo>> {
        match self.format {
            Format::Binary => binary::parse(stream, size, options),
            Format::Txt    => txt::parse(stream, size, options),
            Format::Txt2   => txt2::parse(stream, size, options),
            Format::Txt3   => txt3::parse(stream, size, options),
            Format::Txt4   => txt4::parse(stream, size, options),
        }
    }
}
