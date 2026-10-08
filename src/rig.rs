// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2026 Adrian <adrian.eddy at gmail>

//! What a multi-camera body's pictures mean geometrically, normalised out of a
//! vendor's own calibration block into the shared descriptors.

use serde::{Deserialize, Serialize};

pub use video_types::geometry;
pub use crate::dji::DjiMaskProfile;
pub use crate::insta360::Insta360MaskProfile;

use geometry::{BlendMode, Projection, ProjectionKind};

/// A vendor's stated frame readout as the shared descriptor's [`Readout`].
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

/// A camera body's cameras, their geometry, and the alternatives it ships.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CameraRig {
    pub brand: String,
    pub model: String,
    /// The geometry to use unless a host picks a [`variant`](Self::variants) —
    /// the same default the vendor's own software resolves to.
    /// `Picture.input` indexes [`pictures`](Self::pictures).
    pub projection: Projection,
    /// Which decoded picture each `Picture.input` names, in index order.
    pub pictures: Vec<RigPicture>,
    /// Alternative geometries the camera ships, described generically. Empty
    /// for most cameras.
    pub variants: Vec<RigVariant>,
    /// How to find the other files of a multi-file rig. Hints only — resolving
    /// them touches the filesystem, which is the host's job, not a parser's.
    pub siblings: Vec<SiblingHint>,
    /// Which sibling owns the recording's audio. `None` means this file.
    /// This is a rig fact, independent of picture order and timeline ownership.
    #[serde(default)]
    pub audio_source: Option<SiblingRole>,
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
            siblings: Vec::new(),
            audio_source: None,
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
