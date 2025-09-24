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
use std::collections::HashSet;
use util::*;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TagFilter {
    EntireGroup(tags_impl::GroupId),
    EntireTag(tags_impl::TagId),
    SpecificTag(tags_impl::GroupId, tags_impl::TagId),
}

#[derive(Debug, Clone, Default)]
pub struct InputOptions {
    /// When parsing Betaflight Blackbox, ignore all tags which are not gyro or accelerometer
    pub blackbox_gyro_only: bool,
    /// Parse only until the first metadata frame and first gyro sample, to determine if the file has useful data
    pub probe_only: bool,
    /// Only parse tags on this list
    pub tag_whitelist: HashSet<TagFilter>,
    /// Skip tags on this list
    pub tag_blacklist: HashSet<TagFilter>,
    /// If the main file doesn't contain any data, don't look for sidecar files
    pub dont_look_for_sidecar_files: bool,
}

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
                Self::from_stream_with_options(stream, size, filepath, progress_cb, cancel_flag, InputOptions::default())
            }
            pub fn from_stream_with_options<T: Read + Seek, P: AsRef<std::path::Path>, F: Fn(f64)>(stream: &mut T, size: usize, filepath: P, progress_cb: F, cancel_flag: Arc<AtomicBool>, options: InputOptions) -> Result<Input> {
                let read_mb = if size as u64 > 100u64*1024*1024*1024 { // If file is greater than 100 GB, read 500 MB header/footer
                    500
                } else if size as u64 > 60u64*1024*1024*1024 { // If file is greater than 60 GB, read 100 MB header/footer
                    100
                } else if size as u64 > 30u64*1024*1024*1024 { // If file is greater than 30 GB, read 30 MB header/footer
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
                {$(
                    let exts = <$class>::possible_extensions();
                    let mut check = true;
                    if !exts.is_empty() {
                        if let Some(ref ext) = ext {
                            if !exts.contains(&ext.as_str()) { check = false; }
                        }
                    }
                    if check {
                        if let Some(mut x) = <$class>::detect(&buf, &filepath, &options) {
                            return Ok(Input {
                                samples: x.parse(stream, size, progress_cb, cancel_flag, options).ok(),
                                inner: SupportedFormats::$name(x)
                            });
                        }
                    }
                )*}
                // If nothing was detected, check if there's a file with the same name but different extension
                if !options.dont_look_for_sidecar_files {
                    if ext.as_deref() == Some("mp4") || ext.as_deref() == Some("mov") || ext.as_deref() == Some("mkv") {
                        let fs = filesystem::get_base();
                        for try_ext in ["gcsv", "bbl", "bfl", "csv", "GCSV", "BBL", "BFL", "CSV"] {
                            if let Some(gyro_path) = filepath.as_ref().to_str().and_then(|x| filesystem::file_with_extension(x, try_ext)) {
                                if let Ok(mut f) = filesystem::open_file(&fs, &gyro_path) {
                                    return Self::from_stream(&mut f.file, f.size, &gyro_path, progress_cb, cancel_flag);
                                }
                            }
                        }
                    }
                }
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
    GyroflowGcsv     => gyroflow::GyroflowGcsv,
    GyroflowProtobuf => gyroflow::GyroflowProtobuf,
    BlackBox  => blackbox::BlackBox,
    BlackmagicBraw => blackmagic::BlackmagicBraw,
    RedR3d    => red::RedR3d,
    Runcam    => runcam::Runcam,
    WitMotion => witmotion::WitMotion,
    PhoneApps => phone_apps::PhoneApps,
    ArduPilot => ardupilot::ArduPilot,
    Vuze      => vuze::Vuze,
    KanDao    => kandao::KanDao,
    QoocamEgo => kandao::QoocamEgo,
    Camm      => camm::Camm,
    EspLog    => esplog::EspLog,
    Cooke     => cooke::Cooke,
    SenseFlow => senseflow::SenseFlow,
    Freefly   => freefly::Freefly,
}
