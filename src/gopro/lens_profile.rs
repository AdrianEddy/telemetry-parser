// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2026 Adrian <adrian.eddy at gmail>
//
// Builds a native Gyroflow lens profile from GoPro's in-camera lens calibration.
//
// The profile uses Gyroflow's `gopro` distortion model, which consumes the camera's
// calibration directly:
//   - Radial: `world_radians = POLY(p)`, p = r1·(normalized image radius). The raw
//     POLY coefficients r0..r6 go into `distortion_coeffs` (→ params.k).
//     θ(p) = r0 + r1·p + r2·p² + … + r6·p⁶, with p = x_norm·ZMPL, x_norm = pixel_radius/half_diag.
//   - Digital warp (Superview/Hyperview): the MAPX/MAPY polynomials + the aspect factor
//     ARWA/ARUW go into `digital_lens_params`; the `gopro` model bakes the warp in, so
//     there is no separate digital-lens stage. Wide footage has identity MAPX/MAPY (factor 1).
//
// Focal length `f = half_diag(VRES) / (r1 · ZMPL · (ARWA/ARUW))`, principal point centered,
// `calib_dimension` = VRES.

use crate::*;
use crate::tags_impl::*;

const POLY: u32 = 0x504f4c59; // radial coefficients (normalized radius → angle, radians)
const ZMPL: u32 = 0x5a4d504c; // normalization multiplier applied to the POLY input
const VRES: u32 = 0x56524553; // calibration resolution [w, h]
const VFOV: u32 = 0x56464f56; // lens/FOV setting (W/S/H/L/N/M/X); S=Superview, H=Hyperview
const ZFOV: u32 = 0x5a464f56; // diagonal FOV (deg), corner-to-corner of the recorded frame
const ARUW: u32 = 0x41525557; // aspect ratio of the UnWarped input (e.g. 1.1429 for 8:7)
const ARWA: u32 = 0x41525741; // aspect ratio of the WArped output (e.g. 1.7778 for 16:9)
const MAPX: u32 = 0x4d415058; // horizontal warp polynomial coefficients
const MXCF: u32 = 0x4d584346; // MAPX coefficient power names (e.g. x1, x3, x5, x1y2)
const MAPY: u32 = 0x4d415059; // vertical warp polynomial coefficients
const MYCF: u32 = 0x4d594346; // MAPY coefficient power names (e.g. y1, y3, y5, y1x2)

/// Parsed GoPro lens calibration.
struct GoProLens {
    poly: Vec<f64>,       // r0..r6
    zmpl: f64,
    w: usize,
    h: usize,
    factor: f64,          // ARWA/ARUW
    mapx: [f64; 8],       // canonical MAPX coeffs c0..c7
    mapy: [f64; 6],       // canonical MAPY coeffs d0..d5
    zfov: Option<f64>,    // diagonal FOV in degrees (for the radial fold/FOV limit)
    lens_model: String,   // "Wide" / "Superview" / "Hyperview" / …
}

pub fn insert_lens_profile(samples: &mut [SampleInfo], model: Option<&str>, options: &crate::InputOptions) {
    // POLY/ZMPL/VRES/MAPX/… live in the global device stream (GroupId::Default).
    let mut found: Option<(usize, GoProLens)> = None;
    for (idx, s) in samples.iter().enumerate() {
        let Some(map) = s.tag_map.as_ref().and_then(|m| m.get(&GroupId::Default)) else { continue; };
        let Some(poly) = read_poly(map) else { continue; };
        let Some(zmpl) = (map.get_t(TagId::Unknown(ZMPL)) as Option<&f32>).map(|v| *v as f64) else { continue; };
        let Some((w, h)) = read_resolution(map) else { continue; };

        let factor = match (map.get_t(TagId::Unknown(ARWA)) as Option<&f32>, map.get_t(TagId::Unknown(ARUW)) as Option<&f32>) {
            (Some(&warped), Some(&unwarped)) if warped > 0.0 && unwarped > 0.0 => (warped / unwarped) as f64,
            _ => 1.0,
        };

        // MAPX/MAPY de-stretch polynomials, mapped from GPMF into the canonical slot layout
        // by their MXCF/MYCF power names. Absent (Wide) → identity.
        let mapx = read_mapx(map);
        let mapy = read_mapy(map);

        let zfov = (map.get_t(TagId::Unknown(ZFOV)) as Option<&f32>).map(|v| *v as f64).filter(|z| z.is_finite() && *z > 0.0);

        let lens_model = match (map.get_t(TagId::Unknown(VFOV)) as Option<&String>).map(|v| v.as_str()) {
            Some("S") => "Superview",
            Some("H") => "Hyperview",
            Some("W") => "Wide",
            Some("L") => "Linear",
            Some("N") => "Narrow",
            Some("M") => "Medium",
            Some("X") => "Max SuperView",
            _         => "",
        }.to_string();

        found = Some((idx, GoProLens { poly, zmpl, w, h, factor, mapx, mapy, zfov, lens_model }));
        break;
    }
    let Some((idx, lens)) = found else { return; };

    let Some(profile) = build_profile(&lens, model) else { return; };

    log::debug!("GoPro native lens profile: {profile}");

    if let Some(map) = samples[idx].tag_map.as_mut() {
        util::insert_tag(map, crate::tag!(parsed GroupId::Lens, TagId::Data, "Lens profile", Json, |v| serde_json::to_string(v).unwrap(), profile, Vec::<u8>::new()), options);
    }
}

/// Read the POLY coefficients as `f64`. GoPro encodes them as a single struct of `f32`;
/// depending on the term count GPMF types that struct differently (see `read_f32_list`).
fn read_poly(map: &TagMap) -> Option<Vec<f64>> {
    if let Some(v) = read_f32_list(map, POLY) {
        if v.len() >= 2 { return Some(v); }
    }
    if let Some(v) = map.get_t(TagId::Unknown(POLY)) as Option<&Vec<Vec<f64>>> {
        return v.first().cloned();
    }
    None
}

/// Read VRES `[w, h]`, tolerating the various integer encodings GPMF may use.
fn read_resolution(map: &TagMap) -> Option<(usize, usize)> {
    macro_rules! try_vec { ($t:ty) => {
        if let Some(v) = map.get_t(TagId::Unknown(VRES)) as Option<&Vec<$t>> {
            if v.len() >= 2 && v[0] > 0 as $t && v[1] > 0 as $t {
                return Some((v[0] as usize, v[1] as usize));
            }
        }
    }; }
    try_vec!(u16); try_vec!(u32); try_vec!(i16); try_vec!(i32);
    None
}

/// Read a coefficient list as `f64`, tolerating every GPMF encoding the parser may pick
/// for a single struct of N floats. Crucially, GPMF types the struct by its float count:
/// 1 → scalar `f32`, 3 → `Vec<Vector3<f32>>`, 4 → `Vec<TimeVector3<f32>>`, otherwise
/// `Vec<Vec<f32>>` (and `Vec<f32>` when the values are separate repeats). So Superview's
/// 3-coeff MAPX arrives as a Vector3 and Hyperview's 4-coeff MAPY as a TimeVector3 — both
/// must be handled or the warp silently degrades to identity.
fn read_f32_list(map: &TagMap, tag: u32) -> Option<Vec<f64>> {
    if let Some(v) = map.get_t(TagId::Unknown(tag)) as Option<&Vec<Vec<f32>>> {
        return v.first().map(|row| row.iter().map(|x| *x as f64).collect());
    }
    if let Some(v) = map.get_t(TagId::Unknown(tag)) as Option<&Vec<f32>> {
        return Some(v.iter().map(|x| *x as f64).collect());
    }
    if let Some(v) = map.get_t(TagId::Unknown(tag)) as Option<&Vec<Vector3<f32>>> {
        return v.first().map(|p| vec![p.x as f64, p.y as f64, p.z as f64]);
    }
    if let Some(v) = map.get_t(TagId::Unknown(tag)) as Option<&Vec<TimeVector3<f32>>> {
        return v.first().map(|p| vec![p.t as f64, p.x as f64, p.y as f64, p.z as f64]);
    }
    if let Some(v) = map.get_t(TagId::Unknown(tag)) as Option<&f32> {
        return Some(vec![*v as f64]);
    }
    None
}

/// Read coefficient power names (e.g. `["x1","x3","x5"]`), tolerating `Vec<String>` /
/// `Vec<Vec<String>>` / single `String` encodings.
fn read_names(map: &TagMap, tag: u32) -> Option<Vec<String>> {
    if let Some(v) = map.get_t(TagId::Unknown(tag)) as Option<&Vec<String>> {
        return Some(v.iter().map(|s| s.trim().to_lowercase()).collect());
    }
    if let Some(v) = map.get_t(TagId::Unknown(tag)) as Option<&String> {
        // A single string may pack all names, delimited by whitespace/commas.
        return Some(v.split(|c: char| c.is_whitespace() || c == ',').filter(|s| !s.is_empty()).map(|s| s.trim().to_lowercase()).collect());
    }
    None
}

// new_x = x*(c0 + c1*x² + c2*x⁴ + c3*x⁶ + c4*x⁸ + c5*x¹⁰ + c6*x¹²) + c7*x*y²
fn mapx_slot(name: &str) -> Option<usize> {
    Some(match name {
        "x1"   => 0,
        "x3"   => 1,
        "x5"   => 2,
        "x7"   => 3,
        "x9"   => 4,
        "x11"  => 5,
        "x13"  => 6,
        "x1y2" => 7,
        _ => return None,
    })
}
// new_y = y*(d0 + d1*y² + d2*y⁴ + d3*x² + d4*y²*x² + d5*x⁴)
fn mapy_slot(name: &str) -> Option<usize> {
    Some(match name {
        "y1"   => 0,
        "y3"   => 1,
        "y5"   => 2,
        "y1x2" => 3,
        "y3x2" => 4,
        "y1x4" => 5,
        _ => return None,
    })
}

/// MAPX → canonical 8-slot layout. Identity (`[1,0,…]`) when MAPX is absent or a scalar.
fn read_mapx(map: &TagMap) -> [f64; 8] {
    match read_f32_list(map, MAPX) {
        Some(v) if v.len() > 1 => slots_mapx(&v, read_names(map, MXCF)),
        _ => { let mut o = [0.0; 8]; o[0] = 1.0; o } // absent / scalar `1` → identity
    }
}
fn slots_mapx(values: &[f64], names: Option<Vec<String>>) -> [f64; 8] {
    let mut out = [0.0; 8];
    // MAPX is sequential in both known modes (x1,x3,x5[,x7,x9,x11,x13,x1y2] → c0..c7),
    // so the index fallback is already correct; names just make it future-proof.
    for (i, v) in values.iter().enumerate() {
        let slot = names.as_ref().and_then(|n| n.get(i)).and_then(|name| mapx_slot(name)).unwrap_or(i.min(7));
        if slot < 8 { out[slot] = *v; }
    }
    out
}

/// MAPY → canonical 6-slot layout. Identity (`[1,0,…]`) when MAPY is absent or a scalar.
fn read_mapy(map: &TagMap) -> [f64; 6] {
    match read_f32_list(map, MAPY) {
        Some(v) if v.len() > 1 => slots_mapy(&v, read_names(map, MYCF)),
        _ => { let mut o = [0.0; 6]; o[0] = 1.0; o } // absent / scalar `1` → identity
    }
}
/// Slots come from the MYCF power names; when unavailable, fall back to the known GoPro
/// layouts by coefficient count: 6 → d0..d5 (Superview, sequential), 4 → d0,d1,d3,d5
/// (Hyperview: y1,y3,y1x2,y1x4 — NOT sequential).
fn slots_mapy(values: &[f64], names: Option<Vec<String>>) -> [f64; 6] {
    let mut out = [0.0; 6];
    let default_slots: &[usize] = match values.len() {
        4 => &[0, 1, 3, 5],
        _ => &[0, 1, 2, 3, 4, 5],
    };
    for (i, v) in values.iter().enumerate() {
        let slot = names.as_ref().and_then(|n| n.get(i)).and_then(|name| mapy_slot(name))
                        .or_else(|| default_slots.get(i).copied())
                        .unwrap_or(i.min(5));
        if slot < 6 { out[slot] = *v; }
    }
    out
}

fn build_profile(lens: &GoProLens, model: Option<&str>) -> Option<serde_json::Value> {
    let GoProLens { poly, zmpl, w, h, factor, mapx, mapy, zfov, lens_model } = lens;
    let (w, h, zmpl) = (*w, *h, *zmpl);
    if poly.len() < 2 || !zmpl.is_finite() || zmpl <= 0.0 || w == 0 || h == 0 { return None; }
    let r1 = poly[1];
    if !r1.is_finite() || r1.abs() < 1e-9 { return None; }
    let factor = if factor.is_finite() && *factor > 0.0 { *factor } else { 1.0 };

    let half_diag = 0.5 * (((w * w + h * h) as f64).sqrt());
    let f = half_diag / (r1 * zmpl * factor);
    if !f.is_finite() || f <= 0.0 { return None; }
    let cx = w as f64 / 2.0;
    let cy = h as f64 / 2.0;

    // Radial fold / FOV limit. The `gopro` model's POLY radial map doesn't fold within
    // [0, π/2], so its limit can't be derived from the coefficients (unlike opencv_fisheye).
    // Instead use the captured corner angle = ZFOV/2 (ZFOV = diagonal FOV): rays beyond it
    // are outside the recorded frame, so the renderer clamps them (`r_limit = tan(ZFOV/2)`)
    // instead of folding them back into a wrapped/garbage region.
    let radial_distortion_limit: Option<f64> = zfov.and_then(|z| {
        let half = (z * 0.5).to_radians();
        if half > 0.0 && half < 89.0_f64.to_radians() { Some(half.tan()) } else { None }
    });

    // digital_lens_params layout: [MAPX×8, MAPY×6, factor, unused]
    let mut digital_lens_params = vec![0.0_f64; 16];
    digital_lens_params[0..8].copy_from_slice(mapx);
    digital_lens_params[8..14].copy_from_slice(mapy);
    digital_lens_params[14] = factor;

    // The MAPX/MAPY warp (Superview/Hyperview) is a separate pixel-space digital-lens stage so it
    // composes correctly with the radial model in all render paths (incl. the lens-correction slider).
    // Wide footage has an identity warp → no digital lens.
    let warp_is_identity = (factor - 1.0).abs() < 1e-9
        && (mapx[0] - 1.0).abs() < 1e-9 && mapx[1..].iter().all(|c| c.abs() < 1e-12)
        && (mapy[0] - 1.0).abs() < 1e-9 && mapy[1..].iter().all(|d| d.abs() < 1e-12);
    let digital_lens: Option<&str> = if warp_is_identity { None } else { Some("gopro_warp") };

    let (out_w, out_h) = default_output_dimension(w, h);

    let model = model.unwrap_or("GoPro").trim().to_string();

    Some(serde_json::json!({
        "calibrated_by":   "GoPro",
        "camera_brand":    "GoPro",
        "camera_model":    model,
        "lens_model":      lens_model,
        "note":            "",
        "calib_dimension":  { "w": w,     "h": h },
        "orig_dimension":   { "w": w,     "h": h },
        "output_dimension": { "w": out_w, "h": out_h },
        "official":        true,
        "asymmetrical":    false,
        "input_horizontal_stretch": 1.0,
        "input_vertical_stretch":   1.0,
        "fisheye_params": {
            "camera_matrix": [
                [ f,   0.0, cx  ],
                [ 0.0, f,   cy  ],
                [ 0.0, 0.0, 1.0 ]
            ],
            "distortion_coeffs": poly,
            "radial_distortion_limit": radial_distortion_limit
        },
        "distortion_model": "gopro",
        "digital_lens": digital_lens,
        "digital_lens_params": digital_lens_params,
        "calibrator_version": "---"
    }))
}

/// GoPro's default delivery is 16:9. Wide/Linear modes capture taller frames (4:3 or
/// 8:7), so default the rendered output to a centered 16:9 crop — full width, reduced
/// height (matching e.g. the manual HERO11 Wide 4:3 profile, 5312×3984 → 5312×2988).
/// Modes already at or below 16:9 keep their native size.
fn default_output_dimension(w: usize, h: usize) -> (usize, usize) {
    let sixteen_by_nine = 9.0 / 16.0;
    if (h as f64) > (w as f64) * sixteen_by_nine + 1.0 {
        let mut out_h = ((w as f64) * sixteen_by_nine).round() as usize;
        out_h -= out_h % 2; // keep even
        (w, out_h)
    } else {
        (w, h)
    }
}
