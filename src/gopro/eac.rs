// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2026 Adrian <adrian.eddy at gmail>

//! GoPro's split equi-angular cubemap — the `.360` file of a Max or a Max 2.
//!
//! The camera ships the sphere already dewarped, as two video tracks each
//! holding three cube faces side by side. There is no lens to describe: the
//! whole geometry is the `PRJT` fourcc that names the projection and the `PMOD`
//! triple that says how wide a face is stored.
//!
//! Side-face descriptors cover WHOLE stored cells: `[0, post)`,
//! `[post, post + height)`, `[post + height, width)`. A side cell contains
//! two registrations meeting at `z = 0`, each extending `overlap` capture
//! texels past the cut. The renderer reconstructs both and blends the overlap;
//! insetting to one core loses a registration and introduces cross-track tears.
//!
//! Max: cells 1376 / 1344 / 1376; Max 2: 2016 / 1920 / 2016.
//! The doubled band's stored width is `2 * overlap * post / pre`.
//! `pre == height + 2 * overlap` and `width == 2 * post + height` are
//! checked below. Face order and orientation are pinned by
//! `pixelgraph-spherical-ops/tests/reproject_eac_max_continuity.rs` on the
//! camera's frame 30. Earlier inset-core measurements do not describe this
//! implementation and cannot attribute the resulting tears to the camera.

use video_types::{geometry::{CubeFace, Face, FaceOrientation, Picture, Projection, ProjectionKind, Rect}};

use crate::rig::{CameraRig, RigPicture};

use super::udta::{self, Device};

/// The projection fourcc the camera writes at `PRJT`, in the vendor's own
/// numbering:
///
/// | value | fourcc | meaning |
/// |---|---|---|
/// | 2 | `ERP0` | equirectangular |
/// | 3 | `EAC0` | single equi-angular cubemap |
/// | 4 / 5 | `EACT` / `EACB` | split EAC, top- or bottom-first |
/// | **6** | **`EACO`** | **split OVERLAPPING equi-angular cubemap** |
/// | 7 / 8 / 9 | `PEAC` / `PEAT` / `PEAB` | padded EAC |
/// | 10 / 11 | `FSHF` / `FSHB` | single fisheye, front or back |
/// | 12 / 13 | `FSFB` / `FSBF` | dual fisheye |
/// | 14 / 15 | `PANO` / `TINY` | panorama, tiny planet |
///
/// Only `EACO` has a camera in the corpus, so only `EACO` is built. The rest
/// are refused by name rather than approximated by the nearest one that is:
/// a `.360` rendered as the wrong member of this list is a plausible-looking
/// picture of somewhere else.
const OVERLAPPING_EAC: &str = "EACO";

/// The `.360` geometry, or `None` when this clip is not one.
///
/// `picture` is the coded size of ONE video track — both strips are the same
/// size, and the face windows are laid out across it.
pub(crate) fn camera_rig(devices: &[Device], picture: (u32, u32)) -> Option<CameraRig> {
    let global = udta::device(devices, "Global Settings")?;
    let projection = udta::text(global, b"PRJT")?;
    if projection != OVERLAPPING_EAC {
        log::warn!("This GoPro clip declares projection {projection:?}, which this build does not render.");
        return None;
    }
    // `[overlap, pre, post]` on the Max; the Max 2 appends a fourth value whose
    // meaning is unknown and which nothing here needs.
    let pmod = udta::u32s(global, b"PMOD")?;
    let (&overlap, &pre_scale, &post_scale) = (pmod.first()?, pmod.get(1)?, pmod.get(2)?);
    let model = udta::text(global, b"MINF").filter(|m| !m.is_empty())?;

    let faces = face_windows(picture, overlap, pre_scale, post_scale)?;
    Some(CameraRig {
        brand: "GoPro".into(),
        model,
        projection: Projection::new(ProjectionKind::Eac { faces, overlap, pre_scale, post_scale }),
        pictures: vec![RigPicture::VideoTrack(0), RigPicture::VideoTrack(1)],
        variants: Vec::new(),
        audio_source: None,
        siblings: Vec::new(),
    })
}

/// The six face windows of the two strips.
///
/// # The layout
///
/// A strip is exactly three cells across — `post ┊ H ┊ post` — which is the
/// camera's own arithmetic: 1376 + 1344 + 1376 = 4096 on the Max and
/// 2016 + 1920 + 2016 = 5952 on the Max 2. A side cell stores two
/// registrations of its angular core, joined across a doubled overlap band.
/// `pre = H + 2 * overlap` is squeezed to `post`; the angular core therefore
/// spans `H * post / pre` stored pixels. That scale belongs to sampling inside
/// the whole cell, not to the descriptor's window. Max 2 has `pre == post`,
/// so the scale is one; its side cells are still wider than the centre.
///
/// The `2·post + H == width` identity is required rather than assumed: it is
/// the whole of what says this clip is laid out the way the two bodies in the
/// corpus are, and a mode that is not would otherwise be sampled with windows
/// that fit and mean nothing.
///
/// # The face table
///
/// Top strip `[−X │ +Z │ +X]` upright, bottom strip `[+Y │ −Z │ −Y]` at one
/// clockwise quarter turn. The vendor's own description of the same table reads
/// "the poles at `(v, 1−u)` and the back face at `(1−v, u)` — opposite senses",
/// plus a `1 − fu` on the `±X` faces; all three of those are artefacts of the
/// vendor shader's face basis and disappear in the one the renderer uses, where
/// the whole bottom strip is one quarter turn and nothing is mirrored. That
/// re-expression is not asserted here — it is pinned by the renderer's own
/// round trip of this exact table through an independent equirect oracle.
fn face_windows(picture: (u32, u32), overlap: u32, pre: u32, post: u32) -> Option<[CubeFace; 6]> {
    let (width, height) = picture;
    if pre == 0 || post == 0 || height == 0 {
        return None;
    }
    // A side cell stores its face TWICE: back-registered content on its first
    // half and front-registered on its second, cut on the `z = 0` plane at the
    // cell's exact middle, each half carrying `overlap` texels past the cut.
    // Measured on the camera's own pixels — the only lag that correlates across
    // a side cell is `2·overlap` (96 px on a Max 2, ZNCC 0.74/0.78/0.60; 62 px
    // on a Max, 0.86/0.88), and GoPro Player's own shaders read it as two
    // half-cubes melded through a Laplacian pyramid across the blurred cut.
    //
    // So the rect is the WHOLE cell, and `overlap` describes the crossfade the
    // consumer runs across the cut rather than a margin to inset here. An inset
    // window keeps ONE registration and puts every direction past the cut
    // `2·overlap` — 4.3° on a Max, 4.5° on a Max 2 — from where the
    // neighbouring centre face has it.
    // `2·overlap` scaled into stored texels is the width of the doubled band,
    // not a margin to inset — but it still has to FIT inside the cell, so the
    // bound stays as a validation. In `u64` throughout: `overlap · post`
    // overflows `u32` well inside the range the fields can hold, and a wrapped
    // product would pass every bound below.
    let (pre, post, height, width) = (u64::from(pre), u64::from(post), u64::from(height), u64::from(width));
    let band = (2 * u64::from(overlap) * post + pre / 2) / pre;
    // The pre-squeeze cell IS the face plus both overlaps — 1344 + 2·32 = 1408
    // on a Max, 1920 + 2·48 = 2016 on a Max 2 — which is what makes the stored
    // cell exactly `post` wide. That is the vendor's own definition, not an
    // inference from those two bodies: `GoPro.Harmony.ProjectionData` names
    // `PMOD[1]` `facePlusOverlapPreScale` and `PMOD[2]` `facePlusOverlapPostScale`,
    // and the Player's `split_eac2halfcube` derives its window as
    // `s = H·post/(W·pre)`, `o = overlap·post/(W·pre)`, which spans the stored
    // cell only under this relation.
    //
    // Refusing is STRICTER than the vendor — the Player never checks it and
    // would play such a file with a window that is not the cell — but a file
    // violating it describes no cell at all, and the field name says it cannot.
    if pre != height + 2 * u64::from(overlap) {
        log::warn!("A {pre}-wide cell is not a {height} face with two {overlap} overlaps.");
        return None;
    }
    if 2 * post + height != width || band >= post {
        log::warn!(
            "A {width}×{height} strip is not three {post}/{height}/{post} cells holding a {band}-pixel doubled band."
        );
        return None;
    }
    let cell = |left: u64, w: u64| Rect { x: left as u32, y: 0, w: w as u32, h: height as u32 };
    let row = [cell(0, post), cell(post, height), cell(post + height, post)];

    let upright = FaceOrientation::UPRIGHT;
    let turned = FaceOrientation::new(1, false);
    let mut faces = [Face::NegX, Face::PosZ, Face::PosX, Face::PosY, Face::NegZ, Face::NegY].into_iter();
    let mut out = Vec::with_capacity(6);
    for (input, orient) in [(0, upright), (1, turned)] {
        for rect in row {
            out.push(CubeFace { which: faces.next()?, picture: Picture { input, rect: Some(rect) }, orient });
        }
    }
    out.try_into().ok()
}
