// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2026 Adrian <adrian.eddy at gmail>

//! What a multi-camera body's pictures mean geometrically, normalised out of a
//! vendor's own calibration block into the shared descriptors.

use serde::{Deserialize, Serialize};

pub use video_types::geometry;

use geometry::{BlendMode, Projection, ProjectionKind};

pub use crate::insta360::extra_info::OffsetState;
pub use crate::insta360::extra_info::extra_metadata::{CoolingShellAlgoDetectedState, CoolingShellSelectedState};

/// A vendor's stated frame readout as the shared descriptor's [`geometry::Readout`].
///
/// The **sign carries the direction**. That is the convention every producer of
/// this number already writes and no vendor in the corpus states separately: a
/// negative readout is a sensor read from the bottom up, a positive one from
/// the top down. A body that reads across the frame instead states its
/// direction in its own tag and does not reach here.
///
/// `None` for a body that states no readout, and for a stated zero: a global
/// shutter has no row to correct, and answering `None` costs a consumer one
/// branch where a zero would cost it a division.
#[must_use]
pub fn readout(frame_readout_time_ms: Option<f64>) -> Option<geometry::Readout> {
    let ms = frame_readout_time_ms?;
    if !ms.is_finite() || ms == 0.0 {
        return None;
    }
    Some(geometry::Readout {
        time_us: ms.abs() * 1000.0,
        direction: if ms < 0.0 { geometry::ReadoutDirection::BottomUp } else { geometry::ReadoutDirection::TopDown },
    })
}

/// A camera body's cameras and their geometry, as the file states them.
///
/// Only what the file stores, read correctly: every calibration it carries,
/// each lens's own field and the masks the file itself draws, and the facts a
/// vendor's software reads to decide how to stitch ([`vendor`](Self::vendor)).
/// What that software then decides — its blend, its masks, which calibration
/// it stitches with, an accessory's conversion — is not a fact of the file and
/// is not stated here: a lens's [`blend`](geometry::Lens::blend) is a hard cut
/// at its own field ("no vendor blend known") and its masks are the file's.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CameraRig {
    pub brand: String,
    pub model: String,
    /// The calibration the file states for the body as it was recorded — what
    /// it is calibrated for is [`projection_kind`](Self::projection_kind).
    /// `Picture.input` indexes [`pictures`](Self::pictures).
    pub projection: Projection,
    /// Which decoded picture each `Picture.input` names, in index order.
    pub pictures: Vec<RigPicture>,
    /// Every other calibration the file ships, each labelled, in the file's
    /// own order. Empty for most cameras.
    pub variants: Vec<RigVariant>,
    /// What [`projection`](Self::projection) is calibrated for, labelled as a
    /// [`variant`](RigVariant::kind) is. DJI: the housing slot the clip's lens
    /// mode names (the lens guards', the dive case's), or with nothing on the
    /// lenses the first refined slot the file fills. `None` for a body that
    /// ships one calibration.
    #[serde(default)]
    pub projection_kind: Option<VariantKind>,
    /// How to find the other files of a multi-file rig. Hints only — resolving
    /// them touches the filesystem, which is the host's job, not a parser's.
    pub siblings: Vec<SiblingHint>,
    /// Which sibling owns the recording's audio. `None` means this file.
    /// This is a rig fact, independent of picture order and timeline ownership.
    #[serde(default)]
    pub audio_source: Option<SiblingRole>,
    /// What the file states beyond the geometry that a vendor's software reads
    /// to stitch it.
    #[serde(default)]
    pub vendor: VendorFacts,
}

/// The facts a vendor's own software stitches a clip by that are not geometry:
/// which lens each calibration was made for, how the clip was recorded, what
/// the lenses were wearing.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum VendorFacts {
    /// Nothing beyond the geometry.
    #[default]
    None,
    Insta360(Insta360Facts),
}

/// What an Insta360 clip states beside its calibration strings.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Insta360Facts {
    /// Each camera's lens type, as the calibration string the rig is built from
    /// states it, in camera order.
    pub lens_types: Vec<i32>,
    /// Recorded as Bullet Time (`file_group_info.type == 1`).
    pub bullet_time: bool,
    /// `ExtraMetadata.offset_states`: what the camera recorded its lenses as
    /// wearing. `None` for a value the vendor's schema does not name.
    pub offset_state: Option<OffsetState>,
    /// `ExtraMetadata.cooling_shell_selected_state`: whether the cooling shell
    /// (Thermo Grip Cover) was on, or left to the camera's detection. `None`
    /// for a value the schema does not name.
    pub cooling_shell_selected_state: Option<CoolingShellSelectedState>,
    /// `ExtraMetadata.cooling_shell_algo_state`: what that detection found.
    /// `None` for a value the schema does not name.
    pub cooling_shell_algo_state: Option<CoolingShellAlgoDetectedState>,
}

/// A [`CameraRig`] with no cameras — the value a tag falls back to when its
/// payload could not be parsed, which the geometry validator refuses as an
/// empty rig rather than accepting as a degenerate one.
impl Default for CameraRig {
    fn default() -> Self {
        Self {
            brand: String::new(),
            model: String::new(),
            projection: Projection::new(ProjectionKind::Rig {
                cameras: Vec::new(),
                blend: BlendMode::Feather,
                convergence_m: None,
            }),
            pictures: Vec::new(),
            variants: Vec::new(),
            projection_kind: None,
            siblings: Vec::new(),
            audio_source: None,
            vendor: VendorFacts::None,
        }
    }
}

/// Where one of a rig's pictures comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RigPicture {
    VideoTrack(u32),
    Sibling { role: SiblingRole, video_track: u32 },
}

/// Which body of a multi-file rig a file is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SiblingRole {
    Front,
    Back,
    Index(u8),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RigVariant {
    pub kind: VariantKind,
    pub projection: Projection,
}

/// Why a vendor ships more than one calibration for the same body.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum VariantKind {
    /// Calibrated for subjects at this distance, in **metres**
    ConvergenceM(f64),
    /// A vendor-labelled ordinal calibration rung.
    ///
    /// `nearness` is the historical field name, retained for API/serde
    /// compatibility. It is a label, not metres or inverse distance. Neither
    /// its scale nor its direction is universal: DJI's larger labels stitch
    /// farther away. Do not compare these labels with `ConvergenceM` values.
    ConvergenceLadder { nearness: Option<f64> },
    /// Calibrated for a physical accessory or medium.
    Housing(Housing),
    /// A coarser calibration of the same body that the vendor also ships, beside a refined one.
    Unrefined,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Housing {
    LensGuard,
    AboveWater,
    UnderWater,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SiblingHint {
    pub role: SiblingRole,
    pub directory: SiblingDir,
    pub file_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SiblingDir {
    /// Beside the file the hint came from.
    Same,
    /// A path relative to that file's directory.
    Relative(String),
}
