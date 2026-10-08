// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2026 Adrian <adrian.eddy at gmail>

//! Normalising DJI's `PanoDewarpParams` into a [`CameraRig`].
//!
//! Everything DJI-specific about the 360 bodies lives here: which schema a
//! product's packets are written in, which of the 24 calibration slots is the
//! default, which video track each lens was recorded to, and the shape of the
//! lens model itself. Downstream sees only the shared geometry descriptors.

use video_types::{geometry::{
    Affine2, Blend, BlendMode, Intrinsics, Lens, LensModel, MAX_MASK_POINTS, MaskPolarity, MaskPoly, Picture,
    Projection, ProjectionKind, Quat, Readout, ThermalFocal,
}};

use crate::rig::{CameraRig, Housing, RigPicture, RigVariant, VariantKind};

#[path = "rig_body_masks.rs"]
pub(super) mod body_masks;

/// How far past the equator each lens is used, as a fraction of a half turn —
/// **the vendor's own number**, `seam_overlap_ratio` in its stitching bundle
/// (`Stitcher-Mac.bundle/case_00/seam_overlap_ratio.json` = `0.038`; the
/// player's `panorama.stitcher.seam_overlap_ratio` defaults to the same, and
/// its `-[DualStitcher getSeamOverlap:]` doubles it for the strip height).
///
/// It is what the vendor's final view is cut by: its sphere shader takes
/// lens 0 alone where the polar coordinate is below `0.5 − ratio`, lens 1
/// alone above `0.5 + ratio`, and the stitched band between — so neither
/// lens is ever read past `90° + 0.038·180°`, and the two are mixed over
/// exactly twice that.
const SEAM_OVERLAP_RATIO: f64 = 0.038;

/// How far from its axis each lens is TRUSTED in the composite — the vendor's
/// blend limit, `90° + `[`SEAM_OVERLAP_RATIO`]` · 180°` = **96.84°** — and,
/// below, the width of its crossfade: twice the overlap, **13.68°**, centred on
/// the equator of each lens's polar frame, which is the great circle between
/// the two. Read straight off the vendor's own compositor:
/// `sphereEisCpuMeshSamplingShader` takes lens 0 alone below `0.5 − ratio`,
/// lens 1 alone above `0.5 + ratio`, and the stitched band between.
///
/// A BLEND limit, not the field of the lens: the picture runs on past it
/// (see [`validity_rad`]) and the vendor keeps that picture, it simply never
/// reads it. Nor is it the number in the bundled masks' names (`91_4`,
/// `91_8`, `92_2`, `92_5`): those name the BODY arc each mask cuts in the
/// downward wedge — the same thing the clip's own `occlusion_pt_*` states at
/// 92.13° — while the rest of every mask is on to the edge of the picture. A
/// lens clipped at 92° would throw away seven degrees of picture the vendor
/// keeps.
pub(crate) const BLEND_LIMIT_DEG: f64 = 90.0 + SEAM_OVERLAP_RATIO * 180.0;

/// The width of the vendor's crossfade — see [`BLEND_LIMIT_DEG`].
pub(crate) const BLEND_WIDTH_DEG: f64 = 2.0 * SEAM_OVERLAP_RATIO * 180.0;

/// The step [`validity_rad`] walks the calibration's radius in: a hundredth of
/// a degree, a fifth of a pixel at these focal lengths.
const VALIDITY_STEP_DEG: f64 = 0.01;

/// How far from its axis a lens's picture is READ — image validity.
///
/// DJI states no field of view and no image circle. What its own masks state
/// is the FRAME: away from the body wedge every bundled mask is on out to the
/// picture's edge (`docs/360/DJI.md` §10.2 — rim θ ≈ 97.7–99.1° under the
/// clip's calibration), and the picture is bright to within a degree of it
/// (azimuth-averaged luma flat to 99° on the Osmo 360 and to 99.5° on the
/// Osmo 360 II). So the validity is the frame, stated as the angle at which
/// the calibration's own radius reaches the frame's NEAREST edge — the
/// largest θ at which every direction still lands on a pixel that exists:
/// 98.56° / 99.12° on the Osmo 360's two lenses, 99.61° / 99.58° on the
/// Avata 360's, 98.96° / 99.00° on the Osmo 360 II's — or the angle the
/// polynomial turns over at, if it does so first (no body here does). Both
/// are read off the clip; nothing is a fitted constant. The frame's corners
/// reach further and are cut by this; nothing is composited past the blend
/// limit in any case.
///
/// Radial only, each axis with its own focal: the tangential terms move a
/// rim sample by a fraction of a pixel and cannot change which edge is
/// nearest. `None` when the radius never reaches the frame — a calibration
/// describing a picture wholly inside its own edge is not one of these
/// cameras'.
fn validity_rad(d: &RawDewarp, k: &[f64; 5]) -> Option<f64> {
    let theta_d = |theta: f64| -> f64 {
        let t2 = theta * theta;
        theta * (1.0 + k[0] * t2 + k[1] * t2.powi(2) + k[2] * t2.powi(3) + k[3] * t2.powi(4) + k[4] * t2.powi(5))
    };
    // `r = f·θ_d`, so the edge is reached where `θ_d` reaches `edge / f`, per
    // axis; the nearer of the two is the limit.
    let target = (d.cx.min(d.width - d.cx) / d.fx).min(d.cy.min(d.height - d.cy) / d.fy);
    if !(target > 0.0) {
        return None;
    }
    let step = VALIDITY_STEP_DEG.to_radians();
    let mut prev = 0.0;
    let mut theta = step;
    while theta <= std::f64::consts::PI {
        let td = theta_d(theta);
        if td < prev {
            // The polynomial folds: past here it re-reads the interior.
            return Some(theta - step);
        }
        if td >= target {
            return Some(theta);
        }
        prev = td;
        theta += step;
    }
    None
}

/// The schema a product's `CAM meta` packets are written in.
///
/// Chosen by **product name**, never by the clip's self-declared
/// `proto_file_name`: DJI reuses one schema across products under different
/// declared names, and no `dvtm_AVATA360.proto` or `dvtm_OQ102.proto` exists in
/// any shipped build. The vendor's own player resolves a plugin by product name
/// through a plain hash lookup with no fallback, and feeding an Avata 360
/// packet to the Osmo 360 schema silently yields a clip with no calibration at
/// all — the two put `pano_dewarp_params` at different field numbers, so the
/// wire types disagree and the submessage is dropped.
#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub(crate) enum Schema {
    /// `dvtm_wm169` — the non-360 DJI bodies. No pano dewarp.
    Wm169,
    /// The Eagle 4 layout: `pano_dewarp_params` at `StreamMeta` field 5.
    /// DJI Avata 360, Osmo 360 II.
    Wa530,
    /// The Osmo 360 layout: `pano_dewarp_params` at `StreamMeta` field 6.
    Oq101,
}

impl Schema {
    /// `product_name` first; the declared `proto_file_name` only for products
    /// that are not in the table.
    pub(crate) fn detect(product_name: &str, proto_file_name: &str) -> Self {
        match product_name {
            "Osmo 360" | "DJI Osmo360" => Self::Oq101,
            "DJI Avata360" | "Osmo 360 II" | "Osmo 360 Air" => Self::Wa530,
            _ => {
                if proto_file_name.contains("oq101") || proto_file_name.contains("OQ101") {
                    Self::Oq101
                } else if proto_file_name.contains("wa530") || proto_file_name.contains("WA530") {
                    Self::Wa530
                } else {
                    Self::Wm169
                }
            }
        }
    }
}

/// Which video track the **master** lens — the one at yaw ≈ 0 — was recorded
/// to. The other lens took the other track.
///
/// Read out of the vendor player's own product table: it maps the product to an
/// internal id, and three of the 360 bodies are in the set whose two decoders
/// are bound the other way round from every other product. A model that is not
/// in the table takes the majority branch, which is also what a full-resolution
/// seam metric on the reference clip prefers.
pub(crate) fn master_video_track(product_name: &str) -> u32 {
    match product_name {
        "Osmo 360 II" | "Osmo 360 Air" | "DJI Avata360" => 0,
        _ => 1,
    }
}

/// One calibration slot of `PanoDewarpParams`, by what it is calibrated for.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Family {
    /// The per-unit refined calibration converged at infinity: the ladder's
    /// label-0 member (`fx` is linear in the label, and this is its intercept).
    NativeRefine,
    /// Refined, on the stitch-distance ladder, rung unlabelled.
    RefineFar,
    /// Refined, on the stitch-distance ladder, at labelled rung `.0`.
    RefineFarAt(f64),
    LensGuards,
    AboveWater,
    UnderWater,
    /// The factory calibration the refinement started from.
    Native,
}

impl Family {
    /// What this slot is, in terms nothing downstream has to know DJI to read.
    /// Every family has one — which of them is the default is a choice made
    /// per clip in [`default_families`], and whichever family that lands on is
    /// the rig's own projection rather than a variant.
    fn variant_kind(self) -> VariantKind {
        match self {
            Self::NativeRefine => VariantKind::ConvergenceLadder { nearness: Some(0.0) },
            Self::RefineFar => VariantKind::ConvergenceLadder { nearness: None },
            Self::RefineFarAt(l) => VariantKind::ConvergenceLadder { nearness: Some(l) },
            Self::LensGuards => VariantKind::Housing(Housing::LensGuard),
            Self::AboveWater => VariantKind::Housing(Housing::AboveWater),
            Self::UnderWater => VariantKind::Housing(Housing::UnderWater),
            Self::Native => VariantKind::Unrefined,
        }
    }
}

/// The default slot, in preference order, for the lens mode the clip states.
///
/// First choice is **rung 11 of the ladder** — what the vendor's own player
/// stitches the Osmo 360 from, read out of the running application rather
/// than guessed: hooked live, DJI Studio (Windows 1.0.0.28782) loaded
/// `native_refine_far_11` for both lenses of the reference clip, bit-exact,
/// and its rigid alignment then matches this crate's to the last bit
/// (`docs/360/DJI-STUDIO-RE.md` §12). The refined-at-infinity pair leaves the
/// far background 5.8 px short at the seam on that body; rung 11 registers it.
///
/// `extri_lens_mode` is a proto3 enum, so an absent field is the zero value.
/// A body that does not ship the labelled rung falls through to the
/// refined-at-infinity pair — the Osmo 360 II — and then to the unlabelled
/// ladder pair, which is what makes the Avata 360 work: it ships only that
/// one, so the fallback is that product's normal path rather than a corner
/// case. The housing families come first in their own modes, as before.
pub(crate) fn default_families(extri_lens_mode: i32) -> &'static [Family] {
    const NATIVE: &[Family] = &[Family::RefineFarAt(11.0), Family::NativeRefine, Family::RefineFar];
    const GUARDS: &[Family] = &[Family::LensGuards, Family::RefineFarAt(11.0), Family::NativeRefine, Family::RefineFar];
    const UNDER: &[Family] = &[Family::UnderWater, Family::RefineFarAt(11.0), Family::NativeRefine, Family::RefineFar];
    match extri_lens_mode {
        1 => GUARDS,
        2 => UNDER,
        _ => NATIVE,
    }
}

/// The schema-independent subset of a `DewarpParams` message. The generated
/// dialects declare the same field numbers under the same names, so this is a
/// copy rather than a conversion.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RawDewarp {
    pub fx: f64,
    pub fy: f64,
    pub cx: f64,
    pub cy: f64,
    /// `k1..k5`. `k6..k9` exist in the schema and are not read by the model.
    pub k: [f64; 5],
    /// Field 20. Not field 27 `tangent_coeff`: the vendor's own extractor reads
    /// only this one, and the two happen to carry the same values in the clips
    /// at hand, so reading the wrong one is silent until it is not.
    pub p: Vec<f64>,
    pub width: f64,
    pub height: f64,
    pub yaw: f64,
    pub pitch: f64,
    pub roll: f64,
    pub lens_model: f64,
    pub temperature: f64,
    pub temp_compen_enable: bool,
    pub temp_compen_k: f64,
    pub temp_compen_k_order: Vec<f64>,
    pub occlusion_pt_x: Vec<f64>,
    pub occlusion_pt_y: Vec<f64>,
}

/// Copy one dialect's `DewarpParams` into [`RawDewarp`]. A slot the camera left
/// empty has `fx == 0` and is not a calibration.
macro_rules! raw_dewarp {
    ($d:expr) => {{
        let d = $d;
        if d.fx == 0.0 {
            None
        } else {
            Some($crate::dji::rig::RawDewarp {
                fx: d.fx as f64,
                fy: d.fy as f64,
                cx: d.cx as f64,
                cy: d.cy as f64,
                k: [d.k1 as f64, d.k2 as f64, d.k3 as f64, d.k4 as f64, d.k5 as f64],
                p: d.p.iter().map(|v| *v as f64).collect(),
                width: d.width as f64,
                height: d.height as f64,
                yaw: d.yaw as f64,
                pitch: d.pitch as f64,
                roll: d.roll as f64,
                lens_model: d.lens_model as f64,
                temperature: d.temperature as f64,
                temp_compen_enable: d.temp_compen_enable,
                temp_compen_k: d.temp_compen_k as f64,
                temp_compen_k_order: d.temp_compen_k_order.iter().map(|v| *v as f64).collect(),
                occlusion_pt_x: d.occlusion_pt_x.iter().map(|v| *v as f64).collect(),
                occlusion_pt_y: d.occlusion_pt_y.iter().map(|v| *v as f64).collect(),
            })
        }
    }};
}
pub(crate) use raw_dewarp;

/// Pull the twelve `(family, slave, master)` pairs out of one dialect's
/// `PanoDewarpParams`.
///
/// The slot numbering is the vendor's enum + 1 and is stable across every
/// schema: even enum = slave, odd = master, so the two fields of a pair are
/// always adjacent with the slave first.
macro_rules! pano_pairs {
    ($p:expr) => {{
        let p = $p;
        let mut out: Vec<$crate::dji::rig::Pair> = Vec::with_capacity(12);
        macro_rules! pair {
            ($fam:expr, $slave:ident, $master:ident) => {
                out.push((
                    $fam,
                    p.$slave.as_ref().and_then(|d| $crate::dji::rig::raw_dewarp!(d)),
                    p.$master.as_ref().and_then(|d| $crate::dji::rig::raw_dewarp!(d)),
                ));
            };
        }
        use $crate::dji::rig::Family::*;
        pair!(NativeRefine, native_refine_slave, native_refine_master);
        pair!(RefineFar, native_refine_far_slave, native_refine_far_master);
        pair!(LensGuards, lens_guards_slave, lens_guards_master);
        pair!(AboveWater, water_proof_above_water_slave, water_proof_above_water_master);
        pair!(UnderWater, water_proof_under_water_slave, water_proof_under_water_master);
        pair!(Native, native_slave, native_master);
        pair!(RefineFarAt(7.0), native_refine_far_07_slave, native_refine_far_07_master);
        pair!(RefineFarAt(9.0), native_refine_far_09_slave, native_refine_far_09_master);
        pair!(RefineFarAt(11.0), native_refine_far_11_slave, native_refine_far_11_master);
        pair!(RefineFarAt(12.5), native_refine_far_12_5_slave, native_refine_far_12_5_master);
        pair!(RefineFarAt(14.0), native_refine_far_14_slave, native_refine_far_14_master);
        pair!(RefineFarAt(16.0), native_refine_far_16_slave, native_refine_far_16_master);
        out
    }};
}
pub(crate) use pano_pairs;

/// A calibration slot pair, with each half present only if the camera filled it.
pub(crate) type Pair = (Family, Option<RawDewarp>, Option<RawDewarp>);

/// Build the rig from the calibration pairs the clip populated.
///
/// `picture_dim` is the coded size of a video track; the calibration is
/// expressed in its own space, which equals the track for every 360 body seen
/// so far and is mapped onto the track when it does not.
pub(crate) fn camera_rig(
    product_name: &str,
    pairs: &[Pair],
    extri_lens_mode: i32,
    picture_dim: Option<(u32, u32)>,
    readout: Option<Readout>,
) -> Option<CameraRig> {
    let complete = |f: Family| -> Option<(&RawDewarp, &RawDewarp)> {
        pairs.iter().find(|(fam, _, _)| *fam == f).and_then(|(_, s, m)| Some((s.as_ref()?, m.as_ref()?)))
    };

    let master_track = master_video_track(product_name);
    let slave_track = 1 - master_track;
    let projection_for = |family, slave, master| {
        let mut projection = rig_projection(slave, slave_track, master, master_track, picture_dim, readout)?;
        if matches!(product_name, "Osmo 360" | "DJI Osmo360") {
            use body_masks::DjiMaskProfile;
            let profile = match family {
                Family::LensGuards => DjiMaskProfile::LensProtector,
                Family::AboveWater => DjiMaskProfile::AboveWater,
                Family::UnderWater => DjiMaskProfile::UnderWater,
                _ => DjiMaskProfile::Standard,
            };
            profile.apply(&mut projection);
        }
        Some(projection)
    };


    // A slot the camera filled with something that is not a calibration is
    // passed over exactly as an unfilled one is: the preference list is already
    // a fallback chain — it is what makes the Avata 360 work, that body
    // shipping only the nearer-stitch pair — and "populated but unreadable" is
    // the same answer to the same question. What it does not do is fall through
    // to a rig: when no family reads, there is none.
    let (default_family, projection) = default_families(extri_lens_mode).iter().find_map(|f| {
        let (slave, master) = complete(*f)?;
        Some((*f, projection_for(*f, slave, master)?))
    })?;

    let mut variants = Vec::new();
    for (family, s, m) in pairs {
        if *family == default_family {
            continue;
        }
        let (Some(s), Some(m)) = (s.as_ref(), m.as_ref()) else {
            continue;
        };
        let Some(projection) = projection_for(*family, s, m) else {
            continue;
        };
        variants.push(RigVariant { kind: family.variant_kind(), projection });
    }

    Some(CameraRig {
        brand: "DJI".into(),
        model: product_name.to_owned(),
        projection,
        // Index 0 is the first video track, index 1 the second; which lens each
        // holds is `Lens::picture`, set by the product-keyed binding above.
        pictures: vec![RigPicture::VideoTrack(0), RigPicture::VideoTrack(1)],
        variants,
        audio_source: None,
        siblings: Vec::new(),
    })
}

/// The two lenses in the vendor's own order — slave first, as the slot
/// numbering has them — each bound to the track it was recorded to.
fn rig_projection(
    slave: &RawDewarp,
    slave_track: u32,
    master: &RawDewarp,
    master_track: u32,
    picture_dim: Option<(u32, u32)>,
    readout: Option<Readout>,
) -> Option<Projection> {
    let projection = Projection::new(ProjectionKind::Rig {
        cameras: vec![
            lens(slave, slave_track, picture_dim, readout)?,
            lens(master, master_track, picture_dim, readout)?,
        ],
        blend: BlendMode::Feather,
        convergence_m: None,
    });
    Some(projection)
}

/// One `DewarpParams` as a [`Lens`].
///
/// The model is the equidistant θ-polynomial with a tangential pair:
///
/// ```text
/// θ_d = θ·(1 + k1θ² + k2θ⁴ + k3θ⁶ + k4θ⁸ + k5θ¹⁰)
/// x' = θ_d·X/ρ,  y' = θ_d·Y/ρ
/// xd = x' + 2p1·x'y' + p2(θ_d² + 2x'²),   yd = y' + p1(θ_d² + 2y'²) + 2p2·x'y'
/// u  = fx·xd + cx,  v = fy·yd + cy
/// ```
///
/// `xi` is not on this path — it belongs to the unified-sphere model the same
/// `lens_model` field selects with a different value, and it reads 0 here.
///
/// `None` when the packet does not describe a lens — see
/// [`checked_calib_dim`]. A lens is not the body mask: dropping the mask costs
/// a cosmetic detail of an otherwise correct picture, and dropping this costs
/// the rig, which is the point of dropping it. A rig built on a calibration
/// that is not one renders a wrong image and says nothing, where no rig at all
/// leaves the host to say so.
pub(crate) fn lens(
    d: &RawDewarp,
    video_track: u32,
    picture_dim: Option<(u32, u32)>,
    readout: Option<Readout>,
) -> Option<Lens> {
    let (k, p) = model_coefficients(d);

    let calib_dim = match checked_calib_dim(d, &k, &p, picture_dim) {
        Ok(dim) => dim,
        Err(quantity) => {
            log::warn!("Dropping the calibration for video track {video_track}: its {quantity} is not a usable value.");
            return None;
        }
    };
    let Some(validity) = validity_rad(d, &k) else {
        log::warn!("Dropping the calibration for video track {video_track}: its radius never reaches the frame's edge.");
        return None;
    };
    let calib_to_picture = match picture_dim {
        Some((w, h)) if (w, h) != (calib_dim[0], calib_dim[1]) => {
            Affine2::scale(f64::from(w) / d.width, f64::from(h) / d.height)
        }
        _ => Affine2::IDENTITY,
    };

    Some(Lens {
        picture: Picture::whole(video_track),
        intrinsics: Intrinsics {
            model: LensModel::ThetaPoly { k, p },
            calib_dim,
            focal: [d.fx, d.fy],
            centre: [d.cx, d.cy],
            skew: 0.0,
            thermal: thermal(d),
        },
        calib_to_picture,
        mirror: false,
        // DJI states each lens's pose twice in the same packet — as `cam_extri_q`
        // and as this euler triple — and the quaternion is the triple composed
        // `Ry(yaw)·Rz(roll)·Rx(pitch)` (to 2e-6° on the reference clip). It is
        // an extrinsic in the computer-vision sense, **camera from rig**, in a
        // frame whose up is `z`, so `pitch ≈ 90°` is a level lens; re-basing
        // the pitch onto the rig frame's `y`-down and inverting is what makes it
        // the `rig_from_cam` the descriptor carries.
        //
        // Both halves of that reading are load-bearing. Composing the same
        // triple `Rz·Ry·Rx` flips the sign of the back lens's roll, because a
        // half turn of yaw sits between the two orders; taking the result as
        // `rig_from_cam` without inverting flips every angle's sense. Each
        // costs the back-to-back pair over a degree of relative roll, which the
        // stitch renders as the two hemispheres sliding past each other
        // VERTICALLY at both seams — 40 px on a 4096-wide equirect of the Osmo
        // 360, 27 px on the Avata 360 — and only the inverted `Ry·Rz·Rx`
        // registers both products' seams to within a pixel and puts their two
        // optical axes on one line (179.92° apart, against 178.77° the other
        // way, on a body that is machined back to back).
        //
        // `checked_calib_dim` has already refused a non-finite yaw/pitch/roll,
        // so the composition here is a rotation and the normalisation only
        // removes the drift of three multiplications.
        rotation: Quat::from_euler_yzx(d.yaw.to_radians(), d.roll.to_radians(), (d.pitch - 90.0).to_radians())
            .inverse()
            .normalized()
            .expect("a finite euler triple composes to a rotation"),
        // Zero, and not an omission: this vendor bakes the stitching distance
        // into a focal ladder instead of stating where the lens sits.
        translation: [0.0; 3],
        fov_limit: Some(validity),
        // DJI states no image circle: the frame is the boundary, and the angle
        // above is where the calibration's radius reaches it.
        image_circle: None,
        masks: occlusion_mask(d).into_iter().collect(),
        // The vendor's blend, stated beside the validity and not derived from
        // it: two numbers from two places.
        blend: Blend { limit: BLEND_LIMIT_DEG.to_radians(), width: BLEND_WIDTH_DEG.to_radians() },
        photometric: None,
        // The body states ONE readout for the sensor and both lenses are read
        // by it, so the two cameras carry the same window.
        readout,
        eye: None,
    })
}

/// The calibration frame in whole pixels, once every quantity the lens is built
/// out of has been checked to be one a camera could have written. `Err` names
/// the first that is not, which is what the caller's warning says.
///
/// Everything here arrives as an IEEE float out of a vendor packet and nothing
/// on the wire says it is a real number, so this is where that is decided.
/// Leaving it to the shared descriptors is not the same thing twice: their
/// answer is `Err` for the ENTIRE rig — the projection, both lenses and both
/// pictures — where this one costs a single lens and names the quantity. The
/// orientation test carried a second reason that no longer holds and is
/// corrected here rather than left standing: a euler triple with a NaN in it
/// once produced the IDENTITY rotation, because `Quat::from_axis_angle`
/// answered a non-finite angle with it and `Quat::normalized` answered a
/// non-finite norm with it, so the lens came out pointing along the rig's
/// forward axis and validated cleanly. Both are loud now (`Quat::NAN`, and
/// `None`). And the frame size is divided by only when the calibration space
/// differs from the picture,
/// which it does not on any 360 body seen so far — so on those, a frame size
/// that is not one is silent all the way to the screen.
///
/// Every test is a positive statement of what the value has to be, negated once
/// at the front, because every comparison against a NaN is false whichever way
/// round it is put and only the negated form makes that a refusal.
fn checked_calib_dim(
    d: &RawDewarp,
    k: &[f64; 5],
    p: &[f64; 2],
    picture_dim: Option<(u32, u32)>,
) -> Result<[u32; 2], &'static str> {
    // Not `d.fx != 0.0`, which is what `raw_dewarp!` asks to tell a slot the
    // camera left empty from one it filled. That question has a different
    // answer: `NaN != 0.0` is true, so an empty-slot test passes a NaN through
    // as a populated calibration.
    if ![d.fx, d.fy].iter().all(|v| v.is_finite() && *v > 0.0) {
        return Err("focal length");
    }
    if ![d.cx, d.cy].iter().all(|v| v.is_finite()) {
        return Err("principal point");
    }
    if !k.iter().chain(p).all(|v| v.is_finite()) {
        return Err("distortion table");
    }
    if ![d.yaw, d.pitch, d.roll].iter().all(|v| v.is_finite()) {
        return Err("orientation");
    }
    // Rounded before the range is tested and not after, because a float cast to
    // `u32` saturates rather than wrapping, so the value that has to be in range
    // is the one being cast. The range is the whole test — false for a NaN, for
    // either infinity, for a negative or sub-pixel frame, and for one no `u32`
    // holds — and nothing is clamped to reach it: `f64::max` returns its
    // non-NaN operand, so a clamp written as one hands back its own bound for a
    // NaN, substituting a plausible-looking frame size for a value that was
    // never a frame size at all.
    let frame = [d.width.round(), d.height.round()];
    if !frame.iter().all(|v| (1.0..=f64::from(u32::MAX)).contains(v)) {
        return Err("calibration frame size");
    }
    // The coded picture is not part of the packet, but the map onto it is built
    // by dividing by the frame size, and a picture with a zero side collapses
    // that map instead of scaling it.
    if picture_dim.is_some_and(|(w, h)| w == 0 || h == 0) {
        return Err("coded picture size");
    }
    Ok([frame[0] as u32, frame[1] as u32])
}

/// The radial and tangential coefficients, with the vendor's own legacy fixup
/// applied.
///
/// When both tangential terms are zero the table is one written in the OpenCV
/// `[k1, k2, p1, p2, k3]` order, whose third and fourth entries landed in `k3`
/// and `k4`: DJI reinterprets them as the tangential pair and zeroes them.
fn model_coefficients(d: &RawDewarp) -> ([f64; 5], [f64; 2]) {
    let mut k = d.k;
    let mut p = [d.p.first().copied().unwrap_or(0.0), d.p.get(1).copied().unwrap_or(0.0)];
    if p[0] == 0.0 && p[1] == 0.0 {
        p = [k[2], k[3]];
        k[2] = 0.0;
        k[3] = 0.0;
    }
    (k, p)
}

/// The focal length's temperature model, when the camera measured one.
///
/// The multi-order coefficients win over the single one when the camera ships
/// them — that is the vendor's own precedence — and a body that ships neither,
/// or that clears the enable flag, has no model rather than an identity one.
///
/// This is the one quantity in the packet that degrades instead of costing the
/// lens, and that is what it means rather than a softer standard for it:
/// [`Intrinsics::focal`] is defined as the focal AT the reference temperature —
/// the calibrated value, unmodified — so a lens carrying no model renders at
/// its calibration temperature, which is what every camera that never measured
/// one does. That is a correct image, merely an uncorrected one, and it is a
/// reading a focal length that is not a number does not have.
fn thermal(d: &RawDewarp) -> Option<ThermalFocal> {
    if !d.temp_compen_enable {
        return None;
    }
    let k = match d.temp_compen_k_order.as_slice() {
        [k0, rest @ ..] if *k0 != 0.0 || rest.first().is_some_and(|k1| *k1 != 0.0) => {
            [*k0, rest.first().copied().unwrap_or(0.0)]
        }
        _ => [d.temp_compen_k, 0.0],
    };
    if k == [0.0, 0.0] {
        return None;
    }
    if ![k[0], k[1], d.temperature].iter().all(|v| v.is_finite()) {
        log::warn!("Dropping a temperature model of {k:?} at {} °C: it is not made of real numbers.", d.temperature);
        return None;
    }
    Some(ThermalFocal { reference_c: d.temperature, k })
}

/// Widest a body silhouette may sweep around the optical axis and still say
/// which side of itself it occludes.
///
/// The contour is closed by carrying its two ends away from the axis, so the
/// occluded side is the far one. Past a half turn that stops being a
/// statement: the closed curve then winds a whole turn about the principal
/// point and encloses it, and the even-odd fill comes out exactly inverted —
/// the body reads clear and the rest of the picture reads masked.
///
/// What is bounded is the contour's own **accumulated** turning about the
/// principal point, and not the separation of its two ends. The separation of
/// the ends is folded into `[0, π]` by construction, so it cannot report the
/// very case this bound exists for. The accumulated turn is exact rather than
/// approximate: the closing chord contributes `−wrap(S)` to the winding of the
/// closed curve, so the total is `S − wrap(S)`, which vanishes precisely when
/// `|S| ≤ π`. The gate is the invariant itself.
///
/// It catches that winding flip and nothing else. A contour that doubles back
/// over itself at another radius — out at −10°, round to 170°, back to −20°
/// further out — accumulates 10°, passes, and still self-intersects. Only an
/// O(n²) simplicity test would catch that one, which is not worth running on a
/// 13-point vendor table.
///
/// No clip in the corpus comes near the limit — the Osmo 360's body accumulates
/// 120° — and a table that did would be describing something other than a body.
const MAX_OCCLUSION_SWEEP_DEG: f64 = 170.0;

/// The camera body, which pokes into the bottom of both fisheye circles.
///
/// The vendor writes the silhouette as an **open contour** in calibration
/// pixels — 13 points marching across the bottom of the frame, at a very
/// nearly constant angle from the optical axis — prefixed with the one point
/// of it nearest the axis, which the contour already carries. Read as a closed
/// ring, that repeat pinches the ring into two lobes meeting at the repeated
/// vertex, and the even-odd rule then fills the two crescents between the
/// contour and the chords back to it: thin lens-shaped holes a few degrees
/// wide sitting in the middle of the picture, where the body is not, while the
/// body itself stays unmasked.
///
/// So the repeat is dropped, and the contour is closed where the body actually
/// is: on the far side of it from the optical axis. Both ends are carried
/// radially outward far enough that the segment joining them clears every
/// corner of the calibration frame, which needs nothing but the principal
/// point and holds whichever edge of the frame the body enters from.
///
/// Which repeat that is, is a question of **where** it sits. DJI's prefix
/// duplicates an interior vertex; a repeat in the last position instead means
/// a ring that already closes on its own first vertex, and such a ring is the
/// polygon already. Dropping its leading point rather than its trailing one
/// yields the same ring read from one vertex on, whose closing edge the
/// closure then replaces with a radial detour: the fill becomes ring XOR
/// wedge, the wedge as wide as the ring's vertex spacing and two frame
/// diagonals deep, either painting a stripe across the picture or cutting one
/// out of the body.
///
/// Nothing is emitted when the table cannot describe a body: fewer than three
/// points, an unfilled all-zero slot, a value that is not a real number, a
/// sweep past [`MAX_OCCLUSION_SWEEP_DEG`], or more points than the shared
/// descriptors carry. The last of those is a deliberate trade —
/// `occlusion_pt_x` is unbounded in the schema and the closure adds two, so a
/// long enough table would fail `MAX_MASK_POINTS` at validation instead, and
/// that takes the whole rig with it: the projection, both lenses and both
/// pictures, over one cosmetic mask on one of them.
fn occlusion_mask(d: &RawDewarp) -> Option<MaskPoly> {
    let n = d.occlusion_pt_x.len().min(d.occlusion_pt_y.len());
    if n < 3 {
        return None;
    }
    let mut points: Vec<[f64; 2]> = (0..n).map(|i| [d.occlusion_pt_x[i], d.occlusion_pt_y[i]]).collect();
    // An all-zero array is an unpopulated slot, not a polygon at the origin.
    if points.iter().all(|p| p[0] == 0.0 && p[1] == 0.0) {
        return None;
    }
    // Everything the closure is built out of has to be a real number, and this
    // is where that is decided. A NaN at an *interior* vertex never reaches the
    // sweep gate below and would ride out inside the polygon; an infinite
    // principal point turns the outward step into `inf/inf`. Either way the
    // only thing left to catch it is the geometry validator, which answers by
    // failing the entire rig.
    if !points.iter().flatten().chain([&d.cx, &d.cy, &d.width, &d.height]).all(|v| v.is_finite()) {
        return None;
    }

    // An interior repeat is the vendor's prefix and comes off the front; a
    // trailing one is a ring closing on itself and comes off the back, leaving
    // a polygon that must not then be closed a second time.
    let closed_ring = match points[1..].iter().position(|p| *p == points[0]) {
        Some(j) if j + 2 == points.len() => {
            points.pop();
            true
        }
        Some(_) => {
            points.remove(0);
            false
        }
        None => false,
    };

    if !closed_ring {
        let angle = |p: [f64; 2]| (p[1] - d.cy).atan2(p[0] - d.cx);
        let wrap = |a: f64| a - std::f64::consts::TAU * (a / std::f64::consts::TAU).round();
        let extent: f64 = points.windows(2).map(|w| wrap(angle(w[1]) - angle(w[0]))).sum();
        // Negated, so a contour that reduced to NaN above is refused too.
        if !(extent.abs() <= MAX_OCCLUSION_SWEEP_DEG.to_radians()) {
            return None;
        }
        let (first, last) = (*points.first()?, *points.last()?);
        // The joining segment runs between the outward images of the two ends,
        // so what sets its closest approach to the principal point is the ends'
        // own separation — `far·cos(sweep/2)` — whatever the contour did in
        // between. This puts it at twice the farthest corner of the frame.
        let sweep = {
            let raw = (angle(first) - angle(last)).abs();
            if raw > std::f64::consts::PI { std::f64::consts::TAU - raw } else { raw }
        };
        let far_x = d.cx.abs().max((d.width - d.cx).abs());
        let far_y = d.cy.abs().max((d.height - d.cy).abs());
        let far = 2.0 * far_x.hypot(far_y) / (0.5 * sweep).cos();
        let outward = |p: [f64; 2]| {
            let (dx, dy) = (p[0] - d.cx, p[1] - d.cy);
            let r = dx.hypot(dy);
            (r > 0.0).then(|| [d.cx + dx / r * far, d.cy + dy / r * far])
        };
        points.push(outward(last)?);
        points.push(outward(first)?);
    }

    if points.len() < 3 {
        return None;
    }
    if points.len() > MAX_MASK_POINTS as usize {
        log::warn!("Dropping a {}-point body mask: the geometry descriptors carry {MAX_MASK_POINTS}.", points.len());
        return None;
    }
    Some(MaskPoly { polarity: MaskPolarity::Exclude, points })
}
