// SPDX-License-Identifier: MIT OR Apache-2.0

use std::fmt::{Debug};

pub const FIX_MULT : i32 = 27;

#[derive(Copy, Clone, Debug, Default)]
pub struct State {
    pub v: [i32; 3], // decoder's angular velocity
}

impl State {
    pub fn new() -> State {
        Self::default()
    }

    pub fn dequant_one(&mut self, data: &[i8], qp: u8) -> Option<[i32; 3]> {
        let upd = [data[0] as i8, data[1] as i8, data[2] as i8];
        self.v = [
            self.v[0] + ((upd[0] as i32) << qp),
            self.v[1] + ((upd[1] as i32) << qp),
            self.v[2] + ((upd[2] as i32) << qp),
        ];

        if !is_saturated(upd, 127) {
            return Some(self.v);
        }
        return None;
    }
}

fn is_saturated(v: [i8; 3], lim: i8) -> bool {
    v[0].abs() == lim || v[1].abs() == lim || v[2].abs() == lim
}

pub struct DecompressResult {
    pub new_state: State,
    pub bytes_eaten: usize,
    pub quats_put: usize,
}

const VAR_TABLE: [f64; 16] = [
    0.015625, 0.03125, 0.0625, 0.125, 0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 128.0,
    256.0, 512.0,
];
const SCALE: i32 = 15;

pub fn decompress_block(
    state: &State,
    data: &[u8],
    quats: &mut [[i32;3]],
) -> Option<DecompressResult> {
    let qp = data[0];
    let i_var = (data[1] & 0x1f) as usize;
    let cksum = data[1] >> 5;

    let mdl = LaplaceCdf::new(VAR_TABLE[i_var], SCALE);

    let mut rstate = ((data[2] as u32) << 0)
        | ((data[3] as u32) << 8)
        | ((data[4] as u32) << 16)
        | ((data[5] as u32) << 24);
    let mut bytes_eaten = 6;

    let mut quats_put = 0;
    let mut new_state = state.clone();
    let mut own_cksum = 0;
    let mask = (1 << mdl.scale()) - 1;
    while quats_put < quats.len() {
        let mut s = [0, 0, 0];
        for i in 0..3 {
            let cum = rstate & mask;
            let sym = mdl.icdf(cum);
            if sym == -128 {
                return None;
            }
            s[i] = sym as i8;
            own_cksum = (s[i] as u8).wrapping_add(own_cksum);

            let start = mdl.cdf(sym);
            let freq = mdl.cdf(sym + 1) - start;

            rstate = freq * (rstate >> mdl.scale()) + (rstate & mask) - start;

            while rstate < RANS_BYTE_L {
                if bytes_eaten >= data.len() {
                    dbg!(bytes_eaten);
                    return None;
                }
                rstate = (rstate << 8) | data[bytes_eaten] as u32;
                bytes_eaten += 1;
            }
        }

        if let Some(q) = new_state.dequant_one(&s, qp) {
            if quats_put >= quats.len() {
                dbg!(quats_put);
                return None;
            }
            quats[quats_put] = q;
            quats_put += 1;
        }
    }
    // dbg!(own_cksum & 0x07, cksum, bytes_eaten, quats_put);
    if (own_cksum & 0x07) != cksum {
        return None;
    }
    Some(DecompressResult {
        new_state,
        bytes_eaten,
        quats_put,
    })
}

const RANS_BYTE_L: u32 = 1 << 23;

pub trait Cdf {
    fn cdf(&self, x: i32) -> u32;
    fn icdf(&self, y: u32) -> i32;
    fn scale(&self) -> i32;
}

#[derive(Copy, Clone)]
pub struct LaplaceCdf {
    b: f64,
    scale: i32,
}

impl LaplaceCdf {
    pub fn new(var: f64, scale: i32) -> LaplaceCdf {
        LaplaceCdf {
            b: (var / 2.0).sqrt(),
            scale,
        }
    }
}

impl Cdf for LaplaceCdf {
    fn cdf(&self, x: i32) -> u32 {
        if x <= -128 {
            return 0;
        }
        if x > 128 {
            return 1 << self.scale;
        }

        let xs = x as f64 - 0.5;
        let cum = if xs < 0.0 {
            (xs / self.b).exp() / 2.0
        } else {
            1.0 - (-xs / self.b).exp() / 2.0
        };

        (cum * (((1 << self.scale) as f64) - 257.0)) as u32 + (x + 128) as u32
    }

    fn icdf(&self, y: u32) -> i32 {
        let mut l = -129;
        let mut r = 129;
        while l + 1 != r {
            let mid = (l + r) / 2;
            if self.cdf(mid) <= y && self.cdf(mid + 1) > y {
                return mid;
            }
            if self.cdf(mid) <= y {
                l = mid;
            } else {
                r = mid;
            }
        }
        r
    }

    fn scale(&self) -> i32 {
        self.scale
    }
}
