// SPDX-License-Identifier: MIT OR Apache-2.0

use std::fmt::{Debug, Display};
use std::ops::{Add, Div, Mul, Neg, Sub};

#[derive(Copy, Clone, PartialOrd, PartialEq, Ord, Eq)]
pub struct Fix32<const N: usize> {
    v: i32,
}

impl<const N: usize> Fix32<N> {
    pub const MULT: i32 = 1i32 << N;

    pub const E: Fix32<N> = Self::from_fixed(6267931151224907085i64, 61);
    pub const PI: Fix32<N> = Self::from_fixed(7244019458077122842i64, 61);
    pub const HALF_PI: Fix32<N> = Self::from_fixed(7244019458077122842i64, 62);
    pub const TWO_PI: Fix32<N> = Self::from_fixed(7244019458077122842i64, 60);

    const fn from_fixed(v: i64, s: usize) -> Fix32<N> {
        if s > N {
            Fix32 {
                v: (v / (1 << (s - N)) + (v / (1 << (s - N - 1)) % 2)) as i32,
            }
        } else {
            Fix32 {
                v: (v * (1 << (N - s))) as i32,
            }
        }
    }

    pub fn from_raw(x: i32) -> Fix32<N> {
        Fix32 { v: x }
    }

    pub fn from_i32(x: i32) -> Fix32<N> {
        Fix32 { v: x * Self::MULT }
    }

    pub fn to_raw(&self) -> i32 {
        self.v
    }

    pub fn from_float(x: f32) -> Fix32<N> {
        Fix32 {
            v: (if x >= 0.0 {
                x * (Self::MULT as f32) + 0.5
            } else {
                x * (Self::MULT as f32) - 0.5
            }) as i32,
        }
    }

    pub fn to_float(&self) -> f32 {
        (self.v as f64 / Self::MULT as f64) as f32
    }

    pub fn atan2(&self, x: Fix32<N>) -> Fix32<N> {
        fn atan_s<const N: usize>(x: Fix32<N>) -> Fix32<N> {
            debug_assert!(x.v >= 0 && x.v <= Fix32::<N>::MULT);
            let fa = Fix32::from_fixed(716203666280654660i64, 63);
            let fb = Fix32::from_fixed(-2651115102768076601i64, 63);
            let fc = Fix32::from_fixed(9178930894564541004i64, 63);

            let xx = x * x;
            return ((fa * xx + fb) * xx + fc) * x;
        }
        fn atan_div<const N: usize>(y: Fix32<N>, x: Fix32<N>) -> Fix32<N> {
            debug_assert!(x.v != 0);
            if y.v < 0 {
                if x.v < 0 {
                    atan_div(-y, -x)
                } else {
                    atan_div(-y, x)
                }
            } else if x.v < 0 {
                -atan_div(y, -x)
            } else {
                debug_assert!(y.v >= 0);
                debug_assert!(x.v > 0);
                if y.v > x.v {
                    Fix32::<N>::HALF_PI - atan_s(x / y)
                } else {
                    atan_s(y / x)
                }
            }
        }
        if x.v == 0 {
            debug_assert!(self.v != 0);
            if self.v > 0 {
                Self::HALF_PI
            } else {
                -Self::HALF_PI
            }
        } else {
            let ret = atan_div(*self, x);
            if x.v < 0 {
                if self.v >= 0 {
                    ret + Self::PI
                } else {
                    ret - Self::PI
                }
            } else {
                ret
            }
        }
    }

    pub fn sin(&self) -> Fix32<N> {
        let x = self.fmod(Self::TWO_PI);
        let x = x / Self::HALF_PI;
        let x = if x.v < 0 { x + Self::from_i32(4) } else { x };
        let (x, sign) = if x > Self::from_i32(2) {
            (x - Self::from_i32(2), -1)
        } else {
            (x, 1)
        };
        let x = if x > Self::from_i32(1) {
            Self::from_i32(2) - x
        } else {
            x
        };
        let x2 = x * x;
        Self::from_i32(sign)
            * x
            * (Self::PI
                - x2 * (Self::TWO_PI - Self::from_i32(5) - x2 * (Self::PI - Self::from_i32(3))))
            / Self::from_i32(2)
    }

    pub fn cos(&self) -> Fix32<N> {
        (Self::HALF_PI + *self).sin()
    }

    pub fn sqrt(&self) -> Fix32<N> {
        debug_assert!(self.v >= 0);
        if self.v == 0 {
            return *self;
        }

        let mut num: i64 = (self.v as i64) << N;
        let mut res: i64 = 0;
        let mut bit = 1 << ((find_highest_bit(self.v) + (N as u32)) / 2 * 2);

        while bit != 0 {
            let val = res + bit;
            res >>= 1;
            if num >= val {
                num -= val;
                res += bit;
            }
            bit >>= 2;
        }

        if num > res {
            res += 1;
        }

        fn find_highest_bit(x: i32) -> u32 {
            let mut x = x;
            let mut r = 0;
            loop {
                x >>= 1;
                if x == 0 {
                    break;
                }
                r += 1;
            }
            r
        }
        Fix32 { v: res as i32 }
    }

    pub fn fmod(&self, m: Fix32<N>) -> Fix32<N> {
        Fix32 { v: self.v % m.v }
    }
}

impl<const N: usize> Add for Fix32<N> {
    type Output = Fix32<N>;

    fn add(self, rhs: Self) -> Self::Output {
        Fix32 { v: self.v + rhs.v }
    }
}

impl<const N: usize> Sub for Fix32<N> {
    type Output = Fix32<N>;

    fn sub(self, rhs: Self) -> Self::Output {
        Fix32 { v: self.v - rhs.v }
    }
}

impl<const N: usize> Mul for Fix32<N> {
    type Output = Fix32<N>;

    fn mul(self, rhs: Self) -> Self::Output {
        let val = (self.v as i64) * (rhs.v as i64) / (Self::MULT as i64 / 2);
        Fix32 {
            v: ((val / 2) + (val % 2)) as i32,
        }
    }
}

impl<const N: usize> Div for Fix32<N> {
    type Output = Fix32<N>;

    fn div(self, rhs: Self) -> Self::Output {
        let val = (self.v as i64) * (Self::MULT as i64) * 2 / (rhs.v as i64);
        Fix32 {
            v: ((val / 2) + (val % 2)) as i32,
        }
    }
}

impl<const N: usize> Neg for Fix32<N> {
    type Output = Fix32<N>;

    fn neg(self) -> Self::Output {
        Fix32 { v: -self.v }
    }
}

impl<const N: usize> Debug for Fix32<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fix32")
            .field("v", &self.to_float())
            .finish()
    }
}

impl<const N: usize> Display for Fix32<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(format!("{}", self.to_float()).as_str())?;
        Ok(())
    }
}

pub type Fix = Fix32<27>;
pub type RVec = Vec3<27>;

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Quat {
    pub w: Fix,
    pub x: Fix,
    pub y: Fix,
    pub z: Fix,
}

impl Default for Quat {
    fn default() -> Self {
        Self {
            w: Fix::from_i32(1),
            x: Fix::from_i32(0),
            y: Fix::from_i32(0),
            z: Fix::from_i32(0),
        }
    }
}

impl Quat {
    pub fn new(w: Fix, x: Fix, y: Fix, z: Fix) -> Quat {
        Quat { w, x, y, z }
    }

    pub fn from_rvec(v: &RVec) -> Quat {
        let theta2 = v.x * v.x + v.y * v.y + v.z * v.z;
        if theta2.to_raw() > 16 {
            let theta = theta2.sqrt();
            let half_theta = theta * Fix::from_float(0.5);
            let k = half_theta.sin() / theta;
            Quat {
                w: half_theta.cos(),
                x: v.x * k,
                y: v.y * k,
                z: v.z * k,
            }
        } else {
            let k = Fix::from_float(0.5);
            Quat {
                w: Fix::from_i32(1),
                x: v.x * k,
                y: v.y * k,
                z: v.z * k,
            }
        }
    }

    pub fn to_rvec(&self) -> RVec {
        let sin_theta2 = self.x * self.x + self.y * self.y + self.z * self.z;
        if sin_theta2.to_raw() <= 0 {
            return RVec {
                x: self.x * Fix::from_i32(2),
                y: self.y * Fix::from_i32(2),
                z: self.z * Fix::from_i32(2),
            };
        }

        let sin_theta = sin_theta2.sqrt();
        let cos_theta = self.w;
        let two_theta = Fix::from_i32(2)
            * if cos_theta.to_raw() < 0 {
                (-sin_theta).atan2(-cos_theta)
            } else {
                sin_theta.atan2(cos_theta)
            };
        let k = two_theta / sin_theta;
        RVec {
            x: self.x * k,
            y: self.y * k,
            z: self.z * k,
        }
    }

    pub fn conj(&self) -> Quat {
        Quat {
            w: self.w,
            x: -self.x,
            y: -self.y,
            z: -self.z,
        }
    }

    pub fn norm(&self) -> Fix {
        (self.w * self.w + self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    pub fn normalize(&mut self) -> Quat {
        let norm = self.norm();
        self.sdiv(norm)
    }

    pub fn normalize_safe(&mut self) -> Quat {
        let norm = self.norm();
        if norm.to_raw() == 0 {
            return Quat {
                w: Fix::from_raw(0),
                x: Fix::from_raw(0),
                y: Fix::from_raw(0),
                z: Fix::from_raw(0),
            };
        }
        self.sdiv(norm)
    }

    pub fn rotate_point(&self, p: &RVec) -> RVec {
        let qq = (*self)
            * Quat {
                w: Fix::from_raw(0),
                x: p.x,
                y: p.y,
                z: p.z,
            }
            * self.conj();
        RVec {
            x: qq.x,
            y: qq.y,
            z: qq.z,
        }
    }

    pub fn smul(&self, x: Fix) -> Quat {
        Quat {
            w: self.w * x,
            x: self.x * x,
            y: self.y * x,
            z: self.z * x,
        }
    }

    pub fn sdiv(&self, x: Fix) -> Quat {
        Quat {
            w: self.w / x,
            x: self.x / x,
            y: self.y / x,
            z: self.z / x,
        }
    }
}

impl Add for &Quat {
    type Output = Quat;

    fn add(self, rhs: Self) -> Self::Output {
        Quat {
            w: self.w + rhs.w,
            x: self.x + rhs.x,
            y: self.y + rhs.y,
            z: self.z + rhs.z,
        }
    }
}

impl Mul for &Quat {
    type Output = Quat;

    fn mul(self, rhs: Self) -> Self::Output {
        Quat {
            w: self.w * rhs.w - self.x * rhs.x - self.y * rhs.y - self.z * rhs.z,
            x: self.w * rhs.x + self.x * rhs.w + self.y * rhs.z - self.z * rhs.y,
            y: self.w * rhs.y - self.x * rhs.z + self.y * rhs.w + self.z * rhs.x,
            z: self.w * rhs.z + self.x * rhs.y - self.y * rhs.x + self.z * rhs.w,
        }
    }
}

impl Add for Quat {
    type Output = Quat;

    fn add(self, rhs: Self) -> Self::Output {
        &self + &rhs
    }
}

impl Mul for Quat {
    type Output = Quat;

    fn mul(self, rhs: Self) -> Self::Output {
        &self * &rhs
    }
}

#[derive(Copy, Clone, Debug)]
pub struct Vec3<const N: usize> {
    pub x: Fix32<N>,
    pub y: Fix32<N>,
    pub z: Fix32<N>,
}

impl<const N: usize> Vec3<N> {
    pub fn new(x: Fix32<N>, y: Fix32<N>, z: Fix32<N>) -> Vec3<N> {
        Vec3 { x, y, z }
    }

    pub fn smul(&self, k: Fix32<N>) -> Vec3<N> {
        Vec3 {
            x: self.x * k,
            y: self.y * k,
            z: self.z * k,
        }
    }

    pub fn sdiv(&self, k: Fix32<N>) -> Vec3<N> {
        Vec3 {
            x: self.x / k,
            y: self.y / k,
            z: self.z / k,
        }
    }

    pub fn norm(&self) -> Fix32<N> {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    pub fn normalized(&self) -> Vec3<N> {
        self.sdiv(self.norm())
    }
}

impl<const N: usize> Add for Vec3<N> {
    type Output = Vec3<N>;

    fn add(self, rhs: Self) -> Self::Output {
        Vec3 {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
            z: self.z + rhs.z,
        }
    }
}

impl<const N: usize> Sub for Vec3<N> {
    type Output = Vec3<N>;

    fn sub(self, rhs: Self) -> Self::Output {
        Vec3 {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
            z: self.z - rhs.z,
        }
    }
}

impl<const N: usize> Neg for Vec3<N> {
    type Output = Vec3<N>;

    fn neg(self) -> Self::Output {
        Vec3 {
            x: -self.x,
            y: -self.y,
            z: -self.z,
        }
    }
}

impl<const N: usize> Default for Vec3<N> {
    fn default() -> Self {
        Self {
            x: Fix32::<N>::from_i32(0),
            y: Fix32::<N>::from_i32(0),
            z: Fix32::<N>::from_i32(0),
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct State {
    pub q: Quat, // decoder's quat
    pub v: RVec, // decoder's angular velocity
}

#[derive(Copy, Clone, Debug)]
pub struct DequantResult {
    pub new_state: State,
    pub quats_put: usize,
}

impl State {
    pub fn new() -> State {
        State {
            q: Quat::default(),
            v: RVec::default(),
        }
    }

    pub fn dequant_one(&mut self, data: &[i8], qp: u8) -> Option<Quat> {
        let upd = [data[0] as i8, data[1] as i8, data[2] as i8];
        self.v = self.v + dequant_update(upd, qp);

        if !is_saturated(upd, 127) {
            self.q = (self.q * Quat::from_rvec(&self.v)).normalize_safe();
            return Some(self.q)
        }
        return None
    }
}

fn dequant_update(update: [i8; 3], scale: u8) -> RVec {
    RVec::new(
        Fix::from_raw((update[0] as i32) << scale),
        Fix::from_raw((update[1] as i32) << scale),
        Fix::from_raw((update[2] as i32) << scale),
    )
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
    quats: &mut [Quat],
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
                return None
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
