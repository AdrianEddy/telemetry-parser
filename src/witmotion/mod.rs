use std::io::*;
use std::sync::{ Arc, atomic::AtomicBool };

use crate::*;
use memchr::memmem;

mod binary;
mod txt;

#[derive(Default)]
pub struct WitMotion {
    pub model: Option<String>,
    txt: bool
}

impl WitMotion {
    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], _filepath: P) -> Option<Self> {
        if buffer.len() > 11 && buffer[0..2] == [0x55, 0x50] && buffer[11] == 0x55 {
            return Some(Self { txt: false, model: None });
        }
        if memmem::find(buffer, b"Time(s)").is_some() && memmem::find(buffer, b"AngleX(deg)").is_some() {
            return Some(Self { txt: true, model: None });
        }
        None
    }

    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, size: usize, _progress_cb: F, _cancel_flag: Arc<AtomicBool>) -> Result<Vec<SampleInfo>> {
        if self.txt {
            txt::parse(stream, size)
        } else {            
            binary::parse(stream, size)
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
