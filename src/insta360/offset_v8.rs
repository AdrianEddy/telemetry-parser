// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2026 Adrian <adrian.eddy at gmail>

//! `offset_v8`: the calibration string of a body whose lens focuses.
//!
//! It is the `offset_v6` model — the unified sphere with the 13-term distortion block — with every quantity the focus
//! moves stated as a polynomial in the autofocus motor position (the *AF code*) instead of a number: the focal
//! lengths, the principal point, the five radial terms and the four decentering terms. The mirror parameter, the
//! orientation, the translation and the four thin-prism terms are numbers. A polynomial is evaluated at the raw code,
//! `c₀ + c₁·code + c₂·code² + …`, and the result is an ordinary `offset_v6` calibration of the lens at that focus.
//!
//! The codes a clip can use are `ExtraMetadata.af_lens_info`, from `range_far` to `range_near`. `range_far` is the
//! smaller of the two in every clip seen, but the pair is taken as it stands even when it isn't: the far end is the
//! one an attachment's focus scale starts at. Each frame's code is in the exposure record; a frame without one, or
//! with one outside the range, is the lens at `range_far`.
//!
//! A clip shot through one of the body's lens attachments states which in `ExtraMetadata.lens_accessory_type`, and
//! its string still describes the bare lens. [`Accessory::calibration`] is the attachment's own calibration in the
//! same form, and [`OffsetV8::with_accessory`] the lens the picture was taken through: the attachment's focal, mirror
//! parameter and radial terms, the body's principal point, decentering terms and pose. [`Optics::of`] says which
//! attachments leave the body's lens as it is (filters, a flat housing), which are one of the two calibrated lenses,
//! and which change the lens in a way no calibration here describes.

use super::rig::{Offset, OffsetLens, Version};

/// One lens of an `offset_v8` string. The vectors are polynomial coefficients in the AF code, lowest power first.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LensV8 {
    /// The unified sphere's mirror parameter.
    pub xi: f64,
    /// `fx`, `fy`.
    pub focal: [Vec<f64>; 2],
    /// `cx`, `cy`.
    pub centre: [Vec<f64>; 2],
    /// `yaw, pitch, roll`, degrees.
    pub ypr: [f64; 3],
    /// Camera origin, metres.
    pub translation: [f64; 3],
    /// `d0..d4` of the 13-term block: the radial terms of `r², r⁴ … r¹⁰`.
    pub radial: [Vec<f64>; 5],
    /// `d5..d8`: the decentering terms, the second pair scaled by `r²`.
    pub decentering: [Vec<f64>; 4],
    /// `d9..d12`: the thin-prism terms.
    pub prism: [f64; 4],
    /// The calibration frame, all lenses side by side.
    pub width: f64,
    pub height: f64,
    pub lens_type: i32,
}

/// A parsed `offset_v8` string.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct OffsetV8 {
    /// Bits 8..15 of the packed tail, as in every other generation of the string.
    pub x: u8,
    pub lenses: Vec<LensV8>,
}

/// The generation the packed tail states for an `offset_v8` string, `(8 << 16) | (X << 8)`.
const VERSION: i64 = 8;

/// `c₀ + c₁·x + c₂·x² + …`, summed from the constant term up.
pub(crate) fn polynomial(c: &[f64], x: f64) -> f64 {
    c.iter().enumerate().fold(0.0, |sum, (i, c)| sum + c * x.powi(i as i32))
}

/// The coefficients of `p(a·x + b)` as a polynomial in `x`, of degree `degree` (the coefficients `p` is short of are
/// zeros, and the ones past `degree` are dropped, as they are zero in every string).
fn substitute(p: &[f64], degree: usize, a: f64, b: f64) -> Vec<f64> {
    let n = degree + 1;
    let c: Vec<f64> = (0..n).map(|i| p.get(i).copied().unwrap_or(0.0)).collect();
    let a_pow: Vec<f64> = (0..n).scan(1.0, |acc, _| { let v = *acc; *acc *= a; Some(v) }).collect();
    let b_pow: Vec<f64> = (0..n).scan(1.0, |acc, _| { let v = *acc; *acc *= b; Some(v) }).collect();
    let mut binomial = vec![vec![1.0f64; n]; n];
    for i in 1..n {
        for j in 1..i {
            binomial[i][j] = binomial[i - 1][j - 1] + binomial[i - 1][j];
        }
    }
    (0..n).map(|j| (j..n).map(|i| c[i] * binomial[i][j] * a_pow[j] * b_pow[i - j]).sum()).collect()
}

/// `p` held at the constant `v`: the first coefficient replaced, the others zeroed, the degree kept.
fn constant(p: &[f64], v: f64) -> Vec<f64> {
    if p.is_empty() { return vec![v]; }
    let mut out = vec![0.0; p.len()];
    out[0] = v;
    out
}

/// Parse one `offset_v8` string, or `None` when it is not one.
///
/// Per lens: `ξ`, then `n₁` and the `n₁` coefficients of `fx` followed by the `n₁` of `fy`, then `n₂` and the
/// principal point's the same way, `yaw pitch roll`, `tx ty tz`, `n₃` and the five radial terms' `n₃` coefficients
/// each, `n₄` and the four decentering terms' `n₄` each, the four thin-prism terms, the frame's width and height and
/// the lens type. The string opens with the lens count and closes with the packed tail. Every count is at least one,
/// every number finite, the frame not empty, and the string holds nothing more.
pub(crate) fn parse(s: &str) -> Option<OffsetV8> {
    let tokens: Vec<&str> = s.split('_').collect();
    let flags: i64 = tokens.last()?.trim().parse().ok()?;
    if flags < 0 || flags >> 16 != VERSION {
        return None;
    }
    let num: usize = tokens.first()?.trim().parse().ok()?;
    if !(1..=8).contains(&num) {
        return None;
    }
    let mut t = Tokens { tokens: &tokens[..tokens.len() - 1], at: 1 };
    let mut lenses = Vec::with_capacity(num);
    for _ in 0..num {
        let xi = t.number()?;
        let n = t.count()?;
        let fx = t.numbers(n)?;
        let fy = t.numbers(n)?;
        let n = t.count()?;
        let cx = t.numbers(n)?;
        let cy = t.numbers(n)?;
        let ypr = [t.number()?, t.number()?, t.number()?];
        let translation = [t.number()?, t.number()?, t.number()?];
        let n = t.count()?;
        let mut radial: [Vec<f64>; 5] = Default::default();
        for r in radial.iter_mut() {
            *r = t.numbers(n)?;
        }
        let n = t.count()?;
        let mut decentering: [Vec<f64>; 4] = Default::default();
        for d in decentering.iter_mut() {
            *d = t.numbers(n)?;
        }
        let prism = [t.number()?, t.number()?, t.number()?, t.number()?];
        let (width, height) = (t.number()?, t.number()?);
        let lens_type = t.number()?;
        // The calibration's pixels are scaled to the pictures' by the frame: an empty one scales them to nothing
        if lens_type.fract() != 0.0 || !(width > 0.0 && height > 0.0) {
            return None;
        }
        lenses.push(LensV8 {
            xi, focal: [fx, fy], centre: [cx, cy], ypr, translation, radial, decentering, prism, width, height,
            lens_type: lens_type as i32,
        });
    }
    // Everything up to the tail, and nothing else
    if t.at != t.tokens.len() {
        return None;
    }
    Some(OffsetV8 { x: ((flags >> 8) & 0xff) as u8, lenses })
}

/// The tokens of a string between its lens count and its tail, read in order.
struct Tokens<'a> {
    tokens: &'a [&'a str],
    at: usize,
}

impl Tokens<'_> {
    /// The next number. Every number has to be finite: `"nan"` and `"inf"` parse as `f64`.
    fn number(&mut self) -> Option<f64> {
        let v: f64 = self.tokens.get(self.at)?.trim().parse().ok()?;
        self.at += 1;
        v.is_finite().then_some(v)
    }
    fn numbers(&mut self, n: usize) -> Option<Vec<f64>> {
        (0..n).map(|_| self.number()).collect()
    }
    /// A count of terms: a whole number, at least one, which a string written back with every number in decimals
    /// states as `2.000000`.
    fn count(&mut self) -> Option<usize> {
        let v = self.number()?;
        (v.fract() == 0.0 && (1.0..=64.0).contains(&v)).then_some(v as usize)
    }
}

impl OffsetV8 {
    /// The calibration of the lens at AF code `code`: the `offset_v6` string the polynomials state there.
    pub(crate) fn at(&self, code: f64) -> Offset {
        let lenses = self.lenses.iter().map(|l| {
            let mut coeffs = Vec::with_capacity(13);
            coeffs.extend(l.radial.iter().map(|p| polynomial(p, code)));
            coeffs.extend(l.decentering.iter().map(|p| polynomial(p, code)));
            coeffs.extend(l.prism);
            OffsetLens {
                radius: 0.0,
                xi: l.xi,
                focal: [polynomial(&l.focal[0], code), polynomial(&l.focal[1], code)],
                centre: [polynomial(&l.centre[0], code), polynomial(&l.centre[1], code)],
                ypr: l.ypr,
                translation: l.translation,
                coeffs,
                width: l.width,
                height: l.height,
                lens_type: l.lens_type,
            }
        }).collect();
        Offset { version: Version::V6, x: self.x, lenses }
    }

    /// The lens the picture was taken through, with the attachment `accessory` on this body's lens 0, for a clip whose
    /// AF codes run from `far` to `near` (`af_range`).
    ///
    /// The attachment's calibration is stated for a focus position running over `0..1000/300` as the body's AF code
    /// runs from `far` to `near`, so its polynomials are restated in the AF code first; the two are taken as they
    /// stand, a `far` above `near` making the focus position fall as the code rises. A range of one code has the
    /// attachment at its focus position 0 throughout. Its focal is stated for its own calibration frame and is scaled
    /// to the body's. From the body it takes the principal point and the decentering terms (each at its constant term)
    /// and the pose; everything else is the attachment's. `None` only when either string has no lens.
    pub(crate) fn with_accessory(&self, accessory: &OffsetV8, far: i32, near: i32) -> Option<OffsetV8> {
        let body = self.lenses.first()?;
        let mut out = accessory.clone();
        let lens = out.lenses.first_mut()?;

        let span = f64::from(near) - f64::from(far);
        let a = if span == 0.0 { 0.0 } else { 1000.0 / 300.0 / span };
        let b = -a * f64::from(far);

        let focal_degree = lens.focal[0].len().max(lens.focal[1].len()) - 1;
        let centre_degree = lens.centre[0].len().max(lens.centre[1].len()).max(2) - 1;
        let radial_degree = lens.radial.iter().map(Vec::len).max().unwrap_or(1) - 1;

        lens.centre = [constant(&lens.centre[0], body.centre[0][0]), constant(&lens.centre[1], body.centre[1][0])];
        for (d, b) in lens.decentering.iter_mut().zip(&body.decentering) {
            *d = constant(d, b[0]);
        }
        lens.ypr = body.ypr;
        lens.translation = body.translation;

        for f in lens.focal.iter_mut() {
            *f = substitute(f, focal_degree, a, b);
        }
        for c in lens.centre.iter_mut() {
            *c = substitute(c, centre_degree, a, b);
        }
        for r in lens.radial.iter_mut() {
            *r = substitute(r, radial_degree, a, b);
        }

        if lens.width > 0.0 && lens.height > 0.0 && body.width > 0.0 && body.height > 0.0 {
            let (sx, sy) = (body.width / lens.width, body.height / lens.height);
            lens.focal[0].iter_mut().for_each(|c| *c *= sx);
            lens.focal[1].iter_mut().for_each(|c| *c *= sy);
            lens.width = body.width;
            lens.height = body.height;
        }
        Some(out)
    }
}

/// The AF codes a clip can use: `af_lens_info`'s `(range_far, range_near)` as they stand, in either order (see
/// [`OffsetV8::with_accessory`]). `None` when a code is negative, which is no motor position.
pub(crate) fn af_range(range_near: i32, range_far: i32) -> Option<(i32, i32)> {
    (range_far >= 0 && range_near >= 0).then_some((range_far, range_near))
}

/// The code a frame is evaluated at: its own when it is a code of the range (`(far, near)`, either order), else the
/// range's far end. Without a range, its own when it states one (`0xFFFF` is none).
pub(crate) fn frame_code(code: Option<u16>, range: Option<(i32, i32)>) -> Option<i32> {
    let own = code.filter(|&c| c != u16::MAX).map(i32::from);
    match range {
        Some((far, near)) => Some(own.filter(|c| (far.min(near)..=far.max(near)).contains(c)).unwrap_or(far)),
        None => own,
    }
}

/// What a clip's attachment (`ExtraMetadata.lens_accessory_type`) does to the lens the pictures are taken through.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum Optics {
    /// Nothing that moves a ray: none, a filter (black mist, star, ND, UV, soft focus), or a housing with a flat port,
    /// whose effect under water is the refraction a stabilizer takes as a setting of its own. The body's lens.
    #[default]
    Body,
    /// One of the attachments with a calibration of its own, composed with the body's.
    Composed(Accessory),
    /// A lens that changes the image (macro, anamorphic, telephoto, the Miceo adapters) or a type not known here:
    /// no calibration of the clip's lens is available.
    Unknown,
}

impl Optics {
    pub(crate) fn of(lens_accessory_type: i32) -> Self {
        match lens_accessory_type {
            // unknown (unset), standard, black mist, star, ND, ND16/32/64, UV, diving housing, UV + soft focus,
            // UV + ND16/32/64, UV + diving housing
            0 | 1 | 2 | 6 | 7 | 16..=18 | 20 | 21 | 27..=31 => Self::Body,
            // the wide-angle lens, alone or with soft focus, ND16/32/64 or its diving housing (the same glass with a
            // filter or a flat port in front)
            3 | 22..=26 => Self::Composed(Accessory::WideAngle),
            14 => Self::Composed(Accessory::WideAngle180),
            _ => Self::Unknown,
        }
    }
}

/// A lens attachment with a calibration of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Accessory {
    /// `LENS_ACCESSORIES_TYPE_WIDE_ANGLE_LENS` (3): the wide-angle lens, a fisheye whose focal follows the focus.
    WideAngle,
    /// `LENS_ACCESSORIES_TYPE_WIDE_ANGLE_LENS_180` (14): the 180° wide-angle lens.
    WideAngle180,
}

impl Accessory {

    /// The attachment's calibration, an `offset_v8` string of a lens of its own type (202, 203), in a 4096² frame.
    pub(crate) fn calibration(self) -> OffsetV8 {
        let s = match self {
            Self::WideAngle => concat!(
                "1_4_2_7951.002258604_26.347909701_7951.002258604_26.347909701_2_",
                "2048_0_2048_0_0_0_0_0_0_0_2_",
                "5.19476613041858_0.008037301355989_32.401983701830957_-3.644264659255166_",
                "-711.908309911304968_167.328997050470917_14190.427729163764525_-3740.487256556767989_",
                "-118056.512295514825382_29807.432384996784094_",
                "1_0_0_0_0_0_0_0_0_4096_4096_202_525312",
            ),
            Self::WideAngle180 => concat!(
                "1_1.295215_2_2702.99557342589_0_2703.09519959028_0_2_",
                "2048_0_2048_0_0_0_0_0_0_0_2_",
                "-0.0778157852316288_0_-0.362098893704817_0_0.564212257065139_0_-0.264023008134613_0_0_0_",
                "2_-0.000379915990743029_0_0.00048980200663209_0_0.000328660009108717_0_-0.000425127995549701_0_",
                "0_0_0_0_4096_4096_203_525312",
            ),
        };
        parse(s).expect("the attachment calibrations are well-formed")
    }
}
