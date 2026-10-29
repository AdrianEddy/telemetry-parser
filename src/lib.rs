// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2021-2022 Adrian <adrian.eddy at gmail>

mod sony;
mod gopro;
pub mod gyroflow;
mod insta360;
mod blackbox;
mod runcam;
mod witmotion;
mod dji;
mod xtra;
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
mod canon;
mod nikon;
mod zcam;
mod motioncam;

pub mod rig;
pub mod segment_table;
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
            pub samples: Option<Vec<SampleInfo>>,
            /// The file's segment table, `Err` with why it can't be used when the file has one that isn't valid for it
            segment_table: Option<segment_table::Found>,
            /// Whether the readings are at the frames of the table's segments (`readings_at_table_frames`)
            readings_at_table_frames: bool,
        }
        impl Input {
            pub fn from_stream<T: Read + Seek, P: AsRef<std::path::Path>, F: Fn(f64)>(stream: &mut T, size: usize, filepath: P, progress_cb: F, cancel_flag: Arc<AtomicBool>) -> Result<Input> {
                Self::from_stream_with_options(stream, size, filepath, progress_cb, cancel_flag, InputOptions::default())
            }
            pub fn from_stream_with_options<T: Read + Seek, P: AsRef<std::path::Path>, F: Fn(f64)>(stream: &mut T, size: usize, filepath: P, progress_cb: F, cancel_flag: Arc<AtomicBool>, options: InputOptions) -> Result<Input> {
                let read_mb = if size as u64 > 100u64*1024*1024*1024 { // If file is greater than 100 GB, read 500 MB header/footer
                    500
                } else if size as u64 > 60u64*1024*1024*1024 { // If file is greater than 60 GB, read 220 MB header/footer
                    220
                } else if size as u64 > 30u64*1024*1024*1024 { // If file is greater than 30 GB, read 180 MB header/footer
                    180
                } else if size as u64 > 5u64*1024*1024*1024 { // If file is greater than 5 GB, read 50 MB header/footer
                    50
                } else {
                    5
                };
                let buf = util::read_beginning_and_end(stream, size, read_mb*1024*1024)?;
                if buf.is_empty() {
                    return Err(Error::new(ErrorKind::Other, "File is empty or there was an error trying to load it."));
                }
                let ext = Some(filesystem::get_extension(filepath.as_ref().to_str().unwrap_or_default()).to_ascii_lowercase()).filter(|x| !x.is_empty());
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
                            let probe_only = options.probe_only;
                            let mut input = Input {
                                samples: x.parse(stream, size, progress_cb, cancel_flag, options).map_err(|e| log::error!("Failed to parse {} as {}: {e}", filepath.as_ref().display(), stringify!($name))).ok(),
                                inner: SupportedFormats::$name(x),
                                segment_table: None,
                                readings_at_table_frames: true,
                            };
                            // A probe reads the start of the data only, which the table can't be checked against
                            if !probe_only {
                                input.segment_table = input.read_segment_table(stream, size);
                                if let Some(Err(e)) = &input.segment_table {
                                    log::warn!("{}: the segment table isn't valid for the file, ignored: {e}", filepath.as_ref().display());
                                }
                            }
                            return Ok(input);
                        }
                    }
                )*}
                // If nothing was detected, check if there's a file with the same name but different extension
                if !options.dont_look_for_sidecar_files {
                    if let Some(path) = filepath.as_ref().to_str() {
                        if ext.as_deref() == Some("mp4") || ext.as_deref() == Some("mov") || ext.as_deref() == Some("mkv") {
                            // jsonl first: it carries the full per-frame protobuf telemetry
                            // (lens profile, timing, IBIS/OIS), so when a clip has both it is
                            // the one the user meant.
                            for try_ext in ["jsonl", "gcsv", "bbl", "bfl", "csv", "JSONL", "GCSV", "BBL", "BFL", "CSV"] {
                                if let Some(gyro_path) = filesystem::file_with_extension(path, try_ext) {
                                    if let Ok(mut f) = filesystem::open_file(&gyro_path) {
                                        let mut input = Self::from_stream(&mut f.file, f.size, &gyro_path, progress_cb, cancel_flag)?;
                                        // The log is the video's telemetry: its segments are the video's (a
                                        // probe reads none, as above)
                                        if !options.probe_only {
                                            input.read_video_segment_table(stream, size, &mut f.file, f.size, &gyro_path);
                                        }
                                        return Ok(input);
                                    }
                                }
                            }
                        }
                        if let Some(gyro_path) = util::find_gcsv_sidecar_for_image_sequence(path, ext.as_deref()) {
                            if let Ok(mut f) = filesystem::open_file(&gyro_path) {
                                return Self::from_stream(&mut f.file, f.size, &gyro_path, progress_cb, cancel_flag);
                            }
                        }
                    }
                }
                return Err(Error::new(ErrorKind::Other, "Unsupported file format"));
            }
            /// The segment table of the file (`segment_table`), when it has one that is valid for it
            pub fn segment_table(&self) -> Option<&segment_table::SegmentTable> {
                self.segment_table.as_ref().and_then(|x| x.as_ref().ok())
            }
            /// The segments of the file's segment table (`segment_table`), when it has one that is valid for it
            pub fn trim_segments(&self) -> Option<&[segment_table::TrimSegment]> {
                self.segment_table().map(|x| x.segments.as_slice())
            }
            /// Why the file's segment table can't be used, when it has one that isn't valid for it
            pub fn unusable_segment_table(&self) -> Option<&str> {
                self.segment_table.as_ref().and_then(|x| x.as_ref().err()).map(String::as_str)
            }
            /// Takes the segment table of the video `video` (`video_size` bytes) whose telemetry this was read as: this
            /// is the telemetry log `log` (`log_size` bytes, at `log_path`), which has no frames, nor a table, of its
            /// own. Its table is then the video's, by the rule of the table's telemetry logs
            /// ([`segment_table::read_for_log`]): with the table's clocks when the table names this log, without them
            /// (and a warning) otherwise
            pub fn read_video_segment_table<V: Read + Seek, L: Read + Seek, P: AsRef<std::path::Path>>(&mut self, video: &mut V, video_size: usize, log: &mut L, log_size: usize, log_path: P) {
                let name = filesystem::get_filename(log_path.as_ref().to_str().unwrap_or_default());
                let table = segment_table::read_for_log(video, video_size as u64, log, log_size as u64, &name);
                self.readings_at_table_frames = table.as_ref().is_none_or(|t| t.names_log);
                self.segment_table = table.map(|t| t.found);
                if let Some(Err(e)) = &self.segment_table {
                    log::warn!("{}: the segment table of the video it's the telemetry of isn't valid for the video, ignored: {e}", log_path.as_ref().display());
                }
            }
            /// Whether the readings are known to be at the frames of the segment table's segments (`segment_table`): a
            /// file's own are, and so are a telemetry log's that the table of the video it was read for names
            /// (`read_video_segment_table`). Another log's aren't: the table's segments are then the video's frames'
            /// only, and where each one's readings are in the log is found by synchronizing it
            pub fn readings_at_table_frames(&self) -> bool {
                self.readings_at_table_frames
            }
            /// The table of the file read: that of the file the parser read the motion and the frames from, which is the
            /// file's sidecar when it was read through one. A RED clip's is in its RMD sidecar, a MotionCam recording's in
            /// the `.mcraw` its video was read with, a Z CAM file's is read by its parser, any other file's in the file
            fn read_segment_table<T: Read + Seek>(&self, stream: &mut T, size: usize) -> Option<segment_table::Found> {
                match &self.inner {
                    SupportedFormats::RedR3d(x) => x.segment_table(),
                    SupportedFormats::MotionCam(x) => x.segment_table(stream, size as u64),
                    // Read before its readings, which it times by (`zcam_imu_origin_us`)
                    SupportedFormats::Zcam(x) => x.segment_table.clone(),
                    _ => segment_table::read(stream, size as u64),
                }
            }
            pub fn parser_name(&self) -> &'static str {
                match &self.inner {
                    $(SupportedFormats::$name(_) => stringify!($name),)*
                }
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
    Canon     => canon::Canon,
    Nikon     => nikon::Nikon,
    Dji       => dji::Dji,
    Xtra      => xtra::Xtra,
    Insta360  => insta360::Insta360,
    GyroflowGcsv     => gyroflow::GyroflowGcsv,
    GyroflowProtobuf => gyroflow::GyroflowProtobuf,
    BlackBox  => blackbox::BlackBox,
    BlackmagicBraw => blackmagic::BlackmagicBraw,
    RedR3d    => red::RedR3d,
    MotionCam => motioncam::MotionCam,
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
    Zcam      => zcam::Zcam,
}
