// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2026 Adrian <adrian.eddy at gmail>
//
// Parser for the Gyroflow Protobuf telemetry container (see ./gyroflow.proto).
//

use std::io::*;
use std::sync::{ Arc, atomic::AtomicBool };

use memchr::memmem;
use prost::Message;

use crate::tags_impl::*;
use crate::*;
use crate::util::insert_tag;

use super::gyroflow_proto;
use super::gyroflow_proto::lens_data::Distortion;
use super::gyroflow_proto::eis_data::Data as EisDataInner;
use super::gyroflow_proto::gps_data::FixType as GpsFixType;
use super::gyroflow_proto::header::clip_metadata::ReadoutDirection;

#[derive(Default)]
pub struct GyroflowProtobuf {
    pub model: Option<String>,
    vendor: String,
    pub frame_readout_time: Option<f64>,
    imu_orientation: String,
    // CameraMetadata.imu_rotation (proto field 14): additional rigid rotation
    // applied to raw IMU 3-vectors AFTER the imu_orientation axis remap.
    // Stored as (w, x, y, z) Hamilton unit quaternion; None = identity.
    imu_rotation: Option<(f64, f64, f64, f64)>,
    // CameraMetadata.quats_rotation (proto field 15): rigid rotation that
    // rotates the AXIS of each fused-orientation quaternion via CONJUGATION
    // (q' = R · q · R⁻¹). Stored as (w, x, y, z); None = identity.
    quats_rotation: Option<(f64, f64, f64, f64)>,

    // Parsed proto header (set on the first sample that carries one).
    // Plain header fields are read through `camera()` / `clip()` helpers
    // below — only state that requires decoding (enum, sign-packing) or
    // is refined per-frame gets its own dedicated field.
    header: Option<gyroflow_proto::Header>,

    readout_direction: ReadoutDirection,
    // Mirrors clip.frame_readout_time_us but is refined from per-frame
    // start/end timestamps in process_frame, so it can diverge from header.
    frame_readout_time_us: f64,

    distortion_model_name: Option<String>,
    lens_profile_emitted: bool,

    // ---- timing reference ----
    // start_timestamp_us of the very first FrameMetadata seen. Used to make
    // IMU / quaternion / per-frame timestamps file-relative (start at 0) so
    // they line up with MP4 sample composition timestamps.
    first_start_ts_us: Option<f64>,
}

// Hamilton quaternion vector rotation: v' = q · v · q⁻¹ for unit q.
// Expanded form avoids the intermediate pure-quaternion allocation.
#[inline]
fn rotate_vec3_by_quat(v: (f64, f64, f64), q: (f64, f64, f64, f64)) -> (f64, f64, f64) {
    let (qw, qx, qy, qz) = q;
    let (vx, vy, vz) = v;
    // v' = 2·(u·v)·u + (qw² − u·u)·v + 2·qw·(u × v),  where u = (qx, qy, qz)
    let dot_uv = qx * vx + qy * vy + qz * vz;
    let qw_sq_minus_uu = qw * qw - (qx * qx + qy * qy + qz * qz);
    let cross_x = qy * vz - qz * vy;
    let cross_y = qz * vx - qx * vz;
    let cross_z = qx * vy - qy * vx;
    (
        2.0 * dot_uv * qx + qw_sq_minus_uu * vx + 2.0 * qw * cross_x,
        2.0 * dot_uv * qy + qw_sq_minus_uu * vy + 2.0 * qw * cross_y,
        2.0 * dot_uv * qz + qw_sq_minus_uu * vz + 2.0 * qw * cross_z,
    )
}

// Hamilton quaternion product q = a · b.
#[inline]
fn quat_mul(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> (f64, f64, f64, f64) {
    let (aw, ax, ay, az) = a;
    let (bw, bx, by, bz) = b;
    (
        aw * bw - ax * bx - ay * by - az * bz,
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
    )
}

// Quaternion conjugation by unit r: q' = r · q · r⁻¹  (r⁻¹ = (rw, −rx, −ry, −rz) for unit r).
#[inline]
fn conjugate_quat_by(q: (f64, f64, f64, f64), r: (f64, f64, f64, f64)) -> (f64, f64, f64, f64) {
    let r_inv = (r.0, -r.1, -r.2, -r.3);
    quat_mul(quat_mul(r, q), r_inv)
}

// Pack readout direction into the magnitude of the readout time, matching the
// scheme gyroflow's gcsv.rs / Sony parser uses (decoded by gyro_source/mod.rs:386-402).
#[inline]
fn pack_readout_time_ms(us: f64, dir: ReadoutDirection) -> f64 {
    let ms = us / 1000.0;
    match dir {
        ReadoutDirection::TopToBottom =>  ms,
        ReadoutDirection::BottomToTop => -ms,
        ReadoutDirection::LeftToRight =>  ms + 10000.0,
        ReadoutDirection::RightToLeft => -(ms + 10000.0),
    }
}

// Apply imu_orientation axis remap to a 3-vector. Same semantics as
// tags_impl::Vector3<f64>::orient but inline so we don't allocate.
#[inline]
fn orient_vec3(v: (f64, f64, f64), io: &[u8]) -> (f64, f64, f64) {
    let map = |o: u8| -> f64 {
        match o as char {
            'X' =>  v.0, 'x' => -v.0,
            'Y' =>  v.1, 'y' => -v.1,
            'Z' =>  v.2, 'z' => -v.2,
            _ => 0.0,
        }
    };
    (map(io[0]), map(io[1]), map(io[2]))
}

impl GyroflowProtobuf {
    pub fn camera_type(&self) -> String {
        self.vendor.clone()
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

    fn camera(&self) -> Option<&gyroflow_proto::header::CameraMetadata> {
        self.header.as_ref()?.camera.as_ref()
    }
    fn clip(&self) -> Option<&gyroflow_proto::header::ClipMetadata> {
        self.header.as_ref()?.clip.as_ref()
    }

    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], _filepath: P, _options: &crate::InputOptions) -> Option<Self> {
        if memmem::find(buffer, b"GyroflowProtobuf").is_some() {
            Some(Self {
                vendor: "Gyroflow".into(),
                imu_orientation: "XYZ".into(),
                ..Default::default()
            })
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
            if memmem::find(data, b"GyroflowProtobuf").is_none() {
                log::warn!("Unexpected data: {}", pretty_hex::pretty_hex(&data));
            }

            let parsed = match gyroflow_proto::Main::decode(data) {
                Ok(p) => p,
                Err(e) => {
                    log::error!("Failed to parse protobuf: {e:?}");
                    log::error!("Data: {}", pretty_hex::pretty_hex(&data));
                    return;
                }
            };

            let mut tag_map = GroupedTagMap::new();

            if let Some(ref header) = parsed.header {
                self.process_header(header, &mut tag_map, &options);
            }

            if let Some(ref frame) = parsed.frame {
                if self.first_start_ts_us.is_none() {
                    self.first_start_ts_us = Some(frame.start_timestamp_us);
                }
                self.process_frame(frame, &info, &mut tag_map, &options);
            }

            // Lens profile JSON is emitted once, after we have both header and
            // the first frame (we need the per-frame distortion variant to set
            // the right `distortion_model` on the lens profile).
            if !self.lens_profile_emitted && self.distortion_model_name.is_some() && self.camera().is_some_and(|c| !c.camera_brand.is_empty()) && self.clip().is_some() {
                if let Some(profile_json) = self.build_lens_profile_json() {
                    insert_tag(&mut tag_map,
                        tag!(parsed GroupId::Lens, TagId::Data, "Lens profile", Json,
                             |v| serde_json::to_string(v).unwrap_or_default(),
                             profile_json, vec![]),
                        &options);
                    self.lens_profile_emitted = true;
                }
            }

            // Optional preference-only lens identifier from header.lens_profile.
            // Use TagId::Name when it isn't a JSON document so the existing
            // gyro_source/mod.rs Lens.Name path picks it up as a profile string.
            if let Some(pref) = self.camera().and_then(|c| c.lens_profile.as_deref()) {
                if !pref.is_empty() && !pref.starts_with('{') {
                    insert_tag(&mut tag_map,
                        tag!(parsed GroupId::Lens, TagId::Name, "Lens profile name", String,
                             |v| v.to_string(), pref.to_string(), vec![]),
                        &options);
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

    fn process_header(&mut self, header: &gyroflow_proto::Header, tag_map: &mut GroupedTagMap, options: &crate::InputOptions) {
        self.header = Some(header.clone());

        if let Some(ref cam) = header.camera {
            if let Some(ref io) = cam.imu_orientation {
                self.imu_orientation = io.clone();
            }
            // Optional rigid rotations (proto fields 14, 15) — applied to raw
            // IMU vectors and to fused-orientation quaternions respectively.
            self.imu_rotation = cam.imu_rotation.as_ref().map(|q| {
                (q.w as f64, q.x as f64, q.y as f64, q.z as f64)
            });
            self.quats_rotation = cam.quats_rotation.as_ref().map(|q| {
                (q.w as f64, q.x as f64, q.y as f64, q.z as f64)
            });

            // Vendor / model surfaces through Input::camera_type / camera_model.
            // Keep the original camera brand so CameraIdentifier and downstream
            // lens-profile lookups behave like native Sony / GoPro / etc.
            if !cam.camera_brand.is_empty() {
                self.vendor = cam.camera_brand.clone();
            }
            self.model = if cam.camera_model.is_empty() { None } else { Some(cam.camera_model.clone()) };

            insert_tag(tag_map,
                tag!(parsed GroupId::Default, TagId::SerialNumber, "Camera serial number", String,
                     |v| v.to_string(), cam.camera_serial_number.clone().unwrap_or_default(), vec![]),
                options);

            if let Some(ref add) = cam.additional_data {
                if add.starts_with('{') {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(add) {
                        insert_tag(tag_map,
                            tag!(parsed GroupId::Default, TagId::Metadata, "Additional metadata", Json,
                                 |v| v.to_string(), json, vec![]),
                            options);
                    }
                }
            }

            // ImageOrientation override for IMU axis remap is applied later
            // when Gyroscope tags are written.
        }

        if let Some(ref clip) = header.clip {
            self.frame_readout_time_us = clip.frame_readout_time_us;
            self.readout_direction = ReadoutDirection::try_from(clip.frame_readout_direction).unwrap_or(ReadoutDirection::TopToBottom);
            self.frame_readout_time = Some(pack_readout_time_ms(clip.frame_readout_time_us, self.readout_direction));

            insert_tag(tag_map,
                tag!(parsed GroupId::Default, TagId::FrameRate, "Frame rate", f64,
                     |v| format!("{:.3}fps", v), clip.record_frame_rate as f64, vec![]),
                options);

            if let Some(ref cp) = clip.color_profile {
                insert_tag(tag_map,
                    tag!(parsed GroupId::Colors, TagId::CaptureGammaEquation, "Color profile", String,
                         |v| v.to_string(), cp.clone(), vec![]),
                    options);
            }
        }
    }

    fn process_frame(&mut self, frame: &gyroflow_proto::FrameMetadata, info: &SampleInfo, tag_map: &mut GroupedTagMap, options: &crate::InputOptions) {
        let (sensor_w, sensor_h, raw_pp_x, raw_pp_y) = self.camera()
            .map_or((0u32, 0u32, 0u32, 0u32), |c| (c.sensor_pixel_width, c.sensor_pixel_height, c.pixel_pitch_x_nm, c.pixel_pitch_y_nm));
        // The proto requires both pitches (square-pixel sensors must set them
        // equal). Tolerate producers that emit only one by mirroring the set
        // axis into the missing one — better than emitting a half-zero pair
        // that downstream treats as zero pitch.
        let pixel_pitch_x_nm = if raw_pp_x > 0 { raw_pp_x } else { raw_pp_y };
        let pixel_pitch_y_nm = if raw_pp_y > 0 { raw_pp_y } else { raw_pp_x };
        let lens_model_str = self.camera().map(|c| c.lens_model.clone()).unwrap_or_default();
        let (frame_w, frame_h, imu_sample_rate) = self.clip()
            .map_or((0u32, 0u32, 0u32), |c| (c.frame_width, c.frame_height, c.imu_sample_rate));

        // ---- Imager group ----
        let crop_origin = (
            frame.crop_x.unwrap_or(0.0),
            frame.crop_y.unwrap_or(0.0),
        );
        let crop_size = (
            frame.crop_width.unwrap_or(sensor_w as f32),
            frame.crop_height.unwrap_or(sensor_h as f32),
        );

        if pixel_pitch_x_nm > 0 && pixel_pitch_y_nm > 0 {
            insert_tag(tag_map,
                tag!(parsed GroupId::Imager, TagId::PixelPitch, "Pixel pitch", u32x2,
                     |v| format!("{:?}", v), (pixel_pitch_x_nm, pixel_pitch_y_nm), vec![]),
                options);
        }
        if sensor_w > 0 && sensor_h > 0 {
            insert_tag(tag_map,
                tag!(parsed GroupId::Imager, TagId::SensorSizePixels, "Sensor pixel size", u32x2,
                     |v| format!("{:?}", v), (sensor_w, sensor_h), vec![]),
                options);
        }
        insert_tag(tag_map,
            tag!(parsed GroupId::Imager, TagId::CaptureAreaOrigin, "Sensor crop origin", f32x2,
                 |v| format!("{:?}", v), crop_origin, vec![]),
            options);
        insert_tag(tag_map,
            tag!(parsed GroupId::Imager, TagId::CaptureAreaSize, "Sensor crop size", f32x2,
                 |v| format!("{:?}", v), crop_size, vec![]),
            options);

        // ---- Per-frame timing (see file-header rationale) ----
        // Recover Sony's RTMD per-frame jitter J_i = (camera-clock start_ts) - (video PTS).
        // The proto says start_timestamp_us is on the camera's internal clock and may
        // differ from the video track PTS by a per-frame jitter; recovering exactly J_i
        // (NOT J_i shifted by J_0) matters because sony::stab_collect feeds
        // first_frame_ts straight into stab_calc_splines' top_offset for IBIS / OIS
        // spline timing — a J_0 offset there picks DIFFERENT IBIS samples than native
        // by exactly J_0 µs and degrades rolling-shutter / IBIS stability.
        let first_frame_ts_ms = frame.start_timestamp_us / 1000.0 - info.timestamp_ms;
        insert_tag(tag_map,
            tag!(parsed GroupId::Imager, TagId::FirstFrameTimestamp, "First frame timestamp", f64,
                 |v| format!("{:.4} ms", v), first_frame_ts_ms, vec![]),
            options);

        // Effective exposure time (apply EXPOSURE PRECEDENCE from the proto).
        let exposure_time_us = self.resolve_exposure_us(frame);
        insert_tag(tag_map,
            tag!(parsed GroupId::Imager, TagId::ExposureTime, "Exposure time", f64,
                 |v| format!("{:.4} ms", v), exposure_time_us / 1000.0, vec![]),
            options);

        // Per-frame readout time: per the proto, `end_timestamp_us - start_timestamp_us`
        // is AUTHORITATIVE for per-row interpolation. clip.frame_readout_time_us is a
        // float helper that takes lower precedence ("the two timestamp fields are
        // doubles measured directly by the camera clock and take precedence over the
        // float helper frame_readout_time_us if the two ever disagree" — proto comment
        // on FrameMetadata.end_timestamp_us). We mirror that ordering here.
        let per_frame_readout_us = (frame.end_timestamp_us - frame.start_timestamp_us).max(0.0);
        let frame_readout_us_unsigned = if per_frame_readout_us > 0.0 {
            per_frame_readout_us
        } else {
            self.frame_readout_time_us
        };
        insert_tag(tag_map,
            tag!(parsed GroupId::Imager, TagId::FrameReadoutTime, "Frame readout time", f64,
                 |v| format!("{:.4} ms", v), frame_readout_us_unsigned / 1000.0, vec![]),
            options);

        // Promote the authoritative per-frame readout to the clip-level
        // self.frame_readout_time on the very first frame (which then surfaces via
        // input.frame_readout_time() → file_metadata.frame_readout_time, the single
        // value gyroflow's stab_calc_splines / frame_transform pipeline operates on).
        // We only do this when the proto producer either omitted clip.frame_readout_time_us
        // or when the clip-level helper looks stale (zero); the per-frame doubles are
        // measured directly by the camera clock per the proto spec and never disagree
        // with themselves the way clip-level can with per-frame.
        if self.first_start_ts_us == Some(frame.start_timestamp_us) && per_frame_readout_us > 0.0 {
            if self.frame_readout_time_us <= 0.0 {
                self.frame_readout_time_us = per_frame_readout_us;
                self.frame_readout_time = Some(pack_readout_time_ms(per_frame_readout_us, self.readout_direction));
            }
        }

        // ---- Per-frame Exposure / Lens scalars ----
        if let Some(iso) = frame.iso {
            insert_tag(tag_map,
                tag!(parsed GroupId::Exposure, TagId::ISOValue, "ISO Sensitivity", u16,
                     |v| format!("{}", v), iso.min(u16::MAX as u32) as u16, vec![]),
                options);
        }
        if let (Some(num), Some(den)) = (frame.shutter_speed_numerator, frame.shutter_speed_denominator) {
            if num != 0 && den != 0 {
                insert_tag(tag_map,
                    tag!(parsed GroupId::Exposure, TagId::ShutterSpeed, "Shutter speed", u32x2,
                         |v| format!("{}/{}s", v.0, v.1), (num.unsigned_abs(), den.unsigned_abs()), vec![]),
                    options);
            }
        }
        if let Some(angle) = frame.shutter_angle_degrees {
            insert_tag(tag_map,
                tag!(parsed GroupId::Exposure, TagId::ShutterAngle, "Shutter angle", f32,
                     |v| format!("{:.1}°", v), angle, vec![]),
                options);
        }
        if let Some(wbk) = frame.white_balance_kelvin {
            insert_tag(tag_map,
                tag!(parsed GroupId::Colors, TagId::WhiteBalance, "White balance", u16,
                     |v| format!("{}K", v), wbk.min(u16::MAX as u32) as u16, vec![]),
                options);
        }
        if let Some(tint) = frame.white_balance_tint {
            insert_tag(tag_map,
                tag!(parsed GroupId::Colors, TagId::Unknown(0x57425254/*WBRT*/), "White balance tint", f32,
                     |v| format!("{:.2}", v), tint, vec![]),
                options);
        }
        if let Some(zoom) = frame.digital_zoom_ratio {
            // gyro_source/mod.rs:268-274 reads DJI-native DZST/DZMX tags to
            // populate FileMetadata.digital_zoom. Emit the proto's
            // digital_zoom_ratio under those same keys so the existing
            // consumer picks it up without code changes: encode `ratio` such
            // that gyroflow's formula `1.0 + (DZST/100) * (DZMX - 1)` yields
            // the desired ratio. We use DZMX = ratio and DZST = 100 so the
            // formula gives 1 + 1*(ratio − 1) = ratio. When zoom == 1.0 we
            // omit the tags so the consumer treats it as "no digital zoom".
            if zoom > 1.000001 {
                insert_tag(tag_map,
                    tag!(parsed GroupId::Default, TagId::Unknown(0x445a5354/*DZST*/), "Digital zoom state", u32,
                         |v| format!("{}", v), 100u32, vec![]),
                    options);
                insert_tag(tag_map,
                    tag!(parsed GroupId::Default, TagId::Unknown(0x445a4d58/*DZMX*/), "Digital zoom max", f32,
                         |v| format!("{:.4}", v), zoom, vec![]),
                    options);
            }
        }

        // ---- Lens (focal length, distortion, intrinsic) ----
        // We use the FIRST LensData entry per frame as the per-frame value.
        // Multiple entries (intra-frame zoom/focus actuator motion) are not
        // currently consumed downstream and would require time-interpolation
        // support that gyroflow's lens_params machinery does not yet expose.
        // Warn the producer (once per parse) so the dropped data isn't silent.
        if frame.lens.len() > 1 && self.first_start_ts_us == Some(frame.start_timestamp_us) {
            log::warn!(
                "Proto producer emitted {} LensData entries on frame 0 (intra-frame zoom / \
                 focus actuator motion). Gyroflow currently consumes only the first; \
                 per-sample interpolation is not yet wired through lens_params.",
                frame.lens.len()
            );
        }
        if let Some(lens) = frame.lens.first() {
            if let Some(fl_mm) = lens.focal_length_mm {
                insert_tag(tag_map,
                    tag!(parsed GroupId::Lens, TagId::FocalLength, "Focal length", f32,
                         |v| format!("{:.2} mm", v), fl_mm, vec![]),
                    options);
            }
            if let Some(fnum) = lens.f_number {
                insert_tag(tag_map,
                    tag!(parsed GroupId::Lens, TagId::IrisFStop, "Iris F-stop", f32,
                         |v| format!("f/{:.1}", v), fnum, vec![]),
                    options);
            }
            if let Some(fd) = lens.focus_distance_mm {
                // Sony's native RTMD parser emits Lens.FocusDistance in METERS
                // (sony/rtmd_tags.rs:22-23, display fmt "{:.2}m"), and gyroflow's
                // gyro_source/mod.rs:219-221 stores the tag value verbatim into
                // lens_info.focus_distance with no unit conversion. The proto
                // field is millimeters per gyroflow.proto, so divide by 1000
                // here to match Sony's native unit and keep lens_info.focus_distance
                // consistent across the native and proto-roundtrip paths.
                insert_tag(tag_map,
                    tag!(parsed GroupId::Lens, TagId::FocusDistance, "Focus distance", f32,
                         |v| format!("{:.2} m", v), fd / 1000.0, vec![]),
                    options);
            }

            // Pixel focal length AND principal point come straight from the
            // intrinsic matrix (matrix is in OUTPUT pixel units per the proto
            // spec). We emit BOTH axes (f_x, f_y) and BOTH principal-point
            // components (c_x, c_y) so the downstream consumer can honor
            // non-square pixels, anamorphic squeeze, off-center crops, shift
            // lenses, and IBIS roll about the true principal point. The
            // shader / CPU kernels already consume KernelParams.f and .c as
            // 2-vectors — see gpu/stabilize_spirv/src/stabilize.rs:44.
            if lens.camera_intrinsic_matrix.len() >= 9 {
                let f_x = lens.camera_intrinsic_matrix[0];
                let f_y = lens.camera_intrinsic_matrix[4];
                let c_x = lens.camera_intrinsic_matrix[2];
                let c_y = lens.camera_intrinsic_matrix[5];
                if f_x > 0.0 && f_y > 0.0 {
                    insert_tag(tag_map,
                        tag!(parsed GroupId::Lens, TagId::PixelFocalLength, "Pixel focal length", f32x2,
                             |v| format!("({:.2}, {:.2}) px", v.0, v.1), (f_x, f_y), vec![]),
                        options);
                }
                // Only emit the principal point when it carries information
                // (i.e. it's not the trivial centered default). The decoder
                // falls back to the LensProfile baseline when this tag is
                // absent, which is the correct behavior for centered optics.
                let centered_cx = frame_w as f32 / 2.0;
                let centered_cy = frame_h as f32 / 2.0;
                if (c_x - centered_cx).abs() > 0.5 || (c_y - centered_cy).abs() > 0.5 {
                    insert_tag(tag_map,
                        tag!(parsed GroupId::Lens, TagId::PrincipalPoint, "Principal point", f32x2,
                             |v| format!("({:.2}, {:.2}) px", v.0, v.1), (c_x, c_y), vec![]),
                        options);
                }
            } else if let Some(fl_mm) = lens.focal_length_mm {
                // Derive pixel focal length from physical mm + sensor geometry
                // (matches LensProfile-side fallback for proto-style data).
                // Derive BOTH axes from their matching pixel pitch so non-
                // square-pixel sensors don't get silently squared.
                if pixel_pitch_x_nm > 0 && pixel_pitch_y_nm > 0
                    && crop_size.0 > 0.0 && crop_size.1 > 0.0
                    && frame_w > 0 && frame_h > 0
                {
                    let sensor_w_mm = (pixel_pitch_x_nm as f64 * crop_size.0 as f64) / 1.0e6;
                    let sensor_h_mm = (pixel_pitch_y_nm as f64 * crop_size.1 as f64) / 1.0e6;
                    if sensor_w_mm > 0.0 && sensor_h_mm > 0.0 {
                        let f_x = (fl_mm as f64 / sensor_w_mm) * frame_w as f64;
                        let f_y = (fl_mm as f64 / sensor_h_mm) * frame_h as f64;
                        insert_tag(tag_map,
                            tag!(parsed GroupId::Lens, TagId::PixelFocalLength, "Pixel focal length", f32x2,
                                 |v| format!("({:.2}, {:.2}) px", v.0, v.1), (f_x as f32, f_y as f32), vec![]),
                            options);
                    }
                }
                // No PrincipalPoint emitted in this branch — without an
                // intrinsic matrix the producer didn't claim an off-center PP.
            }

            if !lens_model_str.is_empty() {
                insert_tag(tag_map,
                    tag!(parsed GroupId::Lens, TagId::DisplayName, "Lens name", String,
                         |v| v.to_string(), lens_model_str.clone(), vec![]),
                    options);
            }

            // Distortion coefficients + model-name tag for downstream pickup.
            let (model_name, coeffs) = Self::distortion_payload(lens);
            if let Some(name) = model_name {
                if self.distortion_model_name.is_none() {
                    self.distortion_model_name = Some(name.clone());
                }
                insert_tag(tag_map,
                    tag!(parsed GroupId::Lens, TagId::DistortionModel, "Distortion model", String,
                         |v| v.to_string(), name, vec![]),
                    options);
            }
            if !coeffs.is_empty() {
                insert_tag(tag_map,
                    tag!(parsed GroupId::Lens, TagId::DistortionCoefficients, "Distortion coefficients", Vec_f64,
                         |v| format!("{:?}", v), coeffs, vec![]),
                    options);
            }
        }

        // ---- Raw IMU samples → Gyroscope / Accelerometer / Magnetometer ----
        // TimeVector3.t convention for Vec_TimeVector3_f64 is SECONDS. We keep IMU
        // sample times on the CAMERA CLOCK (no rebase) — per the proto spec,
        // sample_timestamp_us is on the same clock as start_timestamp_us, and the
        // downstream gyro_source code only ever consumes RELATIVE timings
        // (durations, BTreeMap lookups by an arbitrary key origin). Keeping the
        // camera clock means FirstFrameTimestamp can stay = pure J_i above (no J_0
        // shift), which is what stab_collect's IBIS-spline timing requires.
        // See telemetry-parser/src/util.rs:544-560 (Vec_TimeVector3_f64 branch in
        // normalized_imu_interpolated, which keys gyro_map by us = (t * 1000).round()).
        let mut gyro: Vec<TimeVector3<f64>> = Vec::with_capacity(frame.imu.len());
        let mut acc:  Vec<TimeVector3<f64>> = Vec::with_capacity(frame.imu.len());
        let mut mag:  Vec<TimeVector3<f64>> = Vec::new();

        // imu_rotation handling (proto Header.CameraMetadata.imu_rotation):
        // spec'd to be applied AFTER imu_orientation axis remap. Since util.rs's
        // normalized_imu_interpolated unconditionally applies the Orientation tag,
        // when imu_rotation is set we pre-apply both the orient remap AND the
        // rotation here, then write Orientation = "XYZ" so util.rs's later orient
        // is a no-op. (orient and rotation don't commute in general, so we can't
        // hand them to util.rs separately.)
        let apply_rotation = self.imu_rotation.is_some();
        let emit_orientation = if apply_rotation { "XYZ".to_string() } else { self.imu_orientation.clone() };
        let orient_bytes_for_prebake: Vec<u8> = if apply_rotation && self.imu_orientation.len() == 3 {
            self.imu_orientation.as_bytes().to_vec()
        } else {
            b"XYZ".to_vec()
        };

        for imu in &frame.imu {
            // Per the proto, sample_timestamp_us is required for multi-sample entries.
            // When omitted we fall back to start_timestamp_us so a degenerate
            // single-sample-per-frame stream still places the sample at frame start.
            let t_abs_us = imu.sample_timestamp_us.unwrap_or(frame.start_timestamp_us);
            let t_seconds = t_abs_us / 1.0e6;

            let (mut gx, mut gy, mut gz) = (imu.gyroscope_x as f64, imu.gyroscope_y as f64, imu.gyroscope_z as f64);
            let (mut ax, mut ay, mut az) = (imu.accelerometer_x as f64, imu.accelerometer_y as f64, imu.accelerometer_z as f64);
            if apply_rotation {
                // orient → rotation, per spec.
                let g_o = orient_vec3((gx, gy, gz), &orient_bytes_for_prebake);
                let a_o = orient_vec3((ax, ay, az), &orient_bytes_for_prebake);
                let g_r = rotate_vec3_by_quat(g_o, self.imu_rotation.unwrap());
                let a_r = rotate_vec3_by_quat(a_o, self.imu_rotation.unwrap());
                gx = g_r.0; gy = g_r.1; gz = g_r.2;
                ax = a_r.0; ay = a_r.1; az = a_r.2;
            }

            gyro.push(TimeVector3 { t: t_seconds, x: gx, y: gy, z: gz });
            acc.push(TimeVector3 { t: t_seconds, x: ax, y: ay, z: az });
            if let (Some(mx), Some(my), Some(mz)) = (imu.magnetometer_x, imu.magnetometer_y, imu.magnetometer_z) {
                let (mut mx, mut my, mut mz) = (mx as f64, my as f64, mz as f64);
                if apply_rotation {
                    let m_o = orient_vec3((mx, my, mz), &orient_bytes_for_prebake);
                    let m_r = rotate_vec3_by_quat(m_o, self.imu_rotation.unwrap());
                    mx = m_r.0; my = m_r.1; mz = m_r.2;
                }
                mag.push(TimeVector3 { t: t_seconds, x: mx, y: my, z: mz });
            }
        }

        if !gyro.is_empty() {
            insert_tag(tag_map, tag!(parsed GroupId::Gyroscope,     TagId::Data,        "Gyroscope data",     Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]), options);
            insert_tag(tag_map, tag!(parsed GroupId::Gyroscope,     TagId::Unit,        "Gyroscope unit",     String,              |v| v.to_string(),     "deg/s".into(), vec![]), options);
            insert_tag(tag_map, tag!(parsed GroupId::Gyroscope,     TagId::Orientation, "IMU orientation",    String,              |v| v.to_string(),     emit_orientation.clone(), vec![]), options);
            // Frequency / TimeOffset are read by sony::get_time_offset:
            //   * Frequency = gyro sample rate (Hz)
            //   * TimeOffset = 0 because our IMU `t` values are already file-relative,
            //     which is exactly what sony::get_time_offset's `- offset` term assumes.
            if imu_sample_rate > 0 {
                insert_tag(tag_map,
                    tag!(parsed GroupId::Gyroscope, TagId::Frequency, "Gyroscope frequency", i32,
                         |v| format!("{} Hz", v), imu_sample_rate as i32, vec![]),
                    options);
            }
            insert_tag(tag_map,
                tag!(parsed GroupId::Gyroscope, TagId::TimeOffset, "Gyroscope offset", f64,
                     |v| format!("{:.4} ms", v), 0.0_f64, vec![]),
                options);
        }
        if !acc.is_empty() {
            insert_tag(tag_map, tag!(parsed GroupId::Accelerometer, TagId::Data,        "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), acc, vec![]), options);
            insert_tag(tag_map, tag!(parsed GroupId::Accelerometer, TagId::Unit,        "Accelerometer unit", String,              |v| v.to_string(),     "m/s²".into(), vec![]), options);
            insert_tag(tag_map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation",    String,              |v| v.to_string(),     emit_orientation.clone(), vec![]), options);
            if imu_sample_rate > 0 {
                insert_tag(tag_map,
                    tag!(parsed GroupId::Accelerometer, TagId::Frequency, "Accelerometer frequency", i32,
                         |v| format!("{} Hz", v), imu_sample_rate as i32, vec![]),
                    options);
            }
            insert_tag(tag_map,
                tag!(parsed GroupId::Accelerometer, TagId::TimeOffset, "Accelerometer offset", f64,
                     |v| format!("{:.4} ms", v), 0.0_f64, vec![]),
                options);
        }
        if !mag.is_empty() {
            insert_tag(tag_map, tag!(parsed GroupId::Magnetometer, TagId::Data,        "Magnetometer data",  Vec_TimeVector3_f64, |v| format!("{:?}", v), mag, vec![]), options);
            insert_tag(tag_map, tag!(parsed GroupId::Magnetometer, TagId::Unit,        "Magnetometer unit",  String,              |v| v.to_string(),     "µT".into(), vec![]), options);
            insert_tag(tag_map, tag!(parsed GroupId::Magnetometer, TagId::Orientation, "IMU orientation",    String,              |v| v.to_string(),     emit_orientation.clone(), vec![]), options);
        }

        // ---- Quaternions (fused orientation) ----
        // TimeQuaternion.t is MILLISECONDS; gyro_source/mod.rs:177 does
        // (v.t * 1000.0) → us key. Like raw IMU above, we keep the camera-clock
        // origin (no rebase) so quat keys stay on the same axis as gyro_map keys.
        if !frame.quaternions.is_empty() {
            let quats: Vec<TimeQuaternion<f64>> = frame.quaternions.iter().filter_map(|q| {
                let qu = q.quat.as_ref()?;
                let t_abs_us = q.sample_timestamp_us.unwrap_or(frame.start_timestamp_us);
                let t_ms = t_abs_us / 1000.0;
                let mut q_tuple = (qu.w as f64, qu.x as f64, qu.y as f64, qu.z as f64);
                // quats_rotation: per the proto spec, applied to each fused-orientation
                // quaternion via CONJUGATION (q' = R · q · R⁻¹) — rotates the axis of
                // each rotation by R while preserving the angle, effectively
                // re-expressing the same physical orientation in a rotated frame.
                if let Some(r) = self.quats_rotation {
                    q_tuple = conjugate_quat_by(q_tuple, r);
                }
                Some(TimeQuaternion {
                    t: t_ms,
                    v: Quaternion { w: q_tuple.0, x: q_tuple.1, y: q_tuple.2, z: q_tuple.3 },
                })
            }).collect();
            if !quats.is_empty() {
                insert_tag(tag_map,
                    tag!(parsed GroupId::Quaternion, TagId::Data, "Quaternion data", Vec_TimeQuaternion_f64,
                         |v| format!("{:?}", v), quats, vec![]),
                    options);
            }
        }

        // ---- IBIS (in-body image stabilization) ----
        // gyroflow's sony::stab_collect expects:
        //   * IBIS.Data  : Vec<TimeVector3<i32>>  with t in microseconds, FRAME-RELATIVE
        //                  in [0, frame_interval). Required because
        //                  sony::ISTemp::calc_time_diff resolves cross-frame jumps
        //                  by adding `frame_interval` once when dt is negative — only
        //                  correct when each frame's t values stay within one cycle.
        //                  x/y are nanometers (image displacement, signs already +X right
        //                  / +Y down per the proto's UNIFIED STABILIZER SIGN CONVENTION).
        //   * IBIS.Data2 : Vec<TimeVector3<i32>> aligned 1:1 with Data,
        //                  z = roll_angle_milli_degrees (gyroflow does s.z / 1000.0 to
        //                  reach degrees in frame_transform.rs).
        //
        // Guard samples (proto-spec) that fall outside this frame's nominal interval
        // are intentionally dropped — they are physically located in adjacent frames
        // and the consumer reaches them by walking is.t backward/forward across the
        // global per-frame index, which is exactly what sony::search_top_idx2 does.
        // The IBIS roll's pivot in the proto is the principal point in sensor coords;
        // gyroflow's existing kernel pivots about (cx, cy) of the OUTPUT image. For
        // typical sensors with the principal point at the sensor center this matches.
        let frame_interval_us = self.nominal_frame_interval_us();
        if !frame.ibis.is_empty() {
            let mut shifts:  Vec<TimeVector3<i32>> = Vec::with_capacity(frame.ibis.len());
            let mut angles:  Vec<TimeVector3<i32>> = Vec::with_capacity(frame.ibis.len());
            let mut combined: Vec<(f64, &gyroflow_proto::IbisData)> = frame.ibis.iter()
                .map(|s| (s.sample_timestamp_us.unwrap_or(frame.start_timestamp_us), s))
                .collect();
            combined.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            for (t_abs_us, s) in combined {
                let dt = t_abs_us - frame.start_timestamp_us;
                if dt < 0.0 || dt >= frame_interval_us { continue; }
                let t_rel_us = dt.round() as i32;
                // SIGN FLIP: the proto's UNIFIED STABILIZER SIGN CONVENTION reports IBIS
                // shift as IMAGE-CONTENT displacement (= negative of the sensor's mechanical
                // displacement). gyroflow's existing IBIS pipeline (sony::stab_collect →
                // frame_transform.rs → opencl_undistort.cl) carries Sony's NATIVE sensor-
                // displacement sign and the shader does `uv - matrix[9]`. To make that
                // pipeline yield the proto-spec'd `uv + image_disp` result, we flip here.
                shifts.push(TimeVector3 {
                    t: t_rel_us,
                    x: (-s.shift_x_nm).round() as i32,
                    y: (-s.shift_y_nm).round() as i32,
                    z: 0,
                });
                angles.push(TimeVector3 {
                    t: t_rel_us,
                    x: 0,
                    y: 0,
                    z: (s.roll_angle_degrees * 1000.0).round() as i32,
                });
            }
            if !shifts.is_empty() {
                insert_tag(tag_map,
                    tag!(parsed GroupId::IBIS, TagId::Data, "IBIS shift table", Vec_TimeVector3_i32,
                         |v| format!("{:?}", v), shifts, vec![]),
                    options);
                insert_tag(tag_map,
                    tag!(parsed GroupId::IBIS, TagId::Data2, "IBIS angle table", Vec_TimeVector3_i32,
                         |v| format!("{:?}", v), angles, vec![]),
                    options);
            }
        }

        // ---- Lens OIS ----
        // gyroflow's sony::stab_collect expects LensOSS.Data : Vec<TimeVector3<i32>>
        // with t in microseconds (frame-relative, in [0, frame_interval) — same
        // wrap reasoning as IBIS above) and x/y in nanometers (image displacement).
        // The proto's signs already match (+X right / +Y down).
        if !frame.ois.is_empty() {
            let mut combined: Vec<(f64, &gyroflow_proto::LensOisData)> = frame.ois.iter()
                .map(|s| (s.sample_timestamp_us.unwrap_or(frame.start_timestamp_us), s))
                .collect();
            combined.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            let shifts: Vec<TimeVector3<i32>> = combined.into_iter().filter_map(|(t_abs_us, s)| {
                let dt = t_abs_us - frame.start_timestamp_us;
                if dt < 0.0 || dt >= frame_interval_us { return None; }
                Some(TimeVector3 {
                    t: dt.round() as i32,
                    x: s.shift_x_nm.round() as i32,
                    y: s.shift_y_nm.round() as i32,
                    z: 0,
                })
            }).collect();
            if !shifts.is_empty() {
                insert_tag(tag_map,
                    tag!(parsed GroupId::LensOSS, TagId::Data, "Lens OSS shift table", Vec_TimeVector3_i32,
                         |v| format!("{:?}", v), shifts, vec![]),
                    options);
            }
        }

        // ---- EIS (in-camera applied transform) ----
        // * `mesh_warp` → gyroflow's MeshCorrection group + Sony-shaped JSON
        //   (see sony::get_mesh_correction in gyroflow/src/core/gyro_source/sony.rs).
        // * `quaternion` → GroupId::ImageOrientation in the GoPro IORI shape
        //   (Vec<Quaternion<i16>> + scale 32767). gyroflow consumes this in
        //   gyro_source/mod.rs:290-302 and then zips it 1:1 with QuaternionData
        //   (mod.rs:336-338) to build `image_orientations` and to re-express
        //   gravity in the encoded-image frame (mod.rs:347-349).
        //
        //   CADENCE NOTE: gyroflow's zip is by INDEX, not timestamp — the
        //   consumer assumes the EIS quaternion stream and QuaternionData stream
        //   share a sample-for-sample cadence (just like GoPro CORI/IORI). A
        //   producer that wants this path must emit one EIS quaternion per
        //   QuaternionData sample. If counts don't match, only the prefix is
        //   used (mod.rs:336-338 zips the shorter); we let the consumer enforce.
        // * `matrix_4x4` → no grounded gyroflow consumer yet; ignored.
        // * FocalPlaneDistortion is intentionally absent from the proto per spec.
        let mut eis_quat_i16: Vec<Quaternion<i16>> = Vec::new();
        for eis in &frame.eis {
            match eis.data.as_ref() {
                Some(EisDataInner::MeshWarp(mesh)) => {
                    if let Some(json) = self.build_mesh_correction_json(mesh) {
                        insert_tag(tag_map,
                            tag!(parsed GroupId::Custom("MeshCorrection".into()), TagId::Enabled, "MeshCorrection enabled", bool,
                                 |v| format!("{}", v), true, vec![]),
                            options);
                        insert_tag(tag_map,
                            tag!(parsed GroupId::Custom("MeshCorrection".into()), TagId::Data, "MeshCorrection mesh", Json,
                                 |v| v.to_string(), json, vec![]),
                            options);
                    }
                }
                Some(EisDataInner::Quaternion(q)) => {
                    // Quantize float quaternion → i16 with scale 32767 (the same
                    // scale GoPro emits and gyroflow defaults to at mod.rs:291).
                    // gyroflow does `i16 as f64 / scale` with no sign flips on
                    // this read path (see mod.rs:294-300), so a clean Hamilton
                    // quaternion round-trips with no axis remap.
                    let s = 32767.0f32;
                    eis_quat_i16.push(Quaternion {
                        w: (q.w.clamp(-1.0, 1.0) * s).round() as i16,
                        x: (q.x.clamp(-1.0, 1.0) * s).round() as i16,
                        y: (q.y.clamp(-1.0, 1.0) * s).round() as i16,
                        z: (q.z.clamp(-1.0, 1.0) * s).round() as i16,
                    });
                }
                Some(EisDataInner::Matrix4x4(_)) | None => {
                    // No grounded gyroflow consumer for matrix_4x4.
                }
            }
        }
        if !eis_quat_i16.is_empty() {
            insert_tag(tag_map,
                tag!(parsed GroupId::ImageOrientation, TagId::Data, "Image orientation", Vec_Quaternioni16,
                     |v| format!("{:?}", v), eis_quat_i16, vec![]),
                options);
            insert_tag(tag_map,
                tag!(parsed GroupId::ImageOrientation, TagId::Scale, "Image orientation scale", i16,
                     |v| format!("{}", v), 32767i16, vec![]),
                options);
        }

        // ---- GPS / GNSS samples ----
        // Map per-frame proto::GpsData entries into telemetry-parser's canonical
        // GpsData struct (km/h, degrees, meters — see tags_impl.rs:352) and emit
        // them under GroupId::GPS / TagId::Data as Vec_GpsData, matching what the
        // GoPro (klv.rs:190), Insta360 (record.rs:206), and CAMM (mod.rs:176)
        // parsers produce. This keeps downstream consumers (telemetry export,
        // map overlays in dependent tools) working through the same code path
        // regardless of source format.
        //
        // UNIT CONVERSIONS:
        //   * speed:    proto m/s → tag km/h  (×3.6 — the canonical tag unit)
        //   * altitude: proto meters         → tag meters    (no conversion)
        //   * track:    proto degrees        → tag degrees   (no conversion)
        //
        // is_acquired RESOLUTION: trust the proto bool, but also treat any
        // fix_type ∈ {Fix2D, Fix3D, RTK} as acquired — matches the proto's
        // "consumers MAY treat fix_type as implying is_acquired" allowance and
        // means a producer that only sets fix_type still yields acquired samples
        // downstream.
        //
        // TIMESTAMP RESOLUTION: prefer unix_timestamp_s (absolute UTC seconds)
        // when set — that's what the existing GpsData.unix_timestamp field is
        // documented as. Fall back to sample_timestamp_us / 1e6 (camera-clock
        // seconds) so producers that only have camera-clock alignment still
        // emit a usable monotonic timeline; consumers that need wall-clock can
        // detect this by noting unrealistically small values (≪ 1e9).
        if !frame.gps.is_empty() {
            let mut gps_vec: Vec<GpsData> = Vec::with_capacity(frame.gps.len());
            for g in &frame.gps {
                let fix_type = g.fix_type.and_then(|n| GpsFixType::try_from(n).ok());
                let acquired = g.is_acquired
                    || matches!(fix_type, Some(GpsFixType::Fix2D | GpsFixType::Fix3D | GpsFixType::Rtk));
                let unix_timestamp = g.unix_timestamp_s
                    .or_else(|| g.sample_timestamp_us.map(|us| us / 1.0e6))
                    .unwrap_or(0.0);
                // Prefer the explicit (speed, track) pair when present. When only
                // the ENU velocity decomposition is provided (CAMM type-6 native
                // form), derive speed and track per the self-consistency formula
                // in the proto's GPSData docs so the canonical GpsData stays
                // populated regardless of which representation the producer used.
                let (speed_kmh, track_deg) = match (g.speed_mps, g.track_degrees) {
                    (Some(s), Some(t)) => (s as f64 * 3.6, t as f64),
                    (Some(s), None) => {
                        let track = match (g.velocity_east_mps, g.velocity_north_mps) {
                            (Some(ve), Some(vn)) if ve != 0.0 || vn != 0.0 => {
                                let mut t = (ve as f64).atan2(vn as f64).to_degrees();
                                if t < 0.0 { t += 360.0; }
                                t
                            }
                            _ => 0.0,
                        };
                        (s as f64 * 3.6, track)
                    }
                    (None, _) => {
                        match (g.velocity_east_mps, g.velocity_north_mps) {
                            (Some(ve), Some(vn)) => {
                                let speed_mps = ((ve as f64).powi(2) + (vn as f64).powi(2)).sqrt();
                                let track = if ve != 0.0 || vn != 0.0 {
                                    let mut t = (ve as f64).atan2(vn as f64).to_degrees();
                                    if t < 0.0 { t += 360.0; }
                                    t
                                } else {
                                    g.track_degrees.unwrap_or(0.0) as f64
                                };
                                (speed_mps * 3.6, track)
                            }
                            _ => (0.0, g.track_degrees.unwrap_or(0.0) as f64),
                        }
                    }
                };
                gps_vec.push(GpsData {
                    is_acquired:    acquired,
                    unix_timestamp,
                    lat:            g.latitude_degrees,
                    lon:            g.longitude_degrees,
                    altitude:       g.altitude_m.unwrap_or(0.0) as f64,
                    speed:          speed_kmh,
                    track:          track_deg,
                });
            }
            insert_tag(tag_map,
                tag!(parsed GroupId::GPS, TagId::Data, "GPS data", Vec_GpsData,
                     |v| format!("{:?}", v), gps_vec, vec![]),
                options);
        }
    }

    /// Frame interval in microseconds derived from the most appropriate header
    /// frame rate. Matches the `frame_interval` value gyroflow's
    /// `sony::ISTemp::calc_time_diff` uses for cross-frame wraparound math
    /// (= `(1_000_000.0 / fps) as i32` in `sony::stab_collect`).
    fn nominal_frame_interval_us(&self) -> f64 {
        // record_frame_rate is the muxed cadence — same as the per-frame
        // wall-clock cadence used by `info.timestamp_ms`. Fall back to
        // sensor_frame_rate, then 60fps, to avoid div-by-zero.
        let clip = self.clip();
        let fps = match clip {
            Some(c) if c.record_frame_rate > 0.0 => c.record_frame_rate as f64,
            Some(c) if c.sensor_frame_rate > 0.0 => c.sensor_frame_rate as f64,
            _ => 60.0,
        };
        1.0e6 / fps
    }

    /// EXPOSURE PRECEDENCE per the proto: exposure_time_us → numerator/denominator
    /// → shutter_angle_degrees / sensor_frame_rate. Returns microseconds.
    fn resolve_exposure_us(&self, frame: &gyroflow_proto::FrameMetadata) -> f64 {
        if let Some(t) = frame.exposure_time_us { return t.max(0.0); }
        if let (Some(num), Some(den)) = (frame.shutter_speed_numerator, frame.shutter_speed_denominator) {
            if num != 0 && den != 0 {
                return (num as f64 / den as f64).abs() * 1.0e6;
            }
        }
        if let Some(angle) = frame.shutter_angle_degrees {
            // Per the proto, shutter angle MUST be divided by sensor_frame_rate
            // (not record_frame_rate) — sensor cycle is what the angle indexes.
            let rate = match self.clip() {
                Some(c) if c.sensor_frame_rate > 0.0 => c.sensor_frame_rate as f64,
                Some(c) if c.record_frame_rate > 0.0 => c.record_frame_rate as f64,
                _ => 0.0,
            };
            if rate > 0.0 {
                return (angle as f64 / 360.0) * 1.0e6 / rate;
            }
        }
        0.0
    }

    /// Maps a proto LensData distortion variant to the gyroflow
    /// `DistortionModel::id()` string and the flat coefficient array
    /// gyroflow's KernelParams.k expects (per the per-model layout
    /// documented in gyroflow/src/core/stabilization/distortion_models/*).
    fn distortion_payload(lens: &gyroflow_proto::LensData) -> (Option<String>, Vec<f64>) {
        match lens.distortion.as_ref() {
            Some(Distortion::NoDistortion(_)) => (Some("opencv_fisheye".into()), Vec::new()),
            Some(Distortion::OpencvFisheye(c))     => (Some("opencv_fisheye".into()),     c.coefficients.iter().map(|x| *x as f64).collect()),
            Some(Distortion::OpencvStandard(c))    => (Some("opencv_standard".into()),    c.coefficients.iter().map(|x| *x as f64).collect()),
            Some(Distortion::LensfunPoly3(c))      => (Some("poly3".into()),              c.coefficients.iter().map(|x| *x as f64).collect()),
            Some(Distortion::LensfunPoly5(c))      => (Some("poly5".into()),              c.coefficients.iter().map(|x| *x as f64).collect()),
            Some(Distortion::LensfunPtlens(c))     => (Some("ptlens".into()),             c.coefficients.iter().map(|x| *x as f64).collect()),
            Some(Distortion::GenericPolynomial(c)) => {
                // Gyroflow's `generic_polynomial` distortion model accepts up to 12
                // dimensionless polynomial coefficients in indices k[0..=11]. Producers
                // emitting fewer terms (typical 6 or 8) are zero-padded — trailing zero
                // slots are a mathematical no-op (0·θⁿ = 0).
                let mut out: Vec<f64> = c.coefficients.iter().take(12).map(|x| *x as f64).collect();
                while out.len() < 12 { out.push(0.0); }
                (Some("generic_polynomial".into()), out)
            }
            None => (None, Vec::new()),
        }
    }

    /// Builds the lens_profile JSON consumed by gyro_source/mod.rs:209-213
    /// (Lens.Data → file_metadata.lens_profile). Carries enough info that
    /// `LensProfile::from_value` can hydrate a usable profile; per-frame
    /// values still override via `lens_params` (FocalLength /
    /// PixelFocalLength / DistortionCoefficients tags emitted above).
    fn build_lens_profile_json(&self) -> Option<serde_json::Value> {
        let clip = self.clip()?;
        let cam = self.camera()?;
        if clip.frame_width == 0 || clip.frame_height == 0 { return None; }
        let dist_model = self.distortion_model_name.as_deref().unwrap_or("opencv_fisheye");

        let is_vertical = clip.rotation_degrees.abs() == 90 || clip.rotation_degrees.abs() == 270;
        let out_w = if is_vertical { clip.frame_height } else { clip.frame_width };
        let out_h = if is_vertical { clip.frame_width  } else { clip.frame_height };

        // Default camera_matrix (overridden per-frame by Lens.PixelFocalLength).
        // We don't have the per-frame intrinsic matrix on the header path, so
        // place a neutral pinhole matrix here — the per-frame override fills in.
        let cx = clip.frame_width as f64 / 2.0;
        let cy = clip.frame_height as f64 / 2.0;
        let camera_matrix = serde_json::json!([
            [ 1.0, 0.0, cx ],
            [ 0.0, 1.0, cy ],
            [ 0.0, 0.0, 1.0 ]
        ]);

        let lens_model_full = if !cam.lens_brand.is_empty() && !cam.lens_model.is_empty() {
            format!("{} {}", cam.lens_brand, cam.lens_model)
        } else {
            cam.lens_model.clone()
        };

        let frame_readout_time_ms = if self.frame_readout_time_us > 0.0 {
            self.frame_readout_time_us / 1000.0
        } else {
            0.0
        };

        let mut profile = serde_json::json!({
            "calibrated_by": "Gyroflow Protobuf",
            "camera_brand":  cam.camera_brand,
            "camera_model":  cam.camera_model,
            "lens_model":    lens_model_full,
            "calib_dimension":  { "w": clip.frame_width, "h": clip.frame_height },
            "orig_dimension":   { "w": clip.frame_width, "h": clip.frame_height },
            "output_dimension": { "w": out_w,            "h": out_h },
            "frame_readout_time": frame_readout_time_ms,
            "official": true,
            "asymmetrical": false,
            "fisheye_params": {
                "camera_matrix":     camera_matrix,
                "distortion_coeffs": [],
            },
            "distortion_model": dist_model,
            "fps": if clip.record_frame_rate > 0.0 { clip.record_frame_rate } else { clip.sensor_frame_rate },
            "input_horizontal_stretch": if clip.pixel_aspect_ratio > 0.0 { clip.pixel_aspect_ratio as f64 } else { 1.0 },
            "input_vertical_stretch":   1.0,
            "sync_settings": {
                "initial_offset": 0,
                "initial_offset_inv": false,
                "search_size": 0.3,
                "max_sync_points": 5,
                "every_nth_frame": 1,
                "time_per_syncpoint": 0.5,
                "do_autosync": false
            },
            "calibrator_version": "---"
        });

        if let Some(cf) = cam.crop_factor {
            profile["crop_factor"] = serde_json::json!(cf);
        }
        Some(profile)
    }

    /// Convert proto MeshWarpData into the JSON shape gyroflow's
    /// sony::get_mesh_correction expects (gyroflow/src/core/gyro_source/sony.rs:435+).
    /// Only `size`, `divisions`, `mesh` and a placeholder non-zero `raw_mesh`
    /// are required — the consumer interpolates & inverse-maps the mesh.
    fn build_mesh_correction_json(&self, mesh: &gyroflow_proto::MeshWarpData) -> Option<serde_json::Value> {
        let gw = mesh.grid_width as usize;
        let gh = mesh.grid_height as usize;
        let expected = 2 * gw * gh;
        if gw < 2 || gh < 2 || mesh.warped_xy.len() != expected { return None; }

        // Compute anchor positions (uniform grid over [0, region_*]) so we can
        // synthesize a `raw_mesh` (displacement = warped − anchor) — only its
        // non-zero-ness matters for the validity check at sony.rs:454-463.
        let step_x = mesh.region_width  as f64 / (gw as f64 - 1.0);
        let step_y = mesh.region_height as f64 / (gh as f64 - 1.0);

        let mut mesh_arr   = Vec::with_capacity(gw * gh);
        let mut raw_mesh   = Vec::with_capacity(gw * gh);
        for j in 0..gh {
            for i in 0..gw {
                let k = j * gw + i;
                let wx = mesh.warped_xy[2 * k]     as f64;
                let wy = mesh.warped_xy[2 * k + 1] as f64;
                let ax = step_x * i as f64;
                let ay = step_y * j as f64;
                mesh_arr.push(serde_json::json!([wx, wy]));
                raw_mesh.push(serde_json::json!([wx - ax, wy - ay]));
            }
        }

        Some(serde_json::json!({
            "size":        [mesh.region_width, mesh.region_height],
            "divisions":   [gw, gh],
            "mesh":        mesh_arr,
            "raw_mesh":    raw_mesh,
            "divisions_2d":[1, 1],
        }))
    }
}
