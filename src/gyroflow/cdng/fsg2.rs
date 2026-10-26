// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2026 Adrian <adrian.eddy at gmail>

//! FSG2 payload kind 2: the SIGMA fp's gyro record, one per frame, written by the fpSup firmware mod. The camera copies
//! raw sensor counts and positions in the sample stream; the units and the times are worked out here, for the whole
//! take at once, as Gyroflow Protobuf messages.
//!
//! Little endian:
//! ```text
//!   0 u8   format version, 1
//!   1 u8   orientation code, 0
//!   2 u16  flags, FLAG_*
//!   4 u32  clip id: the camera's timer at the start of the take, the same in all its frames
//!   8 u32  the camera's frame number, from 0
//!  12 u32  gyro sample period, ps
//!  16 f32  rad/s per gyro count
//!  20 f32  g per accelerometer count
//!  24 u32  readout time of the sensor mode, ns
//!  28 u16  sensor mode
//!  30 u16  readout window x, y, width, height, in sensor pixels
//!  38 u16  frame mark: where in the samples the frame's hook ran (it ends the block, so it is the sample count)
//!  40 u32  index of the first sample in the take's sample stream
//!  44 u16  sample count
//!  46 u16  event count
//!  48      samples: i16 x, y, z
//!          events: u16 sample position, u8 type, u8 length, value
//!            1 accelerometer level, i16 x, y, z
//!            2 focus (reserved, not written)
//!            3 Vd, the sensor's frame sync: u32 camera timer, µs
//!          lens table, with FLAG_LENS_TABLE: u32 focal length (0.1 mm), u32 x 5 focus support points,
//!            f64 x 5 x 3 x 4 coefficients (see lensfit.rs)
//! ```
//! A sample's time is its index in the take's stream times the sample period. Neighbouring blocks may share samples
//! (guards), and a lost block leaves a gap in the indices.

use std::io::*;
use byteorder::{ ReadBytesExt, LittleEndian };

use super::lensfit::{ self, LensTable };
use super::tiff::{ DngInfo, Pictures };
use crate::gyroflow::gyroflow_proto as pb;
use pb::header::clip_metadata::ReadoutDirection;

pub const FLAG_DATA_LOST:   u16 = 1 << 0;
pub const FLAG_TRUNCATED:   u16 = 1 << 1;
pub const FLAG_FIRST_FRAME: u16 = 1 << 2;
pub const FLAG_LAST_FRAME:  u16 = 1 << 3;
pub const FLAG_OIS_ON:      u16 = 1 << 4;
pub const FLAG_EIS_ON:      u16 = 1 << 5;
pub const FLAG_LENS_TABLE:  u16 = 1 << 6;

const EVENT_LEVEL: u8 = 1;
const EVENT_FOCUS: u8 = 2;
const EVENT_VD:    u8 = 3;

/// The IMX410: 6000 x 4000 pixels of 5.983 µm
const SENSOR_SIZE: (u32, u32) = (6000, 4000);
const PIXEL_PITCH_NM: u32 = 5983;
const STANDARD_GRAVITY: f64 = 9.80665;

/// From a frame's Vd to the readout of the first row of its readout window, µs. The Vd is tied to the readout, not to
/// the exposure; the format's author measured this with Gyroflow's autosync to about ±1 ms, and A001_874 agrees
/// optically within 3 ms.
const VD_TO_READOUT_US: f64 = 900.0;

/// The gyro's axes in Gyroflow's IMU frame (x up, y left, towards the viewer; the integrators turn it into the camera's,
/// x right, y up, as `(-y, x, z)`): every axis negated. Its X turns the picture sideways and its Y up and down, as the
/// image motion of A001_874 shows (correlation 0.94 and 0.70). The accelerometer is a quarter turn from the gyro (its Y
/// is the gyro's X, its X negated the gyro's Y), so it's turned into the gyro's axes first, after which this puts
/// gravity's reaction on +X, up, for an upright camera.
const IMU_ORIENTATION: &str = "xyz";

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub position: u16,
    pub kind: u8,
    pub value: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub version: u8,
    pub orientation: u8,
    pub flags: u16,
    pub clip_id: u32,
    pub frame_seq: u32,
    pub sample_period_ps: u32,
    pub gyro_scale: f32,
    pub accel_scale: f32,
    pub readout_ns: u32,
    pub mode: u16,
    /// x, y, width, height of the sensor readout
    pub window: [u16; 4],
    pub frame_mark: u16,
    pub base_index: u32,
    pub samples: Vec<[i16; 3]>,
    pub events: Vec<Event>,
    pub lens_table: Option<LensTable>,
    /// Bytes after the record, which a later version of the format may add
    pub trailing_bytes: usize,
}

fn invalid(msg: String) -> Error {
    Error::new(ErrorKind::InvalidData, msg)
}

fn vector(x: &[u8]) -> [i16; 3] {
    [i16::from_le_bytes([x[0], x[1]]), i16::from_le_bytes([x[2], x[3]]), i16::from_le_bytes([x[4], x[5]])]
}

impl Record {
    pub fn parse(payload: &[u8]) -> Result<Self> {
        let truncated = |_| invalid("SIGMA fp gyro record is truncated".into());
        let mut d = Cursor::new(payload);
        let version = d.read_u8().map_err(truncated)?;
        if version != 1 {
            return Err(invalid(format!("Unsupported SIGMA fp gyro record version {version}")));
        }
        let orientation = d.read_u8().map_err(truncated)?;
        if orientation != 0 {
            return Err(invalid(format!("Unknown SIGMA fp gyro orientation {orientation}")));
        }
        let flags            = d.read_u16::<LittleEndian>().map_err(truncated)?;
        let clip_id          = d.read_u32::<LittleEndian>().map_err(truncated)?;
        let frame_seq        = d.read_u32::<LittleEndian>().map_err(truncated)?;
        let sample_period_ps = d.read_u32::<LittleEndian>().map_err(truncated)?;
        let gyro_scale       = d.read_f32::<LittleEndian>().map_err(truncated)?;
        let accel_scale      = d.read_f32::<LittleEndian>().map_err(truncated)?;
        let readout_ns       = d.read_u32::<LittleEndian>().map_err(truncated)?;
        let mode             = d.read_u16::<LittleEndian>().map_err(truncated)?;
        let mut window = [0u16; 4];
        d.read_u16_into::<LittleEndian>(&mut window).map_err(truncated)?;
        let frame_mark       = d.read_u16::<LittleEndian>().map_err(truncated)?;
        let base_index       = d.read_u32::<LittleEndian>().map_err(truncated)?;
        let sample_count     = d.read_u16::<LittleEndian>().map_err(truncated)? as usize;
        let event_count      = d.read_u16::<LittleEndian>().map_err(truncated)? as usize;

        if sample_period_ps == 0 {
            return Err(invalid("SIGMA fp gyro sample period is zero".into()));
        }
        if !(gyro_scale.is_finite() && gyro_scale > 0.0 && accel_scale.is_finite() && accel_scale > 0.0) {
            return Err(invalid("SIGMA fp gyro or accelerometer scale is not a positive number".into()));
        }

        let mut samples = vec![[0i16; 3]; sample_count];
        d.read_i16_into::<LittleEndian>(samples.as_flattened_mut()).map_err(truncated)?;

        let mut events = Vec::with_capacity(event_count);
        for _ in 0..event_count {
            let position = d.read_u16::<LittleEndian>().map_err(truncated)?;
            let kind     = d.read_u8().map_err(truncated)?;
            let mut value = vec![0u8; d.read_u8().map_err(truncated)? as usize];
            d.read_exact(&mut value).map_err(truncated)?;
            let expected = match kind { EVENT_LEVEL => Some(6), EVENT_FOCUS | EVENT_VD => Some(4), _ => None };
            if expected.is_some_and(|x| x != value.len()) {
                return Err(invalid(format!("SIGMA fp gyro event of type {kind} is {} bytes", value.len())));
            }
            events.push(Event { position, kind, value });
        }

        let lens_table = if flags & FLAG_LENS_TABLE != 0 {
            let calib_focal_tenths_mm = d.read_u32::<LittleEndian>().map_err(truncated)?;
            let mut axis = [0u32; 5];
            d.read_u32_into::<LittleEndian>(&mut axis).map_err(truncated)?;
            let mut nodes = [[[0f64; 4]; 3]; 5];
            d.read_f64_into::<LittleEndian>(nodes.as_flattened_mut().as_flattened_mut()).map_err(truncated)?;
            Some(LensTable { calib_focal_tenths_mm, axis, nodes })
        } else {
            None
        };

        Ok(Self {
            version, orientation, flags, clip_id, frame_seq, sample_period_ps, gyro_scale, accel_scale, readout_ns, mode, window,
            frame_mark, base_index, samples, events, lens_table,
            trailing_bytes: payload.len() - d.position() as usize,
        })
    }

    /// Positions of the Vd events in the take's sample stream
    fn vds(&self) -> impl Iterator<Item = f64> + '_ {
        self.events.iter().filter(|x| x.kind == EVENT_VD).map(|x| self.base_index as f64 + x.position as f64)
    }

    /// Everything the record holds but the samples (which are the gyro data), for the raw metadata
    pub fn to_json(&self) -> serde_json::Value {
        let flag_names = [
            (FLAG_DATA_LOST, "data_lost"), (FLAG_TRUNCATED, "truncated"), (FLAG_FIRST_FRAME, "first_frame"), (FLAG_LAST_FRAME, "last_frame"),
            (FLAG_OIS_ON, "ois_on"), (FLAG_EIS_ON, "eis_on"), (FLAG_LENS_TABLE, "lens_table"),
        ];
        let events: Vec<serde_json::Value> = self.events.iter().map(|x| match x.kind {
            EVENT_LEVEL => serde_json::json!({ "position": x.position, "type": "level", "accelerometer": vector(&x.value) }),
            EVENT_VD    => serde_json::json!({ "position": x.position, "type": "vd", "timer_us": u32::from_le_bytes(x.value[..4].try_into().unwrap()) }),
            EVENT_FOCUS => serde_json::json!({ "position": x.position, "type": "focus", "value": u32::from_le_bytes(x.value[..4].try_into().unwrap()) }),
            kind        => serde_json::json!({ "position": x.position, "type": kind, "value": pretty_hex::simple_hex(&x.value) }),
        }).collect();
        serde_json::json!({
            "version":             self.version,
            "orientation":         self.orientation,
            "flags":               flag_names.iter().filter(|x| self.flags & x.0 != 0).map(|x| x.1).collect::<Vec<_>>(),
            "clip_id":             self.clip_id,
            "frame_seq":           self.frame_seq,
            "sample_period_ps":    self.sample_period_ps,
            "gyro_scale_rad_s":    self.gyro_scale,
            "accel_scale_g":       self.accel_scale,
            "readout_ns":          self.readout_ns,
            "sensor_mode":         self.mode,
            "readout_window":      self.window,
            "frame_mark":          self.frame_mark,
            "base_index":          self.base_index,
            "sample_count":        self.samples.len(),
            "events":              events,
            "lens_table":          self.lens_table,
            "trailing_bytes":      self.trailing_bytes,
        })
    }
}

/// One frame of the take: its position in the sequence, its record and its DNG's own metadata
pub struct Frame {
    pub position: usize,
    pub record: Record,
    pub dng: DngInfo,
}

/// Where each frame's Vd (the sensor's frame sync) is in the sample stream, fractional.
///
/// The Vd fires once per sensor frame on the sensor's clock, so all of a take's Vd events lie on one line: Vd number n
/// at `start + n·step`, `step` being the samples a frame takes on the IMU clock (fitted, so it takes in the drift
/// between the clocks). A frame's hook comes a steady time after its Vd, except that now and then it runs late by more
/// than a frame. So a frame's Vd number is its `frame_seq` plus the offset the hooks around it agree on: the median of
/// nine frames' offsets, which one late hook doesn't move, while a sensor frame the recording skipped (a lasting step)
/// does.
fn vd_positions(frames: &[Frame], fps: f64) -> Result<Vec<f64>> {
    let period_s = frames[0].record.sample_period_ps as f64 * 1e-12;
    let nominal = 1.0 / (fps * period_s);
    let mut vds: Vec<f64> = frames.iter().flat_map(|x| x.record.vds()).collect();
    vds.sort_by(f64::total_cmp);
    vds.dedup();
    if vds.is_empty() {
        return Err(invalid("The SIGMA fp gyro records have no Vd frame sync (it needs a newer fpSup version)".into()));
    }
    // Least squares line through the events, numbered by the nominal frame time; one event (a take of a frame or two)
    // gets the nominal step
    let numbers: Vec<f64> = vds.iter().map(|x| ((x - vds[0]) / nominal).round()).collect();
    let count = vds.len() as f64;
    let (mean_n, mean_v) = (numbers.iter().sum::<f64>() / count, vds.iter().sum::<f64>() / count);
    let snn: f64 = numbers.iter().map(|n| (n - mean_n).powi(2)).sum();
    let snv: f64 = numbers.iter().zip(&vds).map(|(n, v)| (n - mean_n) * (v - mean_v)).sum();
    let step = if snn > 0.0 { snv / snn } else { nominal };
    if (step / nominal - 1.0).abs() > 0.01 {
        return Err(invalid(format!("SIGMA fp Vd every {step:.2} gyro samples doesn't match {fps:.3} fps ({nominal:.2} samples)")));
    }
    let start = mean_v - step * mean_n;

    let hooks: Vec<f64> = frames.iter().map(|x| x.record.base_index as f64 + x.record.frame_mark as f64).collect();
    let mut lags: Vec<f64> = hooks.iter().filter(|x| **x >= start).map(|x| (x - start).rem_euclid(step)).collect();
    if lags.is_empty() {
        return Err(invalid("No SIGMA fp frame hook after a Vd".into()));
    }
    lags.sort_by(f64::total_cmp);
    let lag = lags[lags.len() / 2];

    let offsets: Vec<i64> = frames.iter().zip(&hooks).map(|(x, hook)| ((hook - lag - start) / step).round() as i64 - x.record.frame_seq as i64).collect();
    Ok(frames.iter().enumerate().map(|(i, x)| {
        let mut around = offsets[i.saturating_sub(4)..(i + 5).min(offsets.len())].to_vec();
        around.sort_unstable();
        start + step * (x.record.frame_seq as i64 + around[around.len() / 2]) as f64
    }).collect())
}

/// The take as Gyroflow Protobuf messages, one per frame, the first with the header. `lens_table` is the take's, for
/// when the frames read don't include the take's first.
pub fn to_messages(frames: &[Frame], lens_table: Option<&LensTable>) -> Result<Vec<pb::Main>> {
    let first = frames.first().ok_or_else(|| invalid("No SIGMA fp gyro records".into()))?;
    // The frame is the picture the DNG defines, its default crop: what a DNG reader shows (Resolve, Adobe). A decoder
    // that leaves the crop out (FFmpeg) shows the active area around it, which the parser names alongside
    let (width, height) = first.dng.pictures().ok_or_else(|| invalid("The DNG has no image size".into()))?.crop_size;
    let fps = first.dng.frame_rate.ok_or_else(|| invalid("The DNG has no frame rate (FrameRate tag)".into()))?;
    let r = &first.record;
    if let Some(x) = frames.iter().find(|x| (x.record.sample_period_ps, x.record.gyro_scale, x.record.accel_scale) != (r.sample_period_ps, r.gyro_scale, r.accel_scale)) {
        return Err(invalid(format!("The SIGMA fp gyro sample period or scale changes within the take, at frame {}", x.position + 1)));
    }
    if frames.iter().any(|x| x.record.flags & FLAG_OIS_ON != 0) {
        log::warn!("SIGMA fp: the lens' optical stabilization was on, which isn't recorded and will fight the stabilization");
    }
    if frames.iter().any(|x| x.record.flags & FLAG_EIS_ON != 0) {
        log::warn!("SIGMA fp: the camera's electronic stabilization was on, which isn't recorded");
    }
    for x in frames.iter().filter(|x| x.record.flags & FLAG_TRUNCATED != 0) {
        log::warn!("SIGMA fp: frame {}: the camera cut the gyro record short to fit it in the MakerNote", x.position + 1);
    }

    let period_us = r.sample_period_ps as f64 * 1e-6;
    let deg_s = r.gyro_scale as f64 * 180.0 / std::f64::consts::PI;
    let m_s2 = r.accel_scale as f64 * STANDARD_GRAVITY;
    let accelerometer = |x: &Event| { let v = vector(&x.value); [(v[1] as f64 * m_s2) as f32, (-v[0] as f64 * m_s2) as f32, (v[2] as f64 * m_s2) as f32] };
    // Until the take's first level reading, that one, so no sample has a zero vector
    let mut held = frames.iter().flat_map(|x| &x.record.events).find(|x| x.kind == EVENT_LEVEL).map(accelerometer).unwrap_or_default();
    let lens_table = frames.iter().find_map(|x| x.record.lens_table.as_ref()).or(lens_table);

    let dng = &first.dng;
    let header = pb::Header {
        camera: Some(pb::header::CameraMetadata {
            camera_brand:         dng.make.clone().unwrap_or_else(|| "SIGMA".into()),
            camera_model:         camera_model(dng).unwrap_or_else(|| "fp".into()),
            camera_serial_number: dng.serial_number.clone(),
            firmware_version:     dng.software.clone(),
            lens_brand:           dng.lens_make.clone().unwrap_or_default(),
            lens_model:           dng.lens_model.clone().unwrap_or_default(),
            pixel_pitch_x_nm:     PIXEL_PITCH_NM,
            pixel_pitch_y_nm:     PIXEL_PITCH_NM,
            sensor_pixel_width:   SENSOR_SIZE.0,
            sensor_pixel_height:  SENSOR_SIZE.1,
            imu_orientation:      Some(IMU_ORIENTATION.into()),
            ..Default::default()
        }),
        clip: Some(pb::header::ClipMetadata {
            frame_width:             width,
            frame_height:            height,
            duration_us:             (frames.last().unwrap().position + 1) as f64 * 1e6 / fps,
            record_frame_rate:       fps as f32,
            sensor_frame_rate:       fps as f32,
            file_frame_rate:         fps as f32,
            imu_sample_rate:         (1e12 / r.sample_period_ps as f64).round() as u32,
            frame_readout_time_us:   Geometry::new(r.window, &first.dng).map_or(r.readout_ns as f64 / 1000.0, |g| g.readout_us(r).1),
            frame_readout_direction: ReadoutDirection::TopToBottom as i32,
            pixel_aspect_ratio:      1.0,
            ..Default::default()
        }),
    };

    let vds = vd_positions(frames, fps)?;
    let mut next_index = 0u64;
    let mut messages = Vec::with_capacity(frames.len());
    for (i, (frame, vd)) in frames.iter().zip(vds).enumerate() {
        let record = &frame.record;
        let geometry = Geometry::new(record.window, &frame.dng);
        // The readout of the picture's rows, which are the window's less those above and below the picture
        let (start_offset_us, readout_us) = geometry.as_ref().map_or((0.0, record.readout_ns as f64 / 1000.0), |g| g.readout_us(record));
        let start = vd * period_us + VD_TO_READOUT_US + start_offset_us;

        // A level event comes after the samples before it: it holds from the sample at its position on
        let mut levels: Vec<&Event> = record.events.iter().filter(|x| x.kind == EVENT_LEVEL).collect();
        levels.sort_by_key(|x| x.position);
        let mut levels = levels.into_iter().peekable();
        let mut imu = Vec::new();
        if record.flags & FLAG_DATA_LOST == 0 {
            for (position, sample) in record.samples.iter().enumerate() {
                while let Some(level) = levels.next_if(|x| x.position as usize <= position) {
                    held = accelerometer(level);
                }
                // A guard sample the block before already had
                let index = record.base_index as u64 + position as u64;
                if index < next_index { continue; }
                next_index = index + 1;
                imu.push(pb::ImuData {
                    sample_timestamp_us: Some(index as f64 * period_us),
                    gyroscope_x: (sample[0] as f64 * deg_s) as f32,
                    gyroscope_y: (sample[1] as f64 * deg_s) as f32,
                    gyroscope_z: (sample[2] as f64 * deg_s) as f32,
                    accelerometer_x: held[0],
                    accelerometer_y: held[1],
                    accelerometer_z: held[2],
                    ..Default::default()
                });
            }
        }
        if let Some(level) = levels.last() {
            held = accelerometer(level);
        }

        let area = geometry.as_ref().map(|g| g.picture_area());
        messages.push(pb::Main {
            magic_string: "GyroflowProtobuf".into(),
            protocol_version: 1,
            header: (i == 0).then(|| header.clone()),
            frame: Some(pb::FrameMetadata {
                start_timestamp_us: start,
                end_timestamp_us:   start + readout_us,
                frame_number:       u32::try_from(frame.position + 1).map_err(|_| invalid("Too many frames".into()))?,
                iso:                frame.dng.iso,
                exposure_time_us:   frame.dng.exposure_time_s.map(|x| x * 1e6),
                crop_x:             area.map(|x| x.0 as f32),
                crop_y:             area.map(|x| x.1 as f32),
                crop_width:         area.map(|x| x.2 as f32),
                crop_height:        area.map(|x| x.3 as f32),
                lens:               geometry.as_ref().and_then(|g| lens_data(&frame.dng, g, lens_table)).into_iter().collect(),
                imu,
                ..Default::default()
            }),
        });
    }
    Ok(messages)
}

/// The DNG's model without the make it starts with ("SIGMA fp" -> "fp")
fn camera_model(dng: &DngInfo) -> Option<String> {
    let model = dng.model.as_deref()?;
    let model = dng.make.as_deref().and_then(|make| model.strip_prefix(make)).unwrap_or(model).trim();
    (!model.is_empty()).then(|| model.to_owned())
}

/// Where a frame's pictures are on the sensor. The fp scales the full width of its readout window onto the active area
/// and shows as many rows as the active area's aspect takes, in the middle of the window (UHD reads 3412 rows and shows
/// 3376.6 of them in the active area, 3361 in the default crop); pixels are square
struct Geometry {
    pictures: Pictures,
    window: [f64; 4],
    /// Sensor pixels per picture pixel
    scale: f64,
    /// The active area's top left corner on the sensor
    image_origin: (f64, f64),
}

impl Geometry {
    fn new(window: [u16; 4], dng: &DngInfo) -> Option<Self> {
        let pictures = dng.pictures()?;
        let window = window.map(|v| v as f64);
        let (image_w, image_h) = pictures.image_f64();
        if window[2] <= 0.0 || window[3] <= 0.0 || image_w <= 0.0 {
            return None;
        }
        let scale = window[2] / image_w;
        let rows = (image_h * scale).min(window[3]);
        Some(Self { pictures, window, scale, image_origin: (window[0], window[1] + (window[3] - rows) / 2.0) })
    }

    /// The picture's (the default crop's) area on the sensor: x, y, width, height
    fn picture_area(&self) -> (f64, f64, f64, f64) {
        let (x, y) = self.pictures.crop_origin;
        let (w, h) = self.pictures.crop_f64();
        (self.image_origin.0 + x * self.scale, self.image_origin.1 + y * self.scale, w * self.scale, h * self.scale)
    }

    /// How long after the window's first row the picture's first row is read, and how long its rows take, µs: the
    /// readout time is the window's, whose rows are read one after another at a steady rate
    fn readout_us(&self, record: &Record) -> (f64, f64) {
        let row_us = record.readout_ns as f64 / 1000.0 / self.window[3];
        let (_, y, _, h) = self.picture_area();
        ((y - self.window[1]).max(0.0) * row_us, h * row_us)
    }
}

/// A frame's lens: focal length, aperture, focus distance and, when the camera's correction reaches the frame, the
/// camera matrix and fisheye coefficients for its focus, so breathing and distortion follow a focus pull
fn lens_data(dng: &DngInfo, geometry: &Geometry, table: Option<&LensTable>) -> Option<pb::LensData> {
    let mut lens = pb::LensData { f_number: dng.f_number.map(|x| x as f32), ..Default::default() };
    // No focal length: a lens without contacts. The camera's table is then the last electronic lens's, so neither it
    // nor the opcode describe this one
    if let Some(lens_mm) = dng.focal_length_mm {
        let distance_mm = dng.subject_distance_m.map(|x| x * 1000.0);
        let opcode = dng.warp_rectilinear;
        // The table at the frame's focus, normalized like the frame's own opcode (the camera writes both), else the opcode
        let (kr, calibrated_mm) = match table.and_then(|t| Some((t.coefficients_at(distance_mm)?, t.calib_focal_tenths_mm as f64 / 10.0))) {
            Some((kr, calibrated_mm)) => (Some(kr), Some(calibrated_mm)),
            None => (opcode.map(|x| x.kr), None),
        };
        let centre = opcode.map_or((0.5, 0.5), |x| x.centre);
        let focal_mm = lensfit::focal_mm(calibrated_mm, Some(lens_mm));
        lens.focal_length_mm = focal_mm.map(|x| x as f32);
        lens.focus_distance_mm = distance_mm.map(|x| x as f32);
        // The opcodes apply to the active area: its corners set their normalization, and the optical centre is in it
        let (image_w, image_h) = geometry.pictures.image_f64();
        if let Some((kr, mm)) = kr.zip(focal_mm) {
            let radius = lensfit::WarpRectilinear { kr, centre }.radius(image_w, image_h);
            if let Some((k, fx)) = lensfit::fit(&kr, radius, lensfit::focal_px(mm, geometry.scale)) {
                let (origin_x, origin_y) = geometry.pictures.crop_origin;
                let (fx, cx, cy) = (fx as f32, (centre.0 * image_w - origin_x) as f32, (centre.1 * image_h - origin_y) as f32);
                lens.camera_intrinsic_matrix = vec![fx, 0.0, cx, 0.0, fx, cy, 0.0, 0.0, 1.0];
                lens.distortion = Some(pb::lens_data::Distortion::OpencvFisheye(pb::OpenCvFisheye { coefficients: k.iter().map(|x| *x as f32).collect() }));
            }
        }
    }
    (lens != pb::LensData::default()).then_some(lens)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    const PERIOD_PS: u32 = 400_085_400;
    const GYRO_SCALE: f32 = 0.000137923;
    const ACCEL_SCALE: f32 = 0.0009765625;

    fn record(base_index: u32, frame_seq: u32, samples: Vec<[i16; 3]>) -> Record {
        Record {
            version: 1, orientation: 0, flags: 0, clip_id: 0x1234_5678, frame_seq, sample_period_ps: PERIOD_PS,
            gyro_scale: GYRO_SCALE, accel_scale: ACCEL_SCALE, readout_ns: 21_325_000, mode: 123, window: [0, 294, 6000, 3412],
            frame_mark: samples.len() as u16, base_index, samples, events: vec![], lens_table: None, trailing_bytes: 0,
        }
    }

    /// The camera's byte layout
    pub fn encode(r: &Record) -> Vec<u8> {
        let mut out = vec![r.version, r.orientation];
        out.extend_from_slice(&r.flags.to_le_bytes());
        for v in [r.clip_id, r.frame_seq, r.sample_period_ps] { out.extend_from_slice(&v.to_le_bytes()); }
        out.extend_from_slice(&r.gyro_scale.to_le_bytes());
        out.extend_from_slice(&r.accel_scale.to_le_bytes());
        out.extend_from_slice(&r.readout_ns.to_le_bytes());
        out.extend_from_slice(&r.mode.to_le_bytes());
        for v in r.window { out.extend_from_slice(&v.to_le_bytes()); }
        out.extend_from_slice(&r.frame_mark.to_le_bytes());
        out.extend_from_slice(&r.base_index.to_le_bytes());
        out.extend_from_slice(&(r.samples.len() as u16).to_le_bytes());
        out.extend_from_slice(&(r.events.len() as u16).to_le_bytes());
        for v in r.samples.as_flattened() { out.extend_from_slice(&v.to_le_bytes()); }
        for e in &r.events {
            out.extend_from_slice(&e.position.to_le_bytes());
            out.extend_from_slice(&[e.kind, e.value.len() as u8]);
            out.extend_from_slice(&e.value);
        }
        if let Some(t) = &r.lens_table {
            out.extend_from_slice(&t.calib_focal_tenths_mm.to_le_bytes());
            for v in t.axis { out.extend_from_slice(&v.to_le_bytes()); }
            for v in t.nodes.as_flattened().as_flattened() { out.extend_from_slice(&v.to_le_bytes()); }
        }
        out
    }

    fn level(position: u16, v: [i16; 3]) -> Event {
        Event { position, kind: EVENT_LEVEL, value: v.iter().flat_map(|x| x.to_le_bytes()).collect() }
    }
    fn vd(position: u16) -> Event {
        Event { position, kind: EVENT_VD, value: vec![0; 4] }
    }

    fn dng() -> DngInfo {
        DngInfo {
            make: Some("SIGMA".into()), model: Some("SIGMA fp".into()), image_size: Some((3856, 2170)), frame_rate: Some(25.0),
            exposure_time_s: Some(0.01), iso: Some(800), focal_length_mm: Some(28.0), f_number: Some(2.0), ..Default::default()
        }
    }

    /// A take with a Vd every nominal frame from the stream's start, all in the first block (only their positions count)
    fn take(records: Vec<Record>, dng: DngInfo) -> Vec<Frame> {
        let per_frame = 1e12 / (25.0 * PERIOD_PS as f64);
        let mut frames: Vec<Frame> = records.into_iter().enumerate().map(|(position, record)| Frame { position, record, dng: dng.clone() }).collect();
        let count = frames.len() + 2;
        for n in 0..count {
            frames[0].record.events.push(vd((n as f64 * per_frame).round() as u16));
        }
        frames
    }

    #[test]
    fn records_round_trip_through_the_camera_layout() {
        let mut r = record(1000, 7, vec![[1, -2, 3], [-32768, 32767, 0]]);
        r.flags = FLAG_FIRST_FRAME | FLAG_LENS_TABLE;
        r.events = vec![level(1, [10, -20, 1024]), vd(0), Event { position: 1, kind: 77, value: vec![1, 2, 3] }];
        r.lens_table = Some(LensTable { calib_focal_tenths_mm: 394, axis: [0, 18641, 37283, 49637, 55924], nodes: [[[1.0, -0.01, 0.005, -0.001]; 3]; 5] });
        let bytes = encode(&r);
        assert_eq!(&bytes[12..16], &PERIOD_PS.to_le_bytes());
        assert_eq!(&bytes[40..44], &1000u32.to_le_bytes());
        assert_eq!(Record::parse(&bytes).unwrap(), r);

        let mut longer = bytes.clone();
        longer.extend_from_slice(&[0; 3]);
        assert_eq!(Record::parse(&longer).unwrap().trailing_bytes, 3);
    }

    #[test]
    fn broken_records_are_refused() {
        let good = encode(&record(0, 0, vec![[1, 2, 3]]));
        let mut version = good.clone();
        version[0] = 2;
        assert!(Record::parse(&version).unwrap_err().to_string().contains("version 2"));
        assert!(Record::parse(&good[..good.len() - 1]).unwrap_err().to_string().contains("truncated"));
        let mut short_level = record(0, 0, vec![]);
        short_level.events = vec![Event { position: 0, kind: EVENT_LEVEL, value: vec![0; 4] }];
        assert!(Record::parse(&encode(&short_level)).is_err());
        let mut no_table = record(0, 0, vec![]);
        no_table.flags = FLAG_LENS_TABLE;
        assert!(Record::parse(&encode(&no_table)).unwrap_err().to_string().contains("truncated"));
        let mut zero_period = record(0, 0, vec![]);
        zero_period.sample_period_ps = 0;
        assert!(Record::parse(&encode(&zero_period)).is_err());
    }

    #[test]
    fn each_frame_finds_its_vd_past_late_hooks_and_skipped_frames() {
        let fps = 25.0;
        let per_frame = 1e12 / (fps * PERIOD_PS as f64);
        // The clocks drift (0.2 %, exaggerated)
        let vd_at = |n: f64| 1000.0 + n * per_frame * 1.002;
        // 40 frames; the recording skipped sensor frame 25, frame_seq goes on
        let sensor: Vec<f64> = (0..40).map(|k| if k < 25 { k as f64 } else { k as f64 + 1.0 }).collect();
        let lag = [62.0, 61.0, 63.0, 62.0, 62.0];
        let hooks: Vec<u32> = sensor.iter().enumerate().map(|(k, n)| (vd_at(*n) + lag[k % 5] + if k == 12 { 70.0 } else { 0.0 }).round() as u32).collect();
        // Every sensor frame's Vd fires, but some never make it into a block
        let vds: Vec<u32> = (0..41).filter(|n| n % 7 != 3).map(|n| vd_at(n as f64).round() as u32).collect();
        let mut frames = Vec::new();
        let mut from = 900;
        for (k, hook) in hooks.iter().enumerate() {
            let mut r = record(from, k as u32, vec![]);
            r.frame_mark = (hook - from) as u16;
            r.events = vds.iter().filter(|v| **v >= from && **v < *hook).map(|v| vd((v - from) as u16)).collect();
            from = *hook;
            frames.push(Frame { position: k, record: r, dng: dng() });
        }
        let got = vd_positions(&frames, fps).unwrap();
        for (k, n) in sensor.iter().enumerate() {
            assert!((got[k] - vd_at(*n)).abs() < 0.6, "frame {k}: {} vs {}", got[k], vd_at(*n));
        }
        for x in &mut frames { x.record.events.clear(); }
        assert!(vd_positions(&frames, fps).unwrap_err().to_string().contains("no Vd"));
    }

    #[test]
    fn units_times_and_orientation() {
        let messages = to_messages(&take(vec![record(100, 0, vec![[131, 0, -131]; 4])], dng()), None).unwrap();
        let frame = messages[0].frame.as_ref().unwrap();
        let period_us = PERIOD_PS as f64 / 1e6;
        // The frame's hook is 4 samples after the Vd at ~100, which is its own. The picture (the whole active area
        // here) shows the middle 3376.6 of the window's 3412 rows: its readout starts 17.7 rows in and takes its rows'
        let rows = 6000.0 * 2170.0 / 3856.0;
        let row_us = 21_325.0 / 3412.0;
        assert!((frame.start_timestamp_us - (100.0 * period_us + VD_TO_READOUT_US + (3412.0 - rows) / 2.0 * row_us)).abs() < 0.5 * period_us);
        assert!((frame.end_timestamp_us - frame.start_timestamp_us - rows * row_us).abs() < 1e-6);
        assert!((messages[0].header.as_ref().unwrap().clip.as_ref().unwrap().frame_readout_time_us - rows * row_us).abs() < 1e-6);
        assert!((frame.imu[3].sample_timestamp_us.unwrap() - 103.0 * period_us).abs() < 1e-6);
        let deg_s = 131.0 * GYRO_SCALE as f64 * 180.0 / std::f64::consts::PI;
        assert!((frame.imu[0].gyroscope_x as f64 - deg_s).abs() < 1e-4);
        assert!((frame.imu[0].gyroscope_z as f64 + deg_s).abs() < 1e-4);
        assert_eq!(frame.exposure_time_us.map(|x| x.round()), Some(10000.0));
        assert_eq!(frame.frame_number, 1);
        // The picture's rows in the middle of the readout window
        assert_eq!((frame.crop_x, frame.crop_width), (Some(0.0), Some(6000.0)));
        assert!((frame.crop_height.unwrap() - 6000.0 * 2170.0 / 3856.0).abs() < 1e-3);
        assert!((frame.crop_y.unwrap() + frame.crop_height.unwrap() / 2.0 - (294.0 + 3412.0 / 2.0)).abs() < 1e-3);

        let header = messages[0].header.as_ref().unwrap();
        let camera = header.camera.as_ref().unwrap();
        assert_eq!((camera.camera_brand.as_str(), camera.camera_model.as_str()), ("SIGMA", "fp"));
        assert_eq!(camera.imu_orientation.as_deref(), Some("xyz"));
        let clip = header.clip.as_ref().unwrap();
        assert_eq!((clip.frame_width, clip.frame_height, clip.imu_sample_rate), (3856, 2170, 2499));
    }

    #[test]
    fn guards_are_emitted_once_and_a_lost_block_leaves_a_gap() {
        // Frame 1 has 0..10, frame 2 sends 8 and 9 again and runs to 15, frame 3 lost its data, frame 4 goes on at 30
        let mut lost = record(15, 2, vec![]);
        lost.flags = FLAG_DATA_LOST;
        let messages = to_messages(&take(vec![record(0, 0, vec![[1, 0, 0]; 10]), record(8, 1, vec![[2, 0, 0]; 7]), lost, record(30, 3, vec![[3, 0, 0]; 5])], dng()), None).unwrap();
        let counts: Vec<usize> = messages.iter().map(|x| x.frame.as_ref().unwrap().imu.len()).collect();
        assert_eq!(counts, [10, 5, 0, 5]);
        let period_us = PERIOD_PS as f64 / 1e6;
        assert!((messages[1].frame.as_ref().unwrap().imu[0].sample_timestamp_us.unwrap() - 10.0 * period_us).abs() < 1e-6);
        assert!(messages.iter().skip(1).all(|x| x.header.is_none()));
        let start = |k: usize| messages[k].frame.as_ref().unwrap().start_timestamp_us;
        assert!(start(1) < start(2) && start(2) < start(3));
    }

    #[test]
    fn the_accelerometer_is_held_and_turned_into_the_gyro_axes() {
        let mut first = record(0, 0, vec![[0, 0, 0]; 4]);
        first.events = vec![level(2, [100, 200, 1024])];
        let mut second = record(4, 1, vec![[0, 0, 0]; 2]);
        second.events = vec![level(1, [-50, 0, 1000])];
        let messages = to_messages(&take(vec![first, second], dng()), None).unwrap();
        let g = ACCEL_SCALE as f64 * STANDARD_GRAVITY;
        let accel = |m: usize, i: usize| { let s = &messages[m].frame.as_ref().unwrap().imu[i]; [s.accelerometer_x as f64, s.accelerometer_y as f64, s.accelerometer_z as f64] };
        let near = |a: [f64; 3], b: [f64; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4);
        // Before the take's first reading, that reading
        let reading = [200.0 * g, -100.0 * g, 1024.0 * g];
        assert!(near(accel(0, 0), reading) && near(accel(0, 3), reading) && near(accel(1, 0), reading));
        assert!(near(accel(1, 1), [0.0, 50.0 * g, 1000.0 * g]));
    }

    // The green plane of a LUMIX S 40mm F2 (see lensfit.rs)
    fn lumix_40() -> LensTable {
        let green = [
            [0.987878098,  0.000230792, -0.001804660,  0.012710940],
            [0.995612771, -0.003803673, -0.003186222,  0.011056404],
            [0.999854447, -0.010596089,  0.003693395,  0.003874420],
            [0.999856513, -0.016680599,  0.013089761, -0.003420052],
            [0.999845119, -0.020184787,  0.018855619, -0.007281209],
        ];
        LensTable { calib_focal_tenths_mm: 400, axis: [0, 18641, 37283, 49637, 55924], nodes: green.map(|kr| [kr, kr, kr]) }
    }

    fn fisheye(lens: &pb::LensData) -> Vec<f32> {
        match &lens.distortion {
            Some(pb::lens_data::Distortion::OpencvFisheye(x)) => x.coefficients.clone(),
            x => panic!("expected OpenCV fisheye, got {x:?}"),
        }
    }

    #[test]
    fn the_lens_follows_a_focus_pull_through_the_takes_table() {
        let mut first = record(0, 0, vec![[0, 0, 0]]);
        first.flags |= FLAG_LENS_TABLE;
        first.lens_table = Some(lumix_40());
        first.window = [0, 0, 6000, 3376];
        let mut second = record(1, 1, vec![[0, 0, 0]]);
        second.window = first.window;
        let at = |m: Option<f64>| DngInfo { image_size: Some((1936, 1090)), focal_length_mm: Some(40.0), subject_distance_m: m, ..dng() };
        let mut frames = take(vec![first, second], at(None));
        frames[1].dng = at(Some(0.364));
        let messages = to_messages(&frames, None).unwrap();
        let lens = |k: usize| messages[k].frame.as_ref().unwrap().lens[0].clone();

        let infinity = lens(0);
        assert!((infinity.camera_intrinsic_matrix[0] as f64 - 2130.954872120334).abs() < 1e-2);
        assert_eq!(infinity.camera_intrinsic_matrix[0], infinity.camera_intrinsic_matrix[4]);
        assert_eq!((infinity.camera_intrinsic_matrix[2], infinity.camera_intrinsic_matrix[5]), (968.0, 545.0));
        assert!((fisheye(&infinity)[0] as f64 - 0.3434034356654717).abs() < 1e-6);
        assert_eq!(infinity.focal_length_mm, Some(40.0));
        let near = lens(1);
        assert!((near.camera_intrinsic_matrix[0] as f64 - 2156.792268728652).abs() < 1e-2);
        assert!((fisheye(&near)[0] as f64 - 0.2772084741693018).abs() < 1e-6);
        assert!((near.focus_distance_mm.unwrap() - 364.0).abs() < 1e-3);

        // Without the take's first frame, the table comes from the caller
        let mut later = take(vec![record(0, 5, vec![[0, 0, 0]])], at(None));
        later[0].record.window = [0, 0, 6000, 3376];
        let from_caller = to_messages(&later, Some(&lumix_40())).unwrap();
        assert_eq!(from_caller[0].frame.as_ref().unwrap().lens[0], infinity);
    }

    #[test]
    fn without_a_table_a_frame_uses_its_own_opcode_and_a_lens_without_contacts_gets_nothing() {
        let opcode = DngInfo { warp_rectilinear: Some(lensfit::WarpRectilinear { kr: [1.000334, -0.01484, 0.00518, -0.009761], centre: (0.5, 0.5) }), ..dng() };
        let messages = to_messages(&take(vec![record(0, 0, vec![])], opcode.clone()), None).unwrap();
        let lens = &messages[0].frame.as_ref().unwrap().lens[0];
        assert_eq!(fisheye(lens).len(), 4);
        // 28 mm over the 35.9 mm the 3856 pixels show, with the breathing
        assert!((lens.camera_intrinsic_matrix[0] as f64 - 3856.0 * 28.0 / 35.9 * 1.000334).abs() < 1e-2);

        let plain = to_messages(&take(vec![record(0, 0, vec![])], dng()), None).unwrap();
        let lens = &plain[0].frame.as_ref().unwrap().lens[0];
        assert!(lens.distortion.is_none() && lens.camera_intrinsic_matrix.is_empty());
        assert_eq!(lens.focal_length_mm, Some(28.0));

        // A manual lens: no focal length, no aperture; a stale table and opcode are not used
        let mut stale = record(0, 0, vec![]);
        stale.lens_table = Some(lumix_40());
        let manual = DngInfo { focal_length_mm: None, f_number: None, ..opcode };
        assert!(to_messages(&take(vec![stale], manual), None).unwrap()[0].frame.as_ref().unwrap().lens.is_empty());
    }

    #[test]
    fn the_frame_is_the_default_crop_on_the_sensor_rows_it_shows() {
        // UHD: the active area 3856x2170 is the window's full width, its default crop 3840x2160 at 8, 5
        let cropped = DngInfo {
            default_crop_origin: Some((8.0, 5.0)), default_crop_size: Some((3840.0, 2160.0)),
            warp_rectilinear: Some(lensfit::WarpRectilinear { kr: [1.0, -0.0148, 0.0052, -0.0098], centre: (0.5, 0.5) }),
            ..dng()
        };
        let messages = to_messages(&take(vec![record(0, 0, vec![[0, 0, 0]; 4])], cropped.clone()), None).unwrap();
        let clip = messages[0].header.as_ref().unwrap().clip.as_ref().unwrap();
        assert_eq!((clip.frame_width, clip.frame_height), (3840, 2160));
        let frame = messages[0].frame.as_ref().unwrap();
        let s = 6000.0 / 3856.0;
        let top = 294.0 + (3412.0 - 2170.0 * s) / 2.0 + 5.0 * s;
        let near = |a: Option<f32>, b: f64| (a.unwrap() as f64 - b).abs() < 1e-2;
        assert!(near(frame.crop_x, 8.0 * s) && near(frame.crop_y, top) && near(frame.crop_width, 3840.0 * s) && near(frame.crop_height, 2160.0 * s));
        // The readout of the crop's rows
        assert!((frame.end_timestamp_us - frame.start_timestamp_us - 2160.0 * s * 21_325.0 / 3412.0).abs() < 1e-6);
        // Same pixels as the active area's: the same focal length, the optical centre 8, 5 to the left and up
        let lens = &frame.lens[0];
        assert!((lens.camera_intrinsic_matrix[0] as f64 - 28.0 * 3856.0 / 35.9).abs() < 1e-2);
        assert_eq!((lens.camera_intrinsic_matrix[2], lens.camera_intrinsic_matrix[5]), (1920.0, 1080.0));

        // A default scale other than 1 resamples the crop: the frame is then the active area
        let scaled = DngInfo { default_scale: Some((1.5, 1.0)), ..cropped };
        let clip = to_messages(&take(vec![record(0, 0, vec![])], scaled), None).unwrap()[0].header.clone().unwrap().clip.unwrap();
        assert_eq!((clip.frame_width, clip.frame_height), (3856, 2170));
    }

    #[test]
    fn the_dng_must_give_the_size_and_rate() {
        assert!(to_messages(&take(vec![record(0, 0, vec![])], DngInfo { frame_rate: None, ..dng() }), None).unwrap_err().to_string().contains("FrameRate"));
        assert!(to_messages(&take(vec![record(0, 0, vec![])], DngInfo { image_size: None, ..dng() }), None).unwrap_err().to_string().contains("image size"));
    }
}
