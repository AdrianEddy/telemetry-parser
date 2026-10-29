// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2026 Adrian <adrian.eddy at gmail>

//! The segment table (`MTRM`) of a trimmed or joined recording: a JSON document that maps the file's frames, segment
//! by segment, back to the camera recordings they were cut from. Each segment is a run of frames that follow each other
//! in one source recording; where one segment ends and the next begins, the picture and the motion jump to another
//! moment (or another recording), so a reader treats every segment boundary as a hard boundary.
//!
//! # Placement
//!
//! - ISO base media files (MP4, MOV, …): a `moov/udta/MTRM` box whose payload is the JSON text. It is found by the
//!   file's structure (the top-level boxes, then `moov`, then `udta`): `moov` isn't always in the first or last
//!   megabytes (an Insta360 trailer can follow it), and compressed video can contain the bytes `MTRM`.
//! - MXF: a KLV item of the header metadata, after the primer pack (SMPTE ST 377-1 §9.1 lets KLV packets that aren't
//!   sets sit there; a decoder skips a key it doesn't know by its length, §6.3.2), whose value is the JSON text. Its key
//!   is `06.0e.2b.34.01.01.01.01.0f.4d.54.52.4d.01.00.00`, compared without byte 8 (the registry version, ST 336).
//! - MotionCam `.mcraw`: an item of type `MTRM` (`u32` little-endian `0x4D52544D`) right after the container metadata.
//! - RED clips: the text of an `<mp4trim>` element, a child of the root of the clip's `.RMD` sidecar.
//!
//! # Format, version 1
//!
//! ```json
//! {"version":1,"tool":"…",
//!  "sources":[{"name":"C0538.MP4","size":469906891,"frames":900,"fps":[30000,1001],
//!              "creation_time":"2024-11-17T13:12:46Z","clock":null}],
//!  "segments":[{"source":0,"src_first_frame":240,"frame_count":480,"out_first_frame":0,
//!               "src_start_us":8008000,"out_start_us":0,"duration_us":16016000,
//!               "src_capture_start_us":8008000,"out_capture_start_us":0,"capture_duration_us":16016000,
//!               "requested":{"unit":"frames","start":240,"end":720},"start":"before","entry":"idr",
//!               "end":"reorder_safe","clock_shift":null}]}
//! ```
//!
//! - Frames are presentation-order indices, frame 0 the first frame the file presents. Output frame
//!   `out_first_frame + i` is source frame `src_first_frame + i`, as far after the segment's start in both.
//! - Times are µs, the exact times rounded to the nearest (halves away from zero); `out_start_us + duration_us` is the
//!   next segment's `out_start_us`, and the same holds for the capture times, which equal the playback ones except for
//!   off-speed recordings (capture at a sensor rate other than the playback rate).
//! - `clock_shift`: by how many ticks of the source's `clock` the camera-clock values of the segment were moved back, so
//!   that the file's clock runs on across segments: a value `c` of this file is the source's `c + clock_shift` (modulo
//!   the clock's `modulus` for a clock that wraps). `null` exactly when the source's `clock` is.
//! - Sources may carry members of their own format; a reader ignores members it doesn't know. Two are used here:
//!   `sony_ibis_template` (see [`IbisTemplate`]) and Z CAM's `zcam_imu_origin_us`
//!   ([`TableSource::zcam_imu_origin_us`]).
//! - `telemetry_log`, in the table of a video whose telemetry is a log beside it (see [`TelemetryLog`]): the log that
//!   was written with the video, whose clock the sources' `clock` is.
//!
//! # Telemetry logs
//!
//! The telemetry of a video that no format claims is a log beside it (a `.gcsv`, a Runcam CSV, the Gyroflow protobuf's
//! JSON lines). A log has no frames of its own, nor a place for a table, so the video's table carries the log's: its
//! sources' `clock` is the log's clock, each `clock_shift` how far the log's values of the segment's frames were moved
//! back, and `telemetry_log` names the log, by its size and CRC-32. A log read as the telemetry of a video takes the
//! video's table, valid for the video ([`read_for_log`]):
//! 1. when the table names the log read (`telemetry_log`: the log has its `size` and `crc32`), the log is the one the
//!    video was trimmed with: the table is the log's, its clocks and shifts with it;
//! 2. otherwise (the table names no log, or another one) the log isn't known to be at the video's frames: the table's
//!    segments describe the video's frames whatever its log, every one a hard boundary of them, but no source has a
//!    `clock` and no segment a `clock_shift` (each `null`), and a warning says the log isn't the one the video was
//!    trimmed with.
//!
//! A table without `telemetry_log` never applies a clock to a log. The log's name isn't compared: a video and its log
//! are renamed together.
//!
//! # Validity
//!
//! Other tools copy `moov/udta` into files whose frames differ, so a reader uses a table only when it describes the file
//! it is in ([`SegmentTable::validate`]):
//! 1. the file has exactly one table, and it is a version-1 document with every member present with its type, each
//!    segment naming a source and lying within its frames, times not negative, rates and frame counts positive,
//!    `clock_shift` `null` exactly when its source's clock is (within `[0, modulus)` for a clock that wraps), and
//!    `telemetry_log`, when there, with its members of their types and a clock on every source;
//! 2. the segments follow each other: frames from 0, and each segment's times continue from the one before;
//! 3. they cover the file's video: as many frames as it presents, each segment starting at its first frame's
//!    presentation time, and the last one ending within one nominal frame of the end of the video.

use std::io::{ Read, Seek, SeekFrom, Result, Error, ErrorKind };
use byteorder::{ BigEndian, LittleEndian, ReadBytesExt };
use serde_json::Value;

/// The box type of the table in `moov/udta`, and the item type of a MotionCam recording's table
pub const BOX_TYPE: [u8; 4] = *b"MTRM";

/// The key of the table's KLV item in an MXF file's header metadata (byte 8, the registry version, isn't compared)
pub const MXF_KEY: [u8; 16] = [0x06, 0x0e, 0x2b, 0x34, 0x01, 0x01, 0x01, 0x01, 0x0f, 0x4d, 0x54, 0x52, 0x4d, 0x01, 0x00, 0x00];

/// The version of the format read here
pub const VERSION: u64 = 1;

/// A camera clock of a source: the unit of its segments' `clock_shift`
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TableClock {
    pub name: String,
    /// Ticks per second, `(numerator, denominator)`
    pub rate: (u64, u64),
    /// The tick count at which the clock wraps around to 0, `None` for one that doesn't
    pub modulus: Option<u64>,
}

/// A recording the file's frames come from
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TableSource {
    /// Its file name (no directory)
    pub name: String,
    pub size: u64,
    /// The frames its video presents
    pub frames: u64,
    /// Its nominal frame rate, `(numerator, denominator)`
    pub fps: (u64, u64),
    /// `YYYY-MM-DDTHH:MM:SSZ`
    pub creation_time: Option<String>,
    pub clock: Option<TableClock>,
    /// Members of the source's format, as the document has them
    #[serde(default)]
    pub members: serde_json::Map<String, Value>,
}

/// The stabilizer timing of a source clip (`sony_ibis_template`), a Sony clip's or a Gyroflow protobuf clip's (of an
/// MP4's track, or of its JSON lines log): the times of the clip's first sensor-shift (IBIS) and lens-shift (OIS)
/// samples, as telemetry-parser gives them (µs within their frame), in order. The splines of a frame's
/// stabilizer positions are spaced by the intervals of the clip's first samples, so a trimmed clip, whose first samples
/// aren't its source's, spaces them by these instead to have its source's splines. Each list is at least as long as
/// any frame's spline needs, or the source's samples
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct IbisTemplate {
    pub ibis: Vec<i32>,
    pub ois: Vec<i32>,
}

impl TableSource {
    /// The source's stabilizer timing, when it has one. A member that isn't one (lists of `i32` times) is ignored
    pub fn sony_ibis_template(&self) -> Option<IbisTemplate> {
        let member = self.members.get("sony_ibis_template")?;
        let list = |key: &str| -> Option<Vec<i32>> {
            member.get(key)?.as_array()?.iter().map(|v| v.as_i64().and_then(|v| i32::try_from(v).ok())).collect()
        };
        match (list("ibis"), list("ois")) {
            (Some(ibis), Some(ois)) => Some(IbisTemplate { ibis, ois }),
            _ => {
                log::warn!("The segment table's sony_ibis_template of {} isn't a template, ignored", self.name);
                None
            }
        }
    }
    /// Z CAM's IMU origin of the source (`zcam_imu_origin_us`): the raw `IMUData.timestamp` (µs on the source's
    /// `clock`) of the first IMU batch the source's readings are timed from (the Z CAM reader's), by which a trimmed
    /// file's readings are timed as the source's. A member that isn't a whole number is ignored
    pub fn zcam_imu_origin_us(&self) -> Option<u64> {
        let member = self.members.get("zcam_imu_origin_us")?;
        let origin = member.as_u64();
        if origin.is_none() {
            log::warn!("The segment table's zcam_imu_origin_us of {} isn't a whole number, ignored", self.name);
        }
        origin
    }
    /// The nominal frame rate in frames per second
    pub fn fps_f64(&self) -> f64 {
        self.fps.0 as f64 / self.fps.1 as f64
    }
}

/// One segment: a run of frames that follow each other in one source
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TrimSegment {
    /// Index into the table's sources
    pub source: usize,
    pub src_first_frame: u64,
    pub frame_count: u64,
    pub out_first_frame: u64,
    pub src_start_us: i64,
    pub out_start_us: i64,
    pub duration_us: i64,
    pub src_capture_start_us: i64,
    pub out_capture_start_us: i64,
    pub capture_duration_us: i64,
    pub clock_shift: Option<i64>,
    /// How the range was asked for (informational)
    pub requested: Option<Value>,
    /// The rules that made the segment's boundaries (informational)
    pub start: String,
    pub entry: String,
    pub end: String,
}

impl TrimSegment {
    /// The frame after its last one, in the file
    pub fn out_end_frame(&self) -> u64 { self.out_first_frame + self.frame_count }
    /// The end of its last frame in the file, µs
    pub fn out_end_us(&self) -> i64 { self.out_start_us + self.duration_us }
    /// Whether the file's frame `frame` is one of the segment's
    pub fn has_frame(&self, frame: u64) -> bool { frame >= self.out_first_frame && frame < self.out_end_frame() }
}

/// The telemetry log written with a video, which its table's clocks describe (`telemetry_log`, see [Telemetry
/// logs](self#telemetry-logs))
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TelemetryLog {
    /// Its file name as it was written (informational, never compared: a video and its log are renamed together),
    /// `None` when its trim had no names for its outputs
    pub name: Option<String>,
    /// What it is read as: `gcsv` (a gcsv, also under `.csv`), `runcam` (Runcam's CSV) or `jsonl` (the Gyroflow
    /// protobuf's messages as JSON lines)
    pub kind: String,
    /// Its bytes
    pub size: u64,
    /// The CRC-32 of its bytes, the CRC of ISO/IEC 3309 and ITU-T V.42 that zlib's `crc32()` and gzip (RFC 1952)
    /// compute: with its size, it identifies the log as a gzip member's trailer identifies its content
    pub crc32: u32,
}

/// A segment table, read and checked ([`SegmentTable::parse`])
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SegmentTable {
    pub sources: Vec<TableSource>,
    pub segments: Vec<TrimSegment>,
    /// The telemetry log written with the file, which the sources' clocks are of, `None` for a table that names none
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telemetry_log: Option<TelemetryLog>,
}

/// The frames a file's video presents, which a table must cover (validity rule 3)
#[derive(Debug, Clone)]
pub enum VideoFrames {
    /// Evenly spaced frames: frame `k` at `k · den / num` seconds, the video ending at `frames · den / num`
    Constant { frames: u64, rate: (u64, u64) },
    /// The frames' presentation times
    Times(VideoFrameTimes),
}

/// The presentation times of a video's frames, in ticks of `timescale`, in presentation order, as the decoders give them
/// (an ISO base media file's as FFmpeg reads it: `video_frame_times`)
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VideoFrameTimes {
    pub timescale: u64,
    /// Where each frame starts
    pub starts: Vec<i64>,
    /// Where the last frame ends: the end of the video
    pub end: i64,
    /// The nominal frame duration: the most common one
    pub nominal: i64,
}

impl VideoFrameTimes {
    pub fn frames(&self) -> usize { self.starts.len() }
    /// Frame `k`'s presentation time in ms
    pub fn time_ms(&self, k: usize) -> Option<f64> {
        self.starts.get(k).map(|t| *t as f64 * 1000.0 / self.timescale as f64)
    }
    /// The end of the video in ms
    pub fn end_ms(&self) -> f64 { self.end as f64 * 1000.0 / self.timescale as f64 }
    /// The nominal frame duration in ms
    pub fn nominal_ms(&self) -> f64 { self.nominal as f64 * 1000.0 / self.timescale as f64 }
    /// Whether every frame lasts the nominal duration, the last one ending the video included
    pub fn is_constant(&self) -> bool {
        self.starts.windows(2).all(|w| w[1] - w[0] == self.nominal) && self.starts.last().is_none_or(|last| self.end - last == self.nominal)
    }
}

/// `ticks / timescale` seconds in µs, rounded to the nearest, halves away from zero
fn ticks_to_us(ticks: i128, timescale: u128) -> Option<i64> {
    if timescale == 0 { return None; }
    let n = ticks.checked_mul(1_000_000)?;
    let d = timescale as i128;
    let q = (n.abs() * 2 + d) / (2 * d);
    i64::try_from(if n < 0 { -q } else { q }).ok()
}

impl VideoFrames {
    fn frames(&self) -> u64 {
        match self {
            Self::Constant { frames, .. } => *frames,
            Self::Times(t) => t.starts.len() as u64,
        }
    }
    /// The presentation time of frame `k` (`k` up to the frame count: the end of the video) in µs, as the table rounds it
    fn time_us(&self, k: u64) -> Option<i64> {
        match self {
            Self::Constant { rate, .. } => ticks_to_us(k as i128 * rate.1 as i128, rate.0 as u128),
            Self::Times(t) => {
                let ticks = if k as usize == t.starts.len() { t.end } else { *t.starts.get(k as usize)? };
                ticks_to_us(ticks as i128, t.timescale as u128)
            }
        }
    }
    /// Whether `end_us` is within one nominal frame and half a µs (the rounding of `end_us`) of the end of the video,
    /// exactly: compared in units of 1 / (2 · timescale) µs
    fn ends_near(&self, end_us: i64) -> bool {
        let (end, timescale, nominal) = match self {
            Self::Constant { frames, rate } => (*frames as i128 * rate.1 as i128, rate.0 as i128, rate.1 as i128),
            Self::Times(t) => (t.end as i128, t.timescale as i128, t.nominal as i128),
        };
        // A table's end and a rate it states itself (a recording of one frame's) can overflow the product: such an end
        // is no video's
        let diff = (2 * end_us as i128).checked_mul(timescale).and_then(|t| t.checked_sub(2 * end * 1_000_000)).and_then(i128::checked_abs);
        diff.is_some_and(|diff| diff <= 2 * nominal * 1_000_000 + timescale)
    }
    /// The nominal frame duration in µs, unrounded
    fn nominal_us(&self) -> f64 {
        match self {
            Self::Constant { rate, .. } => rate.1 as f64 * 1e6 / rate.0 as f64,
            Self::Times(t) => t.nominal as f64 * 1e6 / t.timescale as f64,
        }
    }
}

/// Whether `s` is `YYYY-MM-DDTHH:MM:SSZ`, a date of the proleptic Gregorian calendar and a time of day without a leap
/// second (RFC 3339 in UTC, to the second)
fn is_utc_second(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' || b[19] != b'Z' {
        return false;
    }
    let num = |r: std::ops::Range<usize>| -> Option<u32> {
        b[r].iter().try_fold(0u32, |v, &c| c.is_ascii_digit().then(|| v * 10 + (c - b'0') as u32))
    };
    let (Some(y), Some(mo), Some(d), Some(h), Some(mi), Some(se)) = (num(0..4), num(5..7), num(8..10), num(11..13), num(14..16), num(17..19)) else {
        return false;
    };
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let days = match mo {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => if leap { 29 } else { 28 },
        _ => return false,
    };
    (1..=days).contains(&d) && h < 24 && mi < 60 && se < 60
}

impl SegmentTable {
    /// Reads a version-1 document and checks rules 1 and 2 of its validity (the [module documentation](self#validity)):
    /// every member present with its type, and the segments following each other. Members it doesn't know are ignored
    /// (a source keeps them, [`TableSource::members`])
    pub fn parse(text: &[u8]) -> std::result::Result<Self, String> {
        let v: Value = serde_json::from_slice(text).map_err(|e| format!("not a JSON document: {e}"))?;
        let version = v.get("version").and_then(Value::as_u64).ok_or("no version")?;
        if version != VERSION {
            return Err(format!("version {version}, read is version {VERSION}"));
        }
        let pair = |v: &Value, what: &str| -> std::result::Result<(u64, u64), String> {
            match v.as_array().map(Vec::as_slice) {
                Some([a, b]) => Ok((a.as_u64().ok_or_else(|| format!("{what} isn't a pair of whole numbers"))?, b.as_u64().ok_or_else(|| format!("{what} isn't a pair of whole numbers"))?)),
                _ => Err(format!("{what} isn't a pair of numbers")),
            }
        };
        const SOURCE_MEMBERS: [&str; 6] = ["name", "size", "frames", "fps", "creation_time", "clock"];
        let mut sources = Vec::new();
        for s in v.get("sources").and_then(Value::as_array).ok_or("no sources")? {
            let obj = s.as_object().ok_or("a source isn't an object")?;
            let field = |k: &str| obj.get(k).ok_or_else(|| format!("a source has no {k}"));
            let clock = match field("clock")? {
                Value::Null => None,
                c => Some(TableClock {
                    name: c.get("name").and_then(Value::as_str).ok_or("a clock without a name")?.to_owned(),
                    rate: pair(c.get("rate").ok_or("a clock without a rate")?, "a clock's rate")?,
                    modulus: match c.get("modulus").ok_or("a clock without a modulus")? {
                        Value::Null => None,
                        m => Some(m.as_u64().ok_or("a clock's modulus isn't a whole number")?),
                    },
                }),
            };
            sources.push(TableSource {
                name: field("name")?.as_str().ok_or("a source's name isn't a string")?.to_owned(),
                size: field("size")?.as_u64().ok_or("a source's size isn't a whole number")?,
                frames: field("frames")?.as_u64().ok_or("a source's frames isn't a whole number")?,
                fps: pair(field("fps")?, "a source's fps")?,
                creation_time: match field("creation_time")? {
                    Value::Null => None,
                    t => Some(t.as_str().ok_or("a source's creation_time isn't a string")?.to_owned()),
                },
                clock,
                members: obj.iter().filter(|(k, _)| !SOURCE_MEMBERS.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect(),
            });
        }
        let mut segments = Vec::new();
        for e in v.get("segments").and_then(Value::as_array).ok_or("no segments")? {
            let field = |k: &str| e.get(k).ok_or_else(|| format!("a segment has no {k}"));
            let u = |k: &str| field(k).and_then(|v| v.as_u64().ok_or_else(|| format!("a segment's {k} isn't a whole number")));
            let i = |k: &str| field(k).and_then(|v| v.as_i64().ok_or_else(|| format!("a segment's {k} isn't a whole number")));
            let s = |k: &str| field(k).and_then(|v| v.as_str().map(str::to_owned).ok_or_else(|| format!("a segment's {k} isn't a string")));
            segments.push(TrimSegment {
                source: usize::try_from(u("source")?).map_err(|_| "a segment's source is out of range")?,
                src_first_frame: u("src_first_frame")?,
                frame_count: u("frame_count")?,
                out_first_frame: u("out_first_frame")?,
                src_start_us: i("src_start_us")?,
                out_start_us: i("out_start_us")?,
                duration_us: i("duration_us")?,
                src_capture_start_us: i("src_capture_start_us")?,
                out_capture_start_us: i("out_capture_start_us")?,
                capture_duration_us: i("capture_duration_us")?,
                requested: match field("requested")? {
                    Value::Null => None,
                    r @ Value::Object(_) => Some(r.clone()),
                    _ => return Err("a segment's requested isn't an object or null".into()),
                },
                start: s("start")?,
                entry: s("entry")?,
                end: s("end")?,
                clock_shift: match field("clock_shift")? {
                    Value::Null => None,
                    v => Some(v.as_i64().ok_or("a segment's clock_shift isn't a whole number")?),
                },
            });
        }
        let telemetry_log = match v.get("telemetry_log") {
            None => None,
            Some(l) => {
                let obj = l.as_object().ok_or("telemetry_log isn't an object")?;
                let field = |k: &str| obj.get(k).ok_or_else(|| format!("telemetry_log has no {k}"));
                Some(TelemetryLog {
                    name: match field("name")? {
                        Value::Null => None,
                        n => Some(n.as_str().ok_or("telemetry_log's name isn't a string")?.to_owned()),
                    },
                    kind: field("kind")?.as_str().ok_or("telemetry_log's kind isn't a string")?.to_owned(),
                    size: field("size")?.as_u64().ok_or("telemetry_log's size isn't a whole number")?,
                    crc32: field("crc32")?.as_u64().and_then(|c| u32::try_from(c).ok()).ok_or("telemetry_log's crc32 isn't a CRC-32")?,
                })
            }
        };
        let table = Self { sources, segments, telemetry_log };
        table.check()?;
        Ok(table)
    }

    /// Rules 1 and 2 on the document alone
    fn check(&self) -> std::result::Result<(), String> {
        if self.sources.is_empty() {
            return Err("no sources".into());
        }
        for (k, s) in self.sources.iter().enumerate() {
            if s.frames == 0 || s.fps.0 == 0 || s.fps.1 == 0 {
                return Err(format!("source {k} has {} frames at {}/{} fps", s.frames, s.fps.0, s.fps.1));
            }
            if let Some(t) = s.creation_time.as_deref().filter(|t| !is_utc_second(t)) {
                return Err(format!("source {k}'s creation_time {t:?} isn't a UTC time YYYY-MM-DDTHH:MM:SSZ"));
            }
            if let Some(c) = &s.clock && (c.rate.0 == 0 || c.rate.1 == 0 || c.modulus == Some(0)) {
                return Err(format!("source {k}'s clock runs at {}/{} ticks per second, wrapping at {:?}", c.rate.0, c.rate.1, c.modulus));
            }
            // A table that names its telemetry log has the log's clock on every source
            if self.telemetry_log.is_some() && s.clock.is_none() {
                return Err(format!("the table names a telemetry log, and source {k} has no clock"));
            }
        }
        if self.segments.is_empty() {
            return Err("no segments".into());
        }
        let (mut frame, mut time, mut capture) = (0u64, None, None);
        for (k, e) in self.segments.iter().enumerate() {
            let source = self.sources.get(e.source).ok_or_else(|| format!("segment {k} names source {} of {}", e.source, self.sources.len()))?;
            if e.frame_count == 0 {
                return Err(format!("segment {k} has no frames"));
            }
            let (Some(end), Some(src_end)) = (e.out_first_frame.checked_add(e.frame_count), e.src_first_frame.checked_add(e.frame_count)) else {
                return Err(format!("segment {k}'s frames overflow"));
            };
            if src_end > source.frames {
                return Err(format!("segment {k} has source frames {}..{src_end}, its source has {}", e.src_first_frame, source.frames));
            }
            if [e.src_start_us, e.out_start_us, e.duration_us, e.src_capture_start_us, e.out_capture_start_us, e.capture_duration_us].iter().any(|t| *t < 0) {
                return Err(format!("segment {k} has a negative time"));
            }
            let sum = |a: i64, b: i64| a.checked_add(b).ok_or_else(|| format!("segment {k}'s times overflow"));
            sum(e.src_start_us, e.duration_us)?;
            sum(e.src_capture_start_us, e.capture_duration_us)?;
            if e.out_first_frame != frame {
                return Err(format!("segment {k} starts at frame {}, not {frame}", e.out_first_frame));
            }
            if let (Some(t), Some(c)) = (time, capture) && (t, c) != (e.out_start_us, e.out_capture_start_us) {
                return Err(format!("segment {k} starts at {} µs (capture {} µs), not where the one before ends, {t} µs (capture {c} µs)", e.out_start_us, e.out_capture_start_us));
            }
            frame = end;
            time = Some(sum(e.out_start_us, e.duration_us)?);
            capture = Some(sum(e.out_capture_start_us, e.capture_duration_us)?);
            match (&source.clock, e.clock_shift) {
                (None, None) => {}
                (Some(c), Some(d)) if c.modulus.is_none_or(|m| u64::try_from(d).is_ok_and(|d| d < m)) => {}
                (c, d) => return Err(format!("segment {k} has the clock shift {d:?} for the clock {c:?}")),
            }
        }
        Ok(())
    }

    /// Rule 3: the segments cover the frames of `video`, the video of the file the table is in. As many frames as it
    /// presents; each segment's start the presentation time of its first frame, exactly (both are rounded to the µs the
    /// same way); and the last segment's end the end of the video, within one nominal frame (a file states that end only
    /// as precisely as its last frame's duration and its edit list allow, while the table has it exactly)
    pub fn validate(&self, video: &VideoFrames) -> std::result::Result<(), String> {
        let last = self.segments.last().ok_or("no segments")?;
        let frames = last.out_end_frame();
        if frames != video.frames() {
            return Err(format!("its segments cover {frames} frames, the file's video has {}", video.frames()));
        }
        for e in &self.segments {
            let t = video.time_us(e.out_first_frame).ok_or("a frame time out of range")?;
            if t != e.out_start_us {
                return Err(format!("it puts frame {} at {} µs, the file's video at {t} µs", e.out_first_frame, e.out_start_us));
            }
        }
        if !video.ends_near(last.out_end_us()) {
            return Err(format!("it ends at {} µs, more than a frame of {:.3} µs from the end of the file's video at {} µs", last.out_end_us(), video.nominal_us(), video.time_us(frames).unwrap_or_default()));
        }
        Ok(())
    }

    /// The segment that has the file's frame `frame`; past the last frame, the last segment
    pub fn segment_of_frame(&self, frame: u64) -> usize {
        self.segments.partition_point(|s| s.out_end_frame() <= frame).min(self.segments.len() - 1)
    }
    /// The segment that presents the file's time `t_us`: before the first one, the first; past the last, the last
    pub fn segment_of_time_us(&self, t_us: i64) -> usize {
        self.segments.partition_point(|s| s.out_end_us() <= t_us).min(self.segments.len() - 1)
    }
    /// The source of a segment
    pub fn source_of(&self, segment: &TrimSegment) -> &TableSource {
        &self.sources[segment.source]
    }
}

/// A table found in a file: `Ok` when it is valid for the file, `Err` with the reason otherwise (a reader must ignore it)
pub type Found = std::result::Result<SegmentTable, String>;

/// Checks the JSON text of a file's one table against the file's video
fn check_text(text: &[u8], video: Option<&VideoFrames>) -> Found {
    let table = SegmentTable::parse(text)?;
    table.validate(video.ok_or("the file's video frames can't be read")?)?;
    Ok(table)
}

/// The segment table of a file that is an ISO base media file, an MXF file or a MotionCam recording: `None` when it has
/// none, or none can be found (where one would be can't be read). A RED clip's table is in its RMD sidecar ([`read_rmd`])
pub fn read<R: Read + Seek>(stream: &mut R, size: u64) -> Option<Found> {
    let mut head = [0u8; 8];
    stream.seek(SeekFrom::Start(0)).ok()?;
    let n = stream.read(&mut head).ok()?;
    let found = if n >= 7 && &head[..7] == b"MOTION " {
        read_mcraw(stream, size)
    } else if let Ok(Some(start)) = mxf_header_partition(stream, size) {
        read_mxf(stream, size, start)
    } else {
        read_isobmff(stream, size)
    };
    let _ = stream.seek(SeekFrom::Start(0));
    // A reader's error is one before it found a table: what can't be read after that is the table's (`Ok(Some(Err))`)
    match found {
        Ok(found) => found,
        Err(e) => {
            log::debug!("No segment table found, the file can't be read where one would be: {e}");
            None
        }
    }
}

// ------------------------------- Telemetry logs -------------------------------

impl SegmentTable {
    /// The table as one of a telemetry log that isn't known to be at its frames (rule 2 of [Telemetry
    /// logs](self#telemetry-logs)): its segments, without a clock on any source or a shift on any segment, and naming
    /// no log
    fn without_log_clocks(mut self) -> Self {
        for s in &mut self.sources { s.clock = None; }
        for e in &mut self.segments { e.clock_shift = None; }
        self.telemetry_log = None;
        self
    }
}

/// The CRC-32 of the first `size` bytes of `stream` (`TelemetryLog::crc32`), `None` when it has fewer. The stream is
/// left at its start
fn crc32_of<R: Read + Seek>(stream: &mut R, size: u64) -> Result<Option<u32>> {
    stream.seek(SeekFrom::Start(0))?;
    let mut hasher = crc32fast::Hasher::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut left = size;
    while left > 0 {
        let want = left.min(buf.len() as u64) as usize;
        let n = stream.read(&mut buf[..want])?;
        if n == 0 { break; }
        hasher.update(&buf[..n]);
        left -= n as u64;
    }
    stream.seek(SeekFrom::Start(0))?;
    Ok((left == 0).then(|| hasher.finalize()))
}

/// The segment table a telemetry log takes from the video it is read as the telemetry of ([`read_for_log`])
#[derive(Debug, Clone)]
pub struct LogTable {
    /// The video's table, `Err` with why when it isn't valid for the video: the log's when it names the log, otherwise
    /// its segments without their clocks and shifts
    pub found: Found,
    /// Whether the table names the log (rule 1 of [Telemetry logs](self#telemetry-logs)): the log is the one the
    /// video was trimmed with, so its readings are at the video's frames. Otherwise (rule 2) where each segment's
    /// readings are in the log isn't known: its segments are the video's frames' only
    pub names_log: bool,
}

/// The segment table of a telemetry log of `log_size` bytes (`log`) read as the telemetry of a video (`video`), by
/// the rule of [Telemetry logs](self#telemetry-logs): the video's table, valid for the video (a log has no frames of
/// its own to check it against), which is the log's when it names the log (its size and CRC-32), and otherwise its
/// segments without their clocks and shifts, with a warning that `log_name` isn't the log the video was trimmed with.
/// `None` when the video has no table. Both streams are left at their start
pub fn read_for_log<V: Read + Seek, L: Read + Seek>(video: &mut V, video_size: u64, log: &mut L, log_size: u64, log_name: &str) -> Option<LogTable> {
    let table = match read(video, video_size)? {
        Ok(table) => table,
        Err(e) => return Some(LogTable { found: Err(e), names_log: false }),
    };
    let why = match &table.telemetry_log {
        None => "the video's segment table names none: it was trimmed without its log".to_owned(),
        Some(named) if named.size != log_size => format!("it has {log_size} bytes, the video's segment table names a log of {} bytes", named.size),
        Some(named) => match crc32_of(log, log_size) {
            Ok(Some(crc)) if crc == named.crc32 => return Some(LogTable { found: Ok(table), names_log: true }),
            Ok(Some(crc)) => format!("its CRC-32 is {crc}, the video's segment table names a log of CRC-32 {}", named.crc32),
            Ok(None) => format!("it has fewer than its {log_size} bytes"),
            Err(e) => format!("it can't be read: {e}"),
        },
    };
    let early = table.segments.first().map_or(0.0, |s| s.src_start_us as f64 / 1e6);
    log::warn!("{log_name} isn't the telemetry log the video was trimmed with ({why}), so it isn't known to be at the video's frames: the video's segments are taken without their clocks (a copy of its source's untrimmed log is read {early:.3} s early)");
    Some(LogTable { found: Ok(table.without_log_clocks()), names_log: false })
}

// ------------------------------- ISO base media files -------------------------------

/// The size of a box and of its header, at the start of `data`, `remaining` the bytes from its start to the end of the
/// box it is in (ISO/IEC 14496-12 §4.2: a size of 1 is a 64-bit size after the type, 0 the rest of the enclosing box).
/// `None` for a box that doesn't fit
fn box_size(data: &[u8], remaining: u64) -> Option<(u64, u64)> {
    let size32 = u32::from_be_bytes(data.get(0..4)?.try_into().ok()?) as u64;
    let (size, header) = match size32 {
        0 => (remaining, 8),
        1 => (u64::from_be_bytes(data.get(8..16)?.try_into().ok()?), 16),
        s => (s, 8),
    };
    (size >= header && size <= remaining).then_some((size, header))
}

/// The child boxes of a box's payload, `(type, payload)`, up to the first one that doesn't fit
fn children(payload: &[u8]) -> impl Iterator<Item = ([u8; 4], &[u8])> {
    let mut pos = 0usize;
    std::iter::from_fn(move || {
        let rest = payload.get(pos..).filter(|r| r.len() >= 8)?;
        let (size, header) = box_size(rest, rest.len() as u64)?;
        pos += size as usize;
        Some((rest[4..8].try_into().unwrap(), &rest[header as usize..size as usize]))
    })
}

fn child<'a>(payload: &'a [u8], typ: &[u8; 4]) -> Option<&'a [u8]> {
    children(payload).find(|(t, _)| t == typ).map(|(_, p)| p)
}

/// Where the first box of type `typ` among the boxes in [`start`, `end`) of the stream is: its payload's start and
/// size. `None` when there's none before a box that doesn't fit, or, with `text_types` (a file's top level, where it
/// tells a file that isn't one), before a box whose type isn't text
fn find_box<R: Read + Seek>(stream: &mut R, start: u64, end: u64, typ: &[u8; 4], text_types: bool) -> Result<Option<(u64, u64)>> {
    let mut pos = start;
    while pos + 8 <= end {
        stream.seek(SeekFrom::Start(pos))?;
        let mut head = [0u8; 16];
        let n = if pos + 16 <= end { 16 } else { 8 };
        stream.read_exact(&mut head[..n])?;
        if text_types && !head[4..8].iter().all(|c| c.is_ascii_graphic() || *c == b' ') {
            return Ok(None);
        }
        let Some((box_len, header)) = box_size(&head[..n], end - pos) else { return Ok(None) };
        if &head[4..8] == typ {
            return Ok(Some((pos + header, box_len - header)));
        }
        pos += box_len;
    }
    Ok(None)
}

/// The `moov` box's payload of an ISO base media file, found by walking its top-level boxes. `None` for a file that
/// isn't one (its first box isn't a box that fits) or has no `moov` before a box that can't be stepped over
fn read_moov<R: Read + Seek>(stream: &mut R, size: u64) -> Result<Option<Vec<u8>>> {
    let Some((at, len)) = find_box(stream, 0, size, b"moov", true)? else { return Ok(None) };
    stream.seek(SeekFrom::Start(at))?;
    crate::util::read_data(stream, len).map(Some)
}

/// The table of an ISO base media file. Only `moov/udta` is read to find it, the rest of `moov` (its sample tables,
/// megabytes in a long recording) only to check a table found
fn read_isobmff<R: Read + Seek>(stream: &mut R, size: u64) -> Result<Option<Found>> {
    let Some((moov_at, moov_len)) = find_box(stream, 0, size, b"moov", true)? else { return Ok(None) };
    let Some((udta_at, udta_len)) = find_box(stream, moov_at, moov_at + moov_len, b"udta", false)? else { return Ok(None) };
    stream.seek(SeekFrom::Start(udta_at))?;
    let udta = crate::util::read_data(stream, udta_len)?;
    let tables: Vec<&[u8]> = children(&udta).filter(|(t, _)| *t == BOX_TYPE).map(|(_, p)| p).collect();
    let text = match tables.as_slice() {
        [] => return Ok(None),
        [text] => text,
        _ => return Ok(Some(Err(format!("moov/udta has {} MTRM boxes", tables.len())))),
    };
    let moov = match stream.seek(SeekFrom::Start(moov_at)).and_then(|_| crate::util::read_data(stream, moov_len)) {
        Ok(moov) => moov,
        Err(e) => return Ok(Some(Err(format!("the moov box can't be read: {e}")))),
    };
    Ok(Some(check_text(text, video_frame_times_of_moov(&moov).map(VideoFrames::Times).as_ref())))
}

/// The presentation times of the frames of an ISO base media file's first video track (its `moov`'s first `trak` whose
/// handler is `vide`): what a player shows, through the track's edit list, at the times FFmpeg's demuxer gives them (a
/// sample's decode time plus its composition offset, the first frame of an edit at the edit's start). `None` for a file
/// that isn't one, or whose first video track can't be read or has an edit list of another shape than an optional
/// empty edit followed by one edit of the media at normal rate (ISO/IEC 14496-12 §8.6.6)
pub fn video_frame_times<R: Read + Seek>(stream: &mut R, size: u64) -> Option<VideoFrameTimes> {
    let moov = read_moov(stream, size).ok().flatten();
    let _ = stream.seek(SeekFrom::Start(0));
    video_frame_times_of_moov(&moov?)
}

/// The full box version of a payload, and the payload after its version and flags
fn full_box(p: &[u8]) -> Option<(u8, &[u8])> {
    Some((*p.first()?, p.get(4..)?))
}

fn be_u32(p: &[u8], at: usize) -> Option<u32> { Some(u32::from_be_bytes(p.get(at..at + 4)?.try_into().ok()?)) }
fn be_u64(p: &[u8], at: usize) -> Option<u64> { Some(u64::from_be_bytes(p.get(at..at + 8)?.try_into().ok()?)) }

fn video_frame_times_of_moov(moov: &[u8]) -> Option<VideoFrameTimes> {
    // The movie timescale, which edit list durations are in (§8.2.2)
    let (version, mvhd) = full_box(child(moov, b"mvhd")?)?;
    let movie_timescale = be_u32(mvhd, if version == 1 { 16 } else { 8 })? as u64;
    let trak = children(moov).filter(|(t, _)| t == b"trak").map(|(_, p)| p).find(|trak| {
        child(trak, b"mdia").and_then(|mdia| child(mdia, b"hdlr")).and_then(|h| h.get(8..12)) == Some(b"vide")
    })?;
    let mdia = child(trak, b"mdia")?;
    let (version, mdhd) = full_box(child(mdia, b"mdhd")?)?;
    let timescale = be_u32(mdhd, if version == 1 { 16 } else { 8 })? as u64;
    if timescale == 0 || movie_timescale == 0 { return None; }
    let stbl = child(child(mdia, b"minf")?, b"stbl")?;

    // Decode durations (§8.6.1.2), expanded per sample
    let (_, stts) = full_box(child(stbl, b"stts")?)?;
    let entries = be_u32(stts, 0)? as usize;
    let mut durations: Vec<i64> = Vec::new();
    for e in 0..entries {
        let count = be_u32(stts, 4 + e * 8)? as usize;
        let delta = be_u32(stts, 8 + e * 8)? as i64;
        // A table can't describe more samples than its bytes could hold sizes for: bound by the moov's size
        if durations.len() + count > moov.len() { return None; }
        durations.extend(std::iter::repeat_n(delta, count));
    }
    // Composition offsets (§8.6.1.3), signed in either version: QuickTime's are signed (QTFF, "Composition Offset
    // Atom"), and Apple writes negative ones into version 0 boxes, which FFmpeg reads as signed too (`mov_read_ctts`)
    let mut offsets = vec![0i64; durations.len()];
    if let Some((_, ctts)) = child(stbl, b"ctts").and_then(full_box) {
        let entries = be_u32(ctts, 0)? as usize;
        let mut at = 0usize;
        for e in 0..entries {
            let count = be_u32(ctts, 4 + e * 8)? as usize;
            let offset = be_u32(ctts, 8 + e * 8)? as i32 as i64;
            for o in offsets.iter_mut().skip(at).take(count) { *o = offset; }
            at += count;
        }
    }
    // The samples' decode times, composition times and durations, in decode order
    let mut dts = 0i64;
    let decoded: Vec<(i64, i64, i64)> = durations.iter().zip(&offsets).map(|(d, o)| {
        let f = (dts, dts + o, *d);
        dts += d;
        f
    }).collect();

    // The edit list (§8.6.6): an empty edit (media time -1) first or none, then one edit of the media at rate 1
    let mut empty = 0i64; // movie timescale
    let mut media_time = 0i64;
    let mut media_end = i64::MAX; // media timescale
    let mut edited = false;
    if let Some(elst) = child(trak, b"edts").and_then(|e| child(e, b"elst")) {
        let (version, elst) = full_box(elst)?;
        let entries = be_u32(elst, 0)? as usize;
        let width = if version == 1 { 20 } else { 12 };
        let entry = |e: usize| -> Option<(u64, i64, u32)> {
            let at = 4 + e * width;
            if version == 1 {
                Some((be_u64(elst, at)?, be_u64(elst, at + 8)? as i64, be_u32(elst, at + 16)?))
            } else {
                Some((be_u32(elst, at)? as u64, be_u32(elst, at + 4)? as i32 as i64, be_u32(elst, at + 8)?))
            }
        };
        let mut e = 0;
        if entries > 1 {
            let (duration, time, _) = entry(0)?;
            if time != -1 { return None; }
            empty = i64::try_from(duration).ok()?;
            e = 1;
        }
        if entries > 0 {
            if entries > e + 1 { return None; }
            let (duration, time, rate) = entry(e)?;
            if time < 0 || rate != 0x0001_0000 { return None; }
            media_time = time;
            edited = true;
            if duration > 0 {
                let d = (duration as i128 * timescale as i128 + movie_timescale as i128 / 2) / movie_timescale as i128;
                media_end = media_time.checked_add(i64::try_from(d).ok()?)?;
            }
        }
    }
    let empty_ticks = i64::try_from((empty as i128 * timescale as i128 + movie_timescale as i128 / 2) / movie_timescale as i128).ok()?;
    let in_edit = |cts: i64| cts >= media_time && cts < media_end;
    // Where the presented frames' times start from, as FFmpeg's mov demuxer has them (`mov_fix_index`), which the
    // decoders read the file with. Without an edit list, the composition times as they are. With one, the first frame
    // in the edit in decode order starts at the empty edit's end, the others follow it by their decode times and
    // composition offsets, and all are moved earlier for the first one presented to start there. That's the edit's media
    // time only when a frame starts there: the frame an edit starts within isn't presented (FFmpeg drops it), and the
    // next one starts the edit, not the part of a frame later the timeline of ISO/IEC 14496-12 §8.6.6 has it
    let origin = if edited {
        let first_dts = decoded.iter().find(|(_, cts, _)| in_edit(*cts))?.0;
        let first_cts = decoded.iter().filter(|(_, cts, _)| in_edit(*cts)).map(|(_, cts, _)| *cts).min()?;
        first_cts.max(first_dts)
    } else {
        0
    };
    let mut presented: Vec<(i64, i64)> = decoded.iter().filter(|(_, cts, _)| in_edit(*cts)).map(|(_, cts, d)| (*cts, *d)).collect();
    presented.sort_by_key(|f| f.0);
    let (last_cts, last_duration) = *presented.last()?;
    let end = (last_cts + last_duration).min(media_end) - origin + empty_ticks;
    let starts: Vec<i64> = presented.iter().map(|(cts, _)| cts - origin + empty_ticks).collect();
    // The most common frame duration, the shorter one on a tie
    let mut sorted = durations;
    sorted.sort_unstable();
    let (mut nominal, mut best, mut run) = (sorted[0], 0usize, 0usize);
    for (i, d) in sorted.iter().enumerate() {
        run = if i > 0 && sorted[i - 1] == *d { run + 1 } else { 1 };
        if run > best { best = run; nominal = *d; }
    }
    (nominal > 0).then_some(VideoFrameTimes { timescale, starts, end, nominal })
}

// ------------------------------- MXF -------------------------------

const MXF_PARTITION_PREFIX: [u8; 13] = [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x05, 0x01, 0x01, 0x0d, 0x01, 0x02, 0x01, 0x01];
const MXF_PRIMER: [u8; 16] = [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x05, 0x01, 0x01, 0x0d, 0x01, 0x02, 0x01, 0x01, 0x05, 0x01, 0x00];
const MXF_FILL: [u8; 16] = [0x06, 0x0e, 0x2b, 0x34, 0x01, 0x01, 0x01, 0x02, 0x03, 0x01, 0x02, 0x10, 0x01, 0x00, 0x00, 0x00];
/// Picture essence descriptors (SMPTE ST 377-1 Annex B, ST 381, RP 2057): CDCI, RGBA, generic picture, MPEG, VC-1
const MXF_PICTURE_DESCRIPTORS: [u8; 5] = [0x28, 0x29, 0x27, 0x51, 0x5f];
/// An MXF file's run-in is at most 64 KiB less one byte (ST 377-1 §6.5)
const MXF_MAX_RUN_IN: u64 = 65_535;
/// More header metadata than this isn't read
const MXF_MAX_HEADER: u64 = 256 << 20;

/// Two MXF keys, without byte 8 (the registry version, ST 336)
fn same_ul(a: &[u8], b: &[u8]) -> bool {
    a.len() == 16 && b.len() == 16 && a[..7] == b[..7] && a[8..] == b[8..]
}

/// Where an MXF file's header partition pack starts: at the start of the file, or after a run-in
fn mxf_header_partition<R: Read + Seek>(stream: &mut R, size: u64) -> Result<Option<u64>> {
    let mut head = vec![0u8; size.min(MXF_MAX_RUN_IN + 16) as usize];
    stream.seek(SeekFrom::Start(0))?;
    stream.read_exact(&mut head)?;
    Ok((0..head.len().saturating_sub(15)).find(|&i| {
        let k = &head[i..i + 16];
        k[..13] == MXF_PARTITION_PREFIX && k[13] == 0x02 && (0x01..=0x04).contains(&k[14]) && k[15] == 0
    }).map(|i| i as u64))
}

/// A KLV's key, and where its value starts and how long it is (BER length, ST 336 §3.3; an indefinite length isn't one
/// a header metadata item has)
fn mxf_klv<R: Read + Seek>(stream: &mut R, at: u64, size: u64) -> Result<([u8; 16], u64, u64)> {
    let invalid = |what: &str| Error::new(ErrorKind::InvalidData, format!("{what} at offset {at}"));
    let mut b = [0u8; 25];
    let n = size.saturating_sub(at).min(25) as usize;
    if n < 17 { return Err(invalid("no KLV")); }
    stream.seek(SeekFrom::Start(at))?;
    stream.read_exact(&mut b[..n])?;
    let (len, ll) = match b[16] {
        x if x < 0x80 => (x as u64, 1u64),
        0x80 => return Err(invalid("an indefinite BER length")),
        x => {
            let m = (x & 0x7f) as usize;
            if m > 8 || 17 + m > n { return Err(invalid("a BER length that doesn't fit")); }
            (b[17..17 + m].iter().fold(0u64, |v, &d| v << 8 | d as u64), 1 + m as u64)
        }
    };
    let value = at + 16 + ll;
    if value.checked_add(len).is_none_or(|e| e > size) {
        return Err(invalid("a KLV past the end of the file"));
    }
    Ok((b[..16].try_into().unwrap(), value, len))
}

/// The table of an MXF file: the items of its table key in the header partition's header metadata, which starts with
/// the primer pack after the partition pack and its fill, and is `HeaderByteCount` bytes long (ST 377-1 §7.1, §9). The
/// frames it must cover are the edit units of the picture essence: its descriptor's `ContainerDuration` (local tag
/// 0x3002) at its `SampleRate` (0x3001), the edit rate of frame-wrapped essence
fn read_mxf<R: Read + Seek>(stream: &mut R, size: u64, start: u64) -> Result<Option<Found>> {
    let (_, pack, pack_len) = mxf_klv(stream, start, size)?;
    if pack_len < 40 {
        return Err(Error::new(ErrorKind::InvalidData, "a header partition pack shorter than its fixed items"));
    }
    stream.seek(SeekFrom::Start(pack + 32))?;
    let header_bytes = stream.read_u64::<BigEndian>()?.min(MXF_MAX_HEADER);
    let mut at = pack + pack_len;
    loop {
        let (key, value, len) = mxf_klv(stream, at, size)?;
        if same_ul(&key, &MXF_PRIMER) { break; }
        if !same_ul(&key, &MXF_FILL) { return Ok(None); }
        at = value + len;
    }
    let end = at.saturating_add(header_bytes).min(size);
    let mut tables: Vec<Result<Vec<u8>>> = Vec::new();
    let (mut duration, mut rate) = (None, None);
    // The items up to the end of the header metadata, or up to a damaged one, past which nothing can be found
    let walked = (|| -> Result<()> {
        while at < end {
            let (key, value, len) = mxf_klv(stream, at, size)?;
            if same_ul(&key, &MXF_KEY) {
                tables.push(stream.seek(SeekFrom::Start(value)).and_then(|_| crate::util::read_data(stream, len)));
            } else if key[..4] == [0x06, 0x0e, 0x2b, 0x34] && key[4] == 0x02 && key[5] == 0x53 && key[8..13] == [0x0d, 0x01, 0x01, 0x01, 0x01]
                   && key[13] == 0x01 && MXF_PICTURE_DESCRIPTORS.contains(&key[14]) && duration.is_none() {
                // A local set of 2-byte tags and 2-byte lengths (ST 377-1 §9.6); the two items' tags are static
                stream.seek(SeekFrom::Start(value))?;
                let set = crate::util::read_data(stream, len)?;
                let mut p = 0usize;
                let (mut d, mut r) = (None, None);
                while p + 4 <= set.len() {
                    let tag = u16::from_be_bytes([set[p], set[p + 1]]);
                    let l = u16::from_be_bytes([set[p + 2], set[p + 3]]) as usize;
                    let Some(v) = set.get(p + 4..p + 4 + l) else { break };
                    match (tag, l) {
                        (0x3001, 8) => r = Some((be_u32(v, 0).unwrap() as u64, be_u32(v, 4).unwrap() as u64)),
                        (0x3002, 8) => d = Some(be_u64(v, 0).unwrap()),
                        _ => {}
                    }
                    p += 4 + l;
                }
                if let (Some(d), Some(r)) = (d, r.filter(|r| r.0 > 0 && r.1 > 0)) {
                    duration = Some(d);
                    rate = Some(r);
                }
            }
            at = value + len;
        }
        Ok(())
    })();
    match walked {
        Err(e) if tables.is_empty() => return Err(e),
        // Whether the table found is the file's only one can't be told
        Err(e) => return Ok(Some(Err(format!("the header metadata can't be read after its segment table: {e}")))),
        Ok(()) => {}
    }
    let video = match (duration, rate) {
        (Some(frames), Some(rate)) => Some(VideoFrames::Constant { frames, rate }),
        _ => None,
    };
    Ok(match tables.as_slice() {
        [] => None,
        [Ok(text)] => Some(check_text(text, video.as_ref())),
        [Err(e)] => Some(Err(format!("the segment table can't be read: {e}"))),
        _ => Some(Err(format!("the header metadata has {} segment tables", tables.len()))),
    })
}

// ------------------------------- MotionCam -------------------------------

/// The table of a MotionCam recording: the item of its type right after the container metadata (the header `MOTION ` and
/// the container version byte, then items of `u32` type and `u32` size, little-endian). The frames it must cover are the
/// recording's, in timestamp order, from the frame index the trailer points to (`u32` 0, `u32` 16, `u32` 0x8A905612,
/// `i32` count, `i64` offset of `count × (i64 offset, i64 timestamp)`): frame `k` at its timestamp less the first
/// frame's, in ns, the last one lasting the median interval between frames
fn read_mcraw<R: Read + Seek>(stream: &mut R, size: u64) -> Result<Option<Found>> {
    stream.seek(SeekFrom::Start(8))?;
    let _container = stream.read_u32::<LittleEndian>()?;
    let len = stream.read_u32::<LittleEndian>()? as u64;
    let at = 16u64.checked_add(len).filter(|at| *at + 8 <= size).ok_or_else(|| Error::new(ErrorKind::InvalidData, "the container metadata runs past the end"))?;
    stream.seek(SeekFrom::Start(at))?;
    let typ = stream.read_u32::<LittleEndian>()?;
    if typ.to_le_bytes() != BOX_TYPE { return Ok(None); }
    let text = match stream.read_u32::<LittleEndian>().and_then(|len| crate::util::read_data(stream, len as u64)) {
        Ok(text) => text,
        Err(e) => return Ok(Some(Err(format!("the segment table can't be read: {e}")))),
    };
    Ok(Some(match mcraw_frame_timestamps(stream, size) {
        Some(timestamps) => check_mcraw(&text, &timestamps),
        None => Err("the recording's frame index can't be read".into()),
    }))
}

/// Checks the table of a MotionCam recording whose frames are at `timestamps` (ns, in order)
pub(crate) fn check_mcraw(text: &[u8], timestamps: &[i64]) -> Found {
    let table = SegmentTable::parse(text)?;
    let frames = match mcraw_times(timestamps) {
        Some(times) => VideoFrames::Times(times),
        // A recording of one frame has no interval to time its end by, which the table has from its source
        None if timestamps.len() == 1 => VideoFrames::Constant { frames: 1, rate: table.sources[table.segments[0].source].fps },
        None => return Err("the recording has no frames".into()),
    };
    table.validate(&frames)?;
    Ok(table)
}

/// A MotionCam recording's frame timestamps from its frame index, in order
fn mcraw_frame_timestamps<R: Read + Seek>(stream: &mut R, size: u64) -> Option<Vec<i64>> {
    let tail = size.checked_sub(24)?;
    stream.seek(SeekFrom::Start(tail)).ok()?;
    let (kind, len) = (stream.read_u32::<LittleEndian>().ok()?, stream.read_u32::<LittleEndian>().ok()?);
    let magic = stream.read_u32::<LittleEndian>().ok()?;
    let count = u64::try_from(stream.read_i32::<LittleEndian>().ok()?).ok()?;
    let data = u64::try_from(stream.read_i64::<LittleEndian>().ok()?).ok()?;
    if kind != 0 || len != 16 || magic != 0x8A905612 || data.checked_add(count * 16)? > tail {
        return None;
    }
    stream.seek(SeekFrom::Start(data)).ok()?;
    let index = crate::util::read_data(stream, count * 16).ok()?;
    let mut ts: Vec<i64> = index.as_chunks::<16>().0.iter().map(|x| i64::from_le_bytes(x[8..16].try_into().unwrap())).collect();
    ts.sort_unstable();
    Some(ts)
}

/// Frames at the timestamps `ts` (ns, in order) as a recording's video presents them
pub(crate) fn mcraw_times(ts: &[i64]) -> Option<VideoFrameTimes> {
    let first = *ts.first()?;
    let mut intervals: Vec<i64> = ts.windows(2).map(|w| w[1] - w[0]).collect();
    intervals.sort_unstable();
    // The median interval, the lower one of an even count; a recording of one frame has none
    let nominal = *intervals.get(intervals.len().checked_sub(1)? / 2)?;
    let starts: Vec<i64> = ts.iter().map(|t| t - first).collect();
    let end = starts.last()? + nominal;
    (nominal > 0).then_some(VideoFrameTimes { timescale: 1_000_000_000, starts, end, nominal })
}

// ------------------------------- RED -------------------------------

/// The table of a RED clip, from its RMD sidecar's text (XML): the text of the one `<mp4trim>` child of the root
/// element, with character and entity references resolved (XML 1.0 §4.1, §4.6). `frames` and `rate` are the clip's
/// frames through all its parts and its video frame rate. `None` when the RMD has no table
pub fn read_rmd(rmd: &[u8], frames: u64, rate: Option<(u64, u64)>) -> Option<Found> {
    let text = match xml_text(rmd) {
        Ok(text) => text,
        Err(e) => return Some(Err(format!("the RMD sidecar can't be read: {e}"))),
    };
    let found = match root_children_text(&text, "mp4trim") {
        Ok(found) => found,
        Err(e) => return Some(Err(e)),
    };
    match found.as_slice() {
        [] => None,
        [json] => Some(check_text(json.as_bytes(), rate.map(|rate| VideoFrames::Constant { frames, rate }).as_ref())),
        _ => Some(Err(format!("the RMD's root has {} mp4trim elements", found.len()))),
    }
}

/// An XML document's text: UTF-16 by its byte order mark, otherwise UTF-8 (a UTF-8 byte order mark dropped) or, for
/// bytes that aren't, one byte per character (the table's text is ASCII)
fn xml_text(bytes: &[u8]) -> std::result::Result<String, String> {
    let utf16 = |be: bool| {
        let units: Vec<u16> = bytes[2..].as_chunks::<2>().0.iter().map(|c| if be { u16::from_be_bytes(*c) } else { u16::from_le_bytes(*c) }).collect();
        String::from_utf16(&units).map_err(|e| e.to_string())
    };
    match bytes {
        [0xFF, 0xFE, ..] => utf16(false),
        [0xFE, 0xFF, ..] => utf16(true),
        [0xEF, 0xBB, 0xBF, rest @ ..] => Ok(String::from_utf8_lossy(rest).into_owned()),
        _ => Ok(match std::str::from_utf8(bytes) {
            Ok(s) => s.to_owned(),
            Err(_) => bytes.iter().map(|&b| b as char).collect(),
        }),
    }
}

/// The text of the children of the root element named `name`, references resolved
fn root_children_text(doc: &str, name: &str) -> std::result::Result<Vec<String>, String> {
    let mut found = Vec::new();
    let mut depth = 0usize;
    let mut pos = 0usize;
    // The child being read: where its text starts
    let mut reading: Option<usize> = None;
    let mut text = String::new();
    while let Some(lt) = doc[pos..].find('<').map(|i| pos + i) {
        if reading.is_some() {
            text.push_str(&doc[pos..lt]);
        }
        let rest = &doc[lt..];
        let close = |end: &str| rest.find(end).map(|i| lt + i + end.len()).ok_or_else(|| format!("an unterminated {end} at {lt}"));
        if rest.starts_with("<!--") {
            pos = close("-->")?;
        } else if rest.starts_with("<![CDATA[") {
            let end = close("]]>")?;
            if reading.is_some() { text.push_str(&doc[lt + 9..end - 3]); }
            pos = end;
        } else if rest.starts_with("<?") {
            pos = close("?>")?;
        } else if rest.starts_with("<!") {
            pos = close(">")?;
        } else {
            let end = close(">")?;
            let tag = &doc[lt + 1..end - 1];
            let empty = tag.ends_with('/');
            if let Some(end_name) = tag.strip_prefix('/') {
                depth = depth.checked_sub(1).ok_or("an end tag without its start tag")?;
                if depth == 1 && end_name.trim() == name && reading.take().is_some() {
                    found.push(resolve_references(&std::mem::take(&mut text))?);
                }
            } else {
                let tag_name = tag.trim_end_matches('/').split(|c: char| c.is_whitespace()).next().unwrap_or("");
                if depth == 1 && tag_name == name {
                    if empty { found.push(String::new()); } else { reading = Some(end); text.clear(); }
                }
                if !empty { depth += 1; }
            }
            pos = end;
        }
    }
    Ok(found)
}

/// Character and entity references resolved: the five predefined entities and `&#N;`/`&#xH;`
fn resolve_references(s: &str) -> std::result::Result<String, String> {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let semi = rest[amp..].find(';').ok_or("an unterminated reference")? + amp;
        let name = &rest[amp + 1..semi];
        let c = match name {
            "lt" => '<', "gt" => '>', "amp" => '&', "quot" => '"', "apos" => '\'',
            _ => {
                let code = if let Some(h) = name.strip_prefix("#x") { u32::from_str_radix(h, 16).ok() } else if let Some(d) = name.strip_prefix('#') { d.parse().ok() } else { None };
                code.and_then(char::from_u32).ok_or_else(|| format!("an unknown reference &{name};"))?
            }
        };
        out.push(c);
        rest = &rest[semi + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Cursor;

    /// A segment of `count` frames from frame `src` of a source at `rate`, at frame `out` of a video of that rate whose
    /// frame 0 is `out_ticks` ticks of `rate.0` in
    fn segment(src: u64, count: u64, out: u64, rate: (u64, u64), out_ticks: i128) -> Value {
        let t = |k: u64, at: i128| ticks_to_us(at + k as i128 * rate.1 as i128, rate.0 as u128).unwrap();
        let (start, duration) = (t(out, out_ticks), t(out + count, out_ticks) - t(out, out_ticks));
        json!({"source": 0, "src_first_frame": src, "frame_count": count, "out_first_frame": out,
               "src_start_us": t(src, 0), "out_start_us": start, "duration_us": duration,
               "src_capture_start_us": t(src, 0), "out_capture_start_us": start, "capture_duration_us": duration,
               "requested": {"unit": "frames", "start": src, "end": src + count}, "start": "before", "entry": "idr", "end": "reorder_safe", "clock_shift": null})
    }

    /// Two segments of 240 frames of a 900-frame source at `rate`: its frames 240..480, then 600..840
    fn document(rate: (u64, u64), out_ticks: i128) -> Value {
        json!({"version": 1, "tool": "test",
               "sources": [{"name": "C0538.MP4", "size": 469906891, "frames": 900, "fps": [rate.0, rate.1],
                            "creation_time": "2024-11-17T13:12:46Z", "clock": null, "sony_ibis_template": {"ibis": [0, 1000], "ois": [500]}}],
               "segments": [segment(240, 240, 0, rate, out_ticks), segment(600, 240, 240, rate, out_ticks)]})
    }

    fn text(doc: &Value) -> Vec<u8> { serde_json::to_vec(doc).unwrap() }

    fn constant(frames: u64) -> VideoFrames { VideoFrames::Constant { frames, rate: (30000, 1001) } }

    const NTSC: (u64, u64) = (30000, 1001);

    #[test]
    fn a_table_of_the_video_is_valid() {
        let table = SegmentTable::parse(&text(&document(NTSC, 0))).unwrap();
        table.validate(&constant(480)).unwrap();
        assert_eq!(table.sources[0].sony_ibis_template(), Some(IbisTemplate { ibis: vec![0, 1000], ois: vec![500] }));
        assert_eq!((table.segment_of_frame(239), table.segment_of_frame(240), table.segment_of_frame(10_000)), (0, 1, 1));
        let boundary = table.segments[1].out_start_us;
        assert_eq!((table.segment_of_time_us(-5), table.segment_of_time_us(boundary - 1), table.segment_of_time_us(boundary)), (0, 0, 1));
    }

    #[test]
    fn a_table_that_breaks_its_rules_is_invalid() {
        let changed = |change: &dyn Fn(&mut Value)| {
            let mut doc = document(NTSC, 0);
            change(&mut doc);
            SegmentTable::parse(&text(&doc))
        };
        assert!(changed(&|d| d["version"] = json!(2)).is_err());
        assert!(changed(&|d| { d["sources"][0].as_object_mut().unwrap().remove("clock"); }).is_err());
        assert!(changed(&|d| d["sources"][0]["creation_time"] = json!("2024-02-30T00:00:00Z")).is_err());
        // Past its source's frames
        assert!(changed(&|d| d["segments"][1]["src_first_frame"] = json!(700)).is_err());
        // Not where the segment before ends
        assert!(changed(&|d| d["segments"][1]["out_first_frame"] = json!(241)).is_err());
        assert!(changed(&|d| {
            let start = d["segments"][1]["out_start_us"].as_i64().unwrap();
            d["segments"][1]["out_start_us"] = json!(start + 1);
        }).is_err());
        // A clock shift without a clock
        assert!(changed(&|d| d["segments"][0]["clock_shift"] = json!(0)).is_err());
    }

    #[test]
    fn a_table_of_other_frames_is_invalid() {
        let table = SegmentTable::parse(&text(&document(NTSC, 0))).unwrap();
        assert!(table.validate(&constant(481)).is_err());
        assert!(table.validate(&VideoFrames::Constant { frames: 480, rate: (25, 1) }).is_err());
        // The end within a frame of the video's (33366.7 µs), not further
        let ending = |later_us: i64| {
            let mut doc = document(NTSC, 0);
            let duration = doc["segments"][1]["duration_us"].as_i64().unwrap();
            doc["segments"][1]["duration_us"] = json!(duration + later_us);
            SegmentTable::parse(&text(&doc)).unwrap().validate(&constant(480))
        };
        assert!(ending(33_000).is_ok());
        assert!(ending(34_000).is_err());
    }

    #[test]
    fn a_rate_too_large_for_the_arithmetic_is_no_videos() {
        // A recording of one frame is checked at its source's rate, which the table states
        let doc = json!({"version": 1, "sources": [{"name": "a.mcraw", "size": 1, "frames": 1, "fps": [u64::MAX, 1], "creation_time": null, "clock": null}],
            "segments": [{"source": 0, "src_first_frame": 0, "frame_count": 1, "out_first_frame": 0, "src_start_us": 0, "out_start_us": 0, "duration_us": i64::MAX,
                          "src_capture_start_us": 0, "out_capture_start_us": 0, "capture_duration_us": i64::MAX,
                          "requested": null, "start": "before", "entry": "idr", "end": "reorder_safe", "clock_shift": null}]});
        assert!(check_mcraw(&text(&doc), &[1_000_000]).is_err());
    }

    fn bx(typ: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        [&(payload.len() as u32 + 8).to_be_bytes()[..], &typ[..], payload].concat()
    }
    fn full_bx(typ: &[u8; 4], fields: &[&[u8]]) -> Vec<u8> {
        bx(typ, &[&[0u8; 4][..], &fields.concat()].concat())
    }
    fn be(v: u32) -> [u8; 4] { v.to_be_bytes() }

    /// An MP4 of `frames` frames at 30000/1001 fps after an empty edit of 100 ms, with `tables` in `moov/udta`, its
    /// `moov` after its `mdat`
    fn mp4(frames: u32, tables: &[&[u8]]) -> Vec<u8> {
        let mvhd = full_bx(b"mvhd", &[&[0; 8], &be(1000), &be(0)]);
        let mdhd = full_bx(b"mdhd", &[&[0; 8], &be(30000), &be(0)]);
        let hdlr = full_bx(b"hdlr", &[&[0; 4], b"vide", &[0; 13]]);
        let stbl = bx(b"stbl", &full_bx(b"stts", &[&be(1), &be(frames), &be(1001)]));
        // The empty edit (media time -1), then the frames, in ms
        let elst = full_bx(b"elst", &[&be(2), &be(100), &be(u32::MAX), &be(0x0001_0000), &be(frames * 1001 / 30), &be(0), &be(0x0001_0000)]);
        let mdia = bx(b"mdia", &[mdhd, hdlr, bx(b"minf", &stbl)].concat());
        let trak = bx(b"trak", &[bx(b"edts", &elst), mdia].concat());
        let udta = bx(b"udta", &tables.iter().map(|t| bx(&BOX_TYPE, t)).collect::<Vec<_>>().concat());
        // Compressed video can have the bytes of the table's box type
        [bx(b"ftyp", b"isom\0\0\0\0isom"), bx(b"mdat", b"\0\0\0\x10MTRM{}"), bx(b"moov", &[mvhd, trak, udta].concat())].concat()
    }

    fn read_all(file: &[u8]) -> Option<Found> {
        read(&mut Cursor::new(file), file.len() as u64)
    }

    /// The `moov` of a video track of `frames` frames of 128 ticks of 12800 (100 fps), each `ctts` ticks after its decode
    /// time when given, through `edits` (movie ticks of 12800, media time) when given
    fn track(frames: u32, ctts: Option<i32>, edits: Option<&[(u32, i32)]>) -> Vec<u8> {
        let mvhd = full_bx(b"mvhd", &[&[0; 8], &be(12800), &be(0)]);
        let mdhd = full_bx(b"mdhd", &[&[0; 8], &be(12800), &be(0)]);
        let hdlr = full_bx(b"hdlr", &[&[0; 4], b"vide", &[0; 13]]);
        let mut stbl = full_bx(b"stts", &[&be(1), &be(frames), &be(128)]);
        if let Some(offset) = ctts {
            stbl.extend(full_bx(b"ctts", &[&be(1), &be(frames), &offset.to_be_bytes()]));
        }
        let mdia = bx(b"mdia", &[mdhd, hdlr, bx(b"minf", &bx(b"stbl", &stbl))].concat());
        let edts = edits.map(|edits| {
            let entries: Vec<u8> = edits.iter().flat_map(|(duration, time)| [be(*duration), time.to_be_bytes(), be(0x0001_0000)].concat()).collect();
            bx(b"edts", &full_bx(b"elst", &[&be(edits.len() as u32), &entries]))
        }).unwrap_or_default();
        bx(b"moov", &[mvhd, bx(b"trak", &[edts, mdia].concat())].concat())
    }

    fn times_of(moov: &[u8]) -> (Vec<i64>, i64) {
        let times = video_frame_times(&mut Cursor::new(moov), moov.len() as u64).unwrap();
        (times.starts, times.end)
    }

    #[test]
    fn frames_are_at_the_times_ffmpeg_gives_them() {
        let frames = |from: i64, count: i64| (0..count).map(|k| from + k * 128).collect::<Vec<_>>();
        // A stream copy cut 7.5 frames in: the frame the edit starts within isn't shown, the next one starts the edit
        assert_eq!(times_of(&track(20, None, Some(&[(1600, 960)]))), (frames(0, 12), 1536));
        // And after an empty edit
        assert_eq!(times_of(&track(20, None, Some(&[(500, -1), (1600, 960)]))), (frames(500, 12), 2036));
        // An edit from the first frame's composition time
        assert_eq!(times_of(&track(20, Some(256), Some(&[(2560, 256)]))), (frames(0, 20), 2560));
        // An edit from 0 of frames composed two frames later: FFmpeg presents the first one at the edit's start too
        assert_eq!(times_of(&track(20, Some(256), Some(&[(2816, 0)]))), (frames(0, 20), 2560));
        // Without an edit list, as composed: the first frame two frames late
        assert_eq!(times_of(&track(20, Some(256), None)), (frames(256, 20), 2816));
    }

    #[test]
    fn an_mp4s_table_is_in_moov_udta() {
        // Frame 0 where the empty edit ends: 3000 ticks of 30000
        let table = text(&document(NTSC, 3000));
        let file = mp4(480, &[&table]);
        assert_eq!(read_all(&file).map(|found| found.map(|t| t.segments.len())), Some(Ok(2)));
        let times = video_frame_times(&mut Cursor::new(&file), file.len() as u64).unwrap();
        assert_eq!((times.frames(), times.starts[0], times.end, times.nominal), (480, 3000, 3000 + 480 * 1001, 1001));
        assert_eq!(read_all(&mp4(480, &[])), None);
        // Copied into a file of other frames, or twice
        assert!(matches!(read_all(&mp4(479, &[&table])), Some(Err(_))));
        assert!(matches!(read_all(&mp4(480, &[&table, &table])), Some(Err(_))));
    }

    fn klv(key: &[u8; 16], value: &[u8]) -> Vec<u8> {
        // A BER length of 3 bytes
        [&key[..], &[0x83], &(value.len() as u32).to_be_bytes()[1..], value].concat()
    }

    /// An MXF file: its header partition pack, then its header metadata, the primer pack and `items`
    fn mxf(items: &[Vec<u8>]) -> Vec<u8> {
        let primer = klv(&MXF_PRIMER, &[0; 8]);
        let mut pack = [0u8; 88];
        pack[32..40].copy_from_slice(&((primer.len() + items.concat().len()) as u64).to_be_bytes());
        let mut key = [0u8; 16];
        key[..13].copy_from_slice(&MXF_PARTITION_PREFIX);
        key[13] = 0x02;
        key[14] = 0x04;
        [klv(&key, &pack), primer, items.concat()].concat()
    }

    /// A CDCI picture descriptor of `frames` frames at `rate`
    fn descriptor(frames: u64, rate: (u32, u32)) -> Vec<u8> {
        let key = [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x53, 0x01, 0x01, 0x0d, 0x01, 0x01, 0x01, 0x01, 0x01, 0x28, 0x00];
        klv(&key, &[&[0x30, 0x01, 0x00, 0x08][..], &rate.0.to_be_bytes(), &rate.1.to_be_bytes(), &[0x30, 0x02, 0x00, 0x08], &frames.to_be_bytes()].concat())
    }

    #[test]
    fn an_mxf_files_table_is_in_its_header_metadata() {
        let table = klv(&MXF_KEY, &text(&document(NTSC, 0)));
        let picture = descriptor(480, (30000, 1001));
        // An indefinite length, which no header metadata item has
        let damaged = [&[0x06, 0x0e, 0x2b, 0x34, 0x01, 0x01, 0x01, 0x01, 0x0f, 0, 0, 0, 0, 0, 0, 0][..], &[0x80]].concat();
        assert!(matches!(read_all(&mxf(&[picture.clone(), table.clone()])), Some(Ok(_))));
        assert!(matches!(read_all(&mxf(&[table.clone(), picture.clone()])), Some(Ok(_))));
        assert!(matches!(read_all(&mxf(&[descriptor(481, (30000, 1001)), table.clone()])), Some(Err(_))));
        assert_eq!(read_all(&mxf(&[picture.clone()])), None);
        // Past a damaged item nothing is found: a file without a table before it has none found, one with a table has
        // one that can't be checked
        assert_eq!(read_all(&mxf(&[damaged.clone(), table.clone()])), None);
        assert!(matches!(read_all(&mxf(&[picture, table, damaged])), Some(Err(e)) if e.contains("after its segment table")));
    }

    /// A MotionCam recording of frames at `timestamps` (ns) with `table` after its container metadata
    fn mcraw(table: &[u8], timestamps: &[i64]) -> Vec<u8> {
        let item = |typ: u32, payload: &[u8]| [&typ.to_le_bytes()[..], &(payload.len() as u32).to_le_bytes(), payload].concat();
        let mut file = [&b"MOTION \x03"[..], &item(3, b"{}"), &item(u32::from_le_bytes(BOX_TYPE), table)].concat();
        let index_at = file.len() as i64;
        for ts in timestamps {
            file.extend(0i64.to_le_bytes());
            file.extend(ts.to_le_bytes());
        }
        file.extend(item(0, &[&0x8A905612u32.to_le_bytes()[..], &(timestamps.len() as i32).to_le_bytes(), &index_at.to_le_bytes()].concat()));
        file
    }

    #[test]
    fn a_motioncam_recordings_table_follows_its_container_metadata() {
        let table = text(&document((25, 1), 0));
        let timestamps: Vec<i64> = (0..480).map(|k| 1_000_000_000 + k * 40_000_000).collect();
        assert!(matches!(read_all(&mcraw(&table, &timestamps)), Some(Ok(_))));
        assert!(matches!(read_all(&mcraw(&table, &timestamps[..479])), Some(Err(_))));
        // A probe's one frame isn't the recording's
        assert!(check_mcraw(&table, &timestamps[..1]).is_err());
    }

    #[test]
    fn a_red_clips_table_is_the_text_of_the_rmd_roots_mp4trim() {
        let json = String::from_utf8(text(&document(NTSC, 0))).unwrap();
        let rate = Some(NTSC);
        let rmd = format!("<?xml version=\"1.0\"?>\n<!-- <mp4trim>a comment</mp4trim> -->\n<rmd version=\"1\"><clip><mp4trim>a grandchild</mp4trim></clip>\n<mp4trim>{}</mp4trim><lens/></rmd>", json.replace('"', "&quot;"));
        assert!(matches!(read_rmd(rmd.as_bytes(), 480, rate), Some(Ok(_))));
        let cdata = format!("<rmd><mp4trim><![CDATA[{json}]]></mp4trim></rmd>");
        assert!(matches!(read_rmd(cdata.as_bytes(), 480, rate), Some(Ok(_))));
        // Of other frames, none at the root, or two
        assert!(matches!(read_rmd(rmd.as_bytes(), 481, rate), Some(Err(_))));
        assert_eq!(read_rmd(b"<rmd><clip><mp4trim>{}</mp4trim></clip></rmd>", 480, rate), None);
        let twice = format!("<rmd><mp4trim><![CDATA[{json}]]></mp4trim><mp4trim/></rmd>");
        assert!(matches!(read_rmd(twice.as_bytes(), 480, rate), Some(Err(_))));
        assert_eq!(resolve_references("&#x41;&#66;&lt;&amp;&apos;"), Ok("AB<&'".to_owned()));
    }
}
