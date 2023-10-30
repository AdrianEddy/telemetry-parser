// SPDX-License-Identifier: MIT OR Apache-2.0

use std::io::*;
use std::sync::{atomic::AtomicBool, Arc};

use crate::*;
use memchr::memmem;
mod binary;

#[derive(Default)]
enum Format {
    #[default]
    Binary,
}

#[derive(Default)]
pub struct SenseFlow {
    pub model: Option<String>,
    format: Format,
}

impl SenseFlow {
    pub fn camera_type(&self) -> String {
        "SenseFlow".to_owned()
    }
    pub fn has_accurate_timestamps(&self) -> bool {
        false
    }
    pub fn possible_extensions() -> Vec<&'static str> {
        vec!["bin"]
    }
    pub fn frame_readout_time(&self) -> Option<f64> {
        None
    }
    pub fn normalize_imu_orientation(v: String) -> String {
        v
    }

    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], _filepath: P) -> Option<Self> {
        if buffer.len() > 12 && memmem::find(&buffer[0..12], b"SenseFlow").is_some() {
            return Some(Self { format: Format::Binary, model: None });
        }
        None
    }

    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, size: usize, _progress_cb: F, _cancel_flag: Arc<AtomicBool>) -> Result<Vec<SampleInfo>> {
        match self.format {
            Format::Binary => binary::parse(stream, size),
        }
    }
}
