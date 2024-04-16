// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2021-2022 Adrian <adrian.eddy at gmail>

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
mod vuze;
mod kandao;
mod camm;
mod esplog;
mod cooke;
mod senseflow;
mod freefly;

pub mod tags_impl;
pub mod util;
pub mod filesystem;

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
                let read_mb = if size as u64 > 30u64*1024*1024*1024 { // If file is greater than 30 GB, read 30 MB header/footer
                    30
                } else if size as u64 > 5u64*1024*1024*1024 { // If file is greater than 5 GB, read 10 MB header/footer
                    10
                } else {
                    4
                };
                let buf = util::read_beginning_and_end(stream, size, read_mb*1024*1024)?;
                if buf.is_empty() {
                    return Err(Error::new(ErrorKind::Other, "File is empty or there was an error trying to load it."));
                }
                let ext = filepath.as_ref().extension().map(|x| x.to_ascii_lowercase().to_string_lossy().to_owned().to_string());
                $(
                    let exts = <$class>::possible_extensions();
                    let mut check = true;
                    if !exts.is_empty() {
                        if let Some(ref ext) = ext {
                            if !exts.contains(&ext.as_str()) { check = false; }
                        }
                    }
                    if check {
                        if let Some(mut x) = <$class>::detect(&buf, &filepath) {
                            return Ok(Input {
                                samples: x.parse(stream, size, progress_cb, cancel_flag).ok(),
                                inner: SupportedFormats::$name(x)
                            });
                        }
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
            pub fn has_accurate_timestamps(&self) -> bool {
                match &self.inner {
                    $(SupportedFormats::$name(x) => x.has_accurate_timestamps(),)*
                }
            }
        }
    };
}

impl_formats! {
    GoPro     => gopro::GoPro,
    Sony      => sony::Sony,
    Dji       => dji::Dji,
    Insta360  => insta360::Insta360,
    Gyroflow  => gyroflow::Gyroflow,
    BlackBox  => blackbox::BlackBox,
    BlackmagicBraw => blackmagic::BlackmagicBraw,
    RedR3d    => red::RedR3d,
    Runcam    => runcam::Runcam,
    WitMotion => witmotion::WitMotion,
    PhoneApps => phone_apps::PhoneApps,
    ArduPilot => ardupilot::ArduPilot,
    Vuze      => vuze::Vuze,
    KanDao    => kandao::KanDao,
    Camm      => camm::Camm,
    EspLog    => esplog::EspLog,
    Cooke     => cooke::Cooke,
    SenseFlow => senseflow::SenseFlow,
    Freefly   => freefly::Freefly,
}
