// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2026 Adrian <adrian.eddy at gmail>

//! The GoPro Fusion's raw pair — one fisheye per file, calibrated in the
//! `udta` `GPMF` box that both files carry.
//!
//! Unlike the Max, the Fusion ships nothing dewarped: each file holds one
//! circular fisheye and the box holds the lens polynomial for BOTH of them plus
//! the extrinsics between the two. Which lens THIS file holds is therefore not
//! in the box at all — it is in the file's name, which is why the parser has to
//! be told the path.
//!
//! # The POLY pin
//!
//! **`θ = MFOV` lands on the visible rim, and the pair covers the sphere.**
//! Pinned in `tests/rig_gopro.rs` against the real 829-byte `GPMF` box that
//! `GPFR4425.MP4` / `GPBK4425.MP4` carry, and the pair covers the sphere with
//! every sample inside its own frame.
//!
//! | | front | back |
//! |---|---|---|
//! | `r(90°)`, capture px | 1439.134 | 1440.249 |
//! | `r(100°)`, capture px | 1514.824 | 1516.406 |
//! | `r(90°)` × `2704/3104`, recorded px | **1253.678** | **1254.650** |
//! | `r(100°)` × `2704/3104`, recorded px | **1319.615** | **1320.993** |
//! | measured rim on a real clip | 1330.1 | 1332.9 |
//! | agreement | +0.79 % | +0.90 % |
//!
//! Against a half-height of 1312 and a half-width of 1352, that is a circle
//! whose hemisphere fits with 57-58 px to spare, whose rim passes the top and
//! bottom edges by 8-9 px and stays 31-32 px inside the left and right ones,
//! and which therefore leaves small black corners — which is what the raw frame
//! looks like. Read as RECORDED pixels instead, the hemisphere alone is
//! 1439.1 > 1312 and the sphere is not covered at all: the `calib_to_picture`
//! mutation that drops the `WEWC` ratio fails the sweep at
//! `[-0.850, -0.496, -0.177]`, which is the four black wedges.
//!
//! `dr/dθ` bottoms out at 339.0 (front) / 342.2 (back) capture px per radian
//! over `[0°, 100°]`, so the map never folds inside the field and "the rim" is
//! a single branch.
//!
//! The pair's optical axes are 179.8909° apart, so the direction furthest from
//! both sits at `180 − 179.8909/2 = 90.0546°`. A quarter-degree equirect sweep
//! finds the worst covered direction at **90.0540°** — straight up, at
//! `[0.221, -0.975, -0.001]` — leaving 9.95° of margin under the 100°
//! half-field, and 5.95° under the 96° the blend trusts each lens to
//! ([`BLEND_LIMIT_DEG`]). Clipping `MFOV` to 89° fails the same sweep at
//! `[0.000, -1.000, -0.004]`.
//!
//! The two measured rim radii are M6a's, from the source clips the fixtures
//! were cut from; those clips are not in the corpus on this box, so the
//! measurement is recorded rather than re-derived. Everything else above is
//! computed from the committed 829-byte box.
//!
//! # The convergence sweep (L21, D9.11) — NOT MEASURED, and why
//!
//! **Owner-gated: no raw Fusion pair exists on the development box.** The
//! sweep of `oxivideo/tests/geometry_convergence_sweep.rs` renders each lens
//! into its own equirectangular layer and cross-correlates the two along the
//! seam, which needs the two hemispheres as PICTURES. What this box holds is
//! `fusion.mov`, a 5120×2560 equirect that has already been stitched — the
//! seam it would measure is GoPro's, not this rig's — and the checked-in
//! `GPFR4425.MP4` / `GPBK4425.MP4` fixtures are the `moov` alone with their
//! sample tables emptied. Synthesising a pair would measure the synthesis.
//!
//! What the sweep found on the two bodies it could run on — an Insta360 X5 and
//! a Vuze XR, both with a ~32 mm baseline — is that the plain path leaves a
//! real 13-pixel parallax at the seam and a finite convergence distance removes
//! it, at exactly the rate the stated baseline predicts (0.02 % and 0.46 %). So
//! nothing suggests this camera's `SHF`-derived 34 mm baseline should be
//! zeroed either, and it is not. Running the sweep on a real front/back pair is
//! the owner-gated item that would close it.

use std::f64::consts::FRAC_PI_2;
use std::path::Path;

use video_types::{geometry::{
    Affine2, Blend, BlendMode, Circle, Intrinsics, Lens, LensModel, Picture, Projection, ProjectionKind, Quat,
    Readout,
}};

use crate::rig::{CameraRig, RigPicture, SiblingDir, SiblingHint, SiblingRole};
use crate::tags_impl::TagMap;

use super::udta::{self, Device};

/// How far from its axis a Fusion lens is TRUSTED in the composite: 96°, the
/// last angle at which both lenses' scene-free radial luminance is still
/// within 5 % of the interior — front 95 %, back 93 %; the knee follows, 92 % /
/// 86 % at 98° and 72 % / 66 % at 99.5° — measured on `GPFR4425` / `GPBK4425`
/// as a per-pixel 3rd-max over 30 frames (`docs/360/GOPRO.md` §9). The
/// vendor's own stitch reads no vignetted texel — a Fusion Studio export of the
/// same body is flat to 0.1/255 across the −90° seam — so it stops blending
/// inside 97°. `MFOV` is where the picture ENDS (the mechanical rim, 100°) and
/// stays the validity; this is where the blend ends. Blended out to the rim
/// instead, a direction 97° off the front axis carries that lens's darkened
/// rim at three tenths of its weight: a visible ring.
const BLEND_LIMIT_DEG: f64 = 96.0;

/// Each lens's crossfade: centred on the seam, so both lenses cross at equal
/// weight on the great circle between them (the axes are 179.89° apart, so
/// that circle sits within 0.06° of 90° from each), and ending at
/// [`BLEND_LIMIT_DEG`] — 12° wide, from 84° to 96°.
fn blend() -> Blend {
    let limit = BLEND_LIMIT_DEG.to_radians();
    Blend::centred(FRAC_PI_2, 2.0 * (limit - FRAC_PI_2))
}

/// `r = Σ pᵢ·θ^(i+1)`, capture pixels.
fn radius_at(p: &[f64; 5], theta: f64) -> f64 {
    p.iter().enumerate().map(|(i, c)| c * theta.powi(i as i32 + 1)).sum()
}

/// Which lens a file holds, from its own name. The camera writes `GPFR####` on
/// one card folder and `GPBK####` on the other, and nothing inside either file
/// says which one it is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Role {
    Front,
    Back,
}

/// The rig this file's lens belongs to, or `None` when the clip is not a Fusion
/// half.
///
/// `picture` is the coded size of this file's video track — the encoded width
/// is what picks the capture mode out of the `WEWC` table.
pub(crate) fn camera_rig(
    devices: &[Device],
    model: Option<&str>,
    path: Option<&Path>,
    picture: (u32, u32),
    readout: Option<Readout>,
) -> Option<CameraRig> {
    let geometry = udta::device(devices, "Geometry Calibrations")?;
    let front_tags = udta::device(devices, "Front Lens")?;
    let back_tags = udta::device(devices, "Back Lens")?;
    let Some((role, sibling)) = path.and_then(file_role) else {
        log::warn!("A GoPro clip carries a Fusion lens pair, but its name does not say which half it is.");
        return None;
    };

    // The polynomial's radius is in CAPTURE pixels — the sensor window the
    // calibration was measured on — and the recorded frame is that window
    // scaled by the mode's `WEWC` ratio. Reading the radius as recorded pixels
    // instead puts a full hemisphere outside the frame and leaves four black
    // wedges at the seams.
    let calib = (udta::u32_of(geometry, b"CAPW")?, udta::u32_of(geometry, b"CAPH")?);
    let scale = capture_to_picture(geometry, picture.0)?;

    // The front lens is the rig's origin and the box states the back one
    // relative to it.
    let angles = ["ANGX", "ANGY", "ANGZ"].map(|k| udta::f64_of(geometry, k.as_bytes().try_into().unwrap()));
    let shift = ["SHFX", "SHFY", "SHFZ"].map(|k| udta::f64_of(geometry, k.as_bytes().try_into().unwrap()));
    let (Some(angles), Some(shift)) = (all(angles), all(shift)) else { return None };
    if !angles.iter().chain(&shift).all(|v| v.is_finite()) {
        log::warn!("Dropping a Fusion rig: its extrinsics {angles:?} / {shift:?} are not real numbers.");
        return None;
    }
    // This Fusion mode predates SROT. Its 3104x3000 sensor window,
    // recorded as 2704x2624, has a measured 20 ms row readout (independent
    // image/IMU alignment on GPFR4425). Never apply that calibration to
    // another window, or override a readout supplied by the recording.
    let readout = readout.or_else(|| {
        (calib == (3104, 3000) && picture == (2704, 2624))
            .then(|| crate::rig::readout(Some(20.0))).flatten()
    });
    let cameras = vec![
        lens(front_tags, 0, calib, scale, Quat::IDENTITY, [0.0; 3], readout)?,
        lens(back_tags, 1, calib, scale, back_rotation(angles), shift.map(|mm| mm / 1000.0), readout)?,
    ];
    let this = RigPicture::VideoTrack(0);
    let other = |r| RigPicture::Sibling { role: r, video_track: 0 };
    Some(CameraRig {
        brand: "GoPro".into(),
        // The `GPFR`/`GPBK` pair is written by exactly one body, so the name is
        // known even on a clip whose `udta` never states it.
        model: model.map(str::trim).filter(|m| !m.is_empty()).unwrap_or("Fusion").to_owned(),
        projection: Projection::new(ProjectionKind::Rig {
            cameras,
            blend: BlendMode::Feather,
            convergence_m: None,
        }),
        pictures: match role {
            Role::Front => vec![this, other(SiblingRole::Back)],
            Role::Back => vec![other(SiblingRole::Front), this],
        },
        variants: Vec::new(),
        audio_source: match role { Role::Front => Some(SiblingRole::Back), Role::Back => None },
        siblings: vec![sibling],
    })
}

fn all<const N: usize>(values: [Option<f64>; N]) -> Option<[f64; N]> {
    let mut out = [0.0; N];
    for (slot, v) in out.iter_mut().zip(values) {
        *slot = v?;
    }
    Some(out)
}

/// `R = Rz(ANGZ)·Ry(ANGY)·Rx(ANGX)`, degrees, taken as `q_rig_from_cam`.
///
/// `ANGY` carries the ~180° that puts the second lens back to back; the other
/// two are under 0.2° on both bodies in the corpus, which is also why the
/// corpus cannot say which euler ORDER the vendor used or whether the matrix is
/// this one or its transpose — every alternative moves the optical axis by less
/// than a quarter of a degree. The Z-Y-X order and the untransposed reading are
/// the same choices the other vendor modules make, so the one convention is
/// stated once rather than guessed at twice.
fn back_rotation([x, y, z]: [f64; 3]) -> Quat {
    Quat::from_rotation_z(z.to_radians())
        .mul(Quat::from_rotation_y(y.to_radians()))
        .mul(Quat::from_rotation_x(x.to_radians()))
        // Unreachable: the caller refuses a non-finite `ANGX`/`ANGY`/`ANGZ`
        // before it gets here, and three finite euler factors are three unit
        // quaternions.
        .normalized()
        .expect("three finite euler angles compose to a rotation")
}

/// The capture → recorded-frame scale for this clip's mode, from the `WEWC`
/// table.
///
/// `WEWC` is a flat list of `(encoded, captured)` width pairs — one per capture
/// mode the body supports — and the mode in force is the pair whose encoded
/// width is the one this file actually has. A body that offers a mode this file
/// is not in contributes a ratio that would scale the lens by up to 4×, so the
/// pair is SELECTED and never averaged or defaulted.
fn capture_to_picture(geometry: &TagMap, encoded_width: u32) -> Option<f64> {
    let table = udta::i16s(geometry, b"WEWC")?;
    let (encoded, captured) = table
        .chunks_exact(2)
        .find_map(|pair| (pair[0] == encoded_width as i32).then_some((pair[0], pair[1])))
        .or_else(|| {
            log::warn!("No {encoded_width}-pixel capture mode in the Fusion's WEWC table {table:?}.");
            None
        })?;
    (captured > 0 && encoded > 0).then(|| f64::from(encoded) / f64::from(captured))
}

/// One `POLY` block as a [`Lens`].
///
/// The model is `r = Σ pᵢ·θ^(i+1)` with `θ` in radians and `r` in **capture**
/// pixels — GoPro's own consecutive-integer-power convention, with the `r⁰`
/// term written as zero, running angle → radius here where the HERO block runs
/// radius → angle.
///
/// `centre` is the frame centre and NOT `CTRX`/`CTRY`: on this body those are a
/// design offset of the lens axis on the sensor, and the readout window is
/// placed on that axis, which cancels them in the recorded frame. A measurement
/// of the real image circle puts its centre within 3.2 px of the frame centre
/// on both lenses, and every fixed-point reading of `CTRX` — the raw value, a
/// `/8` and a `/16` — is tens of pixels away from that.
///
/// `focal` is `p₀`, which is `dr/dθ` at the optical axis: the paraxial focal
/// length, in the same capture pixels. The model does not read it — the whole
/// radial map is in the coefficients — but it is the positive number the shared
/// descriptors require of every lens, and a zero or negative leading
/// coefficient is not a lens either way.
fn lens(
    tags: &TagMap,
    input: u32,
    calib: (u32, u32),
    scale: f64,
    rotation: Quat,
    translation: [f64; 3],
    readout: Option<Readout>,
) -> Option<Lens> {
    let poly = udta::f64s(tags, b"POLY")?;
    let mut p = [0.0; 5];
    if poly.len() < 2 || poly.len() > p.len() || !poly.iter().all(|v| v.is_finite()) {
        log::warn!("Dropping a Fusion lens: its {}-term radial polynomial {poly:?} is not one.", poly.len());
        return None;
    }
    p[..poly.len()].copy_from_slice(&poly);
    if !(p[0] > 0.0) {
        log::warn!("Dropping a Fusion lens: a leading radial coefficient of {} is not a focal length.", p[0]);
        return None;
    }
    // Already `θ_max` — this is the reference case for that convention, and the
    // 100° here is a 200° lens.
    let fov = udta::f64_of(tags, b"MFOV")?;
    if !(fov.is_finite() && fov > 0.0 && fov <= 180.0) {
        log::warn!("Dropping a Fusion lens: a half-field of {fov}° is not one.");
        return None;
    }
    if calib.0 == 0 || calib.1 == 0 || !(scale.is_finite() && scale > 0.0) {
        log::warn!("Dropping a Fusion lens: a {}×{} capture window at ×{scale} is not a picture.", calib.0, calib.1);
        return None;
    }

    let centre = [f64::from(calib.0) / 2.0, f64::from(calib.1) / 2.0];
    let rim = radius_at(&p, fov.to_radians());
    if !(rim.is_finite() && rim > 0.0) {
        log::warn!("Dropping a Fusion lens: its polynomial puts the {fov}° rim at {rim} px, which is not a circle.");
        return None;
    }

    Some(Lens {
        picture: Picture::whole(input),
        intrinsics: Intrinsics {
            model: LensModel::GoProPoly { p },
            calib_dim: [calib.0, calib.1],
            focal: [p[0], p[0]],
            centre,
            skew: 0.0,
            thermal: None,
        },
        calib_to_picture: Affine2::scale(scale, scale),
        mirror: false,
        rotation,
        translation,
        fov_limit: Some(fov.to_radians()),
        // The rim as a circle too — exactly the θ = MFOV locus, the model being
        // purely radial — so the lens gets a validity tile and the atlas cuts
        // the calibration RECT as well as the circle: the recorded frame's top
        // and bottom edges cross the circle at 98.9°, and a sample past them is
        // a clamped edge row unless the rect is a boundary.
        image_circle: Some(Circle { centre, radius: rim }),
        masks: Vec::new(),
        blend: blend(),
        photometric: None,
        // Both halves of the body are read by the same sensor timing.
        readout,
        eye: None,
    })
}

/// Which half this file is, and where the other one is.
///
/// The camera writes the pair into two folders whose names differ only in
/// `GFRNT` / `GBACK` (`.../103GFRNT/GPFR4425.MP4` beside
/// `.../103GBACK/GPBK4425.MP4`), so the directory is stated relative to this
/// one. A file that has been moved out of that layout still gets a hint — for
/// the same directory — because the name alone is enough to look for.
fn file_role(path: &Path) -> Option<(Role, SiblingHint)> {
    let name = path.file_name()?.to_str()?;
    // `GPFR####` / `GPBK####`, then whatever extension the file has — the
    // camera's own naming, and the four digits are what tells it from a plain
    // `GOPR`/`GX` clip that happens to start with the same letters.
    if !name.get(4..8)?.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (role, other_role, mine, theirs) = match &name.get(..4)?.to_ascii_uppercase()[..] {
        "GPFR" => (Role::Front, SiblingRole::Back, "GFRNT", "GBACK"),
        "GPBK" => (Role::Back, SiblingRole::Front, "GBACK", "GFRNT"),
        _ => return None,
    };
    let file_name = format!("{}{}", if role == Role::Front { "GPBK" } else { "GPFR" }, name.get(4..)?);
    let directory = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|d| d.to_str())
        .and_then(|d| d.strip_suffix(mine))
        .map_or(SiblingDir::Same, |head| SiblingDir::Relative(format!("../{head}{theirs}")));
    Some((role, SiblingHint { role: other_role, directory, file_name }))
}
