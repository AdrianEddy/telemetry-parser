// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2022 Adrian <adrian.eddy at gmail>

pub mod dvtm_wm169;
pub mod dvtm_eagle4_wa530;
pub mod dvtm_oq101;
pub(crate) mod rig;
pub use rig::body_masks::DjiMaskProfile;

use std::io::*;
use std::sync::{ Arc, atomic::AtomicBool };

use crate::tags_impl::*;
use crate::*;
use crate::util::insert_tag;
use memchr::memmem;
use prost::Message;
use rig::Schema;

mod csv;
pub(crate) mod timing;
use timing::HeldQuaternions;

#[derive(Clone, PartialEq, ::prost::Message)]
struct HeaderProbe {
    #[prost(message, optional, tag = "1")]
    clip_meta: Option<ClipMetaProbe>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
struct ClipMetaProbe {
    #[prost(message, optional, tag = "1")]
    clip_meta_header: Option<ClipMetaHeaderProbe>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
struct ClipMetaHeaderProbe {
    #[prost(string, tag = "1")]
    proto_file_name: String,
    #[prost(string, tag = "10")]
    product_name: String,
}

/// The video's frame rate as its track's time-to-sample table gives it: its timescale over the frames' duration
/// ([`timing::frame_duration`]), a ratio of integers, such as 60000/1001 for 59.94 fps. A frame the camera dropped doesn't
/// move it: the frame before the gap lasts two frames or more, and the frames' average rate on such a 59.94 fps clip is
/// 56.10 fps. Nor does a timescale that doesn't divide the duration, where the frames last the whole ticks either side of
/// it. The rate the clip metadata states is a single-precision float of the nominal rate, 59.939998626708984 there, 1.03
/// ppm off
pub(crate) fn video_frame_rate(ctx: &mp4parse::MediaContext) -> Option<f64> {
    let track = ctx.tracks.iter().find(|x| x.track_type == mp4parse::TrackType::Video)?;
    let timescale = track.timescale?.0;
    let table: Vec<(u32, u32)> = track.stts.as_ref()?.samples.iter().map(|s| (s.sample_count, s.sample_delta)).collect();
    let duration = timing::frame_duration(&table)?;
    (timescale > 0).then(|| timescale as f64 / duration)
}

#[derive(Default)]
pub struct Dji {
    pub model: Option<String>,
    pub frame_readout_time: Option<f64>
}

/// One 360 packet's IMU readings, held until the whole track is read: their
/// spacing is measured from every packet's first-sample stamp.
struct HeldImu {
    /// Index of the packet's sample in the parsed series.
    sample: usize,
    frame_relative_us: i64,
    frame_us: i64,
    /// The first reading's stamp, the reading count, and each valid fused
    /// quaternion by its reading index.
    attitude: Option<(u32, usize, Vec<(usize, Quaternion<f64>)>)>,
    /// The first reading's stamp, and each reading's gyroscope (rad/s) and
    /// accelerometer (g) axes.
    raw: Option<(u64, Vec<[f32; 6]>)>,
}

/// The axes the Osmo 360 II's raw IMU readings are published in, spelt as
/// `normalized_imu` reads an orientation.
///
/// In a 360 recording they are put in the frame of the fused attitude this
/// parser publishes beside them. That attitude is DJI's integration of these
/// very readings, one quaternion per reading, and `zyx` reproduces its body
/// rate to the float precision of the stored quaternions.
///
/// A single-lens recording carries no fused attitude, and its picture faces
/// the other way. On the corpus clip, `zyx` gives the yaw the picture shows
/// and the opposite pitch, in both directions of travel. A half turn about the
/// vertical axis, `zYX`, makes both agree. Which lens recorded the clip is not
/// stated in its metadata.
fn raw_imu_orientation(three_sixty: bool) -> &'static str {
    if three_sixty { "zyx" } else { "zYX" }
}

impl Dji {
    pub fn camera_type(&self) -> String {
        "DJI".to_owned()
    }
    pub fn has_accurate_timestamps(&self) -> bool {
        true
    }
    pub fn possible_extensions() -> Vec<&'static str> {
        vec!["mp4", "mov", "osv", "csv"]
    }
    pub fn frame_readout_time(&self) -> Option<f64> {
        self.frame_readout_time
    }
    pub fn normalize_imu_orientation(v: String) -> String {
        v
    }

    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], _filepath: P, _options: &crate::InputOptions) -> Option<Self> {
        if memmem::find(buffer, b"djmd").is_some() && (memmem::find(buffer, b"DJI meta").is_some() || memmem::find(buffer, b"CAM meta").is_some()) {
            Some(Self {
                model: None,
                frame_readout_time: None
            })
        } else if memmem::find(buffer, b"Clock:Tick").is_some() && memmem::find(buffer, b"IMU_ATTI(0):gyroX").is_some() {
            Some(Self {
                model: Some("CSV flight log".into()),
                frame_readout_time: None
            })
        } else {
            None
        }
    }

    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, size: usize, progress_cb: F, cancel_flag: Arc<AtomicBool>, options: crate::InputOptions) -> Result<Vec<SampleInfo>> {
        if self.model.is_some() {
            return csv::parse(stream, size, options);
        }

        let mut samples = Vec::new();
        // Every frame's sample, its time in the video and its camera timestamp from the first one's: the frames are timed at
        // the rate of this clock
        let mut frames: Vec<timing::Frame> = Vec::new();
        // The first frame's camera timestamp and its time in the video, which the frames are timed from
        let mut first_timestamp: Option<(i64, f64)> = None;

        let mut focal_length = None;
        let mut distortion_coeffs = None;
        let mut fps = 59.94;
        let mut stated_sensor_fps = None;
        // Only the first sample of the track has the clip metadata
        let mut clip_meta_read = false;
        let mut sample_rate = 2000.0;
        // let mut global_quat_i = 0;

        // let mut first_vsync = 0;
        let mut prev_quat: Option<Quaternion<f64>> = None;
        let mut inv = false;

        let mut which_proto: Option<Schema> = None;
        let mut product_name = String::new();
        let mut rig_pairs: Vec<rig::Pair> = Vec::new();
        let mut extri_lens_mode = 0i32;
        let mut pano_readout_ms = None;
        let mut held_imu: Vec<HeldImu> = Vec::new();
        let mut held_quaternions: Vec<HeldQuaternions> = Vec::new();

        let cancel_flag2 = cancel_flag.clone();

        macro_rules! handle_parsed {
            ($info:ident, $parsed:expr, $imu:ident => $attitude:expr, $raw:expr) => {
                let mut tag_map = GroupedTagMap::new();

                if let Some(ref clip) = $parsed.clip_meta {
                    clip_meta_read = true;
                    self.model              = clip.clip_meta_header       .as_ref().map(|h| h.product_name.replace("DJI ", ""));
                    self.frame_readout_time = pano_readout_ms.or_else(|| clip.sensor_readout_time.as_ref().map(|h| h.readout_time as f64 / 1000_000.0));
                    focal_length            = clip.digital_focal_length   .as_ref().map(|h| h.focal_length as f64);
                    distortion_coeffs       = clip.distortion_coefficients.as_ref().map(|h| h.coeffients.clone());

                    if let Some(v) = clip.sensor_fps.as_ref().map(|h| h.sensor_frame_rate as f64) {
                        stated_sensor_fps = Some(v);
                    }
                    if let Some(v) = clip.imu_sampling_rate.as_ref().map(|h| h.imu_sampling_rate as f64) {
                        sample_rate = v;
                    }

                    let v = serde_json::to_value(&clip).map_err(|_| Error::new(ErrorKind::Other, "Serialize error"));
                    if let Ok(vv) = v {
                        log::debug!("Metadata: {:?}", &vv);
                        insert_tag(&mut tag_map, tag!(parsed GroupId::Default, TagId::Metadata, "Metadata", Json, |v| serde_json::to_string(v).unwrap(), vv, vec![]), &options);
                    }
                    if let Some(ref stream) = $parsed.stream_meta {
                        if let Some(ref meta) = stream.video_stream_meta {
                            fps = meta.framerate as f64;
                        }
                    }
                }

                let three_sixty = matches!(which_proto, Some(Schema::Oq101 | Schema::Wa530));

                if let Some(ref frame) = $parsed.frame_meta {
                    let frame_ts = frame.frame_meta_header.as_ref().unwrap().frame_timestamp as i64;
                    // The frames are timed from the first one, whichever entry of the track holds it, which is at its own
                    // time in the video: after a first entry the track didn't deliver, the frames aren't a frame early
                    let (first_frame_ts, _) = *first_timestamp.get_or_insert((frame_ts, $info.timestamp_ms));
                    let frame_relative_ts = frame_ts - first_frame_ts;
                    frames.push(timing::Frame { sample: $info.sample_index, ms: $info.timestamp_ms, us: frame_relative_ts });

                    let mut held_attitude = None;
                    let mut held_raw = None;
                    if let Some(ref $imu) = frame.imu_frame_meta {
                        if let Some(attitude) = $attitude {
                            // let ts = attitude.timestamp as i64;
                            // println!("{} {} {} {}, vsync: {}", frame_ts, ts, frame_relative_ts, ts - frame_ts, attitude.vsync);
                            let len = attitude.attitude.len() as f64;

                            // if first_vsync == 0 {
                            //     first_vsync = attitude.vsync;
                            // }

                            // let frame_timestamp = (attitude.vsync - first_vsync) as f64 * vsync_duration;
                            // println!("fps: {fps}, sensor_fps: {sensor_fps}, ratio: {fps_ratio}, exp: {exposure_time}, ts: {frame_timestamp}, diff: {}", frame_timestamp);
                            // println!("vsync: {}, ts: {:.3}, ts2: {:.3}, diff: {:.3}", (attitude.vsync - first_vsync), frame_timestamp * 1000.0, (frame_relative_ts as f64 / ratio), (frame_relative_ts as f64 / ratio) - (frame_timestamp * 1000.0));

                            // let frame_ratio = self.frame_readout_time.unwrap() / vsync_duration;

                            // let offset_ms = (1000.0 / sample_rate) * attitude.offset as f64;

                            let mut held = Vec::new();
                            let mut readings = Vec::new();
                            for (i, q) in attitude.attitude.iter().enumerate() {
                                let index = i as f64 - attitude.offset as f64;
                                // The reading's time in the frame, in frames. The readings are timed once the whole
                                // track is read (`HeldQuaternions`; a 360 packet's: `HeldImu`)
                                let in_frame = index / len;

                                /*let ts = match std::env::var("OFFSET_METHOD").as_deref() {
                                    Ok("1.3.0") => {
                                        quat_ts - (exposure_time / 2.0)
                                    },
                                    Ok("no-exp") => {
                                        quat_ts
                                    },
                                    Ok("global-quat-index") => {
                                        (global_quat_i as f64 - attitude.offset as f64) * (1000.0 / sample_rate)
                                    },
                                    Ok("global-quat-index-with-readout-time") => {
                                        (global_quat_i as f64 - attitude.offset as f64) * (1000.0 / sample_rate) - (self.frame_readout_time.unwrap() / 2.0)
                                    },
                                    Ok("with-readout-time") => {
                                        quat_ts - (self.frame_readout_time.unwrap() / 2.0)
                                    },
                                    // Default, if no env var
                                    _ => {
                                        quat_ts - exposure_time
                                    }
                                };*/

                                // let ts = (quat_ts1 - exposure_time) / fps_ratio;
                                // println!("ts: {:.2}, diff: {:.4}, vsync: {}, frame_timestamp: {}, fts: {frame_timestamp}, fts2: {frame_timestamp2}", ts, ts - prev_ts, attitude.vsync, frame_ts);

                                // global_quat_i += 1;

                                if q.quaternion_w.is_nan() || q.quaternion_x.is_nan() || q.quaternion_y.is_nan() || q.quaternion_z.is_nan() {
                                    continue;
                                }

                                let quat = util::multiply_quats(
                                    (q.quaternion_w as f64,
                                    q.quaternion_x as f64,
                                    q.quaternion_y as f64,
                                    q.quaternion_z as f64),
                                    (0.5, -0.5, -0.5, 0.5),
                                );
                                // Rotate Y axis 180 deg for horizon lock
                                let quat = util::multiply_quats((0.0, 0.0, 1.0, 0.0), (quat.w, quat.x, quat.y, quat.z));

                                if quat.w == 0.0 && quat.x == 0.0 && quat.y == 0.0 && quat.z == 0.0 {
                                    continue;
                                }

                                if prev_quat.is_some() && (prev_quat.unwrap() - quat).norm_squared().sqrt() > 1.5 {
                                    inv = !inv;
                                }
                                prev_quat = Some(quat.clone());

                                let v = if inv { -quat } else { quat };
                                if three_sixty {
                                    held.push((i, v));
                                } else {
                                    readings.push((in_frame, v));
                                }
                            }

                            if three_sixty {
                                // A packet with no readings has no first reading to stamp.
                                held_attitude = (!attitude.attitude.is_empty()).then(|| (attitude.timestamp, attitude.attitude.len(), held));
                            } else {
                                held_quaternions.push(HeldQuaternions { sample: samples.len(), frame_relative_us: frame_relative_ts, readings });
                            }
                        }
                        held_raw = $raw;
                    }
                    if held_attitude.is_some() || held_raw.is_some() {
                        held_imu.push(HeldImu { sample: samples.len(), frame_relative_us: frame_relative_ts, frame_us: frame_ts, attitude: held_attitude, raw: held_raw });
                    }
                }

                // if info.index == 0 { dbg!(&parsed); }

                $info.tag_map = Some(tag_map);

                samples.push($info);

                if options.probe_only {
                    cancel_flag2.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            };
        }

        macro_rules! handle_pano {
            ($parsed:expr) => {
                if let Some(ref clip) = $parsed.clip_meta {
                    // DJI's 360 EIS uses LRO (ns per sensor line), not the
                    // generic sensor_readout_time field. On Osmo 360 the
                    // two give 18.30 ms and 24.02 ms respectively.
                    pano_readout_ms = clip.lro_value.as_ref().zip(clip.sensor_res.as_ref())
                        .and_then(|(lro, sensor)| timing::line_readout_ms(lro.lro_value, sensor.sensor_height));
                }
                if rig_pairs.is_empty() && let Some(ref stream) = $parsed.stream_meta {
                    if let Some(ref pano) = stream.pano_dewarp_params {
                        rig_pairs = rig::pano_pairs!(pano);
                    }
                    extri_lens_mode = stream.extri_lens_mode.as_ref().map_or(0, |m| m.mode);
                }
            };
        }

        // A sample in the clip's product schema
        macro_rules! handle_sample {
            ($info:ident, $data:expr) => {
                match which_proto {
                    None => { },
                    Some(Schema::Wm169) => match dvtm_wm169::ProductMeta::decode($data) {
                        Ok(parsed) => { handle_parsed!($info, parsed, imu => imu.imu_attitude_after_fusion.as_ref(), None); },
                        Err(e) => { log::warn!("Failed to parse protobuf: {:?}", e); }
                    },
                    Some(Schema::Wa530) => match dvtm_eagle4_wa530::ProductMeta::decode($data) {
                        Ok(parsed) => {
                            handle_pano!(parsed);
                            handle_parsed!($info, parsed, imu => imu.imu_single_attitude_after_fusion.as_ref()
                                .or_else(|| imu.imu_attitude_after_fusion.as_ref().and_then(|m| m.current_frame.as_ref())),
                                imu.imu_raw_attitude.as_ref().filter(|r| !r.imu_raw_data.is_empty()).map(|r| (r.timestamp, r.imu_raw_data.iter()
                                    .map(|s| [s.gyro_x, s.gyro_y, s.gyro_z, s.acc_x, s.acc_y, s.acc_z]).collect())));
                        },
                        Err(e) => { log::warn!("Failed to parse protobuf: {:?}", e); }
                    },
                    Some(Schema::Oq101) => match dvtm_oq101::ProductMeta::decode($data) {
                        Ok(parsed) => { handle_pano!(parsed); handle_parsed!($info, parsed, imu => imu.imu_attitude_after_fusion.as_ref().and_then(|m| m.current_frame.as_ref()), None); },
                        Err(e) => { log::warn!("Failed to parse protobuf: {:?}", e); }
                    },
                }
            };
        }

        let ctx = util::get_metadata_track_samples(stream, size, true, |mut info: SampleInfo, data: &[u8], file_position: u64, _video_md: Option<&VideoMetadata>| {
            if size > 0 {
                progress_cb(file_position as f64 / size as f64);
            }

            if which_proto.is_none() {
                let header = HeaderProbe::decode(data).ok()
                    .and_then(|h| h.clip_meta?.clip_meta_header)
                    .unwrap_or_default();
                product_name = header.product_name.clone();
                let schema = Schema::detect(&header.product_name, &header.proto_file_name);
                log::debug!("Product {:?} declares {:?}, parsing with {schema:?}", header.product_name, header.proto_file_name);
                which_proto = Some(schema);
            }
            handle_sample!(info, data);
        }, cancel_flag)?;

        // The frames are timed by the camera's frame clock, the rate of their own timestamps, and the video's exact frame
        // rate. The stated rates are single-precision floats, of the nominal video rate and of a sensor rate the camera
        // measured or took as nominal (`timing::sensor_rate`)
        let video_fps = video_frame_rate(&ctx).unwrap_or(fps);
        let sensor_fps = timing::sensor_rate(&frames, 1000.0 / video_fps, stated_sensor_fps).unwrap_or(video_fps);
        log::debug!("DJI: timed at a sensor frame rate of {sensor_fps:.6} fps and a video frame rate of {video_fps:.6} fps (stated {fps:.6})");
        let fps_ratio = video_fps / sensor_fps;
        if let Some(ref mut v) = self.frame_readout_time {
            *v /= fps_ratio;
        }
        if !clip_meta_read && which_proto.is_some() {
            if !samples.is_empty() {
                log::warn!("DJI: the clip metadata, which only the first sample of the track has, is missing (the sample is empty or damaged): there's no lens profile, sensor readout time or camera model");
            } else {
                // The product schema is chosen by the product the clip metadata names
                let schema = if product_name.is_empty() { ": without the product it names, they're read in the schema of DJI's single-lens cameras, and a 360 camera writes another" } else { "" };
                log::warn!("DJI: the clip metadata, which only the first sample of the track has, is missing (the sample is empty or damaged), and none of the frames could be read{schema}");
            }
        }

        // The single-lens frames are timed from the first frame's time in the video
        let first_frame_ms = first_timestamp.map_or(0.0, |(_, ms)| ms);
        for (k, h) in held_quaternions.into_iter().enumerate() {
            let Some(tag_map) = samples.get_mut(h.sample).and_then(|s| s.tag_map.as_mut()) else { continue };
            let quats = h.timed(first_frame_ms, sensor_fps, video_fps);
            if k == 0 { log::debug!("Quaternions: {:?}", quats); }
            util::insert_tag(tag_map, tag!(parsed GroupId::Quaternion, TagId::Data, "Quaternion data",  Vec_TimeQuaternion_f64, |v| format!("{:?}", v), quats, Vec::new()), &options);
        }

        // A 360 packet stamps its first IMU reading on the frame's microsecond
        // clock; the flat-camera `offset` convention puts these samples about
        // 25 ms early on an Osmo 360. Each series is placed on the reading grid
        // its own stamps fit (`timing::reading_grid`).
        if !held_imu.is_empty() {
            let anchor = |h: &HeldImu, stamp: u32| timing::anchor_us(h.frame_relative_us, h.frame_us, stamp);
            let attitude: Vec<_> = held_imu.iter().filter_map(|h| h.attitude.as_ref().map(|a| (anchor(h, a.0), a.1))).collect();
            let raw: Vec<_> = held_imu.iter().filter_map(|h| h.raw.as_ref().map(|r| (anchor(h, r.0 as u32), r.1.len()))).collect();
            // One packet cannot be fitted; it is spaced by the stated rate.
            let grid = |packets: Vec<(i64, usize)>| timing::reading_grid(&packets)
                .unwrap_or_else(|| packets.iter().map(|p| (p.0 as f64, 1e6 / sample_rate)).collect())
                .into_iter();
            let mut attitude_grid = grid(attitude);
            let mut raw_grid = grid(raw);
            let orientation = raw_imu_orientation(!rig_pairs.is_empty());

            // The readings are timed from the first frame's time in the video, as the frames are
            for h in held_imu {
                let Some(tag_map) = samples.get_mut(h.sample).and_then(|s| s.tag_map.as_mut()) else { continue };
                let on = |(first, spacing): (f64, f64), i: usize| first_frame_ms + (first + i as f64 * spacing) / 1000.0 / fps_ratio;
                if let Some((_, _, readings)) = h.attitude && let Some(g) = attitude_grid.next() {
                    let quats: Vec<_> = readings.into_iter().map(|(i, v)| TimeQuaternion { t: on(g, i), v }).collect();
                    insert_tag(tag_map, tag!(parsed GroupId::Quaternion, TagId::Data, "Quaternion data", Vec_TimeQuaternion_f64, |v| format!("{:?}", v), quats, vec![]), &options);
                }
                if let Some((_, readings)) = h.raw && let Some(g) = raw_grid.next() {
                    // An IMU vector is timed in seconds.
                    let at = |i| on(g, i) / 1000.0;
                    let axes = |i: usize, r: &[f32; 6], o: usize| TimeVector3 { t: at(i), x: r[o] as f64, y: r[o + 1] as f64, z: r[o + 2] as f64 };
                    let gyro: Vec<_> = readings.iter().enumerate().map(|(i, r)| axes(i, r, 0)).collect();
                    let accl: Vec<_> = readings.iter().enumerate().map(|(i, r)| axes(i, r, 3)).collect();
                    insert_tag(tag_map, tag!(parsed GroupId::Gyroscope,     TagId::Data,        "Gyroscope data",     Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]), &options);
                    insert_tag(tag_map, tag!(parsed GroupId::Gyroscope,     TagId::Unit,        "Gyroscope unit",     String, |v| v.to_string(), "rad/s".into(), Vec::new()), &options);
                    insert_tag(tag_map, tag!(parsed GroupId::Gyroscope,     TagId::Orientation, "IMU orientation",    String, |v| v.to_string(), orientation.into(), Vec::new()), &options);
                    insert_tag(tag_map, tag!(parsed GroupId::Accelerometer, TagId::Data,        "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), accl, vec![]), &options);
                    insert_tag(tag_map, tag!(parsed GroupId::Accelerometer, TagId::Unit,        "Accelerometer unit", String, |v| v.to_string(), "g".into(), Vec::new()), &options);
                    insert_tag(tag_map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation",    String, |v| v.to_string(), orientation.into(), Vec::new()), &options);
                }
            }
        }

        let video_dim = ctx.tracks.iter()
            .filter(|x| x.track_type == mp4parse::TrackType::Video)
            .filter_map(|x| x.tkhd.as_ref())
            .map(|tkhd| (tkhd.width >> 16, tkhd.height >> 16))
            .next();

        match (samples.first_mut(), focal_length, distortion_coeffs) {
            (Some(sample), Some(focal_length), Some(coeffs)) if coeffs.len() >= 4 => {
                if let Some((w, h)) = video_dim {
                    let profile = self.get_lens_profile(w, h, focal_length, &coeffs);
                    if let Some(ref mut tag_map) = sample.tag_map {
                        insert_tag(tag_map, tag!(parsed GroupId::Lens, TagId::Data, "Lens profile", Json, |v| serde_json::to_string(v).unwrap(), profile, vec![]), &options);
                    }
                }
            },
            _ => { }
        }

        if !rig_pairs.is_empty()
            && let Some(sample) = samples.first_mut()
            && let Some(ref mut tag_map) = sample.tag_map
            && let Some(camera_rig) =
                rig::camera_rig(&product_name, &rig_pairs, extri_lens_mode, video_dim, crate::rig::readout(self.frame_readout_time))
        {
            insert_tag(tag_map, tag!(parsed GroupId::Lens, TagId::Rig, "Camera rig", CameraRig, |v: &crate::rig::CameraRig| serde_json::to_string(v).unwrap_or_default(), camera_rig, vec![]), &options);
        }

        Ok(samples)
    }

    fn get_lens_profile(&self, width: u32, height: u32, focal_length: f64, coeffs: &[f32]) -> serde_json::Value {
        let model = self.model.clone().unwrap_or_default();
        let half_width = width as f64 / 2.0;
        let half_height = height as f64 / 2.0;
        let output_size = Self::get_output_size(width, height);
        serde_json::json!({
            "calibrated_by": "DJI",
            "camera_brand": "DJI",
            "camera_model": model,
            "calib_dimension":  { "w": width, "h": height },
            "orig_dimension":   { "w": width, "h": height },
            "output_dimension": { "w": output_size.0, "h": output_size.1 },
            "frame_readout_time": self.frame_readout_time,
            "official": true,
            "fisheye_params": {
              "camera_matrix": [
                [ focal_length, 0.0, half_width ],
                [ 0.0, focal_length, half_height ],
                [ 0.0, 0.0, 1.0 ]
              ],
              "distortion_coeffs": coeffs
            },
            "sync_settings": {
              "initial_offset": 0,
              "initial_offset_inv": false,
              "search_size": 0.5,
              "max_sync_points": 5,
              "every_nth_frame": 1,
              "time_per_syncpoint": 0.6,
              "do_autosync": false
            },
            "calibrator_version": "---"
        })
    }

    fn get_output_size(width: u32, height: u32) -> (u32, u32) {
        let aspect = (width as f64 / height as f64 * 100.0) as u32;
        match aspect {
            133 => (width, (width as f64 / 1.7777777777777).round() as u32), // 4:3 -> 16:9
            _   => (width, height)
        }
    }
}
