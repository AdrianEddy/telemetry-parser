// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::tags_impl::{ Quaternion, TimeQuaternion };

/// A packet's first IMU reading on the frame-relative capture clock, in
/// microseconds. The stamp is the low 32 bits of the microsecond clock;
/// unwrap against THIS frame, not the start of the clip. Signed wrapping
/// subtraction handles samples on either side of the frame, including a wrap.
pub(super) fn anchor_us(frame_relative_us: i64, frame_us: i64, first_sample_us: u32) -> i64 {
    let delta_us = first_sample_us.wrapping_sub(frame_us as u32) as i32;
    frame_relative_us + i64::from(delta_us)
}

/// Packets either side of the one being timed that its reading grid is fitted
/// over. Wide enough to average several cycles of a stamp that is quantized to
/// whole readings, narrow enough that the IMU clock's slow wander against the
/// frame clock is linear inside it.
const GRID_HALF_WINDOW: usize = 25;

/// Where each packet's readings fall on the frame clock: its first reading's
/// time and the spacing of the rest, both in microseconds, from every packet's
/// `(anchor, readings)` in clip order. `None` with fewer than two packets.
///
/// The IMU reads at a steady rate, but neither the stated rate nor the stamps
/// place its readings exactly. The Osmo 360 II states 1000 Hz and delivers a
/// reading every 995 us on this clock, so at the stated rate a packet of 41
/// readings runs into the next. Its stamps sit at a fixed offset from each
/// frame while the reading count drifts, so they are off by up to one reading
/// in a sawtooth. Over a long clip the IMU clock also wanders against the
/// frame clock, by 5 ms across 5 minutes on an Osmo 360, so one straight grid
/// for the whole clip is wrong too.
///
/// So each packet is timed by a straight-line fit of anchor against reading
/// index over the packets around it. The fit averages the sawtooth out and
/// follows the wander. A packet that arrives a whole packet early or late
/// means readings were lost; the grid is split there, so no fit spans the gap.
pub(super) fn reading_grid(packets: &[(i64, usize)]) -> Option<Vec<(f64, f64)>> {
    if packets.len() < 2 {
        return None;
    }
    // The typical period anchors the gap test and times a packet with no
    // neighbour on its side of a gap.
    let mut periods: Vec<f64> = packets.windows(2)
        .filter(|w| w[0].1 > 0 && w[1].0 > w[0].0)
        .map(|w| (w[1].0 - w[0].0) as f64 / w[0].1 as f64)
        .collect();
    if periods.is_empty() {
        return None;
    }
    let mid = periods.len() / 2;
    let period = *periods.select_nth_unstable_by(mid, f64::total_cmp).1;

    let mut grid = Vec::with_capacity(packets.len());
    let mut start = 0;
    while start < packets.len() {
        let mut end = start + 1;
        while end < packets.len() {
            let (previous, next) = (packets[end - 1], packets[end]);
            let expected = previous.1 as f64 * period;
            let tolerance = (previous.1 as f64 / 2.0).max(2.0) * period;
            if ((next.0 - previous.0) as f64 - expected).abs() > tolerance {
                break;
            }
            end += 1;
        }
        // Reading index of each packet's first reading within this run.
        let segment = &packets[start..end];
        let mut index = Vec::with_capacity(segment.len());
        let mut readings = 0usize;
        for p in segment {
            index.push(readings as f64);
            readings += p.1;
        }
        for k in 0..segment.len() {
            let window = k.saturating_sub(GRID_HALF_WINDOW)..(k + GRID_HALF_WINDOW + 1).min(segment.len());
            grid.push(fit_at(segment, &index, window, k).unwrap_or((segment[k].0 as f64, period)));
        }
        start = end;
    }
    Some(grid)
}

/// The least-squares line of anchor against reading index over `window`,
/// evaluated at packet `k`: its first reading's time and the spacing. Centred on
/// `k` so the sums stay small whatever the clock reads.
fn fit_at(packets: &[(i64, usize)], index: &[f64], window: std::ops::Range<usize>, k: usize) -> Option<(f64, f64)> {
    let n = window.len() as f64;
    if n < 2.0 {
        return None;
    }
    let (x0, y0) = (index[k], packets[k].0);
    let (mut sx, mut sy, mut sxx, mut sxy) = (0.0, 0.0, 0.0, 0.0);
    for j in window {
        let (x, y) = (index[j] - x0, (packets[j].0 - y0) as f64);
        sx += x;
        sy += y;
        sxx += x * x;
        sxy += x * y;
    }
    let spread = sxx - sx * sx / n;
    let spacing = (sxy - sx * sy / n) / spread;
    (spread > 0.0 && spacing > 0.0).then(|| (y0 as f64 + (sy - spacing * sx) / n, spacing))
}

/// A frame that the clip's metadata track delivered: its sample's number in
/// the track and its presentation time in milliseconds, and its microsecond
/// stamp on the camera's frame clock, from the first frame's.
pub(crate) struct Frame {
    pub(crate) sample: u64,
    pub(crate) ms: f64,
    pub(crate) us: i64,
}

/// One single-lens frame's fused attitude, held until the whole track is read:
/// it is timed at the rate of the frames' own timestamps ([`sensor_rate`]).
pub(crate) struct HeldQuaternions {
    /// Index of the frame's sample in the parsed series.
    pub(crate) sample: usize,
    pub(crate) frame_relative_us: i64,
    /// Each valid quaternion and its time in the frame, in frames.
    pub(crate) readings: Vec<(f64, Quaternion<f64>)>,
}

impl HeldQuaternions {
    /// The quaternions, timed from the first frame's time in the video,
    /// `first_frame_ms`: a frame lasts one of `sensor_fps` on the camera's
    /// clock, and plays for one of `video_fps`.
    pub(crate) fn timed(self, first_frame_ms: f64, sensor_fps: f64, video_fps: f64) -> Vec<TimeQuaternion<f64>> {
        let vsync_duration = 1000.0 / sensor_fps.max(1.0);
        let fps_ratio = video_fps / sensor_fps;
        let frame_timestamp = (self.frame_relative_us as f64) / 1000.0;
        self.readings.into_iter().map(|(in_frame, v)| TimeQuaternion { t: first_frame_ms + (frame_timestamp + in_frame * vsync_duration) / fps_ratio, v }).collect()
    }
}

impl Frame {
    /// Whether this frame is the one after `previous` in the video: the next
    /// sample of the track, presented one video frame of `frame_ms` later, to
    /// within half a frame. A sample that the track didn't deliver, being
    /// empty or damaged, is between the two frames, and so is a frame that the
    /// track has no sample for: the FC8183 leaves one out now and then, up to
    /// one frame in ten, and the sample before it lasts two frames.
    fn follows(&self, previous: &Frame, frame_ms: f64) -> bool {
        let step = self.ms - previous.ms;
        self.sample == previous.sample + 1 && 2.0 * step > frame_ms && 2.0 * step < 3.0 * frame_ms
    }
}

/// Whether `step` is one `typical` step, to within half of it: a longer one
/// spans a dropped frame or a gap, a shorter one a repeated or broken stamp.
fn one_frame(step: i64, typical: i64) -> bool {
    2 * step > typical && 2 * step < 3 * typical
}

/// The sensor frame rate a clip's frames are timed at: the rate of their own
/// microsecond stamps, the camera's frame clock ([`stamp_fit`]), where the
/// video's frames last `frame_ms`. `None` without two consecutive frames or a
/// stated rate.
///
/// The rate the clip states is taken only where the stamps can't tell it from
/// theirs, being within the fit's standard error of it, or can't be fitted.
/// Elsewhere the stamps show it wrong. A stated rate is the camera's own
/// measurement, within 0.04 ppm of its stamps on most clips and 0.65 ppm on
/// the worst long one, or its nominal rate: the Action 2 states exactly 50 fps
/// where its stamps run at 49.99854 fps, 0.42 frame off over 4.8 minutes.
pub(crate) fn sensor_rate(frames: &[Frame], frame_ms: f64, stated: Option<f64>) -> Option<f64> {
    let Some(fit) = stamp_fit(frames, frame_ms) else { return stated };
    let consistent = stated.filter(|fps| (1e6 / fps - fit.step).abs() <= fit.standard_error);
    log::debug!("Sensor frame rate stated {stated:?}, the frames' timestamps fit {:.6} fps (standard error {:.4} ppm): timed at {}",
        1e6 / fit.step, fit.standard_error / fit.step * 1e6, if consistent.is_some() { "the stated rate, which they can't tell from theirs" } else { "theirs" });
    Some(consistent.unwrap_or(1e6 / fit.step))
}

/// The run fit of the frames' microsecond stamps, in clip order.
struct StampFit {
    /// Microseconds per frame.
    step: f64,
    /// The standard error of `step`, from the stamps' scatter about the lines
    /// of their runs, taken as independent.
    standard_error: f64,
}

/// The stamps are split into runs of consecutive frames wherever a frame isn't
/// the one after the frame before it ([`Frame::follows`]), or its step from it
/// isn't one typical (median) step of such frames: at a frame left out, a gap
/// in the track or a dropped frame, and on both sides of a stamp more than half
/// a frame off. The step is the slope of the least-squares fit of stamp against
/// frame number that all the runs share, each run with its own intercept, so
/// neither a gap nor a broken stamp can pull it. `None` without two
/// consecutive frames.
///
/// Which frames follow each other comes from the track, not from the stamps:
/// with every other sample of a 50 fps clip lost, every step is 40 ms, and
/// taken as the typical step it would give 25 fps.
///
/// The median step alone is off: the stamps are whole microseconds and their
/// steps uneven. On the 360 clips that state their rate it is 1 to 476 ppm
/// off it, and this fit within 0.7 ppm, or 1.4 ppm on a clip of 258 frames.
/// Nor do the frames' sequence numbers count them: an O3 Air Unit clip skips
/// two of them on frames whose stamps are one frame apart.
fn stamp_fit(frames: &[Frame], frame_ms: f64) -> Option<StampFit> {
    // The step from the frame before frame `k`, if it's the one after it
    let step = |k: usize| frames[k].follows(&frames[k - 1], frame_ms).then(|| frames[k].us - frames[k - 1].us);
    let mut steps: Vec<i64> = (1..frames.len()).filter_map(step).filter(|s| *s > 0).collect();
    if steps.is_empty() {
        return None;
    }
    let mid = steps.len() / 2;
    let typical = *steps.select_nth_unstable(mid).1;

    // Each run's sums are centred on its own mean, which keeps them in the
    // precision of the steps.
    let (mut sxx, mut sxy) = (0.0, 0.0);
    let mut runs = Vec::new();
    let mut start = 0;
    for end in 1..=frames.len() {
        if end < frames.len() && step(end).is_some_and(|s| one_frame(s, typical)) {
            continue;
        }
        let run = &frames[start..end];
        let n = run.len() as f64;
        let mean_x = (n - 1.0) / 2.0;
        let mean_y = run.iter().map(|t| (t.us - run[0].us) as f64).sum::<f64>() / n;
        for (k, t) in run.iter().enumerate() {
            let x = k as f64 - mean_x;
            sxx += x * x;
            sxy += x * ((t.us - run[0].us) as f64 - mean_y);
        }
        runs.push((start..end, mean_x, mean_y));
        start = end;
    }
    let us_per_frame = sxy / sxx;
    if !(sxx > 0.0 && us_per_frame > 0.0) {
        return None;
    }
    // One intercept per run and the shared slope are fitted.
    let freedom = frames.len().saturating_sub(runs.len() + 1);
    let squares: f64 = runs.iter().map(|(range, mean_x, mean_y)| {
        let run = &frames[range.clone()];
        run.iter().enumerate().map(|(k, t)| ((t.us - run[0].us) as f64 - mean_y - us_per_frame * (k as f64 - mean_x)).powi(2)).sum::<f64>()
    }).sum();
    let standard_error = if freedom > 0 { (squares / freedom as f64 / sxx).sqrt() } else { f64::INFINITY };
    Some(StampFit { step: us_per_frame, standard_error })
}

/// The duration of a video's frames, in ticks of its timescale, from its
/// time-to-sample table: `(sample count, sample delta)` for each run of
/// samples. It is the run fit of the frames' decode times, as [`stamp_fit`]
/// fits the stamps, computed exactly: the sums are of whole ticks. The last
/// frame lasts to the end of the track, not to a next frame, so only a track
/// of one frame is timed by it. `None` without a frame that lasts, or for a
/// table too long for the sums to be exact: 2^32 frames or 2^53 ticks.
///
/// Where the timescale divides the frame duration, every frame lasts that
/// duration, but for a frame before a dropped one, which lasts two or more,
/// and the fit is the duration exactly: 1001 ticks of 60000 at 59.94 fps.
/// Where it doesn't, each frame's time is rounded to a tick, and the frames
/// last the whole ticks either side of the duration: 16 and 17 ms at 59.94
/// fps on a 1000 Hz timescale, or 1501 and 1502 ticks of 90000, the timescale
/// of DJI's own metadata tracks. The typical (median) duration is then 1.9 %
/// or 333 ppm off, and the fit is within a tick over the run of frames.
pub(super) fn frame_duration(table: &[(u32, u32)]) -> Option<f64> {
    let frames: u64 = table.iter().map(|&(count, _)| u64::from(count)).sum();
    let ticks: u128 = table.iter().map(|&(count, delta)| u128::from(count) * u128::from(delta)).sum();
    if frames > u64::from(u32::MAX) || ticks >= 1 << 53 {
        return None;
    }
    if frames == 1 {
        return table.iter().find(|&&(count, delta)| count == 1 && delta > 0).map(|&(_, delta)| f64::from(delta));
    }
    // The steps from each frame to the next: the durations of every frame but the last
    let mut steps = table.to_vec();
    if let Some(last) = steps.iter_mut().rev().find(|(count, _)| *count > 0) {
        last.0 -= 1;
    }
    let mut durations: Vec<(u32, u64)> = steps.iter().filter(|&&(count, delta)| count > 0 && delta > 0).map(|&(count, delta)| (delta, u64::from(count))).collect();
    durations.sort_unstable();
    let total: u64 = durations.iter().map(|d| d.1).sum();
    let mut counted = 0;
    let typical = durations.iter().find(|d| { counted += d.1; 2 * counted > total })?.0;

    // Twice the sum of the products and twelve times the sum of the squares
    // about each run's means, summed over the runs, in whole ticks: the shared
    // slope is `6 * sxy2 / sxx12`. With fewer than 2^32 frames in 2^53 ticks,
    // no term reaches 2^121.
    let (mut sxy2, mut sxx12) = (0i128, 0i128);
    // The run's last frame, its time from the run's first frame, and the sums
    // of the frames' times and of frame × time
    let (mut k, mut y, mut sy, mut sky) = (0i128, 0i128, 0i128, 0i128);
    for step in steps.iter().filter(|(count, _)| *count > 0).map(Some).chain([None]) {
        if let Some(&(count, delta)) = step && one_frame(i64::from(delta), i64::from(typical)) {
            // Frames k + 1 to k + c, at y + d to y + c × d
            let (c, d) = (i128::from(count), i128::from(delta));
            let (sum_j, sum_jj) = (c * (c + 1) / 2, c * (c + 1) * (2 * c + 1) / 6);
            sy += c * y + d * sum_j;
            sky += c * k * y + (k * d + y) * sum_j + d * sum_jj;
            k += c;
            y += c * d;
        } else {
            // The run of frames 0 to k ends. A run of one frame adds nothing
            let n = k + 1;
            sxy2 += 2 * sky - k * sy;
            sxx12 += n * (n * n - 1);
            (k, y, sy, sky) = (0, 0, 0, 0);
        }
    }
    let (num, den) = (6 * sxy2, sxx12);
    if num <= 0 || den <= 0 {
        return None;
    }
    // In lowest terms, so that a whole number of ticks is exact
    let (mut a, mut b) = (num, den);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    Some((num / a) as f64 / (den / a) as f64)
}

/// Whole sensor readout from DJI's line period in nanoseconds.
pub(super) fn line_readout_ms(line_ns: u32, sensor_rows: u32) -> Option<f64> {
    (line_ns > 0 && sensor_rows > 0).then(|| f64::from(line_ns) * f64::from(sensor_rows) / 1_000_000.0)
}
