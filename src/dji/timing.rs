// SPDX-License-Identifier: MIT OR Apache-2.0

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

/// The capture frame rate from the frames' own microsecond stamps: the median
/// step, which a dropped frame does not move. For a clip that does not state
/// its sensor frame rate. `None` with fewer than two frames.
pub(super) fn capture_fps(frame_relative_us: impl IntoIterator<Item = i64>) -> Option<f64> {
    let stamps: Vec<i64> = frame_relative_us.into_iter().collect();
    let mut steps: Vec<i64> = stamps.windows(2).map(|w| w[1] - w[0]).filter(|s| *s > 0).collect();
    if steps.is_empty() {
        return None;
    }
    let mid = steps.len() / 2;
    Some(1e6 / *steps.select_nth_unstable(mid).1 as f64)
}

/// Whole sensor readout from DJI's line period in nanoseconds.
pub(super) fn line_readout_ms(line_ns: u32, sensor_rows: u32) -> Option<f64> {
    (line_ns > 0 && sensor_rows > 0).then(|| f64::from(line_ns) * f64::from(sensor_rows) / 1_000_000.0)
}
