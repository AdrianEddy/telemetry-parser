// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2026 Adrian <adrian.eddy at gmail>

//! Normalising the Vuze XR's `cali` YAML into a [`CameraRig`].
//!
//! The camera ships an OpenCV stereo calibration verbatim: a camera matrix, the
//! four fisheye coefficients, a rotation matrix, a baseline and an image circle
//! per camera. What it does NOT ship is the size of the frame all of that is
//! expressed in — the calibration space is about 3040 px across while the
//! recorded tracks are 2144 — so the map onto the picture has to be derived:
//! its scale is a measured constant of the camera ([`PICTURE_PER_CALIB`]) and
//! its offset is the `rcrp` / `lcrp` box beside the YAML ([`Crop`]).
//!
//! # The convergence sweep (L21, D9.11)
//!
//! **The 34 mm baseline is real and must not be zeroed.** oxivideo's
//! `tests/geometry_convergence_sweep.rs` renders each camera into its own
//! equirectangular layer and cross-correlates the two along the seam, over the
//! stitching distances `{∞, 10, 5, 2, 1, 0.5} m`. On `HET_1107.MP4`
//! (`‖t‖` = 34.16 mm, two 2144² tracks), on the map this module states
//! (2026-09-05), the residual parallax runs
//!
//! | `R` | ∞ | 10 m | 5 m | 2 m | 1 m | 0.5 m |
//! |---|---|---|---|---|---|---|
//! | disparity, 2048-wide layer px | +5.38 | +4.28 | +3.18 | −0.21 | −5.81 | −17.04 |
//!
//! — a straight line in `1/R` (worst rung 0.024 px off it) whose slope is
//! **−11.220 px per m⁻¹** against the **−11.136** that `t` and the layer's own
//! pixels-per-radian predict, i.e. **0.75 %**. So the plain path is NOT where
//! this body's hemispheres agree: the `cali` document's `t` is a physical
//! baseline the renderer has to correct for, the intrinsics are not
//! pre-compensated for a reference distance, and the convergence sphere does
//! not over-correct.
//!
//! **It does not pin a default `R`, and this module goes on stating
//! `convergence_m: None`.** The zero crossing is the CLIP's content: 2.08 m
//! pooled over ±30° of latitude at four instants — the wall and the houses
//! beside the road, the car the rig rides on below. The stitching distance
//! is a host's slider, and the vendor states none.
//!
//! The same run also measures a **−1.7 px** rotation between the two cameras
//! that the convergence sweep leaves untouched (it moves by 0.17 px over rungs
//! spanning 22 px of disparity) — about 0.3° of residual yaw, recorded here
//! as a measurement and not corrected: at the seam it is one and the same as
//! a 2.6-px horizontal offset between the two crops, and the file cannot say
//! which.
//!
//! Under the map this module stated before — the crop's width as the circle's
//! diameter, the crop read `x y` — the same sweep read +12.42 px on the plain
//! path, a zero crossing at 0.89 m and a −3.3 px rotation, and the far field
//! (the house at 8 m, the sky) sat a constant 2.1° apart at the seams at every
//! instant: not parallax but the map's 1.1 % of scale.

use std::f64::consts::{FRAC_PI_2, PI};

use video_types::{geometry::{
    Affine2, Blend, BlendMode, Circle, Intrinsics, Lens, LensModel, Picture, Projection, ProjectionKind, Quat,
}};

use crate::rig::{CameraRig, RigPicture};

/// One camera's per-track crop, from the `rcrp` / `lcrp` `udta` boxes, in
/// **recorded picture** pixels: where the recorded frame starts in the scaled
/// calibration frame, and its size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Crop {
    /// The horizontal offset — the box's SECOND number.
    pub x: u32,
    /// The vertical offset — the box's FIRST number.
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Crop {
    /// `"24 32 2144 2144"` — the four whole numbers the box carries, as
    /// `row col width height`.
    ///
    /// The box states no order, so the order is measured: at the scale below,
    /// the ring the stated circle lands on sits, on each recorded track, at the
    /// circle's centre less (32, 24) on `CAM_0` and (36, 32) on `CAM_1` — the
    /// second number horizontally to 0.1 / 0.4 px, the first vertically to
    /// 1.5 px — where the other order puts it 7.9 / 3.6 px out horizontally,
    /// which the two tracks' seams carry as a 0.7° yaw.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let v: Vec<u32> = text.split_whitespace().filter_map(|x| x.parse().ok()).collect();
        match v[..] {
            [y, x, width, height] if width > 0 && height > 0 => Some(Self { x, y, width, height }),
            _ => None,
        }
    }
}

/// The recorded picture per calibration pixel, in the 2144-px 360 mode.
///
/// The vendor states the calibration frame's size nowhere, and the one
/// derivation the file offers — the crop's width as the circle's diameter —
/// is refuted by the tracks: the stated circle lands on the OUTER edge of the
/// lens hood's chrome reflection, which the 2144-px frame cuts by 12 px on
/// each side, at r = 1084 / 1080 px (fitted on a 42-frame median of each
/// track at thresholds 32–64: 1082–1085 / 1079–1080), and the two tracks'
/// seams register their far field — a house at 8 m, the sky — only 0.8–1.2 %
/// over that derivation: a constant +24.3 px of across-seam disparity on a
/// 4096-px equirect at three instants otherwise, zero at 0.7307–0.7325
/// (oxivideo `geometry_vuze_rigid.rs`). Both lenses land on ONE ratio
/// (1084.0/1483.6 and 1079.5/1477.4), which is what one sensor readout through
/// one scaler gives, and 2192/3000 is the rational it lands on: half a pixel
/// of the circle's radius on both tracks, 0.1 / 0.4 px of the crops'
/// horizontal offsets.
///
/// Carried with the track's width for the camera's other modes, which are
/// unmeasured.
pub(crate) const PICTURE_PER_CALIB: f64 = 2192.0 / 3000.0;
const MEASURED_TRACK_PX: f64 = 2144.0;

/// Calibration pixels → recorded-picture pixels for a track `crop` wide.
pub(crate) fn picture_scale(crop: Crop) -> f64 {
    PICTURE_PER_CALIB * f64::from(crop.width) / MEASURED_TRACK_PX
}

/// How far inside the stated circle the PICTURE ends, calibration pixels.
///
/// The stated circle is the hood's outer edge. Inside it sit the hood's chrome
/// reflection and a dark gap, 31 px wide in the 2144-px picture on both tracks
/// (the equator row of each track's median: picture from col 28.9 / 29.6, hood
/// to col 18 / 22), and then the field stop, where the picture really ends.
/// 46 calibration px — 34 in the picture — leaves the gap and the ring out on
/// every side of both tracks (the ring's centre sits 5 px from the picture's)
/// and wastes at most 8 px of picture. Without the inset the composite blends
/// the chrome in at up to 40 % and the seam flow takes it for a template.
const HOOD_CALIB_PX: f64 = 46.0;

/// The two cameras' YAML keys, in the order their tracks are written.
const CAMERAS: [&str; 2] = ["CAM_0", "CAM_1"];

/// The rig the `CamModel_V2_Set` document describes.
///
/// `crops` are the `rcrp` and `lcrp` rects, in that order — the file's own
/// order, and the pairing a measurement of the two recorded image circles
/// confirms: the circle each track actually holds lands within a few pixels of
/// where its own camera's calibration, scaled and shifted by its own crop, puts
/// it, and within tens of pixels of where the other camera's does.
pub(crate) fn camera_rig(model: &str, calib: &serde_json::Value, crops: [Crop; 2]) -> Option<CameraRig> {
    let set = calib.get("CamModel_V2_Set")?;
    let cameras: Vec<Lens> = CAMERAS
        .iter()
        .zip(crops)
        .enumerate()
        .map(|(i, (key, crop))| lens(set.get(key)?, i as u32, crop))
        .collect::<Option<Vec<Lens>>>()?;

    Some(CameraRig {
        brand: "Vuze".into(),
        model: model.trim().to_owned(),
        projection: Projection::new(ProjectionKind::Rig {
            cameras,
            blend: BlendMode::Feather,
            convergence_m: None,
        }),
        pictures: vec![RigPicture::VideoTrack(0), RigPicture::VideoTrack(1)],
        variants: Vec::new(),
        audio_source: None,
        siblings: Vec::new(),
    })
}

/// The `n` numbers of an OpenCV-matrix node, or of a plain sequence.
fn numbers(node: Option<&serde_json::Value>, n: usize) -> Option<Vec<f64>> {
    let node = node?;
    let list = node.get("data").unwrap_or(node).as_array()?;
    let v: Vec<f64> = list.iter().filter_map(serde_json::Value::as_f64).collect();
    (v.len() == n && v.iter().all(|x| x.is_finite())).then_some(v)
}

/// One `CAM_i` node as a [`Lens`].
fn lens(cam: &serde_json::Value, input: u32, crop: Crop) -> Option<Lens> {
    let k = numbers(cam.get("K"), 9)?;
    let dist = numbers(cam.get("DistortionCoeffs"), 4)?;
    let r = numbers(cam.get("R"), 9)?;
    let t = numbers(cam.get("t"), 3)?;
    let centre = numbers(cam.get("ImageCircleCenter"), 2)?;
    let radius = cam.get("ImageCircleRadius")?.as_f64().filter(|r| r.is_finite() && *r > 0.0)?;

    let focal = [k[0], k[4]];
    if !focal.iter().all(|f| f.is_finite() && *f > 0.0) {
        log::warn!("Dropping a Vuze camera: a focal length of {focal:?} is not one.");
        return None;
    }
    let model = LensModel::ThetaPoly { k: [dist[0], dist[1], dist[2], dist[3], 0.0], p: [0.0, 0.0] };
    // The picture ends inside the stated circle, at the field stop.
    let usable = radius - HOOD_CALIB_PX;
    if usable <= 0.0 {
        log::warn!("Dropping a Vuze camera: a {radius}-pixel circle is all hood.");
        return None;
    }
    let fov_limit = rim_angle(focal[0], &dist, usable)?;

    // The scale is the camera's, measured; the translation is the crop's
    // origin, where the recorded frame starts in the scaled calibration frame.
    let scale = picture_scale(crop);
    let calib_to_picture = Affine2 { a: [[scale, 0.0], [0.0, scale]], t: [-f64::from(crop.x), -f64::from(crop.y)] };

    Some(Lens {
        picture: Picture::whole(input),
        intrinsics: Intrinsics {
            model,
            calib_dim: calib_dim(&centre, usable, crop, scale)?,
            focal,
            centre: [k[2], k[5]],
            skew: k[1],
            thermal: None,
        },
        calib_to_picture,
        mirror: false,
        rotation: quat_from_matrix(&r)?,
        // Millimetres in the file; `Lens::translation` is metres.
        translation: [t[0] / 1000.0, t[1] / 1000.0, t[2] / 1000.0],
        fov_limit: Some(fov_limit),
        image_circle: Some(Circle { centre: [centre[0], centre[1]], radius: usable }),
        masks: Vec::new(),
        // The vendor's stitcher has not been read for this body, so no vendor
        // blend is known. What is stated is the whole overlap the picture
        // allows — full weight to 90°, none at the field stop — which is what
        // this parser composed before validity and blend were separate
        // statements. Stated here, by this module, rather than left for
        // anything downstream to derive from the rim; reading Vuze's own band
        // is the open item.
        blend: Blend { limit: fov_limit, width: (fov_limit - FRAC_PI_2).max(0.0) },
        photometric: None,
        readout: None,
        eye: None,
    })
}

/// The calibration frame, which the vendor does not state.
///
/// Taken as the smallest whole frame that contains BOTH the image circle the
/// lens states (the picture's, inside the hood) and everything the recorded
/// picture maps back to. The second half is not
/// decoration: the picture is a crop, so its corners reach past the circle on
/// two sides, and a frame sized to the circle alone leaves twenty rows of every
/// recorded track outside the space the masks and the circle are rasterised in.
fn calib_dim(centre: &[f64], radius: f64, crop: Crop, scale: f64) -> Option<[u32; 2]> {
    let picture = [f64::from(crop.x + crop.width) / scale, f64::from(crop.y + crop.height) / scale];
    let mut out = [0u32; 2];
    for (i, slot) in out.iter_mut().enumerate() {
        let extent = (centre[i] + radius).max(picture[i]).ceil();
        if !(1.0..=f64::from(u32::MAX)).contains(&extent) {
            log::warn!("Dropping a Vuze camera: a calibration frame of {extent} pixels is not one.");
            return None;
        }
        *slot = extent as u32;
    }
    Some(out)
}

/// `θ_max` — the angle whose image radius is the stated one.
///
/// The vendor states a field of view nowhere, but it states the image circle,
/// and the picture ends a fixed way inside it ([`HOOD_CALIB_PX`]): the angle
/// that lands there is what the picture is valid out to. Solved rather than
/// assumed because the model is a θ-polynomial and inverting it in closed form
/// is not possible.
///
/// The polynomial turns over — this camera's peaks near 112° — so the search is
/// bounded by that maximum and takes the SMALLER of the two roots. Past the
/// turning point the model is being extrapolated out of its fitted range and
/// its second root is an artefact.
fn rim_angle(focal: f64, dist: &[f64], radius: f64) -> Option<f64> {
    let r = |theta: f64| {
        let t2 = theta * theta;
        focal * theta * (1.0 + dist[0] * t2 + dist[1] * t2 * t2 + dist[2] * t2.powi(3) + dist[3] * t2.powi(4))
    };
    // 0.1 mrad steps over the half sphere — finer than the bisection below
    // needs, and the whole scan is one loop per camera per clip.
    const STEPS: usize = 32768;
    let step = PI / STEPS as f64;
    let mut peak = step;
    for i in 2..=STEPS {
        let theta = i as f64 * step;
        if r(theta) <= r(peak) {
            break;
        }
        peak = theta;
    }
    if !(r(peak).is_finite() && r(peak) > 0.0) {
        return None;
    }
    let theta = if r(peak) < radius {
        // The circle is outside anything the model reaches, so the model's own
        // limit is the best statement of where it stops being one.
        log::warn!("A Vuze image circle of {radius} px is past this lens model's own maximum of {}.", r(peak));
        peak
    } else {
        let (mut lo, mut hi) = (0.0, peak);
        for _ in 0..64 {
            let mid = 0.5 * (lo + hi);
            if r(mid) < radius { lo = mid } else { hi = mid }
        }
        hi
    };
    (theta.is_finite() && theta > 0.0 && theta <= PI).then_some(theta)
}

/// A row-major rotation matrix as a unit quaternion.
///
/// Shepperd's method: the component with the largest magnitude is recovered
/// first, so the divisions never approach zero whatever the rotation. A matrix
/// that is not one — a reflection, a scaling, anything the vendor's own
/// least-squares fit could leave slightly off — comes back as a quaternion that
/// is not unit, and is refused here rather than at the geometry validator,
/// which would fail the whole rig for it.
fn quat_from_matrix(m: &[f64]) -> Option<Quat> {
    let (m00, m11, m22) = (m[0], m[4], m[8]);
    let trace = m00 + m11 + m22;
    let q = if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        Quat::new(0.25 * s, (m[7] - m[5]) / s, (m[2] - m[6]) / s, (m[3] - m[1]) / s)
    } else if m00 > m11 && m00 > m22 {
        let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
        Quat::new((m[7] - m[5]) / s, 0.25 * s, (m[1] + m[3]) / s, (m[2] + m[6]) / s)
    } else if m11 > m22 {
        let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
        Quat::new((m[2] - m[6]) / s, (m[1] + m[3]) / s, 0.25 * s, (m[5] + m[7]) / s)
    } else {
        let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
        Quat::new((m[3] - m[1]) / s, (m[2] + m[6]) / s, (m[5] + m[7]) / s, 0.25 * s)
    };
    // A tenth of a degree of slack on a matrix a factory fit rounded into a
    // YAML file, and nothing like enough to admit a reflection (norm 0 or a
    // negative trace root) or a scaling.
    let norm = q.norm();
    if !(norm.is_finite() && (norm - 1.0).abs() < 1e-3) {
        log::warn!("Dropping a Vuze camera: its rotation matrix {m:?} is not a rotation (‖q‖ = {norm}).");
        return None;
    }
    // The norm test above is what decides; `normalized` restates it in the
    // return type, so the two cannot disagree about which matrices are
    // rotations.
    q.normalized()
}
