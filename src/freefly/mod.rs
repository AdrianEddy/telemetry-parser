// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2024 Adrian <adrian.eddy at gmail>

use std::io::*;
use std::sync::{ Arc, atomic::AtomicBool };

use crate::tags_impl::*;
use crate::*;
use crate::gopro::GoPro;
use memchr::memmem;

#[derive(Default)]
pub struct Freefly {
    pub model: Option<String>,
    frame_readout_time: Option<f64>
}

impl Freefly {
    pub fn camera_type(&self) -> String {
        "Freefly".to_owned()
    }
    pub fn has_accurate_timestamps(&self) -> bool {
        true
    }
    pub fn possible_extensions() -> Vec<&'static str> {
        vec!["mov", "mp4"]
    }
    pub fn frame_readout_time(&self) -> Option<f64> {
        self.frame_readout_time
    }
    pub fn normalize_imu_orientation(v: String) -> String {
        v
    }

    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], _filepath: P, _options: &crate::InputOptions) -> Option<Self> {
        if memmem::find(buffer, b"com.freeflysystems.frame-metadata").is_some() {
            Some(Self::default())
        } else {
            None
        }
    }

    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, size: usize, progress_cb: F, cancel_flag: Arc<AtomicBool>, options: crate::InputOptions) -> Result<Vec<SampleInfo>> {
        let mut samples = Vec::new();

        let mut acc_cal  = (0.00024414808, 0.00024414808, 0.00024414808);
        let mut gyro_cal = (0.00053264847, 0.00053264847, 0.00053264847);

        let mut frame_timestamps = Vec::new();
        let mut imu_timestamps   = Vec::new();
        // Each frame's sample and presentation time, and its `FRTS` stamp from the first frame's, for the sensor's rate
        let mut frames: Vec<crate::dji::timing::Frame> = Vec::new();
        let mut video_fps = None;
        // A file cut from recordings has the frames of its segment table's runs of their sources (`SegmentTable::continues`):
        // the stamps of one run are the camera's own, while a later run's were moved back by the cut (its `clock_shift`)
        // to follow the run before by about a frame, to the µs. Each frame's run, by its sample's middle, as the segments
        // present them
        let runs = (!options.probe_only).then(|| crate::segment_table::read(stream, size as u64)).flatten().and_then(|t| t.ok()).map(|table| {
            let mut run = 0;
            let of_segment: Vec<u64> = (0..table.segments.len()).map(|r| { if r > 0 && !table.continues(r) { run += 1; } run }).collect();
            (table, of_segment)
        });

        let cancel_flag2 = cancel_flag.clone();
        util::get_metadata_track_samples(stream, size, true, |mut info: SampleInfo, data: &[u8], file_position: u64, video_md: Option<&VideoMetadata>| {
            if size > 0 {
                progress_cb(file_position as f64 / size as f64);
            }
            if video_fps.is_none() {
                video_fps = video_md.map(|x| x.fps).filter(|fps| *fps > 0.0);
            }

            if data.len() < 16 { return; }

            let offset = memmem::find(data, b"TYPE").unwrap_or(8);

            if let Ok(mut map) = GoPro::parse_metadata(&data[offset..], GroupId::Default, true, &options) {
                let mut gyro = Vec::new();
                let mut accl = Vec::new();

                for v in map.values() {
                    if let Some(v) = v.get_t(TagId::Unknown(u32::from_be_bytes(*b"FRTS"))) as Option<&Vec<Scalar>> {
                        if v.len() == 2 {
                            let Scalar::u64(ts) = v[1] else { continue; };
                            frame_timestamps.push(ts);
                            // A frame of a later run doesn't follow the one before: its sample is numbered as one more
                            // after it, as though the track had left out a frame between them
                            let run = runs.as_ref().map_or(0, |(table, of_segment)| {
                                let middle_us = ((info.timestamp_ms + info.duration_ms.max(0.0) / 2.0) * 1000.0).round() as i64;
                                of_segment[table.segment_of_time_us(middle_us)]
                            });
                            if let Ok(us) = i64::try_from(ts as i128 - frame_timestamps[0] as i128) {
                                frames.push(crate::dji::timing::Frame { sample: info.sample_index + run, ms: info.timestamp_ms, us });
                            }
                        }
                    }
                    if let Some(v) = v.get_t(TagId::Unknown(u32::from_be_bytes(*b"IMTS"))) as Option<&Vec<Scalar>> {
                        if v.len() == 2 {
                            let Scalar::u64(ts) = v[1] else { continue; };
                            imu_timestamps.push(ts);
                        }
                    }
                    if let Some(v) = v.get_t(TagId::Unknown(u32::from_be_bytes(*b"CAAC"))) as Option<&Vec<Vec<f32>>> {
                        if v.len() == 1 && v[0].len() >= 3 {
                            acc_cal = (v[0][0], v[0][1], v[0][2]);
                        }
                    }
                    if let Some(v) = v.get_t(TagId::Unknown(u32::from_be_bytes(*b"CAGY"))) as Option<&Vec<Vec<f32>>> {
                        if v.len() == 1 && v[0].len() >= 3 {
                            gyro_cal = (v[0][0], v[0][1], v[0][2]);
                        }
                    }

                    // The packet's readings, a structure item of one entry per reading: the KLV reads an item of more
                    // entries as their list, and one of a single entry as that entry itself
                    let acgy = || TagId::Unknown(u32::from_be_bytes(*b"ACGY"));
                    let readings = (v.get_t(acgy()) as Option<&Vec<Vec<Scalar>>>).map(Vec::as_slice)
                        .or_else(|| (v.get_t(acgy()) as Option<&Vec<Scalar>>).map(std::slice::from_ref));
                    if let Some(v) = readings {
                        let first_frame_ts = frame_timestamps.first().cloned().unwrap_or(0) as f64;
                        let last_imu_ts    = imu_timestamps  .last() .cloned().unwrap_or(0) as f64;

                        let num_samples = v.len();
                        for (i, sample) in v.into_iter().enumerate() {
                            if sample.len() == 7 {
                                let Scalar::u32(_t) = sample[0] else { continue; };
                                let Scalar::i16(ax) = sample[1] else { continue; };
                                let Scalar::i16(ay) = sample[2] else { continue; };
                                let Scalar::i16(az) = sample[3] else { continue; };
                                let Scalar::i16(gx) = sample[4] else { continue; };
                                let Scalar::i16(gy) = sample[5] else { continue; };
                                let Scalar::i16(gz) = sample[6] else { continue; };

                                let imu_rate = 1000.0; // Hz

                                let t = ((last_imu_ts - first_frame_ts) / 1000_000.0) - (num_samples - i) as f64 / imu_rate;
                                accl.push(TimeVector3 {
                                    t,
                                    x: ax as f64 * -acc_cal.0 as f64,
                                    y: ay as f64 * -acc_cal.1 as f64,
                                    z: az as f64 * -acc_cal.2 as f64,
                                });
                                gyro.push(TimeVector3 {
                                    t,
                                    x: gx as f64 * gyro_cal.0 as f64,
                                    y: gy as f64 * gyro_cal.1 as f64,
                                    z: gz as f64 * gyro_cal.2 as f64,
                                });
                            }
                        }
                    }
                }

                let imu_orientation = "xYz";
                if !gyro.is_empty() {
                    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Data,        "Gyroscope data",  Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]), &options);
                    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Unit,        "Gyroscope unit",  String,              |v| v.to_string(), "rad/s".into(), Vec::new()), &options);
                    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Orientation, "IMU orientation", String,              |v| v.to_string(), imu_orientation.into(), Vec::new()), &options);
                }
                if !accl.is_empty() {
                    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Data,        "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), accl, vec![]), &options);
                    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Unit,        "Accelerometer unit", String,              |v| v.to_string(), "m/s²".into(),  Vec::new()), &options);
                    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation",    String,              |v| v.to_string(), imu_orientation.into(), Vec::new()), &options);
                }

                info.tag_map = Some(map);
                samples.push(info);

                if options.probe_only {
                    cancel_flag2.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
        }, cancel_flag)?;

        // The rate the sensor captured the frames at, which a high-speed recording plays at the video's rate: the rate of
        // the frames' own stamps (`FRTS`, µs on the camera clock the IMU readings are timed on), all of them, as DJI's and
        // XTRA's frames are timed (`dji::timing::sensor_rate`: the slope of a least-squares fit of stamp against frame,
        // over the runs of frames that follow each other in the track, so that neither a frame the track didn't deliver nor
        // a stamp off the clock bends it), runs of a file cut from recordings within those of its sources. A file whose
        // stamps give none (no two frames that follow each other in a run: a cut of one frame, or of single frames of
        // other moments) has the rate of the capture timeline its segment table puts its frames on, when it has one: its
        // segments' frames over their capture time
        let table_rate = runs.as_ref().and_then(|(table, _)| {
            let frames: u64 = table.segments.iter().map(|s| s.frame_count).sum();
            let capture_us: i64 = table.segments.iter().map(|s| s.capture_duration_us).sum();
            (capture_us > 0).then(|| frames as f64 * 1e6 / capture_us as f64)
        });
        if let Some(sensor_fps) = video_fps.and_then(|fps| crate::dji::timing::sensor_rate(&frames, 1000.0 / fps, None)).or(table_rate) {
            let mut map = GroupedTagMap::new();
            util::insert_tag(&mut map, tag!(parsed GroupId::Default, TagId::FrameRate, "Frame rate", f64, |v| format!("{:?}", v), sensor_fps, vec![]), &options);
            samples.insert(0, SampleInfo { tag_map: Some(map), ..Default::default() });
        }

        Ok(samples)
    }
}
