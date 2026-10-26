// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2023 Adrian <adrian.eddy at gmail>

use crate::*;
use crate::tags_impl::*;
use memchr::memmem;
use serde::de::{ self, DeserializeSeed };
use std::collections::BTreeSet;
use std::sync::atomic::Ordering::Relaxed;

pub mod bin;

/// The `Timestamp` of the temporal records is the lens's high resolution clock, an unsigned 16-bit counter running at 150 kHz,
/// so it rolls over every 65536 ticks, about 0.44 s (Cooke /i Technology Part III, "5 Timing data")
const TIMESTAMP_MODULUS: i64 = 1 << 16;
const CLOCK_HZ: f64 = 150000.0;

const IMU_ORIENTATION: &str = "XYZ";

#[derive(Default)]
pub struct Cooke {
    pub model: Option<String>,
}

impl Cooke {
    pub fn camera_type(&self) -> String { "Cooke /i".to_owned() }
    pub fn has_accurate_timestamps(&self) -> bool { false }
    pub fn possible_extensions() -> Vec<&'static str> { vec!["yml", "yaml"] }
    pub fn frame_readout_time(&self) -> Option<f64> { None }
    pub fn normalize_imu_orientation(v: String) -> String { v }
    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], _filepath: P, _options: &crate::InputOptions) -> Option<Self> {
        if memmem::find(buffer, b"RecordType: rt.header.lens.info").is_some() || memmem::find(buffer, b"RecordType: rt.header.recorder.info").is_some() {
            Some(Self {
                model: Some("YAML metadata".into()),
            })
        } else {
            None
        }
    }

    /// The first sample holds the header records, then there's a sample per Kdi poll of the lens: the accelerometer and gyro
    /// samples it read out, its magnetometer sample and its lens data. The sensor samples are timed on one timeline of the lens clock
    /// and calibrated with the lens's calibration of each sensor: a sensor without one is left out
    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, _size: usize, _progress_cb: F, cancel_flag: Arc<AtomicBool>, options: crate::InputOptions) -> Result<Vec<SampleInfo>> {
        let mut all_data = String::new();
        stream.read_to_string(&mut all_data)?;

        let mut lmf = LensMetadata::default();
        read_records(&all_data, &cancel_flag, |record| lmf.add(record, &options));
        Ok(lmf.into_samples(&options))
    }
}

type Record = serde_json::Map<String, serde_json::Value>;

/// Calls `f` with the records of a Cooke /i Lens Metadata File, in file order
///
/// The records are the YAML documents of the file, separated by `---` lines (Cooke /i Lens Metadata File specification, "LMF Records").
/// LockitPlus leaves the document markers out and separates the records with blank lines, so to a YAML parser its file is one mapping
/// in which the keys of every record repeat: [`Records`] starts the next record where a key repeats. The YAML parser holds all the
/// events of a document in memory, about 40 times the size of its text, so such a file is handed to it in pieces, see [`record_pieces`].
///
/// libyaml can't carry on after a syntax error, so a document that isn't valid YAML is reported, and reading resumes at the next one
fn read_records(text: &str, cancel_flag: &AtomicBool, mut f: impl FnMut(Record)) {
    for piece in record_pieces(text) {
        let mut start = piece.start;
        loop {
            if cancel_flag.load(Relaxed) { return; }
            let rest = &text[start..piece.end];
            let Err(e) = read_documents(rest, cancel_flag, &mut f) else { break };

            let line = 1 + memchr::memchr_iter(b'\n', &text.as_bytes()[..start]).count();
            log::error!("Skipping a document of the Cooke /i lens metadata that isn't valid YAML: {e}, counting the lines from line {line}");

            let at = e.location().map_or(rest.len(), |l| l.index().min(rest.len()));
            match next_document(rest, at) {
                Some(next) => start += next,
                None => break
            }
        }
    }
}

fn read_documents(text: &str, cancel_flag: &AtomicBool, f: &mut impl FnMut(Record)) -> std::result::Result<(), serde_yaml::Error> {
    for document in serde_yaml::Deserializer::from_str(text) {
        if cancel_flag.load(Relaxed) { break; }
        Records(f).deserialize(document)?;
    }
    Ok(())
}

/// `text` cut before each record that LockitPlus separates from the previous one with a blank line instead of a document marker: before
/// a `RecordType` key that follows a blank line, in a YAML document that already has one. `RecordType` is the first key of every record
/// that LockitPlus and the specification write, so [`Records`] would start a new record there anyway: the cuts change nothing but how
/// much of the file the YAML parser holds at once
fn record_pieces(text: &str) -> Vec<std::ops::Range<usize>> {
    let mut pieces = Vec::new();
    let mut start = 0;
    let mut document_has_record_type = false;
    let mut after_blank_line = false;
    for (offset, line) in lines(text, 0) {
        if is_document_marker(line) {
            document_has_record_type = false;
        } else if line.strip_prefix("RecordType:").is_some_and(|v| v.is_empty() || v.starts_with([' ', '\t'])) {
            if document_has_record_type && after_blank_line {
                pieces.push(start..offset);
                start = offset;
            }
            document_has_record_type = true;
        }
        after_blank_line = line.trim().is_empty();
    }
    pieces.push(start..text.len());
    pieces
}

/// The offset of the first document marker in `text` from the line that holds byte `at`, past the start of `text`
fn next_document(text: &str, at: usize) -> Option<usize> {
    let line_start = memchr::memrchr(b'\n', &text.as_bytes()[..at]).map_or(0, |i| i + 1);
    lines(text, line_start).find(|(offset, line)| *offset > 0 && is_document_marker(line)).map(|(offset, _)| offset)
}

/// A `---` (directives end) or `...` (document end) marker line, which always separates YAML documents
fn is_document_marker(line: &str) -> bool {
    (line.starts_with("---") || line.starts_with("...")) && line[3..].chars().next().is_none_or(|c| c == ' ' || c == '\t')
}

/// The lines of `text` from byte `from`, with their offsets and without their line breaks
fn lines(text: &str, from: usize) -> impl Iterator<Item = (usize, &str)> {
    let mut pos = from;
    std::iter::from_fn(move || {
        if pos >= text.len() { return None; }
        let start = pos;
        let end = memchr::memchr(b'\n', &text.as_bytes()[pos..]).map_or(text.len(), |i| pos + i);
        pos = end + 1;
        let line = &text[start..end];
        Some((start, line.strip_suffix('\r').unwrap_or(line)))
    })
}

/// Reads one YAML document into records: a mapping is a record, but a key that is already in the record starts the next one
struct Records<'a, F>(&'a mut F);

impl<'de, F: FnMut(Record)> DeserializeSeed<'de> for Records<'_, F> {
    type Value = ();
    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> std::result::Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de, F: FnMut(Record)> de::Visitor<'de> for Records<'_, F> {
    type Value = ();
    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result { formatter.write_str("a mapping of record keys") }

    fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> std::result::Result<(), A::Error> {
        let mut record = Record::new();
        while let Some(key) = map.next_key::<String>()? {
            let value = map.next_value()?;
            if record.contains_key(&key) {
                (self.0)(std::mem::take(&mut record));
            }
            record.insert(key, value);
        }
        if !record.is_empty() {
            (self.0)(record);
        }
        Ok(())
    }

    // Documents that aren't mappings have no `RecordType` and are disregarded (Cooke /i Lens Metadata File specification, "LMF Records")
    fn visit_unit<E: de::Error>(self) -> std::result::Result<(), E> { Ok(()) }
    fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<(), E> { Ok(()) }
    fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<(), E> { Ok(()) }
    fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<(), E> { Ok(()) }
    fn visit_f64<E: de::Error>(self, _: f64) -> std::result::Result<(), E> { Ok(()) }
    fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<(), E> { Ok(()) }
    fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<(), A::Error> {
        while seq.next_element::<de::IgnoredAny>()?.is_some() { }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Timecode { hh: u32, mm: u32, ss: u32, ff: u32 }

impl Timecode {
    fn from_record(data: &serde_json::Value) -> Option<Self> {
        let tc = data.get("Timecode")?;
        let get = |key| tc.get(key)?.as_u64()?.try_into().ok();
        Some(Self { hh: get("hh")?, mm: get("mm")?, ss: get("ss")?, ff: get("ff")? })
    }
}

/// The recorder's timecode, which every temporal record carries (Cooke /i Lens Metadata File specification, "Time-Varying Documents"):
/// a clock that locates the records to within a frame, and doesn't roll over like the lens clock
#[derive(Clone, Copy)]
struct TimecodeClock {
    frame_rate: f64,
    /// Frames per timecode second, e.g. 30 at 29.97 fps
    base: i64,
    /// The frame numbers that drop-frame timecode skips at the start of every minute but each tenth (SMPTE ST 12-1): 2 at 29.97 fps
    dropped: i64,
}

impl TimecodeClock {
    /// From the `FrameRate` and `DropFrame` of the recorder information (Cooke /i Lens Metadata File specification,
    /// "Recorder Information Document"), `None` for a rate that timecode doesn't count at
    fn new(frame_rate: f64, drop_frame: bool) -> Option<Self> {
        Self::counting(frame_rate, frame_rate.round(), drop_frame)
    }

    /// A timecode that counts `base` frames to its second, at `frame_rate` frames a second of the lens clock. `None` for a base
    /// that doesn't round to one that timecode counts, 24 to 120 frames to its second, at the 1000/1001 rates too (SMPTE ST 12-1,
    /// extended by ST 12-3), and for frames too long to tell the lens clock's whole periods: the offsets of polls are compared
    /// modulo the roll-over period, so the tolerance has to be less than half the period, at more than 4.75 frames a second
    fn counting(frame_rate: f64, base: f64, drop_frame: bool) -> Option<Self> {
        let base = base.round();
        if !(24.0..=120.0).contains(&base) { return None; }
        let base = base as i64;
        let clock = Self { frame_rate, base, dropped: if drop_frame && base % 30 == 0 { base / 15 } else { 0 } };
        (frame_rate.is_finite() && frame_rate > 0.0 && 2.0 * clock.tolerance() < TIMESTAMP_MODULUS as f64).then_some(clock)
    }

    /// The number of frames from 00:00:00:00 to `tc`, `None` for a timecode that doesn't exist at this frame rate
    fn frame(&self, tc: Timecode) -> Option<i64> {
        let (hh, mm, ss, ff) = (tc.hh as i64, tc.mm as i64, tc.ss as i64, tc.ff as i64);
        if hh > 23 || mm > 59 || ss > 59 || ff >= self.base || (ss == 0 && mm % 10 != 0 && ff < self.dropped) {
            return None;
        }
        let minutes = hh * 60 + mm;
        Some((minutes * 60 + ss) * self.base + ff - self.dropped * (minutes - minutes / 10))
    }

    /// The frames from frame `from` to frame `to`. The timecode wraps around at midnight, and one that went back gives a negative
    /// number
    fn frames_between(&self, from: i64, to: i64) -> i64 {
        let day = 24 * 60 * 60 * self.base - self.dropped * (24 * 60 - 24 * 6);
        let frames = (to - from).rem_euclid(day);
        if frames > day / 2 { frames - day } else { frames }
    }

    /// The lens clock ticks from frame `from` to frame `to` ([`Self::frames_between`])
    fn ticks_between(&self, from: i64, to: i64) -> f64 {
        self.frames_between(from, to) as f64 / self.frame_rate * CLOCK_HZ
    }

    /// How much the offset of the lens clock from the timecode varies over polls whose timecode is right, in ticks: a frame, as
    /// the timecode locates each poll only to within its frame, and 8 ms for the polls' scatter about their frames' places on the
    /// lens clock, which is 7.7 and 7.6 ms in the two LockitPlus recordings
    fn tolerance(&self) -> f64 {
        CLOCK_HZ / self.frame_rate + 0.008 * CLOCK_HZ
    }
}

/// How a timecode clock keeps time with the lens clock over a file's polls ([`LensMetadata::keeps_time`])
struct Timekeeping {
    /// Whether it does across the file
    keeps: bool,
    /// The polls' span on the lens clock, in ticks, with the whole periods that the lens clock misread and the clock counts
    span: i64,
}

/// A `Timestamp` placed on the timeline of the lens clock
#[derive(Clone, Copy)]
struct ClockPoint {
    raw: u16,
    /// Ticks on the timeline, where the 16-bit clock doesn't roll over
    ticks: i64,
}

impl ClockPoint {
    /// The time of `raw` that is nearest to this point: right for a time less than half a roll-over period (0.22 s) away
    fn nearest(&self, raw: u16) -> i64 {
        let ticks = (raw as i64 - self.raw as i64).rem_euclid(TIMESTAMP_MODULUS);
        self.ticks + if ticks >= TIMESTAMP_MODULUS / 2 { ticks - TIMESTAMP_MODULUS } else { ticks }
    }

    /// The first time of `raw` at or after this point: right for a time less than a roll-over period (0.44 s) after it
    fn following(&self, raw: u16) -> i64 {
        self.ticks + (raw as i64 - self.raw as i64).rem_euclid(TIMESTAMP_MODULUS)
    }
}

/// Kdi polls in a row whose timecode keeps time with the lens clock ([`LensMetadata::place_on_timeline`])
struct Run {
    /// The polls, by their index
    polls: Vec<usize>,
    /// The offset of the lens clock, read from poll to poll, from the timecode: the median of its polls' offsets, in ticks
    offset: f64,
}

impl Run {
    /// The whole roll-over periods to add to the lens clock's reading of the run to put it where the timecode does, from the
    /// offset `level` of the runs before it. `None` when the two offsets differ by more than the `tolerance`, modulo the period:
    /// the timecode doesn't keep time with the lens clock from those runs to this one
    fn periods_to(&self, level: f64, tolerance: f64) -> Option<i64> {
        let periods = ((level - self.offset) / TIMESTAMP_MODULUS as f64).round();
        ((self.offset + periods * TIMESTAMP_MODULUS as f64 - level).abs() <= tolerance).then_some(periods as i64)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sensor { Accelerometer, Gyroscope }

impl Sensor {
    fn record_type(self) -> &'static str {
        match self {
            Self::Accelerometer => "rt.temporal.lens.accelerometer.raw",
            Self::Gyroscope     => "rt.temporal.lens.gyro.raw",
        }
    }
}

/// An accelerometer or gyro data packet: eight samples, the last one taken at `timestamp`
struct Burst {
    sensor: Sensor,
    timestamp: u16,
    samples: Vec<(f64, f64, f64)>,
    /// The time of the last sample, in ticks on the timeline
    end: i64,
    /// The ticks between the samples, `None` if they can't be timed
    spacing: Option<f64>,
}

/// The records of one Kdi poll of the lens: the accelerometer and gyro bursts it read out of the lens's buffer, a magnetometer sample
/// taken when the lens received the poll, and the lens data (Cooke /i Technology Part III, 3.1)
#[derive(Default)]
struct Poll {
    timecode: Option<Timecode>,
    /// When the lens received the poll: the `Timestamp` of the magnetometer and general records
    kdi: Option<u16>,
    /// `kdi` in ticks on the timeline
    kdi_ticks: Option<i64>,
    bursts: Vec<Burst>,
    magnetometer: Option<(f64, f64, f64)>,
    general: Option<serde_json::Value>,
    /// The general record is the poll's last (Cooke /i Lens Metadata File specification, "Time-Varying Documents")
    complete: bool,
}

/// A sensor's calibration: the 3×N matrix of its `rt.header.lens.cal.*` record, which turns its raw values into physical units
/// (Cooke /i Lens Metadata File specification, "Accelerometer Calibration", "Gyro Calibration", "Magnetometer Calibration")
#[derive(Clone, Copy, Default)]
enum Calibration<const N: usize> {
    /// The file has no record of it
    #[default]
    Missing,
    /// Its record isn't a 3×N matrix of numbers
    Invalid,
    Matrix([[f64; N]; 3]),
}

impl<const N: usize> Calibration<N> {
    /// From the rows of the matrix, `Row_1` to `Row_3`
    fn from_record(data: &serde_json::Value) -> Self {
        let row = |r: usize| -> Option<[f64; N]> {
            let arr = data.get(format!("Row_{r}"))?.as_array()?;
            if arr.len() != N { return None; }
            let mut row = [0.0; N];
            for (x, v) in row.iter_mut().zip(arr) {
                *x = v.as_f64()?;
            }
            Some(row)
        };
        match (row(1), row(2), row(3)) {
            (Some(r1), Some(r2), Some(r3)) => Self::Matrix([r1, r2, r3]),
            _ => Self::Invalid
        }
    }

    /// The matrix of `sensor`, or `None` with the reason in `uncalibrated` when the file has samples of it to calibrate
    fn matrix(self, sensor: &str, has_samples: bool, uncalibrated: &mut Vec<String>) -> Option<[[f64; N]; 3]> {
        match self {
            Self::Matrix(m) => return Some(m),
            _ if !has_samples => { },
            Self::Missing => uncalibrated.push(format!("the {sensor}, as the file has no rt.header.lens.cal.{sensor} record")),
            Self::Invalid => uncalibrated.push(format!("the {sensor}, as its rt.header.lens.cal.{sensor} record isn't a 3×{N} matrix of numbers")),
        }
        None
    }
}

/// A calibrated sample: the matrix `m` pre-multiplies the column of the `terms` of the raw values
fn calibrate<const N: usize>(m: &[[f64; N]; 3], terms: [f64; N]) -> (f64, f64, f64) {
    let row = |r: &[f64; N]| r.iter().zip(terms).map(|(c, t)| c * t).sum::<f64>();
    (row(&m[0]), row(&m[1]), row(&m[2]))
}
/// The terms of the accelerometer and magnetometer calibrations, [X; Y; Z; 1]
fn linear_terms(x: (f64, f64, f64)) -> [f64; 4] { [x.0, x.1, x.2, 1.0] }
/// The terms of the gyro calibration, [X; Y; Z; X²; Y²; Z²; 1]
fn quadratic_terms(x: (f64, f64, f64)) -> [f64; 7] { [x.0, x.1, x.2, x.0 * x.0, x.1 * x.1, x.2 * x.2, 1.0] }

/// What the recorder that wrote the file is known to get wrong, by the `Manufacturer` and `Model` of its recorder information
/// document. Ambient's Lockit+ writes `Manufacturer: Ambient Recording GmbH` and `Model: LockitPlus`. The recorder information of
/// its files has no firmware version (its keys are `Manufacturer`, `Model`, `SerialNumber`, `RecordDate`, `FrameRate` and
/// `DropFrame`), and Ambient's release notes up to firmware 8.08 (February 2026) list no fix, so every Lockit+ file is corrected.
/// A Lockit+ firmware that fixes one of these is to be excluded by an arm before its own, matching the version its recorder
/// information gives
///
/// The evidence is the takes A002C006_230223GM and A002C007_2302234U, recorded both by a VENICE 2, in its RTMD metadata, and by a
/// Lockit+ on the auxiliary /i port of the same S7/i Anamorphic 32 mm (lens firmware 7.63). Each Kdi poll of the camera read out
/// a burst of inertial data that the nearest poll of the Lockit+ also read out (906 and 794 bursts, in 388 and 337 polls)
#[derive(Clone, Copy, Default)]
struct RecorderQuirks {
    /// The recorder wrote the 16-bit inertial values with their bytes swapped
    ///
    /// A lens sends the magnetometer, gyro and accelerometer values of its Kdi response little-endian, although the bit tables of
    /// the specification put their high byte first (Cooke /i Technology Part I, 5.1.28; see [`bin`]). Lockit+ reads them big-endian:
    /// every burst that the camera recorded is in the Lockit+ file, under the same lens timestamp, with the bytes of each of its 24
    /// values swapped. With the lens's calibration, the camera's values and the swapped back Lockit+ values give an accelerometer
    /// magnitude of 9.83 m/s², gravity, against 19.4 m/s² for the values as written. The timestamps of the bursts are written
    /// right, and the sample output of the Cooke /i Lens Metadata File specification has values of the little-endian reading
    swapped_inertial_bytes: bool,
    /// When the recorder converts the lens's distances from tenths of an inch to millimetres, it multiplies the `HorizontalFov` of
    /// the general records by 2.54 as well
    ///
    /// The lens gives its focus, hyperfocal, near and far distances and its entrance pupil position in 1 mm or 0.1 inch steps,
    /// depending on the display units that the channel asking has selected, and its horizontal field of view in "Degrees x 0.1"
    /// (Cooke /i Technology Part I, 5.1.2, 5.1.3, 5.1.13). The Lens Metadata File stores the distances in millimetres, the recorder
    /// converting them (Lens Metadata File specification, "Lens General Information"), but its table gives the horizontal field
    /// of view "in millimeters" too. The camera's channel answered in mm, the Lockit+ channel in tenths of an inch: every distance
    /// in the Lockit+ files is a whole number of 0.1 inch in mm, and matches the camera's value at the same moment, the hyperfocal
    /// distance and entrance pupil position to the 1.27 mm of the 0.1 inch step, the focus, near and far distances to 0.2 % where
    /// the camera saw no change around the polls. Their horizontal field of view is 180.848, 181.102 and 181.356 where the
    /// camera's Kd data is 71.2°, 71.3° and 71.4°: 2.54 times it, in each of the 713 pairs of polls whose lens state didn't change.
    /// The aperture values are written right. The focal length and the normalized zoom position are 0 for this prime lens, so they
    /// can't show whether Lockit+ converts a zoom lens's too, whose Zoom-EFL is always in mm (5.1.2)
    inflated_horizontal_fov: bool,
}

impl RecorderQuirks {
    fn of(recorder: &serde_json::Value) -> Self {
        let text = |key| recorder.get(key).and_then(|v| v.as_str());
        match (text("Manufacturer"), text("Model")) {
            (Some("Ambient Recording GmbH"), Some("LockitPlus")) => Self { swapped_inertial_bytes: true, inflated_horizontal_fov: true },
            _ => Self::default()
        }
    }
}

/// Whether the distances of the general records are whole numbers of tenths of an inch, in millimetres: the lens answered the
/// recorder in imperial units, which the recorder converted. Lockit+ multiplies the number of tenths by 2.54 in single precision
/// and writes the shortest decimal of the `f32` product: 4084.3198 for 1608 × 2.54, as for every distance in its files. That `f32`
/// is at most 3/4 of its unit in the last place (ULP) from the multiple of 2.54 mm: half an ULP from rounding the product, the rest
/// from 2.54 being 1.5e-8 of itself off in single precision. So a distance more than an ULP from every multiple of 2.54 mm wasn't
/// converted, and only one more than an ULP from every whole millimetre, which the lens may have given in millimetres, shows the
/// conversion. The others tell nothing: the multiples of 127 mm (50 tenths of an inch) with zero, and the distances so large that
/// the single precision rounds their fraction away, such as the 24-bit infinity (Cooke /i Technology Part I, 5.1.3). Below 2^17 mm
/// (131 m), where an ULP is at most 1/128 mm, that's just the multiples of 127 mm, as every other multiple of 2.54 mm is at least
/// 0.02 mm from a whole millimetre
fn distances_in_tenths_of_an_inch<'a>(general: impl Iterator<Item = &'a serde_json::Value>) -> bool {
    const DISTANCES: [&str; 5] = ["FocusDistance", "HyperfocalDistance", "NearFocusDistance", "FarFocusDistance", "EntrancePupilPosition"];
    let mut seen = false;
    for v in general.flat_map(|g| DISTANCES.iter().filter_map(|key| g.get(*key)?.as_f64())) {
        let single = v as f32;
        // A distance beyond the range of single precision isn't one that Lockit+ converted
        if !single.is_finite() { return false; }
        let ulp = f64::from(single.abs().next_up() - single.abs());
        let mm = f64::from(single);
        if (mm - (mm / 2.54).round() * 2.54).abs() > ulp { return false; }
        seen |= (mm - mm.round()).abs() > ulp;
    }
    seen
}

/// `v`, a 16-bit integer, with its two bytes swapped. `None` if it isn't one
fn swapped_bytes(v: f64) -> Option<f64> {
    let i = v as i16;
    (i as f64 == v).then(|| i.swap_bytes() as f64)
}

/// The sums of a least-squares fit of `y` on `x`, with an intercept for each stretch: `points` are `(stretch, p, y)`, in stretches in
/// a row, and `x(first, p)` the regressors of a point from the first point of its stretch that has them, `None` to leave it out. The
/// sums are of `xᵢ·xⱼ` and of `xᵢ·y` about the means of each stretch
fn least_squares<P, const N: usize>(points: &[(usize, P, f64)], x: impl Fn(&P, &P) -> Option<[f64; N]>) -> ([[f64; N]; N], [f64; N]) {
    let (mut xx, mut xy) = ([[0.0; N]; N], [0.0; N]);
    for stretch in points.chunk_by(|a, b| a.0 == b.0) {
        let Some(first) = stretch.iter().find(|p| x(&p.1, &p.1).is_some()) else { continue };
        let xs: Vec<([f64; N], f64)> = stretch.iter().filter_map(|p| Some((x(&first.1, &p.1)?, p.2))).collect();
        let n = xs.len() as f64;
        let mean_x: [f64; N] = std::array::from_fn(|i| xs.iter().map(|p| p.0[i]).sum::<f64>() / n);
        let mean_y = xs.iter().map(|p| p.1).sum::<f64>() / n;
        for (x, y) in &xs {
            for i in 0..N {
                for j in 0..N { xx[i][j] += (x[i] - mean_x[i]) * (x[j] - mean_x[j]); }
                xy[i] += (x[i] - mean_x[i]) * (y - mean_y);
            }
        }
    }
    (xx, xy)
}

#[derive(Default)]
struct LensMetadata {
    header: GroupedTagMap,
    frame_rate: Option<f64>,
    drop_frame: bool,
    quirks: RecorderQuirks,
    calibration_accl: Calibration<4>,
    calibration_gyro: Calibration<7>,
    calibration_magn: Calibration<4>,
    polls: Vec<Poll>,
    reported: BTreeSet<String>,
}

impl LensMetadata {
    fn add(&mut self, record: Record, options: &crate::InputOptions) {
        let data = serde_json::Value::Object(record);
        let rtype = data.get("RecordType").and_then(|x| x.as_str()).map(str::to_owned);
        match rtype.as_deref() {
            Some("rt.header.recorder.info") => {
                self.frame_rate = data.get("FrameRate").and_then(|x| x.as_f64());
                // Without it, it's `false` (Cooke /i Lens Metadata File specification, "Recorder Information Document")
                self.drop_frame = data.get("DropFrame").and_then(|x| x.as_bool()).unwrap_or(false);
                self.quirks = RecorderQuirks::of(&data);
                util::insert_tag(&mut self.header, tag!(parsed GroupId::Default, TagId::Metadata, "Recorder info", Json, |v| format!("{:?}", v), data, vec![]), options);
            },
            Some("rt.header.lens.info") => {
                util::insert_tag(&mut self.header, tag!(parsed GroupId::Lens, TagId::Metadata, "Lens info", Json, |v| format!("{:?}", v), data, vec![]), options);
            },
            Some("rt.header.lens.shading") => {
                util::insert_tag(&mut self.header, tag!(parsed GroupId::Lens, TagId::Shading, "Lens shading", Json, |v| format!("{:?}", v), data, vec![]), options);
            },
            Some("rt.header.lens.distortion") => {
                util::insert_tag(&mut self.header, tag!(parsed GroupId::Lens, TagId::Distortion, "Lens distortion", Json, |v| format!("{:?}", v), data, vec![]), options);
            },
            Some("rt.header.lens.cal.accelerometer") => self.calibration_accl = Calibration::from_record(&data),
            Some("rt.header.lens.cal.gyro")          => self.calibration_gyro = Calibration::from_record(&data),
            Some("rt.header.lens.cal.magnetometer")  => self.calibration_magn = Calibration::from_record(&data),
            Some(t @ ("rt.temporal.lens.accelerometer.raw" | "rt.temporal.lens.gyro.raw")) => {
                let sensor = if t == "rt.temporal.lens.accelerometer.raw" { Sensor::Accelerometer } else { Sensor::Gyroscope };
                match (Self::timestamp(&data), Self::datavals(&data)) {
                    (Some(timestamp), Some(samples)) if !samples.is_empty() => {
                        self.poll(&data).bursts.push(Burst { sensor, timestamp, samples, end: 0, spacing: None });
                    },
                    _ => self.report(t, "without a valid Timestamp and Datavals")
                }
            },
            Some(t @ "rt.temporal.lens.magnetometer.raw") => {
                match (Self::timestamp(&data), Self::datavals(&data)) {
                    (Some(timestamp), Some(v)) if v.len() == 1 => {
                        // A second magnetometer record before a general record is the next poll's
                        if let Some(poll) = self.polls.last_mut().filter(|p| p.magnetometer.is_some()) {
                            poll.complete = true;
                        }
                        let poll = self.poll(&data);
                        poll.kdi = Some(timestamp);
                        poll.magnetometer = Some(v[0]);
                    },
                    _ => self.report(t, "without a valid Timestamp and Datavals")
                }
            },
            Some(t @ "rt.temporal.lens.general") => {
                // Lenses before /i version 2 have no lens clock (Cooke /i Lens Metadata File specification, "Lens General Information")
                let timestamp = Self::timestamp(&data);
                if timestamp.is_none() && data.get("Timestamp").is_some() {
                    self.report(t, "with an invalid Timestamp");
                }
                let poll = self.poll(&data);
                let mismatch = matches!((poll.kdi, timestamp), (Some(a), Some(b)) if a != b);
                poll.kdi = poll.kdi.or(timestamp);
                poll.general = Some(data);
                poll.complete = true;
                if mismatch {
                    self.report(t, "with a Timestamp other than its magnetometer record's");
                }
            },
            _ => {
                // The record types can be extended (e.g. with `.derived` data) and documents without a `RecordType`
                // are disregarded (Cooke /i Lens Metadata File specification, "LMF Records")
                if self.reported.insert(format!("{rtype:?}")) {
                    log::debug!("Skipping Cooke /i records of type {rtype:?}");
                }
            }
        }
    }

    /// The poll that a temporal record belongs to
    fn poll(&mut self, data: &serde_json::Value) -> &mut Poll {
        if self.polls.last().is_none_or(|p| p.complete) {
            self.polls.push(Poll::default());
        }
        let poll = self.polls.last_mut().unwrap();
        if poll.timecode.is_none() {
            poll.timecode = Timecode::from_record(data);
        }
        poll
    }

    fn report(&mut self, rtype: &str, problem: &str) {
        if self.reported.insert(format!("{rtype} {problem}")) {
            log::warn!("Skipping Cooke /i {rtype} records {problem}");
        }
    }

    fn timestamp(data: &serde_json::Value) -> Option<u16> {
        data.get("Timestamp")?.as_u64()?.try_into().ok()
    }

    /// The `X`, `Y` and `Z` sets of `Datavals`: an array of them, or a single one for the magnetometer
    fn datavals(data: &serde_json::Value) -> Option<Vec<(f64, f64, f64)>> {
        let xyz = |v: &serde_json::Value| Some((v.get("X")?.as_f64()?, v.get("Y")?.as_f64()?, v.get("Z")?.as_f64()?));
        match data.get("Datavals")? {
            serde_json::Value::Array(arr) => arr.iter().map(xyz).collect(),
            v => Some(vec![xyz(v)?])
        }
    }

    /// Swaps the bytes of the inertial values back, for a recorder that swapped them ([`RecorderQuirks::swapped_inertial_bytes`]).
    /// A record with a value that isn't a 16-bit integer can't be one whose bytes were swapped, so it's left out
    fn swap_inertial_bytes_back(&mut self) {
        let swap = |v: &(f64, f64, f64)| Some((swapped_bytes(v.0)?, swapped_bytes(v.1)?, swapped_bytes(v.2)?));
        let mut invalid = BTreeSet::new();
        for poll in &mut self.polls {
            poll.bursts.retain_mut(|burst| match burst.samples.iter().map(swap).collect::<Option<Vec<_>>>() {
                Some(samples) => { burst.samples = samples; true },
                None => { invalid.insert(burst.sensor.record_type()); false }
            });
            if let Some(v) = poll.magnetometer {
                poll.magnetometer = swap(&v);
                if poll.magnetometer.is_none() { invalid.insert("rt.temporal.lens.magnetometer.raw"); }
            }
        }
        for rtype in invalid {
            self.report(rtype, "with Datavals that aren't 16-bit integers, whose bytes can't be swapped back");
        }
    }

    /// Divides the `HorizontalFov` of the general records by 2.54, for a recorder that multiplied it by 2.54 with the distances
    /// ([`RecorderQuirks::inflated_horizontal_fov`]). The lens gives it in tenths of a degree, so it's rounded to them, which drops
    /// the error of the recorder's single precision product (180.84799 for 71.2 × 2.54)
    fn restore_horizontal_fov(&mut self) {
        for general in self.polls.iter_mut().filter_map(|p| p.general.as_mut()) {
            if let Some(fov) = general.get_mut("HorizontalFov") && let Some(v) = fov.as_f64() {
                *fov = ((v / 2.54 * 10.0).round() / 10.0).into();
            }
        }
    }

    /// Places the Kdi polls and the bursts on one timeline of the lens clock. Two clocks time a poll:
    /// - the lens clock: the `Timestamp` of the poll's magnetometer and general records, when the lens received the poll, on a 16-bit
    ///   counter at 150 kHz that rolls over every 65536 ticks, 0.44 s (Cooke /i Technology Part III, "5 Timing data"). It is exact
    ///   but for whole periods: read from the poll before (5.1), a poll is at its nearest reading when the two are less than half
    ///   a period apart, but a gap of half a period or more, or a poll out of order by as much, reads whole periods off;
    /// - the recorder's timecode, which every temporal record carries (Cooke /i Lens Metadata File specification, "Time-Varying
    ///   Documents"). It locates the record to within a frame and doesn't roll over, but it is only as right as the recorder
    ///   writes it: it can be reset, held, stand still, or be written with another poll's record.
    ///
    /// While the timecode is right, the lens clock read from poll to poll keeps a fixed offset from it, to within
    /// [`TimecodeClock::tolerance`] (and 7 ppm in the LockitPlus recordings), but for the whole periods it misreads. So the polls
    /// are taken in runs, polls in a row whose offsets agree, and a poll that repeats the frame before it, which adds no time, is
    /// left out ([`Self::timecode_runs`]). In a run the two clocks confirm each other, and its polls are where the lens clock puts
    /// them, a stale or out-of-order one included. Each run is then placed against the runs placed before it ([`Self::place_runs`]):
    /// - by the timecode, when its offset agrees with theirs modulo the period and puts its first poll no more than half a period
    ///   before them, a step back that the lens clock reads itself. As the records are in linear-time order ("Time-Varying
    ///   Documents"), the timecode adds the whole periods of a gap, but it doesn't put polls further back than the lens clock sees;
    /// - by its own timecode, out of order, a single poll that the timecode puts further back: a record out of place, a stale or
    ///   moved poll;
    /// - after the latest poll, the polls between placed by the lens clock, when its offset disagrees and no later run's agrees
    ///   with theirs before one agrees with this run's, or when it agrees by whole periods further back: the timecode was reset,
    ///   and the run's offset places the runs after it;
    /// - otherwise its timecode is wrong, until a later run agrees with the runs before again: held over a dropout, or written
    ///   with another slot's poll. The lens clock places the run alone.
    ///
    /// That takes a timecode that keeps time with the lens clock across the file, at the `FrameRate` of the recorder information
    /// or else at the rate that its polls show, and the file is placed as one without a timecode otherwise ([`Self::timecode_clock`]).
    ///
    /// Where the lens clock places a poll alone (in such a run, repeating the frame before it, without a valid timecode, or in a
    /// file without a timecode), the poll follows the latest poll before it, at the first time of its timestamp. Where that is at
    /// or after the next poll that the timecode places, the poll is out of order, and it's placed before the latest poll instead.
    /// Where the polls placed so between two that the timecode places come out whole periods short of the second, the recorder
    /// lost a period or more of polls between them, and the periods go to the step least like the regular one
    /// ([`Self::missed_periods`]).
    ///
    /// A burst is unwrapped from its own poll, which read it out of the lens's buffer of the latest eight packets (Part I, 5.1.28
    /// Kdi), so the accelerometer and gyro data can't slip a whole roll-over period against the magnetometer and lens data (Part
    /// III, 5.2): the data shows the newest burst of a poll between 0.2 ms after and 34 ms before the poll. A poll cut off before
    /// its magnetometer and general records unwraps its bursts from the latest poll, which they follow
    fn place_on_timeline(&mut self) {
        // The lens clock read from poll to poll: each poll at the reading of its timestamp nearest the poll before
        let mut lens: Vec<Option<i64>> = vec![None; self.polls.len()];
        let mut previous: Option<ClockPoint> = None;
        for (ticks, poll) in lens.iter_mut().zip(&self.polls) {
            if let Some(raw) = poll.kdi {
                let point = ClockPoint { raw, ticks: previous.map_or(raw as i64, |p| p.nearest(raw)) };
                *ticks = Some(point.ticks);
                previous = Some(point);
            }
        }
        // The regular step from poll to poll, as the recorder polls: the lens clock's median step
        let mut steps: Vec<i64> = lens.iter().flatten().collect::<Vec<_>>().windows(2).map(|w| w[1] - w[0]).collect();
        let typical = if steps.is_empty() { 0 } else { let mid = steps.len() / 2; *steps.select_nth_unstable(mid).1 };

        let timecode = self.timecode_clock(&lens, typical);
        let (runs, periods) = match &timecode {
            Some(timecode) => {
                let runs = self.timecode_runs(timecode, &lens);
                let periods = self.place_runs(&runs, &lens, timecode.tolerance());
                (runs, periods)
            },
            None => (Vec::new(), Vec::new()),
        };

        // Each poll's run, the periods that place it if they do, and the next poll from it whose run they place in time order
        let mut run_of = vec![None; self.polls.len()];
        for (j, run) in runs.iter().enumerate() {
            for &i in &run.polls { run_of[i] = Some(j); }
        }
        let placed = |i: usize| run_of[i].and_then(|j| periods[j]);
        let mut next_placed = vec![None; self.polls.len() + 1];
        for i in (0..self.polls.len()).rev() {
            next_placed[i] = if placed(i).is_some_and(|(_, in_order)| in_order) { Some(i) } else { next_placed[i + 1] };
        }
        // The lens clock's reading of the polls that it places alone
        let alone_lens: Vec<Option<i64>> = lens.iter().enumerate().map(|(i, ticks)| ticks.filter(|_| placed(i).is_none())).collect();

        // The runs are placed on the lens clock's reading, which starts at the first poll's timestamp. The timeline starts there
        // too, unless polls that the lens clock places alone come first: then the first run follows them, and the whole periods
        // between the two (`shift`) are added to every placed run
        let mut shift = None;
        let mut alone = vec![None; runs.len()];
        // Whether a stretch of polls that the lens clock places alone is being placed, between two that the timecode places, and
        // the periods that the stretch misses, with the poll they come before
        let (mut stretch, mut missed) = (false, None);
        let mut latest: Option<ClockPoint> = None;
        for i in 0..self.polls.len() {
            let anchor = match (self.polls[i].kdi, lens[i]) {
                (Some(raw), Some(ticks)) => {
                    let ticks = match placed(i) {
                        Some((periods, in_order)) => {
                            stretch &= !in_order;
                            let shift = *shift.get_or_insert_with(|| (Self::by_lens_clock(latest, raw, None) - ticks) / TIMESTAMP_MODULUS);
                            ticks + (periods + shift) * TIMESTAMP_MODULUS
                        },
                        None => {
                            let n = next_placed[i + 1];
                            let next = shift.zip(n).and_then(|(shift, n)| Some(lens[n]? + (placed(n)?.0 + shift) * TIMESTAMP_MODULUS));
                            if !stretch {
                                stretch = true;
                                missed = n.zip(latest).zip(next).and_then(|((n, latest), next)| self.missed_periods(i..n, latest, next, &alone_lens, &run_of, typical));
                            }
                            let place = |latest: Option<ClockPoint>| match (missed, latest) {
                                (Some((at, periods)), Some(latest)) if at == i => latest.following(raw) + periods * TIMESTAMP_MODULUS,
                                _ => Self::by_lens_clock(latest, raw, next),
                            };
                            match run_of[i] {
                                // The polls of a run keep their places in it
                                Some(j) => ticks + *alone[j].get_or_insert_with(|| (place(latest) - ticks) / TIMESTAMP_MODULUS) * TIMESTAMP_MODULUS,
                                None => place(latest),
                            }
                        }
                    };
                    ClockPoint { raw, ticks }
                },
                _ => match latest.or_else(|| self.polls[i].bursts.first().map(|b| ClockPoint { raw: b.timestamp, ticks: b.timestamp as i64 })) {
                    Some(anchor) => anchor,
                    None => continue,
                }
            };
            let poll = &mut self.polls[i];
            poll.kdi_ticks = poll.kdi.map(|_| anchor.ticks);
            for burst in &mut poll.bursts {
                burst.end = anchor.nearest(burst.timestamp);
            }
            latest = if poll.kdi.is_some() {
                latest.filter(|l| l.ticks >= anchor.ticks).or(Some(anchor))
            } else {
                poll.bursts.iter().max_by_key(|b| b.end).map(|b| ClockPoint { raw: b.timestamp, ticks: b.end }).or(Some(anchor))
            };
        }
    }

    /// The clock of the recorder's timecode, as the runs of polls need it ([`Self::place_runs`]), from the lens clock read from poll
    /// to poll (`lens`) and its regular step `typical`. The `FrameRate` of the recorder information is the rate of the recording
    /// system, which needn't be the timecode's, so the clock that it and the `DropFrame` give is taken:
    /// - where it keeps time with the lens clock across the file ([`Self::keeps_time`]), across gaps in the recording too;
    /// - or where the timecode keeps its rate on average, closely enough for the runs ([`Self::keeps_rate`]). Then its polls that
    ///   disagree are faults of single polls or of stretches of them, such as jitter, holds or resets, which the runs place.
    ///
    /// Otherwise, and where the recorder information has no `FrameRate` or one that timecode doesn't count at, the clock that the
    /// timecode keeps is taken where that keeps time with the lens clock. Without one, the lens clock places the polls alone, as in
    /// a file without a timecode
    fn timecode_clock(&self, lens: &[Option<i64>], typical: i64) -> Option<TimecodeClock> {
        let stated = self.frame_rate.and_then(|frame_rate| {
            let clock = TimecodeClock::new(frame_rate, self.drop_frame);
            if clock.is_none() {
                log::warn!("The FrameRate of the recorder information of the Cooke /i lens data ({frame_rate}) isn't a rate that timecode counts at");
            }
            clock.map(|clock| (clock, self.keeps_time(&clock, lens, typical)))
        });
        if let Some((stated, time)) = &stated && time.keeps {
            return Some(*stated);
        }
        let measured = self.measured_clock(lens, typical);
        if let Some((stated, time)) = &stated && self.keeps_rate(stated, measured.as_ref(), lens, typical, time.span) {
            return Some(*stated);
        }
        let measured = measured.filter(|clock| self.keeps_time(clock, lens, typical).keeps);
        match (&measured, self.frame_rate) {
            (Some(clock), Some(frame_rate)) => log::warn!("The timecode of the Cooke /i lens data keeps time with the lens clock at {:.6} frames a second, {} to its second, not at the FrameRate of its recorder information ({frame_rate}): the polls are timed at that rate", clock.frame_rate, clock.base),
            (Some(clock), None) => log::warn!("The recorder information of the Cooke /i lens data has no FrameRate, and its timecode keeps time with the lens clock at {:.6} frames a second, {} to its second: the polls are timed at that rate", clock.frame_rate, clock.base),
            (None, Some(frame_rate)) => log::warn!("Disregarding the timecode of the Cooke /i lens data, which keeps time with the lens clock neither at the FrameRate of its recorder information ({frame_rate}) nor at a rate that its polls show: the lens clock places the polls alone"),
            (None, None) => { },
        }
        measured
    }

    /// How the `timecode` keeps time with the lens clock read from poll to poll (`lens`, whose regular step is `typical`) across
    /// the file, as it must for the offsets of its runs, compared modulo the roll-over period, to count the whole periods that the
    /// lens clock misreads ([`Self::place_runs`]). The evidence is a stretch of polls whose offsets lie within the tolerance of
    /// each other. A poll that repeats the frame before it adds no time and is passed over, but unlike in a run
    /// ([`Self::timecode_runs`]) it doesn't end the stretch, which only polls whose offsets keep within the tolerance make longer:
    /// - of the timecode's rate, which keeps time with the lens clock's to within the tolerance over the stretch. A rate as far off
    ///   moves the offset by up to `tolerance × span / stretch` over the polls' span on the lens clock, and two offsets modulo the
    ///   period only take a wrong whole period from a difference of the period less twice the tolerance. So the longest stretch
    ///   must reach `tolerance / (period − 2 × tolerance)` of the span, 14 % at 25 fps;
    /// - of its frames to the second, when the stretch spans the change of a second: a timecode whose second holds more or fewer
    ///   frames than the clock counts jumps against the lens clock at every change of second, whatever its rate. Polls of several
    ///   seconds need such a stretch.
    ///
    /// The lens clock reads the time between polls exactly where they follow each other at the regular step, to within half of
    /// it: one poll step, as polls are sent once per frame (Cooke /i Technology Part III, 5.1). Any other step may hide whole
    /// periods, which the lens clock reads short (a gap of half a period or more; never long, as the records are in time order),
    /// so a stretch goes on across a dropout of polls or a pause of the recording only where the stretch before it bounds the
    /// rate closely enough to count those periods by the offsets: where it spans that share of the step by the timecode, the rate
    /// moves the offset by less than the period less twice the tolerance over the step, and just one whole number of periods puts
    /// the poll's offset within the tolerance of the stretch's. Those periods are added back to the lens clock's reading of the
    /// poll and of the polls after it, the span's too. So is a step that the lens clock reads as a regular one but whose offset is
    /// whole periods off: a dropout of a period and a step, 11 polls at 25 fps. Across a step that a stretch can't count, the
    /// next one starts.
    ///
    /// A timecode counted at another rate than the clock's keeps its offset within the tolerance over only a few polls, one or
    /// two at twice or half the rate. Such runs agree with others modulo the period by chance, about one in five, and the
    /// timecode would move them by whole periods
    fn keeps_time(&self, timecode: &TimecodeClock, lens: &[Option<i64>], typical: i64) -> Timekeeping {
        // A stretch: its first second, the extent of its offsets and of its lens clock readings
        struct Stretch { second: (u32, u32, u32), low: f64, high: f64, from: i64, to: i64 }
        let tolerance = timecode.tolerance();
        let modulus = TIMESTAMP_MODULUS as f64;
        // The share of a step that a stretch has to span to count the whole periods in it
        let share = tolerance / (modulus - 2.0 * tolerance);
        // The polls' span, the longest stretch, and whether a stretch and the polls span a change of second
        let (mut span, mut longest, mut stretch_spans, mut polls_span) = (None, 0, false, false);
        let mut stretch: Option<Stretch> = None;
        let (mut origin, mut previous, mut first_second) = (None, None, None);
        // The whole periods counted so far that the lens clock misread, and whether it read each step since the latest poll of
        // the stretch as a regular one
        let (mut periods, mut previous_ticks, mut regular) = (0, None, true);
        for (poll, ticks) in self.polls.iter().zip(lens) {
            let Some(ticks) = *ticks else { continue };
            if let Some(p) = previous_ticks.replace(ticks) {
                regular &= 2 * (ticks - p) >= typical && 2 * (ticks - p) <= 3 * typical;
            }
            let Some(tc) = poll.timecode else { continue };
            let Some(frame) = timecode.frame(tc) else { continue };
            let before = previous.replace(frame);
            if before == Some(frame) { continue; }
            let second = (tc.hh, tc.mm, tc.ss);
            polls_span |= *first_second.get_or_insert(second) != second;
            let reading = ticks + periods * TIMESTAMP_MODULUS;
            let offset = reading as f64 - timecode.ticks_between(*origin.get_or_insert(frame), frame);
            // The whole periods that the stretch counts in the step, and the poll's offset with them
            let counted = stretch.as_ref().zip(before).and_then(|(s, before)| {
                let k = (((s.low + s.high) / 2.0 - offset) / modulus).round() as i64;
                let offset = offset + (k * TIMESTAMP_MODULUS) as f64;
                let bounded = (s.to - s.from) as f64 >= share * timecode.ticks_between(before, frame).abs();
                (((k == 0 && regular) || (k >= 0 && bounded)) && f64::max(s.high, offset) - f64::min(s.low, offset) <= tolerance).then_some((k, offset))
            });
            regular = true;
            match (&mut stretch, counted) {
                (Some(s), Some((k, offset))) => {
                    periods += k;
                    let reading = reading + k * TIMESTAMP_MODULUS;
                    (s.low, s.high, s.from, s.to) = (s.low.min(offset), s.high.max(offset), s.from.min(reading), s.to.max(reading));
                    stretch_spans |= s.second != second;
                    longest = longest.max(s.to - s.from);
                },
                _ => stretch = Some(Stretch { second, low: offset, high: offset, from: reading, to: reading }),
            }
            let reading = ticks + periods * TIMESTAMP_MODULUS;
            span = Some(span.map_or((reading, reading), |(from, to): (i64, i64)| (from.min(reading), to.max(reading))));
        }
        let span = span.map_or(0, |(from, to)| to - from);
        let keeps = (stretch_spans || !polls_span) && tolerance * span as f64 <= (modulus - 2.0 * tolerance) * longest as f64;
        Timekeeping { keeps, span }
    }

    /// Whether the timecode keeps the `timecode` clock on average, closely enough that over the polls' `span` on the lens clock
    /// ([`Timekeeping::span`]) the offsets of its runs, compared modulo the roll-over period, can't take a wrong whole period: the
    /// evidence of a timecode with faults of single polls, such as frames written a frame or two off, which keep
    /// [`Self::keeps_time`]'s stretches short. The clock that the timecode keeps (`measured`) has to count its frames to the
    /// second, where the polls span a change of second: a timecode with more or fewer jumps against the lens clock at every change
    /// of second. Its rate is the slope of a least-squares fit of the frame count on the lens clock's reading, with an intercept
    /// for each stretch of polls in a row that the lens clock reads exactly: at the regular step `typical`, as in
    /// [`Self::measured_clock`], and without a jump of the offset of half a period or more, which is the whole period of a dropout
    /// read as one step (11 polls at 25 fps) or a reset of the timecode. Polls that repeat the frame before them are left out. The
    /// frames are fitted on the lens clock, whose reading of a poll is exact, and not the other way round, as the timecode's
    /// errors would bias the slope towards zero as regressors. The rate error and four standard errors of it, from the residuals,
    /// have to move the offset by at most the period less twice the tolerance across the span, the bound that
    /// [`Self::keeps_time`]'s longest stretch sets: a rate that is as far off is taken with a chance of 3 in 100,000
    fn keeps_rate(&self, timecode: &TimecodeClock, measured: Option<&TimecodeClock>, lens: &[Option<i64>], typical: i64, span: i64) -> bool {
        // The polls of the fit: their stretch, the lens clock's reading and the frame count from the first poll
        let mut points: Vec<(usize, f64, f64)> = Vec::new();
        let (mut stretch, mut previous_ticks, mut previous) = (0, None, None);
        let (mut frames, mut first_second, mut polls_span) = (0, None, false);
        for (poll, ticks) in self.polls.iter().zip(lens) {
            let Some(ticks) = *ticks else { continue };
            if previous_ticks.replace(ticks).is_some_and(|p: i64| 2 * (ticks - p) < typical || 2 * (ticks - p) > 3 * typical) {
                stretch += 1;
            }
            let Some(tc) = poll.timecode else { continue };
            let Some(frame) = timecode.frame(tc) else { continue };
            if let Some((before_ticks, before, before_stretch)) = previous {
                if before == frame { continue; }
                frames += timecode.frames_between(before, frame);
                if before_stretch == stretch && 2.0 * ((ticks - before_ticks) as f64 - timecode.ticks_between(before, frame)).abs() >= TIMESTAMP_MODULUS as f64 {
                    stretch += 1;
                }
            }
            let second = (tc.hh, tc.mm, tc.ss);
            polls_span |= *first_second.get_or_insert(second) != second;
            previous = Some((ticks, frame, stretch));
            points.push((stretch, ticks as f64, frames as f64));
        }
        if polls_span && !measured.is_some_and(|clock| clock.base == timecode.base && clock.dropped == timecode.dropped) {
            return false;
        }
        // The slope with an intercept for each stretch, and its residuals
        let stretches: Vec<&[(usize, f64, f64)]> = points.chunk_by(|a, b| a.0 == b.0).collect();
        let means: Vec<(f64, f64)> = stretches.iter().map(|s| {
            let n = s.len() as f64;
            (s.iter().map(|p| p.1).sum::<f64>() / n, s.iter().map(|p| p.2).sum::<f64>() / n)
        }).collect();
        let centred = || stretches.iter().zip(&means).flat_map(|(s, &(x, y))| s.iter().map(move |p| (p.1 - x, p.2 - y)));
        let (xx, xy) = centred().fold((0.0, 0.0), |(xx, xy), (x, y)| (xx + x * x, xy + x * y));
        let free = points.len().saturating_sub(stretches.len() + 1);
        if xx <= 0.0 || free == 0 { return false; }
        let slope = xy / xx;
        let residuals = centred().map(|(x, y)| (y - slope * x).powi(2)).sum::<f64>();
        // The rate's error relative to the clock's and the standard error of the slope, in frames per tick of the clock's
        let per_tick = timecode.frame_rate / CLOCK_HZ;
        let (error, standard) = ((slope / per_tick - 1.0).abs(), (residuals / free as f64 / xx).sqrt() / per_tick);
        (error + 4.0 * standard) * span as f64 <= TIMESTAMP_MODULUS as f64 - 2.0 * timecode.tolerance()
    }

    /// The clock that the timecode keeps against the lens clock read from poll to poll (`lens`), from its seconds and frames. The
    /// lens clock reads the time between polls that follow each other at the regular step `typical`, to within half of it: one
    /// poll step, as polls are sent once per frame (Cooke /i Technology Part III, 5.1), not none and not two. A step that isn't
    /// one may hide whole periods, so the stretches of polls between such steps are fitted, by least squares with an intercept
    /// for each stretch, as the lens clock's reading of a poll from the seconds and the frames of its timecode, polls that repeat
    /// the timecode before them left out. That gives the length of the timecode's second and of its frame, and the frames to its
    /// second are their ratio, a whole number. A step that the lens clock reads as one but that is half a period or more off the
    /// fit hides whole periods (a dropout of a period and a step, 11 polls at 25 fps), so the stretch ends there and the fit is
    /// made again. Its rate then comes from the fit on its frame count, which spans the stretches
    /// where its frames span a second at most, and counts the frame numbers that drop-frame timecode skips. `None` without a
    /// stretch that spans a change of second, or for a clock that timecode doesn't count at
    fn measured_clock(&self, lens: &[Option<i64>], typical: i64) -> Option<TimecodeClock> {
        // The polls of the fit: their stretch, their timecode and the lens clock's reading
        let mut polls: Vec<(usize, Timecode, f64)> = Vec::new();
        let (mut stretch, mut previous_ticks, mut previous_tc) = (0, None, None);
        for (poll, ticks) in self.polls.iter().zip(lens) {
            let Some(ticks) = *ticks else { continue };
            if previous_ticks.replace(ticks).is_some_and(|p: i64| 2 * (ticks - p) < typical || 2 * (ticks - p) > 3 * typical) {
                stretch += 1;
            }
            let Some(tc) = poll.timecode.filter(|tc| tc.hh <= 23 && tc.mm <= 59 && tc.ss <= 59) else { continue };
            if previous_tc.replace(tc) == Some(tc) { continue; }
            polls.push((stretch, tc, ticks as f64));
        }
        // The seconds from `from` to `tc`, around midnight as the timecode wraps around there
        let seconds = |from: &Timecode, tc: &Timecode| {
            let s = |tc: &Timecode| (tc.hh as i64 * 60 + tc.mm as i64) * 60 + tc.ss as i64;
            let seconds = (s(tc) - s(from)).rem_euclid(86400);
            (if seconds > 43200 { seconds - 86400 } else { seconds }) as f64
        };
        // The lengths of the timecode's second and of its frame
        let fit = |polls: &[(usize, Timecode, f64)]| {
            let ([[ss, sf], [_, ff]], [ts, tf]) = least_squares(polls, |from, tc| Some([seconds(from, tc), tc.ff as f64]));
            // Zero where the seconds don't change within a stretch, or change only with the frames
            let det = ss * ff - sf * sf;
            (det > 0.0).then(|| ((ts * ff - tf * sf) / det, (tf * ss - ts * sf) / det))
        };
        let (second, frame) = fit(&polls)?;
        // A step that the lens clock reads as a regular one, but half a period or more off the fit's step for the timecode's,
        // hides whole periods: a dropout of a period and a step, 11 polls at 25 fps, reads as one step. That biases the fit,
        // so the stretch ends there, and the fit is made again
        let mut ends = 0;
        let stretches: Vec<usize> = polls.iter().map(|p| p.0).collect();
        for k in 1..polls.len() {
            let ((_, from, before), (_, tc, ticks)) = (polls[k - 1], polls[k]);
            let step = second * seconds(&from, &tc) + frame * (tc.ff as f64 - from.ff as f64);
            if stretches[k - 1] == stretches[k] && 2.0 * (ticks - before - step).abs() >= TIMESTAMP_MODULUS as f64 {
                ends += 1;
            }
            polls[k].0 += ends;
        }
        let (second, frame) = if ends > 0 { fit(&polls)? } else { (second, frame) };
        let base = (second / frame).round();
        let clock = TimecodeClock::counting(base * CLOCK_HZ / second, base, self.drop_frame)?;
        let ([[frames]], [ticks]) = least_squares(&polls, |from, tc| Some([clock.frames_between(clock.frame(*from)?, clock.frame(*tc)?) as f64]));
        TimecodeClock::counting(CLOCK_HZ * frames / ticks, base, self.drop_frame)
    }

    /// The runs of polls whose timecode keeps time with the lens clock read from poll to poll (`lens`): polls in a row, leaving
    /// out those without a valid timecode, whose offsets of the lens clock from the timecode all lie within the tolerance of each
    /// other. In a run the lens clock misreads no period, which would move the offset by a period.
    ///
    /// A poll with the frame of the poll before it adds no time to the timecode's: it may be in the same frame, a repeat of the
    /// poll, or a poll whose timecode was held or stands still, and the frame can't tell these apart. The lens clock places it
    /// alone, as it does a poll without a timecode, and the run ends there, so that no poll after it joins the run by chance.
    /// The first poll of a frame keeps it: a timecode that stands still from a poll on is the right one there
    fn timecode_runs(&self, timecode: &TimecodeClock, lens: &[Option<i64>]) -> Vec<Run> {
        let tolerance = timecode.tolerance();
        let mut runs: Vec<(Run, Vec<f64>)> = Vec::new();
        let (mut origin, mut previous, mut low, mut high, mut open) = (None, None, 0.0, 0.0, false);
        for (i, (poll, ticks)) in self.polls.iter().zip(lens).enumerate() {
            let (Some(ticks), Some(frame)) = (*ticks, poll.timecode.and_then(|tc| timecode.frame(tc))) else { continue };
            if previous.replace(frame) == Some(frame) {
                open = false;
                continue;
            }
            let offset = ticks as f64 - timecode.ticks_between(*origin.get_or_insert(frame), frame);
            match runs.last_mut() {
                Some((run, offsets)) if open && f64::max(high, offset) - f64::min(low, offset) <= tolerance => {
                    run.polls.push(i);
                    offsets.push(offset);
                    (low, high) = (f64::min(low, offset), f64::max(high, offset));
                },
                _ => {
                    runs.push((Run { polls: vec![i], offset }, vec![offset]));
                    (low, high, open) = (offset, offset, true);
                }
            }
        }
        runs.into_iter().map(|(mut run, mut offsets)| {
            offsets.sort_unstable_by(f64::total_cmp);
            run.offset = offsets[offsets.len() / 2];
            run
        }).collect()
    }

    /// The whole roll-over periods to add to the lens clock's reading (`lens`) of each run that the timecode places, and of each
    /// run after a reset of the timecode, which follows the latest poll, and whether the run is in time order: a single poll
    /// out of order, which its own timecode places, doesn't place others. `None` for a run whose timecode is wrong, which the
    /// lens clock places alone ([`Self::place_on_timeline`]). The first run is where the lens clock reads it, and its offset is
    /// the first level that the next runs are placed from
    fn place_runs(&self, runs: &[Run], lens: &[Option<i64>], tolerance: f64) -> Vec<Option<(i64, bool)>> {
        let mut periods = vec![None; runs.len()];
        let Some(first) = runs.first() else { return periods };
        let point = |i: usize, k: i64| ClockPoint { raw: self.polls[i].kdi.unwrap_or_default(), ticks: lens[i].unwrap_or_default() + k * TIMESTAMP_MODULUS };
        let latest = |run: &Run, k: i64| run.polls.iter().map(|&i| point(i, k)).max_by_key(|p| p.ticks);
        // The periods by which the timecode places a run from the `level` of the runs before, if that is in time order after
        // `end`, the latest of them: the run's first poll at most less than half a period before it, a step back that the lens
        // clock reads itself, of a stale or out-of-order poll. The timecode adds the periods of a gap, but it doesn't put a run
        // further back than the lens clock can see, against the records' order
        let by_timecode = |run: &Run, level: f64, end: i64| run.periods_to(level, tolerance)
            .filter(|&k| point(run.polls[0], k).ticks > end - TIMESTAMP_MODULUS / 2);

        periods[0] = Some((0, true));
        let (mut level, mut end, mut last) = (first.offset, latest(first, 0), first.polls.last().copied().unwrap_or_default());
        for (j, run) in runs.iter().enumerate().skip(1) {
            let Some(end_point) = end else { break };
            let agrees = run.periods_to(level, tolerance);
            let k = match by_timecode(run, level, end_point.ticks) {
                Some(k) => k,
                // A single poll that the timecode puts further back is a record out of place, a stale or moved poll: its own
                // timecode places it, out of order
                None if run.polls.len() == 1 && let Some(k) = agrees => {
                    periods[j] = Some((k, false));
                    continue;
                },
                // A timecode that disagrees with the runs before was reset, unless a later run agrees with them again before one
                // agrees with this run, and so was one that runs on further back than the lens clock reads. The run then follows
                // the latest poll through the polls between, as the lens clock places those alone
                None if agrees.is_some() || runs[j + 1..].iter()
                    .find_map(|later| if later.periods_to(run.offset, tolerance).is_some() { Some(true) } else { by_timecode(later, level, end_point.ticks).map(|_| false) })
                    .unwrap_or(true) => {
                    let i = run.polls[0];
                    let followed = self.polls[last + 1..i].iter().filter_map(|p| p.kdi)
                        .fold(end_point, |latest, raw| ClockPoint { raw, ticks: latest.following(raw) });
                    (followed.following(self.polls[i].kdi.unwrap_or_default()) - lens[i].unwrap_or_default()) / TIMESTAMP_MODULUS
                },
                None => continue,
            };
            periods[j] = Some((k, true));
            level = run.offset + (k * TIMESTAMP_MODULUS) as f64;
            last = run.polls.last().copied().unwrap_or(last);
            if let Some(run_end) = latest(run, k).filter(|p| p.ticks > end_point.ticks) {
                end = Some(run_end);
            }
        }
        periods
    }

    /// Where the lens clock alone places a poll received at `raw`: at the first time of its timestamp at or after the `latest`
    /// poll, if that is before `next`, the next poll that the timecode places. Otherwise the poll is out of order, and it's at the
    /// last time of its timestamp before the latest poll
    fn by_lens_clock(latest: Option<ClockPoint>, raw: u16, next: Option<i64>) -> i64 {
        let ticks = latest.map_or(raw as i64, |l| l.following(raw));
        if next.is_some_and(|next| ticks >= next) { ticks - TIMESTAMP_MODULUS } else { ticks }
    }

    /// The whole periods that a `stretch` of polls that the lens clock places alone misses, between the `latest` poll before it
    /// and the poll that ends it, the next one that the timecode places, at `next` (`lens`, the lens clock's reading of the polls
    /// that it places alone). Those polls follow each other at the first time of each timestamp, their runs keeping their places
    /// in them. Where that puts the poll after the stretch periods before `next`, the recorder lost a period or more of polls in
    /// the stretch, and the periods belong to the step that is least like `typical`, the regular step between polls: returned
    /// with the poll the step leads to. `None` where the stretch misses no period
    fn missed_periods(&self, stretch: std::ops::Range<usize>, latest: ClockPoint, next: i64, lens: &[Option<i64>], run_of: &[Option<usize>], typical: i64) -> Option<(usize, i64)> {
        let n = stretch.end;
        let mut latest = latest;
        let mut runs: Vec<(usize, i64)> = Vec::new();
        let mut steps = Vec::new();
        for i in stretch {
            let (Some(raw), Some(ticks)) = (self.polls[i].kdi, lens[i]) else { continue };
            let in_run = run_of[i].and_then(|j| runs.iter().find(|r| r.0 == j)).map(|r| ticks + r.1 * TIMESTAMP_MODULUS);
            let at = in_run.unwrap_or_else(|| {
                let at = latest.following(raw);
                steps.push((i, at - latest.ticks));
                if let Some(j) = run_of[i] { runs.push((j, (at - ticks) / TIMESTAMP_MODULUS)); }
                at
            });
            if at > latest.ticks { latest = ClockPoint { raw, ticks: at }; }
        }
        let reached = latest.following(self.polls[n].kdi?);
        steps.push((n, reached - latest.ticks));
        let periods = (next - reached) / TIMESTAMP_MODULUS;
        if periods <= 0 { return None; }
        steps.iter().max_by_key(|(_, step)| (step - typical).abs()).map(|&(at, _)| (at, periods))
    }

    /// Leaves out the data that repeats earlier data or is out of time order, whose samples would repeat or go back in time: a burst
    /// that doesn't end after the latest earlier burst of its sensor, and the magnetometer and lens data of a poll that the lens didn't
    /// receive after the previous one. The lens stamps each poll with the time it receives it, and reads each packet out of its buffer
    /// once, clearing the buffer (Cooke /i Technology Part I, 5.1.28 Kdi), and the records are in linear-time order (Cooke /i Lens
    /// Metadata File specification, "Time-Varying Documents"), so such data was recorded twice or out of order. The data it repeats
    /// or precedes is kept, and each burst is judged on its own, so a burst that a repeated poll holds is kept if it's new
    fn drop_repeated_data(&mut self) {
        let (mut latest_poll, mut latest_accl, mut latest_gyro) = (None, None, None);
        let (mut polls, mut bursts) = (0, 0);
        for poll in &mut self.polls {
            if let Some(ticks) = poll.kdi_ticks {
                if latest_poll.is_some_and(|latest| ticks <= latest) {
                    (poll.magnetometer, poll.general) = (None, None);
                    polls += 1;
                } else {
                    latest_poll = Some(ticks);
                }
            }
            poll.bursts.retain(|burst| {
                let latest = match burst.sensor { Sensor::Accelerometer => &mut latest_accl, Sensor::Gyroscope => &mut latest_gyro };
                let after = latest.is_none_or(|latest| burst.end > latest);
                if after { *latest = Some(burst.end); } else { bursts += 1; }
                after
            });
        }
        let mut left_out = Vec::new();
        if bursts > 0 { left_out.push(format!("accelerometer and gyro bursts that don't end after the latest earlier burst of their sensor ({bursts})")); }
        if polls > 0 { left_out.push(format!("the magnetometer and lens data of polls that the lens didn't receive after the previous one ({polls})")); }
        if !left_out.is_empty() {
            log::warn!("Leaving out Cooke /i data that repeats earlier data or is out of time order: {}", left_out.join("; "));
        }
    }

    /// Times the samples of each sensor's bursts. The timestamp of a burst is that of its last sample, and the samples are interpolated
    /// between the end of the previous burst and the end of this one (Cooke /i Technology Part III, 5.3). The first burst has no previous
    /// end, and neither has a burst after a gap (the lens keeps only the latest eight packets, Part I, 5.1.28 Kdi): their samples are
    /// extrapolated from the next interval. The bursts come at a steady rate: in LockitPlus recordings every interval is 5132 or 5133
    /// ticks, so the extrapolation is off by at most 7/8 of a tick (6 µs), where discarding the burst would lose 34 ms of data. Each
    /// burst ends after the previous one of its sensor ([`Self::drop_repeated_data`])
    fn time_bursts(&mut self) {
        for sensor in [Sensor::Accelerometer, Sensor::Gyroscope] {
            let mut bursts: Vec<&mut Burst> = self.polls.iter_mut().flat_map(|p| &mut p.bursts).filter(|b| b.sensor == sensor).collect();
            let ends: Vec<i64> = bursts.iter().map(|b| b.end).collect();
            let sizes: Vec<f64> = bursts.iter().map(|b| b.samples.len() as f64).collect();

            // The period of the bursts. A gap makes an interval of at least two periods, and the latest eight packets that the lens keeps
            // are consecutive, so most intervals are one period
            let mut intervals: Vec<i64> = ends.windows(2).map(|w| w[1] - w[0]).collect();
            intervals.sort_unstable();
            let period = intervals.get(intervals.len() / 2).copied();

            // The interval from burst `k - 1` to burst `k`, if `k` directly follows it
            let follows = |k: usize| -> Option<i64> {
                let interval = ends[k] - ends[k - 1];
                (2 * interval <= 3 * period?).then_some(interval)
            };
            for k in 0..bursts.len() {
                bursts[k].spacing = if k > 0 && let Some(interval) = follows(k) {
                    Some(interval as f64 / sizes[k])
                } else {
                    match (k + 1 < ends.len()).then(|| follows(k + 1)).flatten() {
                        Some(interval) => Some(interval as f64 / sizes[k + 1]),
                        None => period.map(|p| p as f64 / sizes[k])
                    }
                };
            }
        }
    }

    fn into_samples(mut self, options: &crate::InputOptions) -> Vec<SampleInfo> {
        if self.quirks.swapped_inertial_bytes {
            self.swap_inertial_bytes_back();
        }
        if self.quirks.inflated_horizontal_fov && distances_in_tenths_of_an_inch(self.polls.iter().filter_map(|p| p.general.as_ref())) {
            self.restore_horizontal_fov();
        }
        self.place_on_timeline();
        self.drop_repeated_data();
        self.time_bursts();

        // The timeline starts at the first sample of the lens's sensors
        let origin = self.polls.iter().flat_map(|p| {
            let bursts = p.bursts.iter().filter_map(|b| Some(b.end as f64 - (b.samples.len() - 1) as f64 * b.spacing?));
            bursts.chain(p.magnetometer.and(p.kdi_ticks).map(|t| t as f64))
        }).fold(f64::INFINITY, f64::min);
        let seconds = |ticks: f64| (ticks - origin) / CLOCK_HZ;

        // Without the lens's calibration the raw values of a sensor have no physical unit, so its samples are left out
        let has_bursts = |sensor| self.polls.iter().any(|p| p.bursts.iter().any(|b| b.sensor == sensor));
        let mut uncalibrated = Vec::new();
        let calibration_accl = self.calibration_accl.matrix("accelerometer", has_bursts(Sensor::Accelerometer), &mut uncalibrated);
        let calibration_gyro = self.calibration_gyro.matrix("gyro", has_bursts(Sensor::Gyroscope), &mut uncalibrated);
        let calibration_magn = self.calibration_magn.matrix("magnetometer", self.polls.iter().any(|p| p.magnetometer.is_some()), &mut uncalibrated);
        if !uncalibrated.is_empty() {
            log::warn!("Leaving out the samples of Cooke /i lens sensors without a valid calibration, as their raw values have no physical unit: {}", uncalibrated.join("; "));
        }

        let mut samples = Vec::with_capacity(self.polls.len() + 1);
        let mut push = |map: GroupedTagMap| {
            if !map.is_empty() {
                samples.push(SampleInfo { sample_index: samples.len() as u64, tag_map: Some(map), ..Default::default() });
            }
        };
        push(self.header);

        for poll in self.polls {
            let mut map = GroupedTagMap::new();

            if let Some(calib) = &calibration_accl {
                let accl = Self::sensor_samples(&poll.bursts, Sensor::Accelerometer, seconds, |x| calibrate(calib, linear_terms(x)));
                if !accl.is_empty() {
                    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Data,        "Accelerometer data",  Vec_TimeVector3_f64, |v| format!("{:?}", v), accl, vec![]), options);
                    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Unit,        "Accelerometer unit",  String, |v| v.to_string(), "m/s²".into(),  Vec::new()), options);
                    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation",     String, |v| v.to_string(), IMU_ORIENTATION.into(), Vec::new()), options);
                }
            }
            if let Some(calib) = &calibration_gyro {
                let gyro = Self::sensor_samples(&poll.bursts, Sensor::Gyroscope, seconds, |x| calibrate(calib, quadratic_terms(x)));
                if !gyro.is_empty() {
                    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope, TagId::Data,        "Gyroscope data",  Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]), options);
                    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope, TagId::Unit,        "Gyroscope unit",  String, |v| v.to_string(), "rad/s".into(), Vec::new()), options);
                    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), IMU_ORIENTATION.into(), Vec::new()), options);
                }
            }
            // The magnetometer is sampled when the lens receives the poll (Cooke /i Technology Part III, 3.1)
            if let (Some(calib), Some(x), Some(ticks)) = (&calibration_magn, poll.magnetometer, poll.kdi_ticks) {
                let data = calibrate(calib, linear_terms(x));
                let magn = vec![TimeVector3 { t: seconds(ticks as f64), x: data.0, y: data.1, z: data.2 }];
                util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer, TagId::Data,        "Magnetometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), magn, vec![]), options);
                util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer, TagId::Unit,        "Magnetometer unit", String, |v| v.to_string(), "T".into(), Vec::new()), options);
                util::insert_tag(&mut map, tag!(parsed GroupId::Magnetometer, TagId::Orientation, "IMU orientation",   String, |v| v.to_string(), IMU_ORIENTATION.into(), Vec::new()), options);
            }
            if let Some(data) = poll.general {
                util::insert_tag(&mut map, tag!(parsed GroupId::Lens, TagId::Data, "Lens data", Json, |v| format!("{:?}", v), data, vec![]), options);
            }
            push(map);
        }

        samples
    }

    /// The timed samples of `sensor` in `bursts`, in seconds on the timeline
    fn sensor_samples(bursts: &[Burst], sensor: Sensor, seconds: impl Fn(f64) -> f64, calibrated: impl Fn((f64, f64, f64)) -> (f64, f64, f64)) -> Vec<TimeVector3<f64>> {
        let mut ret = Vec::new();
        for burst in bursts.iter().filter(|b| b.sensor == sensor) {
            let Some(spacing) = burst.spacing else {
                log::debug!("Discarding a Cooke /i burst that can't be timed, at {} ticks", burst.end);
                continue;
            };
            let last = burst.samples.len() - 1;
            for (i, x) in burst.samples.iter().enumerate() {
                let data = calibrated(*x);
                ret.push(TimeVector3 { t: seconds(burst.end as f64 - (last - i) as f64 * spacing), x: data.0, y: data.1, z: data.2 });
            }
        }
        ret
    }
}
