mod sony;
mod gopro;
mod gyroflow;
mod insta360;
mod blackbox;
mod runcam;
mod witmotion;
mod dji;
mod phone_apps;
mod ardupilot;
mod blackmagic;
mod red;

pub mod tags_impl;
pub mod util;

use std::io::*;
use std::sync::{ Arc, atomic::AtomicBool };
use util::*;

macro_rules! impl_formats {
    ($($name:ident => $class:ty,)*) => {
        pub enum SupportedFormats {
            $($name($class),)*
        }
        pub struct Input {
            inner: SupportedFormats,
            pub samples: Option<Vec<SampleInfo>>
        }
        impl Input {
            pub fn from_stream<T: Read + Seek, P: AsRef<std::path::Path>, F: Fn(f64)>(stream: &mut T, size: usize, filepath: P, progress_cb: F, cancel_flag: Arc<AtomicBool>) -> Result<Input> {
                let buf = util::read_beginning_and_end(stream, size, 2*1024*1024)?; // 2 MB
                if buf.is_empty() {
                    return Err(Error::new(ErrorKind::Other, "File is empty or there was an error trying to load it."));
                }
                $(
                    if let Some(mut x) = <$class>::detect(&buf, &filepath) {
                        return Ok(Input {
                            samples: x.parse(stream, size, progress_cb, cancel_flag).ok(),
                            inner: SupportedFormats::$name(x)
                        });
                    }
                )*
                return Err(Error::new(ErrorKind::Other, "Unsupported file format"));
            }
            pub fn camera_type(&self) -> String {
                match &self.inner {
                    $(SupportedFormats::$name(x) => x.camera_type(),)*
                }
            }
            pub fn camera_model(&self) -> Option<&String> {
                match &self.inner {
                    $(SupportedFormats::$name(x) => x.model.as_ref(),)*
                }
            }
            pub fn normalize_imu_orientation(&self, v: String) -> String {
                match &self.inner {
                    $(SupportedFormats::$name(_) => <$class>::normalize_imu_orientation(v),)*
                }
            }
            pub fn frame_readout_time(&self) -> Option<f64> {
                match &self.inner {
                    $(SupportedFormats::$name(x) => x.frame_readout_time(),)*
                }
            }
        }
    };
}

impl_formats! {
    GoPro     => gopro::GoPro,
    Sony      => sony::Sony,
    Gyroflow  => gyroflow::Gyroflow,
    Insta360  => insta360::Insta360,
    BlackBox  => blackbox::BlackBox,
    Runcam    => runcam::Runcam,
    WitMotion => witmotion::WitMotion,
    Dji       => dji::Dji,
    PhoneApps => phone_apps::PhoneApps,
    ArduPilot => ardupilot::ArduPilot,
    BlackmagicBraw => blackmagic::BlackmagicBraw,
    RedR3d    => red::RedR3d,
}
