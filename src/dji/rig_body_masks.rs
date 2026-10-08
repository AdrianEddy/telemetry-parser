// SPDX-License-Identifier: MIT OR Apache-2.0
//! DJI Studio 1.0.0.28782 mask outlines, normalized to a 3840-square frame.
//! Full 50%-coverage boundaries, within 1.5 calibration pixels of the source.
//! See oxivideo/docs/360/DJI-MIRROR-MASK.md for provenance and reproduction.

use video_types::geometry::{MaskPolarity, MaskPoly, Projection, ProjectionKind};

#[path = "rig_mask_data.rs"]
mod data;
use data::*;

/// User-selected image coverage. This changes masks only, not optical calibration.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DjiMaskProfile {
    /// Preserve the masks of the selected camera calibration (including its housing).
    #[default]
    Auto,
    Off,
    Standard,
    LensProtector,
    AboveWater,
    UnderWater,
    Model102,
    Model530,
    /// The bundled static template; does not run DJI's dynamic segmentation model.
    Model530Dynamic,
}

impl DjiMaskProfile {
    pub const VALUES: &'static [&'static str] = &[
        "auto",
        "off",
        "standard",
        "lens_protector",
        "above_water",
        "under_water",
        "model_102",
        "model_530",
        "model_530_dynamic",
    ];

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "auto" => Self::Auto,
            "off" => Self::Off,
            "standard" => Self::Standard,
            "lens_protector" => Self::LensProtector,
            "above_water" => Self::AboveWater,
            "under_water" => Self::UnderWater,
            "model_102" => Self::Model102,
            "model_530" => Self::Model530,
            "model_530_dynamic" => Self::Model530Dynamic,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Off => "off",
            Self::Standard => "standard",
            Self::LensProtector => "lens_protector",
            Self::AboveWater => "above_water",
            Self::UnderWater => "under_water",
            Self::Model102 => "model_102",
            Self::Model530 => "model_530",
            Self::Model530Dynamic => "model_530_dynamic",
        }
    }

    /// Replace the masks of a DJI projection in its native slave/master order.
    /// Returns false for a projection other than a two-lens rig. `Auto` preserves
    /// the supplied masks: pass the original projection when restoring Auto.
    pub fn apply(self, projection: &mut Projection) -> bool {
        let ProjectionKind::Rig { cameras, .. } = &mut projection.kind else { return false };
        if cameras.len() != 2 {
            return false;
        }
        if self == Self::Auto {
            return true;
        }
        let pair = match self {
            Self::Standard | Self::AboveWater => [STANDARD_SLAVE, STANDARD_MASTER],
            Self::LensProtector => [LENS_PROTECTOR_SLAVE, LENS_PROTECTOR_MASTER],
            Self::UnderWater => [UNDER_WATER_SLAVE, UNDER_WATER_MASTER],
            Self::Model102 => [MODEL_102_SLAVE, MODEL_102_MASTER],
            Self::Model530 => [MODEL_530_SLAVE, MODEL_530_MASTER],
            Self::Model530Dynamic => [MODEL_530_DYNAMIC_SLAVE, MODEL_530_DYNAMIC_MASTER],
            Self::Auto | Self::Off => [&[][..], &[][..]],
        };
        for (lens, points) in cameras.iter_mut().zip(pair) {
            lens.masks.clear();
            if points.is_empty() {
                continue;
            }
            let [w, h] = lens.intrinsics.calib_dim.map(f64::from);
            lens.masks.push(MaskPoly {
                polarity: MaskPolarity::Include,
                points: points.iter().map(|[x, y]| [x * w / 3840.0, y * h / 3840.0]).collect(),
            });
        }
        true
    }
}
