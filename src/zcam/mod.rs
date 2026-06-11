// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2026 Adrian <adrian.eddy at gmail>

pub mod imvtmeta;

use std::io::*;
use std::sync::{ Arc, atomic::AtomicBool };

use memchr::memmem;
use prost::Message;

use crate::tags_impl::*;
use crate::*;
use crate::util::insert_tag;

use imvtmeta::*;
use imvtmeta::image_sensor_info::ReadOutDirection;

#[derive(Default)]
pub struct Zcam {
    pub model: Option<String>,
    pub lens: Option<String>,
    frame_readout_time: Option<f64>,

    // ----- ClipMeta-derived state (set on first sample carrying ClipMeta) -----
    clip_meta_seen: bool,
    serial: Option<String>,
    firmware: Option<String>,
    sensor_pixel_width: u32,
    sensor_pixel_height: u32,
    pixel_size_nm: u32,
    sensor_readout_us: f64,
    sensor_fps: f64,
    crop_x: u32,
    crop_y: u32,
    crop_w: u32,
    crop_h: u32,
    readout_direction_enum: ReadOutDirection,

    imu_sample_rate_hz: f64,
    gyro_scale: f64,
    accel_scale: f64,
    mag_scale: f64,
    imu_time_offset_us: f64,
    imu_orient_str: Option<String>,
    imu_orient_matrix: Option<[f32; 9]>,

    // ----- StreamMeta-derived state -----
    record_fps: f64,
    video_rotation_deg: u32,
    video_w: u32,
    video_h: u32,

    // ----- Lens (clip-level) -----
    lens_name: Option<String>,
    image_circle_diameter_mm: f32,

    first_focal_length_mm: Option<f32>,

    // ----- Per-clip timing reference -----
    first_frame_ts_us: Option<u64>,
    pts_offset_us: Option<f64>,
    next_imu_emit_us: Option<f64>,

    lens_profile_emitted: bool,
}

impl Zcam {
    pub fn camera_type(&self) -> String {
        "Z CAM".to_owned()
    }
    pub fn has_accurate_timestamps(&self) -> bool {
        true
    }
    pub fn possible_extensions() -> Vec<&'static str> {
        vec!["mp4", "mov"]
    }
    pub fn frame_readout_time(&self) -> Option<f64> {
        self.frame_readout_time
    }
    pub fn normalize_imu_orientation(v: String) -> String {
        v
    }

    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], _filepath: P, _options: &crate::InputOptions) -> Option<Self> {
        if memmem::find(buffer, b"imvt_cam.proto").is_some()  || memmem::find(buffer, b"imvt_lib.proto").is_some() || memmem::find(buffer, b"imvtmeta").is_some() {
            Some(Self::default())
        } else {
            None
        }
    }

    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, size: usize, progress_cb: F, cancel_flag: Arc<AtomicBool>, options: crate::InputOptions) -> Result<Vec<SampleInfo>> {
        let mut samples = Vec::new();
        let cancel_flag2 = cancel_flag.clone();

        util::get_metadata_track_samples(stream, size, true, |mut info: SampleInfo, data: &[u8], file_position: u64, _video_md: Option<&VideoMetadata>| {
            if size > 0 {
                progress_cb(file_position as f64 / size as f64);
            }
            let parsed = match ProductMeta::decode(data) {
                Ok(p) => p,
                Err(e) => {
                    log::warn!("zcam: failed to parse ProductMeta: {e:?} | head={}", crate::util::to_hex(&data[..data.len().min(64)]));
                    return;
                }
            };

            let mut tag_map = GroupedTagMap::new();

            if let Some(ref clip) = parsed.clip_meta {
                self.process_clip_meta(clip, &mut tag_map, &options);
            }
            if let Some(ref stream) = parsed.stream_meta {
                self.process_stream_meta(stream, &mut tag_map, &options);
            }
            if let Some(ref frame) = parsed.frame_meta {
                if let Some(ref h) = frame.frame_meta_header {
                    if self.first_frame_ts_us.is_none() {
                        self.first_frame_ts_us = Some(h.frame_timestamp);
                    }
                }
                self.process_frame_meta(frame, &info, &mut tag_map, &options);
            }

            // Build the lens-profile JSON once, after BOTH:
            //   * header is in (clip_meta_seen, video_w/h, pixel_size_nm), AND
            //   * we have seen a per-frame FocalLength (so the camera_matrix
            //     can carry a real pixel focal length — fx=fy=1.0 would make
            //     stabilization treat the FOV as ~180° and output won't shift).
            if !self.lens_profile_emitted && self.clip_meta_seen && self.video_w > 0 && self.video_h > 0 && self.first_focal_length_mm.is_some() && self.pixel_size_nm > 0 {
                if let Some(profile) = self.build_lens_profile_json() {
                    insert_tag(&mut tag_map, tag!(parsed GroupId::Lens, TagId::Data, "Lens profile", Json, |v| serde_json::to_string(v).unwrap_or_default(), profile, vec![]), &options);
                    self.lens_profile_emitted = true;
                }
            }

            info.tag_map = Some(tag_map);
            samples.push(info);

            if options.probe_only {
                cancel_flag2.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }, cancel_flag)?;

        Ok(samples)
    }

    fn process_clip_meta(&mut self, clip: &ClipMeta, tag_map: &mut GroupedTagMap, options: &crate::InputOptions) {
        self.clip_meta_seen = true;

        if let Some(ref h) = clip.clip_meta_header {
            if !h.product_name.is_empty() {
                let cleaned = h.product_name
                    .strip_prefix("Z CAM ").unwrap_or(&h.product_name)
                    .strip_prefix("Z-CAM ").unwrap_or(&h.product_name)
                    .strip_prefix("ZCAM ").unwrap_or(&h.product_name);
                self.model = Some(cleaned.to_string());
            }
            if !h.product_serial_number.is_empty() {
                self.serial = Some(h.product_serial_number.clone());
                insert_tag(tag_map, tag!(parsed GroupId::Default, TagId::SerialNumber, "Camera serial number", String, |v| v.to_string(), h.product_serial_number.clone(), vec![]), options);
            }
            if !h.product_firmware_version.is_empty() {
                self.firmware = Some(h.product_firmware_version.clone());
            }
        }

        if let Some(ref s) = clip.image_sensor_info {
            self.sensor_pixel_width  = s.sensor_pixel_width;
            self.sensor_pixel_height = s.sensor_pixel_height;
            self.pixel_size_nm       = s.pixel_size_nm;
            self.sensor_readout_us   = s.read_out_time as f64;
            self.sensor_fps          = s.frame_rate as f64;
            self.readout_direction_enum = ReadOutDirection::try_from(s.read_out_direction).unwrap_or(ReadOutDirection::TopToBottom);
            self.crop_x = s.crop_x.unwrap_or(0);
            self.crop_y = s.crop_y.unwrap_or(0);
            self.crop_w = s.crop_width .unwrap_or(self.sensor_pixel_width);
            self.crop_h = s.crop_height.unwrap_or(self.sensor_pixel_height);

            if self.sensor_readout_us > 0.0 {
                self.frame_readout_time = Some(self.sensor_readout_us / 1000.0);
            }
        }

        if let Some(ref l) = clip.lens_info {
            if !l.lens_name.is_empty() {
                self.lens_name = Some(l.lens_name.clone());
                self.lens = Some(l.lens_name.clone());
                insert_tag(tag_map, tag!(parsed GroupId::Lens, TagId::DisplayName, "Lens name", String, |v| v.to_string(), l.lens_name.clone(), vec![]), options);
            }
            self.image_circle_diameter_mm = l.image_circle_diameter;
        }

        if let Some(ref imu) = clip.imu_info {
            self.imu_sample_rate_hz = imu.sampling_rate as f64;
            self.gyro_scale  = imu.gyro_scale  as f64;
            self.accel_scale = imu.accel_scale as f64;
            self.mag_scale   = imu.mag_scale   as f64;
            self.imu_time_offset_us = imu.time_offset as f64;
            let mat_str = if imu.orientation_matrix.len() == 9 {
                format!("[[{:.3},{:.3},{:.3}],[{:.3},{:.3},{:.3}],[{:.3},{:.3},{:.3}]]",
                    imu.orientation_matrix[0], imu.orientation_matrix[1], imu.orientation_matrix[2],
                    imu.orientation_matrix[3], imu.orientation_matrix[4], imu.orientation_matrix[5],
                    imu.orientation_matrix[6], imu.orientation_matrix[7], imu.orientation_matrix[8])
            } else {
                format!("(len={})", imu.orientation_matrix.len())
            };
            log::info!(
                "zcam: ImuInfo parsed — sampling_rate={} Hz, gyro_scale={} (deg/s/LSB), accel_scale={} (m/s²/LSB), time_offset={} µs, orientation_matrix={}",
                self.imu_sample_rate_hz, self.gyro_scale, self.accel_scale, self.imu_time_offset_us, mat_str
            );
            if self.gyro_scale == 0.0 {
                log::warn!("zcam: ImuInfo.gyro_scale is 0 — gyro samples will be all zero after scaling, integrator will produce no rotation.");
            }
            if self.accel_scale == 0.0 {
                log::warn!("zcam: ImuInfo.accel_scale is 0 — accel samples will be all zero after scaling.");
            }

            // ----- orientation_matrix → IMU orientation -----
            self.imu_orient_str = None;
            self.imu_orient_matrix = None;
            if imu.orientation_matrix.len() == 9 {
                let mut mfw = [0f32; 9];
                mfw.copy_from_slice(&imu.orientation_matrix[..9]);
                match resolve_imu_orientation(&mfw) {
                    Ok(s) => {
                        self.imu_orient_str = Some(s);
                    }
                    Err(m) => {
                        log::info!("zcam: orientation_matrix is a tilted (non-90°) rotation; multiplying samples by {m:?} and emitting \"XYZ\"");
                        self.imu_orient_matrix = Some(m);
                    }
                }
            } else {
                self.imu_orient_str = Some("XYZ".to_string());
            }
        }
    }

    fn process_stream_meta(&mut self, stream: &StreamMeta, tag_map: &mut GroupedTagMap, options: &crate::InputOptions) {
        // Restrict to STREAM_TYPE_VIDEO; audio streams carry no IMU/imager data.
        if let Some(ref h) = stream.stream_meta_header {
            if h.stream_type != stream_meta_header::StreamType::Video as i32 {
                return;
            }
        }
        if let Some(ref v) = stream.video_stream_meta {
            self.video_w = v.width;
            self.video_h = v.height;
            self.video_rotation_deg = v.rotation;

            // Prefer project_fps for the muxed/playback cadence (matches DJI's
            // `framerate` semantic). shot_fps is the recording cadence which may
            // differ in slow-mo modes.
            let fps = if v.project_fps > 0.0 { v.project_fps as f64 }
                      else if v.shot_fps   > 0.0 { v.shot_fps   as f64 }
                      else { self.sensor_fps };
            if fps > 0.0 {
                self.record_fps = fps;
                insert_tag(tag_map, tag!(parsed GroupId::Default, TagId::FrameRate, "Frame rate", f64, |v| format!("{:.3}fps", v), fps, vec![]), options);
            }
        }
        if let Some(ref p) = stream.image_profile {
            if !p.profile_name.is_empty() {
                insert_tag(tag_map, tag!(parsed GroupId::Colors, TagId::CaptureGammaEquation, "Color profile", String, |v| v.to_string(), p.profile_name.clone(), vec![]), options);
            }
        }
    }

    fn process_frame_meta(&mut self, frame: &FrameMeta, info: &SampleInfo, tag_map: &mut GroupedTagMap, options: &crate::InputOptions) {
        let frame_ts_us = frame.frame_meta_header.as_ref().map(|h| h.frame_timestamp as f64).unwrap_or(0.0);

        // ----------------- Imager group -----------------
        if self.pixel_size_nm > 0 {
            insert_tag(tag_map, tag!(parsed GroupId::Imager, TagId::PixelPitch, "Pixel pitch", u32x2, |v| format!("{:?}", v), (self.pixel_size_nm, self.pixel_size_nm), vec![]), options);
        }
        if self.sensor_pixel_width > 0 && self.sensor_pixel_height > 0 {
            insert_tag(tag_map, tag!(parsed GroupId::Imager, TagId::SensorSizePixels, "Sensor pixel size", u32x2, |v| format!("{:?}", v), (self.sensor_pixel_width, self.sensor_pixel_height), vec![]), options);
        }
        // CaptureAreaOrigin / Size in PIXELS, sensor-native coords
        insert_tag(tag_map, tag!(parsed GroupId::Imager, TagId::CaptureAreaOrigin, "Sensor crop origin", f32x2, |v| format!("{:?}", v), (self.crop_x as f32, self.crop_y as f32), vec![]), options);
        insert_tag(tag_map, tag!(parsed GroupId::Imager, TagId::CaptureAreaSize, "Sensor crop size", f32x2, |v| format!("{:?}", v), (self.crop_w as f32, self.crop_h as f32), vec![]), options);

        let first_frame_ts_ms = frame_ts_us / 1000.0 - info.timestamp_ms;
        insert_tag(tag_map, tag!(parsed GroupId::Imager, TagId::FirstFrameTimestamp, "First frame timestamp", f64, |v| format!("{:.4} ms", v), first_frame_ts_ms, vec![]), options);

        // ----------------- per-frame camera metadata -----------------
        if let Some(ref c) = frame.frame_meta_of_camera {
            if let Some(ref e) = c.exposure_data {
                // Exposure precedence:
                //   1. shutter_unit + exposure_time when ShutterUnit==Time
                //   2. shutter_speed_num/_den (exact rational)
                //   3. shutter_angle → exposure_time_us = angle/360 / sensor_fps
                // matches the gyroflow.proto precedence rule, adapted for the ZCAM
                // schema. exposure_time is in SECONDS in the proto.
                let mut t_us: f64 = 0.0;
                if e.exposure_time > 0.0 {
                    t_us = e.exposure_time as f64 * 1.0e6;
                } else if let (Some(num), Some(den)) = (e.shutter_speed_num, e.shutter_speed_den) {
                    if num != 0 && den != 0 {
                        t_us = (num as f64 / den as f64) * 1.0e6;
                    }
                } else if let Some(angle) = e.shutter_angle {
                    let rate = if self.sensor_fps > 0.0 { self.sensor_fps }
                               else if self.record_fps > 0.0 { self.record_fps }
                               else { 0.0 };
                    if rate > 0.0 && angle > 0.0 {
                        t_us = (angle as f64 / 360.0) * 1.0e6 / rate;
                    }
                }
                let exposure_time_ms = t_us / 1000.0;
                insert_tag(tag_map, tag!(parsed GroupId::Imager, TagId::ExposureTime, "Exposure time", f64, |v| format!("{:.4} ms", v), exposure_time_ms, vec![]), options);

                if e.iso > 0.0 {
                    insert_tag(tag_map, tag!(parsed GroupId::Exposure, TagId::ISOValue, "ISO Sensitivity", u16, |v| format!("{}", v), (e.iso.round() as u32).min(u16::MAX as u32) as u16, vec![]), options);
                }
                if let (Some(num), Some(den)) = (e.shutter_speed_num, e.shutter_speed_den) {
                    if num != 0 && den != 0 {
                        insert_tag(tag_map, tag!(parsed GroupId::Exposure, TagId::ShutterSpeed, "Shutter speed", u32x2, |v| format!("{}/{}s", v.0, v.1), (num, den), vec![]), options);
                    }
                }
                if let Some(angle) = e.shutter_angle {
                    if angle > 0.0 {
                        insert_tag(tag_map, tag!(parsed GroupId::Exposure, TagId::ShutterAngle, "Shutter angle", f32, |v| format!("{:.1}°", v), angle, vec![]), options);
                    }
                }
                if e.f_no > 0.0 {
                    insert_tag(tag_map, tag!(parsed GroupId::Lens, TagId::IrisFStop, "Iris F-stop", f32, |v| format!("f/{:.1}", v), e.f_no, vec![]), options);
                }
            }
            if let Some(ref wb) = c.white_balance_data {
                if wb.kelvin > 0 {
                    insert_tag(tag_map, tag!(parsed GroupId::Colors, TagId::WhiteBalance, "White balance", u16, |v| format!("{}K", v), wb.kelvin.min(u16::MAX as u32) as u16, vec![]), options);
                }
            }
            if let Some(ref fl) = c.focal_length {
                if fl.focal_length > 0.0 {
                    if self.first_focal_length_mm.is_none() {
                        self.first_focal_length_mm = Some(fl.focal_length);
                    }
                    insert_tag(tag_map, tag!(parsed GroupId::Lens, TagId::FocalLength, "Focal length", f32, |v| format!("{:.2} mm", v), fl.focal_length, vec![]), options);
                }
            }
            if let Some(ref fd) = c.focus_data {
                if let Some(ref dist) = fd.focus_distance {
                    // ZCAM proto reports either mm or inches per the FocusDistanceUnit enum.
                    // Convert into meters here so both code paths stay consistent.
                    let unit = focus_distance::FocusDistanceUnit::try_from(dist.focus_distance_unit).unwrap_or(focus_distance::FocusDistanceUnit::Mm);
                    let meters = match unit {
                        focus_distance::FocusDistanceUnit::Mm => dist.focus_distance / 1000.0,
                        focus_distance::FocusDistanceUnit::In => dist.focus_distance * 0.0254,
                    };
                    if meters > 0.0 && meters.is_finite() {
                        insert_tag(tag_map, tag!(parsed GroupId::Lens, TagId::FocusDistance, "Focus distance", f32, |v| format!("{:.2} m", v), meters, vec![]), options);
                    }
                }
            }
        }

        // ----------------- Per-frame readout time ------------------
        // The proto stores `read_out_time` once at clip level (ImageSensorInfo).
        // We don't scale by crop_h / sensor_h here — the proto's `read_out_time` is documented as the sensor readout
        // for the configured mode (which already reflects the crop).
        // frame_transform.rs scales by `crop_size / sensor_size` again
        if self.sensor_readout_us > 0.0 {
            insert_tag(tag_map, tag!(parsed GroupId::Imager, TagId::FrameReadoutTime, "Frame readout time", f64, |v| format!("{:.4} ms", v), self.sensor_readout_us / 1000.0, vec![]), options);
            insert_tag(tag_map, tag!(parsed GroupId::Imager, TagId::FrameReadoutDirection, "Frame readout direction", i32, |v| format!("{}", v), readout_dir_to_gyroflow_i32(self.readout_direction_enum), vec![]), options);
        }

        // ----------------- IMU data -----------------
        if let Some(ref imu_frame) = frame.frame_meta_of_imu {
            if let Some(ref imu) = imu_frame.imu_data {
                self.emit_imu_samples(imu, tag_map, options);
            }
        }
    }

    fn emit_imu_samples(&mut self, imu: &ImuData, tag_map: &mut GroupedTagMap, options: &crate::InputOptions) {
        let n = imu.items.len();
        if n == 0 { return; }
        if self.imu_sample_rate_hz <= 0.0 {
            log::warn!("zcam: IMU samples present but ImuInfo.sampling_rate is 0 — cannot derive per-sample timestamps");
            return;
        }

        // Per-sample period in µs derived from ImuInfo.sampling_rate. The proto's
        // ImuData carries only a single batch timestamp; samples within items[]
        // are documented (via the sampling_rate field on ImuInfo) as evenly spaced.
        let dt_us = 1.0e6 / self.imu_sample_rate_hz;

        // ImuInfo.time_offset (proto: "Unit: us") is the constant clock skew between
        // the IMU clock and the camera/frame clock. We subtract it from each sample's
        // timestamp so the resulting time axis lives on the SAME clock as
        // FrameMetaHeader.frame_timestamp before the PTS rebase below.
        let batch_t0_camera_us = imu.timestamp as f64 - self.imu_time_offset_us;

        // Anchor the IMU stream on ITS OWN first sample, on the same clock the
        // per-sample positions are computed on (see the CLOCK MODEL block at the
        // top of this file). The first IMU sample lands at PTS 0 (video frame 0),
        // and `time_offset` is honoured automatically if firmware ever populates it.
        if self.pts_offset_us.is_none() {
            self.pts_offset_us = Some(-batch_t0_camera_us);
        }
        let pts_offset_us = self.pts_offset_us.unwrap_or(0.0);

        let mut gyro: Vec<TimeVector3<f64>> = Vec::with_capacity(n);
        let mut acc:  Vec<TimeVector3<f64>> = Vec::with_capacity(n);

        let apply_matrix = self.imu_orient_matrix.is_some();
        for (i, item) in imu.items.iter().enumerate() {
            // Camera-clock absolute time → PTS-rebased microseconds.
            let t_camera_us = batch_t0_camera_us + (i as f64) * dt_us;

            // Skip samples we have already emitted from an earlier (overlapping)
            // batch. Compare on the camera clock (un-rebased) so we don't have to
            // round-trip through pts_offset. First batch passes through entirely
            // because next_imu_emit_us is None.
            if let Some(min_t) = self.next_imu_emit_us {
                if t_camera_us < min_t { continue; }
            }

            let t_us = t_camera_us + pts_offset_us;
            let t_seconds = t_us / 1.0e6;

            // Step 1: scale LSB → physical units per the proto's ImuInfo equations.
            let mut gx = item.gx as f64 * self.gyro_scale;
            let mut gy = item.gy as f64 * self.gyro_scale;
            let mut gz = item.gz as f64 * self.gyro_scale;
            let mut ax = item.ax as f64 * self.accel_scale;
            let mut ay = item.ay as f64 * self.accel_scale;
            let mut az = item.az as f64 * self.accel_scale;

            // Step 2: optional residual matrix multiply (non-permutation case).
            // Permutation matrices are absorbed into TagId::Orientation downstream
            if apply_matrix {
                let m = self.imu_orient_matrix.as_ref().unwrap();
                let g = orient_vec_row_matrix((gx, gy, gz), m);
                let a = orient_vec_row_matrix((ax, ay, az), m);
                gx = g.0; gy = g.1; gz = g.2;
                ax = a.0; ay = a.1; az = a.2;
            }

            gyro.push(TimeVector3 { t: t_seconds, x: gx, y: gy, z: gz });
            acc .push(TimeVector3 { t: t_seconds, x: ax, y: ay, z: az });
        }

        // Record the next camera-clock instant we should emit at. We add a tiny
        // epsilon (dt_us / 2) so floating-point rounding can't cause the same
        // physical sample to get accepted again from the next batch.
        if let Some(last) = imu.items.len().checked_sub(1).map(|i| batch_t0_camera_us + (i as f64) * dt_us) {
            self.next_imu_emit_us = Some(last + dt_us * 0.5);
        }

        let orient_str = if apply_matrix {
            "XYZ".to_string()
        } else {
            self.imu_orient_str.clone().unwrap_or_else(|| "XYZ".to_string())
        };

        insert_tag(tag_map, tag!(parsed GroupId::Gyroscope, TagId::Data,        "Gyroscope data",     Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]), options);
        insert_tag(tag_map, tag!(parsed GroupId::Gyroscope, TagId::Unit,        "Gyroscope unit",     String,              |v| v.to_string(),     "deg/s".into(), vec![]), options);
        insert_tag(tag_map, tag!(parsed GroupId::Gyroscope, TagId::Orientation, "IMU orientation",    String,              |v| v.to_string(),     orient_str.clone(), vec![]), options);
        insert_tag(tag_map, tag!(parsed GroupId::Gyroscope, TagId::Frequency,   "Gyroscope frequency", i32,                |v| format!("{} Hz", v), self.imu_sample_rate_hz.round() as i32, vec![]), options);

        insert_tag(tag_map, tag!(parsed GroupId::Accelerometer, TagId::Data,        "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), acc, vec![]), options);
        insert_tag(tag_map, tag!(parsed GroupId::Accelerometer, TagId::Unit,        "Accelerometer unit", String,              |v| v.to_string(),     "m/s²".into(), vec![]), options);
        insert_tag(tag_map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation",    String,              |v| v.to_string(),     orient_str.clone(), vec![]), options);
        insert_tag(tag_map, tag!(parsed GroupId::Accelerometer, TagId::Frequency,   "Accelerometer frequency", i32,            |v| format!("{} Hz", v), self.imu_sample_rate_hz.round() as i32, vec![]), options);
    }

    /// Minimal lens profile JSON for the gyroflow.
    /// ZCAM's proto has no distortion model, but we DO have all the pieces needed
    /// to compute a real pinhole camera_matrix:
    ///   * focal_length_mm — first per-frame value, captured into `first_focal_length_mm`
    ///   * sensor width in mm = pixel_size_nm × crop_w / 1e6
    ///   * frame_width in pixels — from VideoStreamMeta
    /// → fx = focal_length_mm / sensor_width_mm × frame_width  (pixels)
    /// → fy similarly (same pixel pitch on both axes per the proto)
    fn build_lens_profile_json(&self) -> Option<serde_json::Value> {
        let w = self.video_w;
        let h = self.video_h;
        if w == 0 || h == 0 { return None; }
        let is_vertical = self.video_rotation_deg == 90 || self.video_rotation_deg == 270;
        let out_w = if is_vertical { h } else { w };
        let out_h = if is_vertical { w } else { h };

        // Compute physical sensor width / height (mm) from the per-frame crop
        // rectangle (falls back to the full sensor when no crop is set, per
        // process_clip_meta).
        let crop_w_px = if self.crop_w > 0 { self.crop_w as f64 } else { self.sensor_pixel_width  as f64 };
        let crop_h_px = if self.crop_h > 0 { self.crop_h as f64 } else { self.sensor_pixel_height as f64 };
        let pp_nm     = self.pixel_size_nm as f64;
        let sensor_w_mm = (pp_nm * crop_w_px) / 1.0e6;
        let sensor_h_mm = (pp_nm * crop_h_px) / 1.0e6;

        let fl_mm = self.first_focal_length_mm.unwrap_or(0.0) as f64;
        // Build a real pinhole camera_matrix in OUTPUT-pixel units. Required:
        // a placeholder fx=fy=1.0 makes the stabilization kernel see ~180° FOV
        // and rotations don't translate to pixel shifts.
        let (fx, fy) = if fl_mm > 0.0 && sensor_w_mm > 0.0 && sensor_h_mm > 0.0 {
            (
                fl_mm / sensor_w_mm * w as f64,
                fl_mm / sensor_h_mm * h as f64,
            )
        } else {
            // Last-resort fallback: half the frame width gives ~53° horizontal
            // FOV — still wrong, but at least within an order of magnitude of
            // typical lenses so stabilization produces visible output.
            let f_default = w as f64 / 2.0;
            (f_default, f_default)
        };
        let cx = w as f64 / 2.0;
        let cy = h as f64 / 2.0;
        let camera_matrix = serde_json::json!([
            [ fx,  0.0, cx ],
            [ 0.0, fy,  cy ],
            [ 0.0, 0.0, 1.0 ]
        ]);

        let model = self.model.clone().unwrap_or_default();
        let lens_model = self.lens_name.clone().unwrap_or_default();
        let frame_readout_time_ms = if self.sensor_readout_us > 0.0 { self.sensor_readout_us / 1000.0 } else { 0.0 };

        Some(serde_json::json!({
            "calibrated_by":     "Z CAM",
            "camera_brand":      "Z CAM",
            "camera_model":      model,
            "lens_model":        lens_model,
            "calib_dimension":   { "w": w, "h": h },
            "orig_dimension":    { "w": w, "h": h },
            "output_dimension":  { "w": out_w, "h": out_h },
            "frame_readout_time": frame_readout_time_ms,
            "official": false,
            "asymmetrical": false,
            "fisheye_params": {
                "camera_matrix":     camera_matrix,
                "distortion_coeffs": []
            },
            "fps": if self.record_fps > 0.0 { self.record_fps } else { self.sensor_fps },
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
        }))
    }
}

// ---------------- helpers ----------------

/// Apply the row-vector × 3x3 matrix product piecewise to a 3-vector.
/// Matches the proto's `v' = v · M` convention with M stored row-major
/// (M[r*3 + c] = entry in row r, column c).
#[inline]
fn orient_vec_row_matrix(v: (f64, f64, f64), m: &[f32; 9]) -> (f64, f64, f64) {
    let (x, y, z) = v;
    let m00 = m[0] as f64; let m01 = m[1] as f64; let m02 = m[2] as f64;
    let m10 = m[3] as f64; let m11 = m[4] as f64; let m12 = m[5] as f64;
    let m20 = m[6] as f64; let m21 = m[7] as f64; let m22 = m[8] as f64;
    (
        x * m00 + y * m10 + z * m20,
        x * m01 + y * m11 + z * m21,
        x * m02 + y * m12 + z * m22,
    )
}

/// Convert the proto's `ImuInfo.orientation_matrix` (9 floats, row-major) into a
/// telemetry-parser axis-permutation string ("XYZ", "Xzy", "ZXY", …)
/// Returns None when the matrix is not a clean signed permutation.
fn matrix_to_orientation_string(m: &[f32; 9]) -> Option<String> {
    let axis_char = |row: usize, sign: f32| -> u8 {
        let c = [b'X', b'Y', b'Z'][row];
        if sign >= 0.0 { c } else { c.to_ascii_lowercase() }
    };
    let mut out = [0u8; 3];
    let mut used_rows = [false; 3]; // each input axis must map to exactly one output
    for j in 0..3 {                 // output column
        let mut pick: Option<(usize, f32)> = None;
        for i in 0..3 {             // input row
            let v = m[i * 3 + j];
            if (v.abs() - 1.0).abs() < 1.0e-3 {
                if pick.is_some() { return None; } // two ±1 in one column
                pick = Some((i, v));
            } else if v.abs() >= 1.0e-3 {
                return None;                       // non-trivial (non 0/±1) entry
            }
        }
        let (i, sign) = pick?;                     // a column of zeros → not a permutation
        if used_rows[i] { return None; }           // input axis reused → not a permutation
        used_rows[i] = true;
        out[j] = axis_char(i, sign);
    }
    String::from_utf8(out.to_vec()).ok()
}

/// Static offset between ZCAM's `orientation_matrix` target frame and gyroflow's
/// camera-body frame, as a COLUMN-vector signed-permutation operator (out = C·in,
/// stored row-major).
const ZCAM_CORRECTION_COL: [f32; 9] = [0.0, -1.0, 0.0,  -1.0, 0.0, 0.0,  0.0, 0.0, -1.0];

#[inline]
fn mat3_transpose(m: &[f32; 9]) -> [f32; 9] {
    [ m[0], m[3], m[6],
      m[1], m[4], m[7],
      m[2], m[5], m[8] ]
}

#[inline]
fn mat3_mul(a: &[f32; 9], b: &[f32; 9]) -> [f32; 9] {
    let mut o = [0f32; 9];
    for r in 0..3 {
        for c in 0..3 {
            o[r * 3 + c] = a[r * 3] * b[c] + a[r * 3 + 1] * b[3 + c] + a[r * 3 + 2] * b[6 + c];
        }
    }
    o
}

/// Fold the static ZCAM→gyroflow correction onto the firmware `orientation_matrix`
/// and decide how to apply the result downstream:
///
///   combined(column op) = C · Mᵀ      (C = ZCAM_CORRECTION_COL, M = proto matrix)
///   m_stored            = combinedᵀ   (row-vector form for `orient_vec_row_matrix`)
///
/// Returns `Ok(string)` when `combined` is a clean signed permutation — emit it as
/// a `TagId::Orientation` string and leave the samples raw (exact, GUI-visible,
/// autosync-guessable). Returns `Err(m_stored)` for a general (tilted, non-90°)
/// rotation that no axis-permutation string can represent — multiply each sample
/// by `m_stored` and emit "XYZ".
fn resolve_imu_orientation(mfw: &[f32; 9]) -> std::result::Result<String, [f32; 9]> {
    let a = mat3_transpose(mfw);                 // proto row-vector M → column operator
    let combined = mat3_mul(&ZCAM_CORRECTION_COL, &a);
    let m_stored = mat3_transpose(&combined);    // back to row-vector form
    match matrix_to_orientation_string(&m_stored) {
        Some(s) => Ok(s),
        None    => Err(m_stored),
    }
}


/// NOTE: the ZCAM proto numbers the horizontal variants in the OPPOSITE order
/// (RightToLeft=2, LeftToRight=3 — see imvtmeta.rs), so this MUST map by variant,
/// not by `as i32`.
#[inline]
fn readout_dir_to_gyroflow_i32(dir: ReadOutDirection) -> i32 {
    match dir {
        ReadOutDirection::TopToBottom => 0,
        ReadOutDirection::BottomToTop => 1,
        ReadOutDirection::LeftToRight => 2,
        ReadOutDirection::RightToLeft => 3,
    }
}
