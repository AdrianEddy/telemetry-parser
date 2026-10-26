// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2026 Adrian <adrian.eddy at gmail>

//! The SIGMA fp's own lens correction, as Gyroflow lens parameters.
//!
//! For a lens with electronic contacts the camera has a correction table: five focus support points, each with four
//! radial coefficients per colour plane. They are what it writes into a DNG's WarpRectilinear opcode, which maps a
//! corrected radius `ρ` (a fraction of the distance to the farthest corner) to the source radius
//! `ρ·(kr0 + kr1·ρ² + kr2·ρ⁴ + kr3·ρ⁶)`; the table interpolated linearly at the frame's focus distance gives the
//! frame's opcode exactly.
//!
//! `kr0` is a magnification, the focus breathing, so it goes on the focal length. What is left is the distortion,
//! fitted to the OpenCV fisheye model `θd = θ + k1·θ³ + k2·θ⁵ + k3·θ⁷` through three radii.

/// Index of the green plane, the one luminance follows
const GREEN: usize = 1;
/// The radii the fisheye model goes through, as fractions of the corner radius
const FIT_RADII: [f64; 3] = [0.55, 0.85, 1.0];

/// The IMX410's active area: 6000 pixels across 35.9 mm
pub const SENSOR_WIDTH_PX: f64 = 6000.0;
pub const SENSOR_WIDTH_MM: f64 = 35.9;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct LensTable {
    /// The focal length the table was measured at, in tenths of a mm
    pub calib_focal_tenths_mm: u32,
    /// The focus support points, as 2²⁴ / object distance in mm (0 = infinity), increasing
    pub axis: [u32; 5],
    /// Per support point, per colour plane, kr0..kr3
    pub nodes: [[[f64; 4]; 3]; 5],
}

impl LensTable {
    /// The green plane's coefficients at a focus distance in mm, `None` meaning infinity. `None` for a table without
    /// data (a lens without contacts leaves it unset).
    pub fn coefficients_at(&self, distance_mm: Option<f64>) -> Option<[f64; 4]> {
        let position = distance_mm.filter(|x| x.is_finite() && *x > 0.0).map_or(0.0, |mm| 16_777_216.0 / mm);
        // The segment the position falls in, the end ones extended
        let segment = (1..4).rev().find(|&i| position >= self.axis[i] as f64).unwrap_or(0);
        let (from, to) = (self.axis[segment] as f64, self.axis[segment + 1] as f64);
        if !(to > from) {
            return None;
        }
        let t = ((position - from) / (to - from)).clamp(0.0, 1.0);
        let (a, b) = (self.nodes[segment][GREEN], self.nodes[segment + 1][GREEN]);
        Some(std::array::from_fn(|i| a[i] + t * (b[i] - a[i]))).filter(|kr| plausible(kr))
    }
}

/// Coefficients of a real correction: a zeroed table shows as `kr0` far from one
pub fn plausible(kr: &[f64; 4]) -> bool {
    kr.iter().all(|x| x.is_finite()) && kr[0] > 0.5 && kr[0] < 1.5
}

/// The focal length in mm to build the camera matrix with: the one the correction was measured at (a SIGMA 40mm F1.4
/// reports 40 and is calibrated at 39.4), unless it is out of range or more than a quarter off the one the lens
/// reports
pub fn focal_mm(calibrated_mm: Option<f64>, lens_mm: Option<f64>) -> Option<f64> {
    let calibrated_mm = calibrated_mm.filter(|x| (1.0..=2000.0).contains(x));
    let lens_mm = lens_mm.filter(|x| x.is_finite() && *x > 0.0);
    match (calibrated_mm, lens_mm) {
        (Some(c), Some(l)) if (c - l).abs() <= l / 4.0 => Some(c),
        (Some(c), None) => Some(c),
        (_, l) => l,
    }
}

/// The focal length in pixels of a picture whose pixels are `sensor_pixels` sensor pixels wide
pub fn focal_px(focal_mm: f64, sensor_pixels: f64) -> f64 {
    focal_mm / (SENSOR_WIDTH_MM / SENSOR_WIDTH_PX) / sensor_pixels
}

/// The OpenCV fisheye coefficients (k4 always zero) of a WarpRectilinear's coefficients, `radius_px` being the radius
/// they're normalized to (`WarpRectilinear::radius`) and `focal_px` the focal length without the breathing, both in
/// the same pixels; and the focal length with the breathing
pub fn fit(kr: &[f64; 4], radius_px: f64, focal_px: f64) -> Option<([f64; 4], f64)> {
    if !plausible(kr) || !(focal_px.is_finite() && focal_px > 0.0) || !(radius_px.is_finite() && radius_px > 0.0) {
        return None;
    }
    // The normalization radius, in focal lengths
    let corner = radius_px / focal_px;
    let mut a = [[0.0; 3]; 3];
    let mut b = [0.0; 3];
    for (row, rho) in FIT_RADII.iter().enumerate() {
        // An undistorted point at radius r = tan θ is at radius r·g in the source, both in focal lengths
        let r = rho * corner;
        let r2 = rho * rho;
        let g = (kr[0] + kr[1] * r2 + kr[2] * r2 * r2 + kr[3] * r2 * r2 * r2) / kr[0];
        let theta = r.atan();
        let t2 = theta * theta;
        a[row] = [theta * t2, theta * t2 * t2, theta * t2 * t2 * t2];
        b[row] = g * r - theta;
    }
    let k = solve3(a, b)?;
    Some(([k[0], k[1], k[2], 0.0], focal_px * kr[0]))
}

/// Gaussian elimination with partial pivoting
fn solve3(mut a: [[f64; 3]; 3], mut b: [f64; 3]) -> Option<[f64; 3]> {
    for col in 0..3 {
        let pivot = (col..3).max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()))?;
        if !(a[pivot][col].abs() > 1e-300) { return None; }
        a.swap(col, pivot);
        b.swap(col, pivot);
        for row in col + 1..3 {
            let f = a[row][col] / a[col][col];
            for k in col..3 { a[row][k] -= f * a[col][k]; }
            b[row] -= f * b[col];
        }
    }
    let mut x = [0.0; 3];
    for row in (0..3).rev() {
        let sum: f64 = (row + 1..3).map(|k| a[row][k] * x[k]).sum();
        x[row] = (b[row] - sum) / a[row][row];
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}

/// A WarpRectilinear opcode, as far as the fit needs it
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct WarpRectilinear {
    /// The green plane's kr0..kr3
    pub kr: [f64; 4],
    /// The optical centre, as fractions of the image's width and height from its top left corner
    pub centre: (f64, f64),
}

impl WarpRectilinear {
    /// The radius the coefficients are normalized to: the distance from the optical centre to the farthest corner of
    /// the image the opcode applies to (`width` x `height` pixels), as the DNG SDK's `dng_filter_warp` computes it
    pub fn radius(&self, width: f64, height: f64) -> f64 {
        (self.centre.0.max(1.0 - self.centre.0) * width).hypot(self.centre.1.max(1.0 - self.centre.1) * height)
    }
}

/// The first WarpRectilinear in a DNG opcode list (big endian, as the DNG spec has it)
pub fn warp_rectilinear(list: &[u8]) -> Option<WarpRectilinear> {
    let u32_at = |at: usize| Some(u32::from_be_bytes(list.get(at..at + 4)?.try_into().ok()?));
    let f64_at = |at: usize| Some(f64::from_be_bytes(list.get(at..at + 8)?.try_into().ok()?));
    let mut at = 4;
    for _ in 0..u32_at(0)?.min(256) {
        // id, DNG version, flags, size of the parameters
        let (id, size) = (u32_at(at)?, u32_at(at + 12)? as usize);
        let parameters = at + 16;
        if id == 1 {
            let planes = u32_at(parameters)? as usize;
            if !(1..=4).contains(&planes) {
                return None;
            }
            // Per plane kr0..kr3, kt0, kt1, then the optical centre
            let plane = parameters + 4 + (if planes >= 3 { GREEN } else { 0 }) * 6 * 8;
            let kr = [f64_at(plane)?, f64_at(plane + 8)?, f64_at(plane + 16)?, f64_at(plane + 24)?];
            let centre_at = parameters + 4 + planes * 6 * 8;
            let centre = (f64_at(centre_at)?, f64_at(centre_at + 8)?);
            if !plausible(&kr) || !(0.0..=1.0).contains(&centre.0) || !(0.0..=1.0).contains(&centre.1) {
                return None;
            }
            return Some(WarpRectilinear { kr, centre });
        }
        at = parameters.checked_add(size)?;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // Green plane of a LUMIX S 40mm F2 as read off a camera, and what the camera's own interpolation and fit give
    // for a 1936x1090 frame at 40 mm (golden values of the fpSup firmware mod)
    const AXIS: [u32; 5] = [0, 18641, 37283, 49637, 55924];
    const GREEN_NODES: [[f64; 4]; 5] = [
        [0.987878098,  0.000230792, -0.001804660,  0.012710940],
        [0.995612771, -0.003803673, -0.003186222,  0.011056404],
        [0.999854447, -0.010596089,  0.003693395,  0.003874420],
        [0.999856513, -0.016680599,  0.013089761, -0.003420052],
        [0.999845119, -0.020184787,  0.018855619, -0.007281209],
    ];
    const GOLDEN: [(f64, [f64; 4], [f64; 3], f64); 6] = [
        (f64::INFINITY, [9.878780980000000e-01, 2.307920000000000e-04, -1.804660000000000e-03, 1.271094000000000e-02], [3.434034356654717e-01, -9.580878466073323e-02, 1.931069370926159e+00], 2130.954872120334),
        (900.0, [9.956128508894739e-01, -3.803800931162369e-03, -3.186092426458057e-03, 1.105626873155336e-02], [3.266573683372537e-01, -9.813155189668302e-02, 1.595721472338275e+00], 2147.639531278018),
        (450.0, [9.998543792456360e-01, -1.059598050141926e-02, 3.693285108515062e-03, 3.874534721340819e-03], [2.966047906818278e-01, 7.396930653660273e-02, 7.564556456382933e-01], 2156.788945091422),
        (364.0, [9.998559200330400e-01, -1.493427112102856e-02, 1.039289043740427e-02, -1.326450409078700e-03], [2.772084741693018e-01, 2.200148386305785e-01, 2.240453137977921e-01], 2156.792268728652),
        (300.0, [9.998451189999999e-01, -2.018478700000000e-02, 1.885561900000000e-02, -7.281209000000000e-03], [2.540190304919543e-01, 3.954371794339360e-01, -3.550155534415396e-01], 2156.768969787187),
        // Nearer than the nearest support point is that point
        (250.0, [9.998451189999999e-01, -2.018478700000000e-02, 1.885561900000000e-02, -7.281209000000000e-03], [2.540190304919543e-01, 3.954371794339360e-01, -3.550155534415396e-01], 2156.768969787187),
    ];

    fn table() -> LensTable {
        LensTable { calib_focal_tenths_mm: 400, axis: AXIS, nodes: GREEN_NODES.map(|kr| [kr, kr, kr]) }
    }

    #[test]
    fn interpolation_and_fit_match_the_camera() {
        let focal = focal_px(40.0, 6000.0 / 1936.0);
        assert!((focal - 2157.1030640668523).abs() < 1e-9);
        for (distance, kr_want, k_want, fx_want) in GOLDEN {
            let kr = table().coefficients_at(Some(distance)).unwrap();
            for (got, want) in kr.iter().zip(kr_want) {
                assert!((got - want).abs() < 1e-12, "{distance} mm: kr {got} vs {want}");
            }
            let (k, fx) = fit(&kr, 1936_f64.hypot(1090.0) / 2.0, focal).unwrap();
            for (got, want) in k.iter().zip(k_want) {
                assert!((got - want).abs() < 1e-8, "{distance} mm: k {got} vs {want}");
            }
            assert_eq!(k[3], 0.0);
            assert!((fx - fx_want).abs() < 1e-8, "{distance} mm: fx {fx} vs {fx_want}");
        }
    }

    #[test]
    fn the_fit_follows_the_opcode_within_a_tenth_of_a_pixel() {
        let focal = focal_px(40.0, 6000.0 / 1936.0);
        let corner = 1936_f64.hypot(1090.0) / 2.0 / focal;
        for kr in GREEN_NODES {
            let (k, _) = fit(&kr, 1936_f64.hypot(1090.0) / 2.0, focal).unwrap();
            for i in 1..=32 {
                let rho = i as f64 / 32.0;
                let r2 = rho * rho;
                let g = (kr[0] + kr[1] * r2 + kr[2] * r2 * r2 + kr[3] * r2 * r2 * r2) / kr[0];
                let theta = (rho * corner).atan();
                let model = theta + k[0] * theta.powi(3) + k[1] * theta.powi(5) + k[2] * theta.powi(7);
                assert!((model - g * rho * corner).abs() * focal < 0.1, "{kr:?} at {rho}");
            }
        }
    }

    #[test]
    fn unknown_focus_is_infinity_and_an_empty_table_gives_nothing() {
        assert_eq!(table().coefficients_at(None), table().coefficients_at(Some(f64::INFINITY)));
        assert_eq!(table().coefficients_at(Some(0.0)), table().coefficients_at(None));
        // What the fp writes for a lens without contacts
        let empty = LensTable { calib_focal_tenths_mm: 65535, axis: [0x7FFF_FFFF; 5], nodes: [[[0.0; 4]; 3]; 5] };
        assert_eq!(empty.coefficients_at(Some(500.0)), None);
        assert_eq!(fit(&[0.0; 4], 1110.0, 2000.0), None);
    }

    #[test]
    fn the_calibrated_focal_length_wins_unless_implausible() {
        assert_eq!(focal_mm(Some(39.4), Some(40.0)), Some(39.4));
        assert_eq!(focal_mm(Some(0.0), Some(40.0)), Some(40.0));
        assert_eq!(focal_mm(Some(90.0), Some(40.0)), Some(40.0));
        assert_eq!(focal_mm(Some(6553.5), Some(40.0)), Some(40.0));
        assert_eq!(focal_mm(None, Some(28.0)), Some(28.0));
        assert_eq!(focal_mm(Some(28.0), None), Some(28.0));
        assert_eq!(focal_mm(None, None), None);
    }

    #[test]
    fn smaller_picture_pixels_are_a_longer_focal_length() {
        // 28 mm over the sensor's 35.9 mm in a 1920 pixel wide picture, and in one twice as fine
        let full = focal_px(28.0, 6000.0 / 1920.0);
        assert!((full - 1920.0 * 28.0 / 35.9).abs() < 1e-9);
        assert!((focal_px(28.0, 3000.0 / 1920.0) - 2.0 * full).abs() < 1e-9);
    }

    #[test]
    fn warp_rectilinear_reads_the_green_plane() {
        let mut parameters = 3_u32.to_be_bytes().to_vec();
        for plane in 0..3 {
            for v in [1.0 + plane as f64 * 1e-4, -0.0149, 0.0101, -0.0012, 0.0, 0.0] {
                parameters.extend_from_slice(&v.to_be_bytes());
            }
        }
        parameters.extend_from_slice(&0.5_f64.to_be_bytes());
        parameters.extend_from_slice(&0.4_f64.to_be_bytes());
        // A GainMap (id 9) first, then the WarpRectilinear
        let mut list = 2_u32.to_be_bytes().to_vec();
        for v in [9_u32, 0x0103_0000, 0, 8] { list.extend_from_slice(&v.to_be_bytes()); }
        list.extend_from_slice(&[0xAA; 8]);
        for v in [1_u32, 0x0103_0000, 0, parameters.len() as u32] { list.extend_from_slice(&v.to_be_bytes()); }
        list.extend_from_slice(&parameters);
        assert_eq!(warp_rectilinear(&list), Some(WarpRectilinear { kr: [1.0001, -0.0149, 0.0101, -0.0012], centre: (0.5, 0.4) }));
        // Cut off in the green plane
        assert_eq!(warp_rectilinear(&list[..list.len() - 100]), None);
    }

    #[test]
    fn the_normalization_radius_reaches_the_farthest_corner() {
        let centred = WarpRectilinear { kr: [1.0, 0.0, 0.0, 0.0], centre: (0.5, 0.5) };
        assert!((centred.radius(3856.0, 2170.0) - 3856_f64.hypot(2170.0) / 2.0).abs() < 1e-9);
        // Off centre the far corner is farther: 0.7 of the width and 0.6 of the height away
        let off = WarpRectilinear { centre: (0.3, 0.6), ..centred };
        assert!((off.radius(1000.0, 500.0) - 700_f64.hypot(300.0)).abs() < 1e-9);
    }
}
