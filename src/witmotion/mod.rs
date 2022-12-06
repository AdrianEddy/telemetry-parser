// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2021 Adrian <adrian.eddy at gmail>

use std::io::*;
use std::sync::{ Arc, atomic::AtomicBool };

use crate::*;
use memchr::memmem;

mod binary;
mod txt;
mod txt2;

#[derive(Default)]
enum Format {
    #[default]
    Binary,
    Txt,
    Txt2
}

#[derive(Default)]
pub struct WitMotion {
    pub model: Option<String>,
    format: Format
}

impl WitMotion {
    pub fn possible_extensions() -> Vec<&'static str> { vec!["txt", "bin"] }

    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], _filepath: P) -> Option<Self> {
        if buffer.len() > 11 && buffer[0..2] == [0x55, 0x50] && buffer[11] == 0x55 {
            return Some(Self { format: Format::Binary, model: None });
        }
        if memmem::find(buffer, b"Time(s)").is_some() && memmem::find(buffer, b"AngleX(deg)").is_some() {
            return Some(Self { format: Format::Txt, model: None });
        }
        if memmem::find(buffer, b"Time").is_some() && memmem::find(buffer, b"AngleX").is_some() && memmem::find(buffer, b"AngleY").is_some() {
            return Some(Self { format: Format::Txt2, model: None });
        }
        None
    }

    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, size: usize, _progress_cb: F, _cancel_flag: Arc<AtomicBool>) -> Result<Vec<SampleInfo>> {
        match self.format {
            Format::Binary => binary::parse(stream, size),
            Format::Txt    => txt::parse(stream, size),
            Format::Txt2   => txt2::parse(stream, size)
        }
    }

    pub fn normalize_imu_orientation(v: String) -> String {
        v
    }

    pub fn camera_type(&self) -> String {
        "WitMotion".to_owned()
    }

    pub fn frame_readout_time(&self) -> Option<f64> {
        None
    }
}
