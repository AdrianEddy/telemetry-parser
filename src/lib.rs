mod sony;
mod gopro;
mod insta360;

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
            pub fn from_stream<T: Read + Seek>(stream: &mut T, size: usize) -> Result<Input> {
                if let Ok(buf) = util::read_beginning_and_end(stream, 1024*1024) { // 1 MB
                    $(
                        if let Some(mut x) = <$class>::detect(&buf) {
                            return Ok(Input {
                                samples: x.parse(stream, size).ok(),
                                inner: SupportedFormats::$name(x)
                            });
                        }
                    )*
                    return Err(Error::new(ErrorKind::Other, "Unsupported file format"));
                }
                Err(Error::new(ErrorKind::Other, "Unable to read the source file"))
            }
            pub fn camera_type(&self) -> String {
                match &self.inner {
                    $(SupportedFormats::$name(_) => stringify!($name).into(),)*
                }
            }
            pub fn camera_model(&self) -> Option<&String> {
                match &self.inner {
                    $(SupportedFormats::$name(x) => x.model.as_ref(),)*
                }
            }
        }
    };
}

impl_formats! {
    GoPro    => gopro::GoPro,
    Sony     => sony::Sony,
    Insta360 => insta360::Insta360,
}
