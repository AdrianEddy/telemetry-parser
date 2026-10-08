// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2026 Adrian <adrian.eddy at gmail>

//! Normalising Insta360's `offset` strings into a [`CameraRig`].
//!
//! Everything Insta360-specific about the dual-lens bodies lives here: the four
//! generations of the underscore-separated calibration string, which of them to
//! prefer, the euler composition, the back-to-back half turn the string does
//! **not** contain, and which video track each lens was recorded to.
//! Downstream sees only the shared geometry descriptors.
//!
//! # The convergence sweep (L21, D9.11)
//!
//! **The translation is real and must not be zeroed.** oxivideo's
//! `tests/geometry_convergence_sweep.rs` renders each lens into its own
//! equirectangular layer and cross-correlates the two along the seam, over the
//! stitching distances `{∞, 10, 5, 2, 1, 0.5} m`. On an X5 (`‖t‖` = 31.90 mm,
//! two 3840² tracks) the residual parallax runs
//!
//! | `R` | ∞ | 10 m | 5 m | 2 m | 1 m | 0.5 m |
//! |---|---|---|---|---|---|---|
//! | disparity, 2048-wide layer px | +13.12 | +12.07 | +11.03 | +7.88 | +2.69 | −7.67 |
//!
//! — a straight line in `1/R` (worst rung 0.024 px off it) whose slope is
//! **−10.396 px per m⁻¹** against the **−10.397** that `‖t‖` and the layer's own
//! pixels-per-radian predict, i.e. **0.02 %**. So the plain path is NOT where
//! this body's hemispheres agree, and a finite convergence distance is what
//! brings them together: these intrinsics are not pre-compensated for a
//! reference distance, the sphere does not over-correct, and zeroing
//! [`Lens::translation`] (the DJI shape) would throw away a real 13-pixel
//! correction.
//!
//! **It does not pin a default `R`, and this module goes on stating
//! `convergence_m: None`.** The zero crossing is the CLIP's content, not the
//! camera's: 0.79 m on this clip, and within one X5 clip it runs 0.42 m over
//! the road below to 0.83 m over the horizon above. Two different X5 bodies,
//! two scenes and five months apart both read 0.79 m pooled, so the number is
//! stable enough to be worth reporting and content-driven enough not to be a
//! constant. The stitching distance is a host's slider, and the vendor states
//! none.

use std::f64::consts::{FRAC_PI_2, PI};

use video_types::{geometry::{
    Affine2, Blend, BlendMode, Circle, Intrinsics, Lens, LensModel, MeiDistortion, Picture, Projection, ProjectionKind,
    Quat, Readout,
}};

use crate::rig::{CameraRig, RigPicture, SiblingDir, SiblingHint, SiblingRole};

/// The generation of an `offset` string, taken from the **high half of its
/// packed tail** and never from the token count.
///
/// The tail is `(version << 16) | (X << 8)`, with the lens type in the low byte
/// for the oldest form. The token count corroborates — `27n + 2`, `19n + 2`,
/// `16n + 2` and `6n + 4` — but the vendor's own parser dispatches on the tail,
/// and only the tail distinguishes two generations that happen to agree on
/// length for some `n`.
///
/// There is no `V4`, `V5` or `V7` string: the numbering skips, and the `v8`
/// field that exists in the vendor's schema is written by nothing and read by
/// nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Version {
    /// `radius cx cy yaw pitch roll` per lens, with the frame and the lens type
    /// shared in the tail. No coefficients are stored; verified legacy types
    /// use the recovered built-in table, and unknown types supply only a circle.
    V1,
    /// V1 plus a translation and the four coefficients of an
    /// `r = θ·(c₀ + c₁θ + c₂θ² + c₃θ³)` radial polynomial.
    V2,
    /// Unified sphere (Mei) with an OpenCV-ordered Brown radial/tangential
    /// block.
    V3,
    /// Unified sphere with the 13-term extended radial/decentering/thin-prism
    /// block. The vendor's own first choice.
    V6,
}

impl Version {
    /// Tokens per lens. The trailing frame/flags tokens are separate, and V1
    /// keeps its frame outside the per-lens block.
    const fn fields_per_lens(self) -> usize {
        match self {
            Self::V1 => 6,
            Self::V2 => 16,
            Self::V3 => 19,
            Self::V6 => 27,
        }
    }
}

/// The `X` byte (bits 8..15 of the packed tail) that marks lens 0's image as
/// horizontally mirrored. Every other value seen in the corpus — 4 on most
/// bodies, 12 on the ONE X2 — leaves the image alone.
const MIRRORED_LENS_0: u8 = 5;

/// One lens's block of an `offset` string, in the string's own units:
/// **degrees** for the angles, **metres** for the translation, and the
/// **side-by-side** calibration frame for the pixel quantities.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct OffsetLens {
    /// The image radius at the lens's own field limit, V1/V2 only. The one
    /// thing the oldest strings state that the newer ones do not.
    pub radius: f64,
    /// The unified sphere's mirror parameter, V3/V6 only.
    pub xi: f64,
    /// `fx, fy` as the string states them (V3/V6) or as the radial polynomial
    /// implies them (V2 or a verified legacy V1 type). Zero for unknown V1 types.
    pub focal: [f64; 2],
    pub centre: [f64; 2],
    /// `yaw, pitch, roll`, degrees, in the order the string carries them.
    pub ypr: [f64; 3],
    /// Camera origin, metres.
    pub translation: [f64; 3],
    /// The distortion block as read: `c₀..c₃` (V2), `k1 k2 k3 p1 p2` (V3) or
    /// `d0..d12` (V6). Empty for V1.
    pub coeffs: Vec<f64>,
    /// The **whole** calibration frame, both lenses side by side.
    pub width: f64,
    pub height: f64,
    pub lens_type: i32,
}

/// A parsed `offset` string.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Offset {
    pub version: Version,
    /// Bits 8..15 of the packed tail. [`MIRRORED_LENS_0`] is the only value
    /// that changes any geometry.
    pub x: u8,
    pub lenses: Vec<OffsetLens>,
}

impl Offset {
    /// The calibration frame **one** lens occupies: the lenses sit side by side
    /// in one space and the string states its full width.
    fn lens_frame(&self) -> [f64; 2] {
        let first = &self.lenses[0];
        [first.width / self.lenses.len() as f64, first.height]
    }
}

/// Parse one `offset` string, or `None` when it is not one.
///
/// Every number is required to be finite here, at the boundary, and not merely
/// where it is used: `"nan"`, `"inf"` and `"-inf"` all parse successfully as
/// `f64`, so a string carrying one of them is a well-formed string describing
/// nothing, and the only place to say so is the parse.
pub(crate) fn parse(s: &str) -> Option<Offset> {
    let tokens: Vec<&str> = s.split('_').collect();
    let flags: i64 = tokens.last()?.trim().parse().ok()?;
    if flags < 0 {
        return None;
    }
    let version = match flags >> 16 {
        0 => Version::V1,
        2 => Version::V2,
        3 => Version::V3,
        6 => Version::V6,
        _ => return None,
    };
    let x = ((flags >> 8) & 0xff) as u8;
    let num: usize = tokens.first()?.trim().parse().ok()?;
    let per = version.fields_per_lens();
    // V1 states the frame once for the whole string and its lens type in the
    // tail's low byte; every later version repeats both per lens.
    let expected = num.checked_mul(per)?.checked_add(if version == Version::V1 { 4 } else { 2 })?;
    if num == 0 || tokens.len() != expected {
        return None;
    }
    let f = |i: usize| -> Option<f64> {
        let v: f64 = tokens.get(i)?.trim().parse().ok()?;
        v.is_finite().then_some(v)
    };

    let mut lenses = Vec::with_capacity(num);
    for i in 0..num {
        let b = 1 + i * per;
        let mut lens = OffsetLens::default();
        match version {
            Version::V1 => {
                lens.radius = f(b)?;
                lens.centre = [f(b + 1)?, f(b + 2)?];
                lens.ypr = [f(b + 3)?, f(b + 4)?, f(b + 5)?];
                lens.width = f(1 + num * per)?;
                lens.height = f(2 + num * per)?;
                lens.lens_type = (flags & 0xff) as i32;
                if let Some(LensModel::GoProPoly { p }) = lens_model(Version::V1, &lens) {
                    lens.focal = [p[0]; 2];
                }
            }
            Version::V2 => {
                lens.radius = f(b)?;
                lens.centre = [f(b + 1)?, f(b + 2)?];
                lens.ypr = [f(b + 3)?, f(b + 4)?, f(b + 5)?];
                lens.translation = [f(b + 6)?, f(b + 7)?, f(b + 8)?];
                lens.coeffs = (9..13).map(|k| f(b + k)).collect::<Option<Vec<f64>>>()?;
                lens.width = f(b + 13)?;
                lens.height = f(b + 14)?;
                lens.lens_type = f(b + 15)? as i32;
                // This generation states an image radius instead of a focal and
                // leaves the reader to divide. Doing it here keeps `focal` the
                // positive number every later stage reads, whichever generation
                // the rig ends up built from.
                lens.focal = [v2_focal(&lens)?; 2];
            }
            Version::V3 | Version::V6 => {
                let n_coeffs = if version == Version::V3 { 5 } else { 13 };
                lens.xi = f(b)?;
                lens.focal = [f(b + 1)?, f(b + 2)?];
                lens.centre = [f(b + 3)?, f(b + 4)?];
                lens.ypr = [f(b + 5)?, f(b + 6)?, f(b + 7)?];
                lens.translation = [f(b + 8)?, f(b + 9)?, f(b + 10)?];
                lens.coeffs = (11..11 + n_coeffs).map(|k| f(b + k)).collect::<Option<Vec<f64>>>()?;
                lens.width = f(b + 11 + n_coeffs)?;
                lens.height = f(b + 12 + n_coeffs)?;
                lens.lens_type = f(b + 13 + n_coeffs)? as i32;
            }
        }
        lenses.push(lens);
    }
    Some(Offset { version, x, lenses })
}

/// Half the field the vendor's lens-type table states — the lens CLASS: 100°
/// for type 113 (the X5's 200° lens), 95° for a type the table has no entry
/// for (its own 190° default).
///
/// `GetFov` is a jump table over the lens type; the X5 bare, guard and dive
/// entries are transcribed below, and 190° is the table's default for a type with no entry. It is the
/// angle the V1/V2 image radius is stated at and the base both limits below
/// are taken from — not itself a limit anything reads the picture to. The
/// vendor's dewarp-map builder even builds its map wider (`× 1.05`, a 105°
/// half-field, `createStaticDewarpMap` @ 0x17F54E4); what is READ from that
/// map is decided by the mask and the blend, which are the two statements
/// [`Lens`] carries.
fn class_half_field_deg(lens_type: i32) -> f64 {
    let full_deg = match lens_type {
        // Insta360Lens::GetFov (0x17f3d70), distinct from the mask library's
        // Lens::getFov. In particular 115/118/120 use the class default here.
        19 | 33 | 38 | 40 | 41 | 44 | 70 | 71 | 85 | 113 | 131 | 142 | 149 | 150 | 193 => 200.0,
        140 | 141 | 156 => 195.0,
        _ => 190.0,
    };
    full_deg / 2.0
}

/// [`class_half_field_deg`] in radians.
fn class_half_field_rad(lens_type: i32) -> f64 {
    class_half_field_deg(lens_type).to_radians()
}

/// How far from its axis a lens's picture is READ — image validity: the
/// vendor's own fisheye mask disc, `GetFov/2 − 2°` (98° for a 200° lens, 93°
/// for the 190° default).
///
/// Studio 6.0.2 masks every fisheye with `TemplateBlenderImpl::calcFisheyeMaskv`
/// (`libstudio_worker.dylib`, arm64, @ 0x2B4EBDC), whose default field is
/// `GetFov × 0.5 − 2.0` (@ 0x2B4ED7C): a disc no read of the picture ever
/// crosses, whatever the blend. An X5's picture is real to θ ≈ 101–102° at
/// every azimuth (measured on both bodies in the corpus), but what is read is
/// decided by the mask, and this is it. The per-body builders
/// (`calcFisheyeMaskONEX5AndProtector` @ 0x2B54FC8 and its siblings) layer
/// body-side polylines on the disc, including the plain X5. Recovered body outlines are
/// applied below; unknown bodies retain their circular validity limit.
fn mask_disc_rad(lens_type: i32) -> f64 {
    // Subtracted in degrees and converted once, so that on the 190° class —
    // where the disc and the blend's end are both 93° — the two convert to
    // the same bits and the blend is not a rounding error past the validity.
    (super::rig_mask_coefficients::full_fov(lens_type).unwrap_or(190.) / 2.0 - 2.0).to_radians()
}

/// The blend Studio applies to a two-lens body's video: a linear crossfade
/// this many degrees wide, centred on the seam meridians, so each lens is read
/// to `90° + 3°` and no further and the two cross at equal weight on the great
/// circle between them.
///
/// The number is the default block of `StitchingService::setupDynamicStitching`
/// @ 0xE0A7F0 (@ 0xE0BC9C): 6.0° for every body but four Studio special cases
/// keyed on its own camera classes AND stitch modes (`InstaCameraPC` 20° in
/// mode 10, `InstaCameraSphere` 4° in mode 21, `Akiko` / `OneRS` 1° in mode 22,
/// 3° in modes 7 and 8, the user's `custom_blend_angle` in mode 99) — Studio
/// settings, not facts about the clip, so the video default is what a clip
/// states. The alpha is `SeamlessBlenderImpl::getSphereAlpha` @ 0x2AB24AC: a
/// ramp of half the angle either side of the ±90° meridians.
///
/// `ExtraMetadata.blend_Angle` (proto field 128, `int32`) is deliberately NOT
/// read. Its accessor `InstaMetaData::GetBlendAngle` @ 0xB1A504 has no code
/// caller in the worker, and both paths into `RenderModelType::SetBlendAngleRad`
/// @ 0x46F750 — `setupDynamicStitching` above and the command-map case 6 of
/// `UpdateStitchTypeCommand::UpdateRenderModelType` @ 0xCD5434 — carry a
/// Studio setting, never the clip's field. It is 0 on every clip in the corpus
/// besides. The blender's rate is capped at `angle/16 ∈ [0, 1]` (@ 0x2A9D6E0),
/// so 16° — ending exactly on the 98° mask disc — is the widest Studio ever
/// blends; the default is well inside that.
const BLEND_DEG: f64 = 6.0;

/// One lens's blend. A two-lens body crossfades Studio's band about the seam.
/// A body with one lens has no seam to fade at: its picture is read to the
/// mask disc and cut there. (The multi-lens Pro bodies stitch through a
/// pipeline this module does not read; a rig of more than two lenses states
/// the same hard cut rather than a band centred where their seams are not.)
fn blend_of(lens_type: i32, num_lenses: usize) -> Blend {
    match num_lenses {
        // `90° + 3°` in degrees, converted once — see `mask_disc_rad`.
        2 => Blend { limit: (90.0 + BLEND_DEG / 2.0).to_radians(), width: BLEND_DEG.to_radians() },
        _ => Blend::hard((class_half_field_deg(lens_type) - 2.).to_radians()),
    }
}

/// `R = Ry(pitch + π/2)·Rz(yaw)·Rx(roll)`, degrees in, with the back-to-back
/// half turn the string does not contain. **The vendor's own frame**: `R` maps
/// a world vector into the camera.
///
/// The `+90°` is on **pitch**, about `Y` — not on roll, even though roll is the
/// field carrying ~90 in the data. And the 180° that separates the two lenses
/// is nowhere in the string: both lenses carry very nearly the same rotation
/// (about a degree apart), most of which is a fixed image→world permutation,
/// and the vendor's own map builder right-multiplies **lens 0** by a half turn
/// about `Z` when the body has two lenses. Without it the two optical axes come
/// out 1° apart instead of 180°.
fn vendor_rotation(ypr_deg: [f64; 3], lens_index: usize, num_lenses: usize) -> Quat {
    let [yaw, pitch, roll] = ypr_deg.map(f64::to_radians);
    let mut r =
        Quat::from_rotation_y(pitch + FRAC_PI_2).mul(Quat::from_rotation_z(yaw)).mul(Quat::from_rotation_x(roll));
    if lens_index == 0 && num_lenses == 2 {
        r = r.mul(Quat::from_rotation_z(PI));
    }
    // Unreachable: `checked_lens_frame` refuses a non-finite `ypr` before a
    // lens is built, and finite euler factors are unit quaternions.
    r.normalized().expect("a finite euler triple composes to a rotation")
}

/// `q_rig_from_cam` — the vendor's rotation carried into the frame the shared
/// descriptors define (`+x` right, `+y` down, `+z` forward).
///
/// Two changes, and the second is what makes the first mean anything:
///
/// * The stored matrix maps world **into** the camera, so the rig-from-camera
///   rotation is its inverse.
/// * The vendor's world frame is not this one. Nearly all of every stored
///   rotation is a fixed image→world permutation — the real calibration content
///   is a residual under 0.8° — and under it the lenses come out looking along
///   `±x`, which is a body whose forward axis points sideways. So the frame is
///   pinned by the string's OWN identity pose: the rotation a lens with
///   `yaw = pitch = 0, roll = 90` would have is taken as the rig's forward
///   direction, which sends the first lens along `+z` with its picture upright
///   and leaves every measured residual exactly where the vendor put it.
///   Stitching is invariant to this — it is a global rotation of both lenses —
///   but a default view is not.
fn rotation(ypr_deg: [f64; 3], lens_index: usize, num_lenses: usize) -> Quat {
    // Lens 0's nominal pose, whichever lens is being placed: it is the FIRST
    // lens that defines where the body looks.
    let base = vendor_rotation([0.0, 0.0, 90.0], 0, num_lenses);
    base.mul(vendor_rotation(ypr_deg, lens_index, num_lenses).inverse())
        .normalized()
        .expect("a quotient of two rotations is a rotation")
}

/// The calibration frame of one lens in whole pixels, once every quantity the
/// lens is built out of has been checked to be one a camera could have written.
/// `Err` names the first that is not.
///
/// The parse already refused a non-finite token, so nothing here can be reached
/// from a real string — which is the point: it is reachable from a
/// [`OffsetLens`] built any other way, and it is the only thing between a
/// degenerate value and the shared descriptors, which answer `Err` for the
/// ENTIRE rig rather than for one lens.
///
/// The `orientation` test used to be load-bearing for a second reason that no
/// longer holds, and the correction is worth stating because it read as a
/// standing hazard: a euler triple with a NaN in it once produced the IDENTITY
/// rotation — `Quat::from_axis_angle` answered a non-finite angle with it, and
/// `Quat::normalized` answered a non-finite norm with it — so the lens came out
/// pointing along the rig's forward axis and **validated cleanly**. Both now
/// answer loudly (`Quat::NAN`, and `None`), so `Projection::validate` would
/// refuse such a lens on its own. This test is kept because it still names the
/// quantity in the log, one lens at a time, which a rig-wide refusal cannot.
///
/// Every test is a positive statement of what the value has to be, negated once
/// at the front, because every comparison against a NaN is false whichever way
/// round it is put and only the negated form makes that a refusal.
fn checked_lens_frame(lens: &OffsetLens, frame: [f64; 2], picture_dim: (u32, u32)) -> Result<[u32; 2], &'static str> {
    if ![lens.focal[0], lens.focal[1]].iter().all(|v| v.is_finite() && *v > 0.0) {
        return Err("focal length");
    }
    if !lens.centre.iter().all(|v| v.is_finite()) {
        return Err("principal point");
    }
    if !lens.coeffs.iter().chain(&[lens.xi]).all(|v| v.is_finite()) {
        return Err("distortion table");
    }
    if !lens.ypr.iter().all(|v| v.is_finite()) {
        return Err("orientation");
    }
    if !lens.translation.iter().all(|v| v.is_finite()) {
        return Err("translation");
    }
    // Rounded before the range is tested and not after, because a float cast to
    // `u32` saturates rather than wrapping, so the value that has to be in range
    // is the one being cast. The range is the whole test — false for a NaN, for
    // either infinity, for a negative or sub-pixel frame, and for one no `u32`
    // holds — and nothing is clamped to reach it: `f64::max` returns its
    // non-NaN operand, so a clamp written as one hands back its own bound for a
    // NaN, substituting a plausible-looking frame size for one that never was.
    let rounded = [frame[0].round(), frame[1].round()];
    if !rounded.iter().all(|v| (1.0..=f64::from(u32::MAX)).contains(v)) {
        return Err("calibration frame size");
    }
    // The map onto the coded picture is built by dividing by the frame width,
    // and a picture with a zero side collapses it instead of scaling it.
    if picture_dim.0 == 0 || picture_dim.1 == 0 {
        return Err("coded picture size");
    }
    Ok([rounded[0] as u32, rounded[1] as u32])
}

/// One lens block as a [`Lens`], with `circle` from the V1 string when the body
/// wrote one.
///
/// `None` when the block does not describe a lens — see [`checked_lens_frame`].
/// A rig built on a calibration that is not one renders a wrong image and says
/// nothing, where no rig at all leaves the host to say so.
fn lens(
    offset: &Offset,
    index: usize,
    input: u32,
    circle: Option<&OffsetLens>,
    picture_dim: (u32, u32),
    readout: Option<Readout>,
) -> Option<Lens> {
    let raw = &offset.lenses[index];
    let frame = offset.lens_frame();
    let model = lens_model(offset.version, raw)?;

    let calib_dim = match checked_lens_frame(raw, frame, picture_dim) {
        Ok(dim) => dim,
        Err(quantity) => {
            log::warn!("Dropping the calibration for lens {index}: its {quantity} is not a usable value.");
            return None;
        }
    };
    // The two lenses share one side-by-side calibration space, so lens 1's
    // principal point is stated a whole frame to the right of its own.
    let offset_x = frame[0] * index as f64;
    let centre = [raw.centre[0] - offset_x, raw.centre[1]];

    // Uniform, and taken from the width: the calibration space and the coded
    // picture have the same aspect on every body in the corpus, and a
    // non-uniform map would turn the image circle into an ellipse in a space
    // where everything — the circle, the masks, the mirror — is evaluated
    // before it.
    let scale = f64::from(picture_dim.0) / f64::from(calib_dim[0]);

    Some(Lens {
        picture: Picture::whole(input),
        intrinsics: Intrinsics {
            model,
            calib_dim,
            focal: raw.focal,
            centre,
            skew: 0.0,
            // Insta360 measures no temperature model. `focal` is the focal at
            // the calibration temperature, which is what a camera that never
            // measured one renders at.
            thermal: None,
        },
        calib_to_picture: Affine2::scale(scale, scale),
        // An explicit calibration-space fact and never a negative `fx`: the
        // vendor gates it on lens 0 of a two-lens body with the tail's `X` byte
        // at 5, and applies it about the principal point before the map onto
        // the picture.
        mirror: index == 0 && offset.lenses.len() == 2 && offset.x == MIRRORED_LENS_0,
        rotation: rotation(raw.ypr, index, offset.lenses.len()),
        translation: raw.translation,
        fov_limit: Some(if offset.lenses.len() == 2 {
            mask_disc_rad(raw.lens_type)
        } else {
            (class_half_field_deg(raw.lens_type) - 2.).to_radians()
        }),
        image_circle: circle.and_then(|v1| {
            let centre = [v1.centre[0] - offset_x, v1.centre[1]];
            (v1.radius.is_finite() && v1.radius > 0.0 && centre.iter().all(|c| c.is_finite()))
                .then_some(Circle { centre, radius: v1.radius })
        }),
        masks: Vec::new(),
        blend: blend_of(raw.lens_type, offset.lenses.len()),
        photometric: None,
        // One sensor readout, stated once for the body and shared by its lenses.
        readout,
        eye: None,
    })
}

/// The lens model one block describes, or `None` when it describes none.
///
/// V1 carries no coefficients. The verified legacy types use Studio's built-in
/// degree polynomial; other V1 types remain unrenderable. V2+ carries calibration.
fn lens_model(version: Version, raw: &OffsetLens) -> Option<LensModel> {
    match version {
        Version::V1 => {
            // Verified against both GetDegreeCoeffs (0x2e368b0) and getCoeff
            // (0x4c5e48). Modern types have divergent tables and remain V2+.
            if !matches!(raw.lens_type, 19 | 27 | 33 | 38..=43 | 51 | 70 | 71 | 83) {
                return None;
            }
            let c = if raw.lens_type == 42 {
                // The X2 guard's OPTICAL table differs from its MASK table.
                [0., f64::from_bits(0x3f98ffd600528d4d), f64::from_bits(0xbf146c1519e1910d),
                 f64::from_bits(0x3ec253274af6d59f), f64::from_bits(0xbe5074b495b64050)]
            } else { super::rig_mask_coefficients::polynomial(raw.lens_type)? };
            if c[0] != 0. || !(raw.radius.is_finite() && raw.radius > 0.) { return None; }
            let theta = class_half_field_deg(raw.lens_type);
            let denominator = c.into_iter().rev().fold(0., |acc, v| acc * theta + v);
            if !(denominator.is_finite() && denominator > 0.) { return None; }
            let scale = raw.radius / denominator;
            let degrees = 180. / std::f64::consts::PI;
            Some(LensModel::GoProPoly { p: [scale*c[1]*degrees, scale*c[2]*degrees.powi(2),
                scale*c[3]*degrees.powi(3), scale*c[4]*degrees.powi(4), 0.] })
        },
        Version::V2 => {
            // `r = θ·(c₀ + c₁θ + c₂θ² + c₃θ³)` normalised, `u = cx + fx·r·cos φ`,
            // with `fx = fy = radius / r(θ_max)` — the vendor derives the focal
            // from the stated image radius rather than storing one. Folding the
            // focal into the coefficients turns the pair into the `p₀θ + p₁θ² +
            // …` polynomial in calibration pixels that the shared descriptors
            // already carry, which is the same map with nothing left over.
            let c: [f64; 4] = raw.coeffs.get(..4)?.try_into().ok()?;
            let theta_max = class_half_field_rad(raw.lens_type);
            let r = theta_max * (c[0] + c[1] * theta_max + c[2] * theta_max.powi(2) + c[3] * theta_max.powi(3));
            if !(r.is_finite() && r > 0.0 && raw.radius.is_finite() && raw.radius > 0.0) {
                return None;
            }
            let f = raw.radius / r;
            Some(LensModel::GoProPoly { p: [f * c[0], f * c[1], f * c[2], f * c[3], 0.0] })
        }
        Version::V3 => {
            let c: [f64; 5] = raw.coeffs.get(..5)?.try_into().ok()?;
            Some(LensModel::Mei { xi: raw.xi, dist: MeiDistortion::BrownV3 { k: [c[0], c[1], c[2]], p: [c[3], c[4]] } })
        }
        Version::V6 => {
            let d: [f64; 13] = raw.coeffs.get(..13)?.try_into().ok()?;
            Some(LensModel::Mei { xi: raw.xi, dist: MeiDistortion::RadtanPro { d } })
        }
    }
}

/// The focal a V2 block implies. `p[0]` of the folded polynomial is `f·c₀`,
/// which is `dr/dθ` at the optical axis — the paraxial focal, in calibration
/// pixels, and the positive number the shared descriptors require even from a
/// model that does not read one.
fn v2_focal(raw: &OffsetLens) -> Option<f64> {
    let LensModel::GoProPoly { p } = lens_model(Version::V2, raw)? else { return None };
    (p[0].is_finite() && p[0] > 0.0).then_some(p[0])
}

/// The vendor's own preference order over the generations it ships.
///
/// v6 first, then v3, v2 and v1 — the hard-coded `{6, 3, 2, 1}` the vendor's
/// player resolves with. The clip's own `capture_offset_version` is advisory
/// and is deliberately not consulted: it is an enum ordinal rather than a
/// version number, and the player ignores it too.
pub(crate) const PREFERENCE: [Version; 4] = [Version::V6, Version::V3, Version::V2, Version::V1];

/// Every `offset` string a clip carries, in the order [`PREFERENCE`] wants them.
#[derive(Clone, Debug, Default)]
pub(crate) struct Offsets {
    pub v1: Option<Offset>,
    pub v2: Option<Offset>,
    pub v3: Option<Offset>,
    pub v6: Option<Offset>,
}

impl Offsets {
    pub(crate) fn get(&self, version: Version) -> Option<&Offset> {
        match version {
            Version::V1 => self.v1.as_ref(),
            Version::V2 => self.v2.as_ref(),
            Version::V3 => self.v3.as_ref(),
            Version::V6 => self.v6.as_ref(),
        }
    }
    pub(crate) fn set(&mut self, offset: Offset) {
        let version = offset.version;
        *match version {
            Version::V1 => &mut self.v1,
            Version::V2 => &mut self.v2,
            Version::V3 => &mut self.v3,
            Version::V6 => &mut self.v6,
        } = Some(offset);
    }
    /// The string a rig is built from: the first the vendor prefers that
    /// describes a lens and not merely a circle.
    fn best(&self) -> Option<&Offset> {
        PREFERENCE.iter().filter_map(|v| self.get(*v)).find(|o| o.lenses.iter().all(|l| lens_model(o.version, l).is_some()))
    }
}

/// How the camera spread its lenses over files and tracks, as the clip states
/// it (`ExtraMetadata.stream_type`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum StreamLayout {
    /// One video track per lens, in lens order. Also the answer for a body that
    /// states nothing, which is every one before the X-series.
    TrackOrder,
    /// One video track per lens, in the opposite order — what the X5 declares.
    ///
    /// The enum VALUE is the vendor's own (`DUAL_STREAM_TRACK_REVERSE = 4`,
    /// read out of its embedded schema); what "reverse" does to the binding is
    /// the reading it plainly states, and it has not been confirmed against a
    /// front/back picture. On a near-symmetric back-to-back rig the difference
    /// is a swapped hemisphere plus each lens's sub-degree correction applied
    /// to the other one — visible in a stitch, not in a descriptor.
    TrackReversed,
    /// One lens per file.
    PerFile,
}

impl StreamLayout {
    pub(crate) fn from_stream_type(stream_type: i32) -> Self {
        match stream_type {
            2 => Self::PerFile,
            4 => Self::TrackReversed,
            _ => Self::TrackOrder,
        }
    }
}

/// The other file of a two-file body, named from the group the clip declares.
///
/// The camera writes `…_00_<seq>` for the first member and `…_10_<seq>` for the
/// second, and `file_group_info.identify` carries this file's own camera path.
/// Only a group of exactly two is described: that is the only shape the vendor's
/// two-file mode produces, and it is the only one whose member names follow from
/// the convention rather than from a guess.
pub(crate) fn sibling_hint(index: u32, total: u32, identify: &str) -> Option<SiblingHint> {
    if total != 2 || index > 1 {
        return None;
    }
    let other = 1 - index;
    let name = identify.rsplit(['/', '\\']).next()?;
    let mut parts: Vec<String> = name.split('_').map(str::to_owned).collect();
    // `<prefix>_<date>_<time>_<lens><lens>_<seq>.<ext>` — the lens field is the
    // one before the sequence number, and it is two digits.
    let lens_field = parts.len().checked_sub(2)?;
    if parts[lens_field].len() != 2 || !parts[lens_field].bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    parts[lens_field] = format!("{other}0");
    Some(SiblingHint {
        role: if other == 0 { SiblingRole::Front } else { SiblingRole::Back },
        directory: SiblingDir::Same,
        file_name: parts.join("_"),
    })
}

/// Build the rig from the `offset` strings the clip carries.
///
/// `picture_dim` is the coded size of **one** video track, which is what the
/// calibration space is mapped onto.
pub(crate) fn camera_rig(
    model: &str,
    offsets: &Offsets,
    layout: StreamLayout,
    sibling: Option<SiblingHint>,
    picture_dim: (u32, u32),
    bullet_time: bool,
    readout: Option<Readout>,
) -> Option<CameraRig> {
    let chosen = offsets.best()?;
    let num = chosen.lenses.len();
    // The oldest string is the only one that states an image radius, so it is
    // read for the circle even when a newer one supplies the model — but only
    // when the two describe the same calibration space, or the circle would be
    // a radius in somebody else's pixels.
    let circles = offsets.v1.as_ref().filter(|v1| {
        v1.lenses.len() == num && v1.lenses[0].width == chosen.lenses[0].width && v1.lenses[0].height == chosen.lenses[0].height
    });

    // Two files, one lens each, is a layout the camera declares; a file group
    // of two is NOT the same statement — a split or looped recording is also a
    // group of two — so the sibling hint is gated on the layout and not on the
    // group.
    let two_files = layout == StreamLayout::PerFile;
    let mut cameras: Vec<Lens> = (0..num)
        .map(|i| {
            let input = match layout {
                StreamLayout::TrackReversed => (num - 1 - i) as u32,
                _ => i as u32,
            };
            lens(chosen, i, input, circles.map(|v1| &v1.lenses[i]), picture_dim, readout)
        })
        .collect::<Option<Vec<Lens>>>()?;
    super::rig_body_masks::automatic(model, &mut cameras,
        &chosen.lenses.iter().map(|l| l.lens_type).collect::<Vec<_>>(), bullet_time);

    let pictures = if two_files && num == 2 {
        vec![RigPicture::VideoTrack(0), RigPicture::Sibling { role: SiblingRole::Back, video_track: 0 }]
    } else {
        (0..num as u32).map(RigPicture::VideoTrack).collect()
    };

    Some(CameraRig {
        brand: "Insta360".into(),
        model: model.trim_start_matches("Insta360 ").to_owned(),
        projection: Projection::new(ProjectionKind::Rig {
            cameras,
            blend: BlendMode::Feather,
            convergence_m: None,
        }),
        pictures,
        // The generations are the same body described four ways rather than
        // four geometries the camera ships, so none of them is an alternative
        // to the others. The vendor's real alternatives are the housing
        // conversions, which are baked into the string in place and announced
        // by `offset_convert_states` — no clip in the corpus carries one.
        variants: Vec::new(),
        audio_source: None,
        siblings: sibling.filter(|_| two_files).into_iter().collect(),
    })
}
