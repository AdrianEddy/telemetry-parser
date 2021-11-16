mod sony;
mod gopro;
mod insta360;
mod blackbox;
mod runcam;
mod witmotion;
mod dji;
mod phone_apps;

pub mod tags_impl;
pub mod util;

use std::io::*;
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
            pub fn from_stream<T: Read + Seek>(stream: &mut T, size: usize, filename: &str) -> Result<Input> {
                let buf = util::read_beginning_and_end(stream, size, 2*1024*1024)?; // 2 MB
                $(
                    if let Some(mut x) = <$class>::detect(&buf, filename) {
                        return Ok(Input {
                            samples: x.parse(stream, size).ok(),
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
        }
    };
}

impl_formats! {
    GoPro     => gopro::GoPro,
    Sony      => sony::Sony,
    Insta360  => insta360::Insta360,
    BlackBox  => blackbox::BlackBox,
    Runcam    => runcam::Runcam,
    WitMotion => witmotion::WitMotion,
    Dji       => dji::Dji,
    PhoneApps => phone_apps::PhoneApps,
}
