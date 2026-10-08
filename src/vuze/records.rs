// SPDX-License-Identifier: MIT OR Apache-2.0
//! XR bmdt: lengths include the type/camera bytes, but exclude the u16 length.
use std::sync::atomic::{AtomicBool, Ordering};
use crate::tags_impl::{TimeScalar, TimeVector3};

#[derive(Default)]
pub(super) struct Records {
    pub readout_ms: Option<f64>,
    pub gyro: Vec<TimeVector3<f64>>,
    pub accl: Vec<TimeVector3<f64>>,
    pub exposure: Vec<TimeScalar<f64>>,
}

pub(super) fn parse(buf: &[u8], cancel: &AtomicBool, progress: &impl Fn(f64)) -> Records {
    let mut out = Records::default();
    if buf.len() < 14 || u16::from_le_bytes([buf[0], buf[1]]) != 12 { return out; }
    // The header's last u16 is microseconds across the sensor. Studio uses
    // this field both for row correction and exposure-centre timing.
    let readout = u16::from_le_bytes([buf[12], buf[13]]);
    out.readout_ms = (readout > 0).then_some(readout as f64 / 1000.0);
    let mut offset = 14;
    while offset + 4 <= buf.len() && !cancel.load(Ordering::Relaxed) {
        progress(offset as f64 / buf.len() as f64);
        let len = u16::from_le_bytes([buf[offset], buf[offset+1]]) as usize + 2;
        if len < 4 { break; }
        let Some(record) = buf.get(offset..offset+len) else { break; };
        offset += len;
        if record.len() < 12 { continue; }
        let time = u64::from_le_bytes(record[4..12].try_into().unwrap()) as f64 * 1e-6;
        let f32_at = |i| f32::from_le_bytes(record[i..i+4].try_into().unwrap()) as f64;
        match (record[2], len) {
            // Type 1 has this SAME length, but holds three f64 GPS values.
            (0, 36) => {
                let a = [f32_at(12), f32_at(16), f32_at(20)];
                let g = [f32_at(24), f32_at(28), f32_at(32)];
                if a.iter().chain(&g).all(|v| v.is_finite()) {
                    out.accl.push(TimeVector3 { t: time, x: a[0], y: a[1], z: a[2] });
                    out.gyro.push(TimeVector3 { t: time, x: g[0], y: g[1], z: g[2] });
                }
            }
            (2, 34) if record[3] == 0 => {
                let duration = f32_at(12);
                if duration.is_finite() && duration >= 0.0 {
                    out.exposure.push(TimeScalar { t: time, v: duration });
                }
            }
            _ => {}, // GPS, temperature, other sensor IDs and future types.
        }
    }
    out
}
