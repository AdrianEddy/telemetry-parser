// SPDX-License-Identifier: MIT OR Apache-2.0
//! Studio 6.0.2 image-coverage contours. See oxivideo/docs/360/INSTA360-MASKS.md.
//! Profiles change masks only; accessory optical calibration is independent.
use super::rig_mask_coefficients::{full_fov, polynomial};
use video_types::geometry::{
    Intrinsics, Lens, LensModel, MaskPolarity, MaskPoly, MeiDistortion, Projection, ProjectionKind,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Family {
    OneX,
    X2,
    OneR,
    X3,
    X4,
    X5,
    X4Air,
    X6,
    PC,
    DM,
}
impl Family {
    fn model(model: &str) -> Option<Self> {
        Some(
            match model
                .trim_start_matches("Insta360 ")
                .to_ascii_uppercase()
                .as_str()
            {
                "ONE X" => Self::OneX,
                "ONE X2" | "X2" => Self::X2,
                "ONE R" | "ONE RS" | "ONER" | "ONERS" => Self::OneR,
                "X3" => Self::X3,
                "X4" => Self::X4,
                "X5" => Self::X5,
                "X4 AIR" => Self::X4Air,
                "X6" => Self::X6,
                "PC" => Self::PC,
                "DM" => Self::DM,
                _ => return None,
            },
        )
    }
}
#[derive(Clone, Copy)]
struct Recipe {
    family: Family,
    lens_type: i32,
    cooling: bool,
    variant: u8,
}

macro_rules! profiles {
    ($($variant:ident, $value:literal, $label:literal, $family:ident, $lens:literal, $cool:literal, $mode:literal;)*) => {
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
        pub enum Insta360MaskProfile { #[default] Auto, Off, $($variant,)* }
        impl Insta360MaskProfile {
            pub const VALUES: &'static [&'static str] = &["auto", "off", $($value,)*];
            pub fn parse(value: &str) -> Option<Self> {
                Some(match value { "auto" => Self::Auto, "off" => Self::Off, $($value => Self::$variant,)* _ => return None })
            }
            pub fn as_str(self) -> &'static str {
                match self { Self::Auto => "auto", Self::Off => "off", $(Self::$variant => $value,)* }
            }
            pub fn label(self) -> &'static str {
                match self { Self::Auto => "Auto (selected calibration)", Self::Off => "Off", $(Self::$variant => $label,)* }
            }
            fn recipe(self) -> Option<Recipe> {
                Some(match self {
                    $(Self::$variant => Recipe { family: Family::$family, lens_type: $lens, cooling: $cool, variant: $mode },)*
                    Self::Auto | Self::Off => return None,
                })
            }
        }
    };
}
profiles! {
    OneXStandard, "one_x_standard", "ONE X: Standard", OneX, 19, false, 0;
    OneXVentureCase, "one_x_venture_case", "ONE X: Venture case", OneX, 27, false, 0;
    X2LensGuardColdShoeBoth, "x2_lens_guard_cold_shoe_both", "ONE X2: Lens guards + cold shoe (both)", X2, 42, false, 1;
    X2LensGuardColdShoeUpper, "x2_lens_guard_cold_shoe_upper", "ONE X2: Lens guards + cold shoe (upper)", X2, 42, false, 2;
    X2LensGuardColdShoeLower, "x2_lens_guard_cold_shoe_lower", "ONE X2: Lens guards + cold shoe (lower)", X2, 42, false, 3;
    X2Standard, "x2_standard", "ONE X2: Standard", X2, 41, false, 0;
    X2LensGuard, "x2_lens_guard", "ONE X2: Lens guards", X2, 42, false, 0;
    X2DiveUnderWater, "x2_dive_under_water", "ONE X2: Dive case: underwater", X2, 43, false, 0;
    X2ColdShoeBoth, "x2_cold_shoe_both", "ONE X2: Cold shoe (both)", X2, 41, false, 1;
    X2ColdShoeUpper, "x2_cold_shoe_upper", "ONE X2: Cold shoe (upper)", X2, 41, false, 2;
    X2ColdShoeLower, "x2_cold_shoe_lower", "ONE X2: Cold shoe (lower)", X2, 41, false, 3;
    OneRStandard, "one_r_standard", "ONE R / RS: Standard", OneR, 33, false, 0;
    OneRLensGuard, "one_r_lens_guard", "ONE R / RS: Lens guards", OneR, 39, false, 0;
    OneRPremiumLensGuard, "one_r_premium_lens_guard", "ONE R / RS: Premium lens guards", OneR, 51, false, 0;
    OneRDiveAboveWater, "one_r_dive_above_water", "ONE R / RS: Dive case: above water", OneR, 38, false, 0;
    OneRDiveUnderWater, "one_r_dive_under_water", "ONE R / RS: Dive case: underwater", OneR, 40, false, 0;
    X3Standard, "x3_standard", "X3: Standard", X3, 71, false, 0;
    X3GuardA, "x3_guard_a", "X3: Lens guards A", X3, 77, false, 0;
    X3GuardS, "x3_guard_s", "X3: Lens guards S", X3, 76, false, 0;
    X3GuardAS, "x3_guard_as", "X3: Lens guards A + S", X3, 84, false, 0;
    X3DiveAboveWater, "x3_dive_above_water", "X3: Dive case: above water", X3, 79, false, 0;
    X3DiveUnderWater, "x3_dive_under_water", "X3: Dive case: underwater", X3, 78, false, 0;
    X3InvisibleDiveAboveWater, "x3_invisible_dive_above_water", "X3: Invisible dive case: above water", X3, 87, false, 0;
    X3InvisibleDiveUnderWater, "x3_invisible_dive_under_water", "X3: Invisible dive case: underwater", X3, 86, false, 0;
    X4Standard, "x4_standard", "X4: Standard", X4, 71, false, 0;
    X4GuardA, "x4_guard_a", "X4: Lens guards A", X4, 106, false, 0;
    X4GuardS, "x4_guard_s", "X4: Lens guards S", X4, 107, false, 0;
    X4GuardAS, "x4_guard_as", "X4: Lens guards A + S", X4, 108, false, 0;
    X4InvisibleDiveAboveWater, "x4_invisible_dive_above_water", "X4: Invisible dive case: above water", X4, 87, false, 0;
    X4InvisibleDiveUnderWater, "x4_invisible_dive_under_water", "X4: Invisible dive case: underwater", X4, 86, false, 0;
    X4CoolingCase, "x4_cooling_case", "X4: Cooling case", X4, 71, true, 0;
    X4GuardACoolingCase, "x4_guard_a_cooling_case", "X4: Lens guards A + cooling case", X4, 106, true, 0;
    X4GuardSCoolingCase, "x4_guard_s_cooling_case", "X4: Lens guards S + cooling case", X4, 107, true, 0;
    X4GuardASCoolingCase, "x4_guard_as_cooling_case", "X4: Lens guards A + S + cooling case", X4, 108, true, 0;
    X5AiStitch, "x5_ai_stitch", "X5: AI stitching", X5, 113, false, 1;
    X5AiStitchCoolingCase, "x5_ai_stitch_cooling_case", "X5: AI stitching + cooling case", X5, 113, true, 1;
    X5Standard, "x5_standard", "X5: Standard", X5, 113, false, 0;
    X5LensGuard, "x5_lens_guard", "X5: Lens guards", X5, 115, false, 0;
    X5DiveUnderWater, "x5_dive_under_water", "X5: Invisible dive case: underwater", X5, 117, false, 0;
    X5DiveAboveWater, "x5_dive_above_water", "X5: Invisible dive case: above water", X5, 118, false, 0;
    X5Dive2UnderWater, "x5_dive2_under_water", "X5: Invisible dive case 2: underwater", X5, 119, false, 0;
    X5Dive2AboveWater, "x5_dive2_above_water", "X5: Invisible dive case 2: above water", X5, 120, false, 0;
    X5CoolingCase, "x5_cooling_case", "X5: Cooling case", X5, 113, true, 0;
    X5LensGuardCoolingCase, "x5_lens_guard_cooling_case", "X5: Lens guards + cooling case", X5, 115, true, 0;
    X4AirStandard, "x4_air_standard", "X4 Air: Standard", X4Air, 131, false, 0;
    X4AirGuardA, "x4_air_guard_a", "X4 Air: Lens guards A", X4Air, 140, false, 0;
    X4AirGuardAS, "x4_air_guard_as", "X4 Air: Lens guards A + S", X4Air, 142, false, 0;
    X4AirGuardA2, "x4_air_guard_a2", "X4 Air: Lens guards A2", X4Air, 196, false, 0;
    X4AirDiveUnderWater, "x4_air_dive_under_water", "X4 Air: Dive case: underwater", X4Air, 147, false, 0;
    X4AirDive2UnderWater, "x4_air_dive2_under_water", "X4 Air: Dive case 2: underwater", X4Air, 148, false, 0;
    X4AirDiveAboveWater, "x4_air_dive_above_water", "X4 Air: Dive case: above water", X4Air, 149, false, 0;
    X4AirDive2AboveWater, "x4_air_dive2_above_water", "X4 Air: Dive case 2: above water", X4Air, 150, false, 0;
    X4AirCoolingCase, "x4_air_cooling_case", "X4 Air: Cooling case", X4Air, 131, true, 0;
    X4AirGuardACoolingCase, "x4_air_guard_a_cooling_case", "X4 Air: Lens guards A + cooling case", X4Air, 140, true, 0;
    X4AirGuardASCoolingCase, "x4_air_guard_as_cooling_case", "X4 Air: Lens guards A + S + cooling case", X4Air, 142, true, 0;
    X4AirGuardA2CoolingCase, "x4_air_guard_a2_cooling_case", "X4 Air: Lens guards A2 + cooling case", X4Air, 196, true, 0;
    X6Standard, "x6_standard", "X6: Standard", X6, 193, false, 0;
    X6LensGuard, "x6_lens_guard", "X6: Lens guards", X6, 197, false, 0;
    X6DiveUnderWater, "x6_dive_under_water", "X6: Dive case: underwater", X6, 198, false, 0;
    X6DiveAboveWater, "x6_dive_above_water", "X6: Dive case: above water", X6, 199, false, 0;
    X6BulletTime, "x6_bullet_time", "X6: Bullet Time", X6, 193, false, 1;
    X6LensGuardCoolingCase, "x6_lens_guard_cooling_case", "X6: Lens guards + cooling case", X6, 197, true, 0;
    PCStandard, "pc_standard", "PC: Standard", PC, 33, false, 0;
    PCCase, "pc_case", "PC: Protective case", PC, 83, false, 0;
    DMStandard, "dm_standard", "DM: Standard", DM, 85, false, 0;
    DMGuards, "dm_guards", "DM: Propeller guards", DM, 85, false, 1;
    DMLargeGuards, "dm_large_guards", "DM: Large propeller guards", DM, 85, false, 2;
    DMDongle, "dm_dongle", "DM: Guard dongle", DM, 85, false, 3;
}
impl Insta360MaskProfile {
    pub fn supports_model(self, model: &str) -> bool {
        let Some(family) = Family::model(model) else {
            return false;
        };
        self.recipe().is_none_or(|r| r.family == family)
    }

    /// Only the current camera's masks, including Auto and Off. Unknown models
    /// intentionally return no choices instead of offering another body's mask.
    pub fn values_for_model(model: &str) -> Vec<&'static str> {
        Self::VALUES
            .iter()
            .copied()
            .filter(|v| Self::parse(v).unwrap().supports_model(model))
            .collect()
    }

    /// Apply in calibration order. Hosts also check supports_model against the
    /// rig's source. Auto restores masks only when passed the original geometry.
    pub fn apply(self, projection: &mut Projection) -> bool {
        let ProjectionKind::Rig { cameras, .. } = &mut projection.kind else {
            return false;
        };
        if cameras.len() != 2 {
            return false;
        }
        if self == Self::Auto {
            return true;
        }
        if self == Self::Off {
            for lens in cameras {
                lens.masks.clear();
            }
            return true;
        }
        let recipe = self.recipe().unwrap();
        let Some(masks) = cameras
            .iter()
            .enumerate()
            .map(|(i, lens)| mask(lens, recipe, i, recipe.lens_type))
            .collect::<Option<Vec<_>>>()
        else {
            return false;
        };
        for (lens, mask) in cameras.iter_mut().zip(masks) {
            lens.masks = vec![mask];
        }
        true
    }
}

pub(super) fn automatic(model: &str, lenses: &mut [Lens], lens_types: &[i32], bullet_time: bool) {
    let Some(family) = Family::model(model) else {
        return;
    };
    if lenses.len() != 2 || lens_types.len() != 2 {
        return;
    }
    let Some(masks) = lenses
        .iter()
        .enumerate()
        .map(|(i, lens)| {
            mask(
                lens,
                Recipe {
                    family,
                    lens_type: lens_types[i],
                    cooling: false,
                    variant: u8::from(family == Family::X6 && bullet_time),
                },
                i,
                lens_types[0],
            )
        })
        .collect::<Option<Vec<_>>>()
    else {
        return;
    };
    // Native dispatch overrides the default disc for these accessories.
    // This belongs to the parsed rig; a later user mask override preserves it.
    let outer: Option<f64> = match (family, lens_types[0]) {
        (Family::X4, 106 | 107) | (Family::X4Air, 140 | 141 | 196) => Some(96.),
        (Family::X6, 198 | 199) => Some(94.),
        (Family::PC, 83) => Some(91.),
        (Family::DM, _) => Some(98.),
        _ => None,
    };
    for (lens, mask) in lenses.iter_mut().zip(masks) {
        lens.masks = vec![mask];
        if let Some(outer) = outer { lens.fov_limit = Some(outer.to_radians()); }
    }
}

fn mask(lens: &Lens, recipe: Recipe, index: usize, first_type: i32) -> Option<MaskPoly> {
    use Family::*;
    let Recipe {
        family,
        lens_type: t,
        cooling,
        variant,
    } = recipe;
    if family == X5 {
        let pair = if variant == 1 && first_type == 113 {
            [AI_LEFT, if cooling { COOLING } else { AI_RIGHT }]
        } else {
            tables(first_type, cooling)?
        };
        return outline(lens, pair[index], pair[0].last()?[1]);
    }
    let (pair, axis, outer) = match family {
        OneX if matches!(t, 19 | 27) => ([X2_TABLE; 2], if index == 0 { -90. } else { 90. }, None),
        X2 if matches!(t, 41 | 42) => ([X2_TABLE; 2], if index == 0 { 90. } else { -90. }, None),
        OneR if matches!(t, 33 | 83) => ([ONE_R_LEFT, ONE_R_RIGHT], 0., None),
        X2 | OneR if matches!(t, 38 | 39 | 40 | 43 | 51) => ([EMPTY; 2], 0., None),
        X3 if matches!(t, 71 | 76..=79 | 84 | 86 | 87) => (
            [if matches!(t, 86 | 87) {
                X3_DIVE
            } else {
                X3_TABLE
            }; 2],
            0.,
            None,
        ),
        X4 if matches!(t, 71 | 106..=108 | 86 | 87) => (
            if matches!(t, 86 | 87) {
                [X3_DIVE; 2]
            } else {
                [X4_LEFT, if cooling { X4_COOLING } else { X4_RIGHT }]
            },
            0.,
            matches!(t, 106 | 107).then_some(96.),
        ),
        X4Air if matches!(t, 131 | 140..=142 | 147..=150 | 196) => (
            if matches!(t, 147..=150) {
                [X3_DIVE; 2]
            } else {
                [X4_LEFT, if cooling { X4_COOLING } else { X4_RIGHT }]
            },
            0.,
            matches!(t, 140 | 141 | 196).then_some(96.),
        ),
        X6 if t == 193 => (
            if variant == 1 {
                [X6_BULLET_LEFT, X6_BULLET_RIGHT]
            } else {
                [X6_LEFT, X6_RIGHT]
            },
            0.,
            None,
        ),
        X6 if t == 197 => (
            [
                GUARD_LEFT,
                if cooling { GUARD_COOLING } else { GUARD_RIGHT },
            ],
            0.,
            None,
        ),
        X6 if matches!(t, 198 | 199) => ([DIVE; 2], 0., Some(94.)),
        PC if matches!(t, 33 | 83) => ([PC_TABLE; 2], 0., (t == 83).then_some(91.)),
        DM if matches!(t, 85 | 109) => (
            match variant {
                0 => [DM_LEFT, DM_RIGHT],
                1 => [DM_GUARD_LEFT, DM_GUARD_RIGHT],
                2 => [DM_LARGE_LEFT, DM_LARGE_RIGHT],
                3 => [DM_LEFT, DM_DONGLE_RIGHT],
                _ => return None,
            },
            // TemplateBlenderBase ctor (0x2b46c44): tail orientation = 1.
            0.,
            Some(98.),
        ),
        _ => return None,
    };
    let outer = outer.unwrap_or(full_fov(first_type)? / 2. - 2.);
    polynomial_outline(lens, recipe, pair[index], axis, outer, index, first_type)
}

fn evaluate(coeff: [f64; 5], theta: f64) -> f64 {
    coeff.into_iter().rev().fold(0., |acc, c| acc * theta + c)
}

fn interpolate(table: Table, angle: f64) -> Option<f64> {
    table.windows(2).find_map(|s| {
        let [[a, x], [b, y]] = [s[0], s[1]];
        (angle >= a.min(b) && angle <= a.max(b)).then(|| x + (y - x) * (angle - a) / (b - a))
    })
}

fn polynomial_outline(
    lens: &Lens,
    recipe: Recipe,
    table: Table,
    axis: f64,
    outer: f64,
    index: usize,
    first_type: i32,
) -> Option<MaskPoly> {
    let circle = lens.image_circle.as_ref()?;
    let coeff = polynomial(recipe.lens_type)?;
    let denominator = evaluate(polynomial(first_type)?, full_fov(recipe.lens_type)? / 2.);
    if !(denominator.is_finite()
        && denominator > 0.
        && circle.radius.is_finite()
        && circle.radius > 0.)
    {
        return None;
    }
    let radius = |theta| circle.radius * evaluate(coeff, theta) / denominator;
    let base = radius(outer);
    if !(base.is_finite() && base > 0.) {
        return None;
    }
    let mut points = Vec::with_capacity(720);
    for step in 0..720 {
        let degrees = step as f64 * 0.5 - 180.;
        let (sin, cos) = degrees.to_radians().sin_cos();
        let phi = ((degrees - axis + 180.).rem_euclid(360.) - 180.).abs();
        let is_one_r = recipe.family == Family::OneR && !table.is_empty();
        let angle = if is_one_r {
            (-cos).atan2(sin).to_degrees()
        } else {
            phi
        };
        let allowed = if is_one_r {
            cos < -0.17364818
        } else if recipe.family == Family::DM {
            phi <= if recipe.variant == 2 { 180. } else { 90. }
        } else {
            phi < 90.
        };
        let mut r = if allowed {
            interpolate(table, angle).map_or(base, |t| radius(t).min(base))
        } else {
            base
        };
        if matches!(recipe.family, Family::OneX | Family::X2) && recipe.variant != 0 {
            let cold_axis = if index == 0 { 90. } else { -90. };
            let cold_phi = ((degrees - cold_axis + 180.).rem_euclid(360.) - 180.).abs();
            if recipe.variant == 1
                || (recipe.variant == 2 && cos < 0.)
                || (recipe.variant == 3 && cos >= 0.)
            {
                if let Some(theta) = interpolate(COLD_SHOE, cold_phi) {
                    r = r.min(radius(theta));
                }
            }
        }
        if !(r.is_finite() && r > 0.) {
            return None;
        }
        points.push([circle.centre[0] + r * sin, circle.centre[1] + r * cos]);
    }
    Some(MaskPoly {
        polarity: MaskPolarity::Include,
        points,
    })
}

type Table = &'static [[f64; 2]]; // azimuth, half FOV in degrees
const EMPTY: Table = &[];
// Builder addresses and variant provenance are recorded in INSTA360-MASKS.md.
const X2_TABLE: Table = &[
    [0., 90.5],
    [13., 90.5],
    [18., 91.5],
    [24., 92.5],
    [28., 93.5],
    [37., 94.5],
    [45., 95.5],
    [53.5, 96.5],
    [57., 97.5],
    [61., 98.5],
    [70., 99.5],
];
const COLD_SHOE: Table = &[[0., 90.5], [56., 90.5], [57., 97.5]];
const X3_TABLE: Table = &[
    [0., 91.],
    [13., 91.],
    [18., 92.],
    [24., 93.5],
    [28., 95.5],
    [37., 96.5],
    [45., 96.5],
    [53.5, 96.5],
    [57., 97.5],
    [61., 98.5],
    [70., 99.5],
];
const X3_DIVE: Table = &[
    [0., 91.],
    [13., 91.],
    [18., 91.],
    [24., 94.5],
    [28., 95.5],
    [37., 96.5],
    [45., 96.5],
    [53.5, 96.5],
    [57., 97.5],
    [61., 98.5],
    [70., 99.5],
];
const X4_LEFT: Table = &[
    [0., 91.3],
    [13., 91.3],
    [18., 93.25],
    [24., 94.5],
    [28., 95.5],
    [37., 96.5],
    [45., 96.5],
    [53.5, 96.5],
    [57., 97.5],
    [61., 98.5],
    [70., 99.5],
];
const X4_RIGHT: Table = &[
    [0., 91.5],
    [10., 91.5],
    [15., 92.],
    [18., 93.5],
    [23., 95.25],
    [30., 97.],
    [31., 97.5],
];
const X4_COOLING: Table = &[
    [0., 90.8],
    [14., 91.],
    [19., 91.5],
    [24., 92.5],
    [28., 93.5],
    [37., 94.5],
    [45., 94.5],
    [53.5, 95.5],
    [57., 96.5],
    [61., 97.5],
    [70., 99.5],
];
const X6_LEFT: Table = &[
    [0., 90.2],
    [13., 90.2],
    [18., 91.25],
    [24., 92.25],
    [28., 93.25],
    [37., 94.25],
    [45., 95.5],
    [53.5, 96.5],
    [57., 97.5],
    [61., 98.5],
    [70., 99.5],
];
const X6_RIGHT: Table = &[
    [0., 91.],
    [10., 91.],
    [13., 91.],
    [18., 91.5],
    [40., 95.5],
    [57., 98.5],
    [61., 100.],
];
const X6_BULLET_LEFT: Table = &[
    [0., 91.2],
    [13., 91.2],
    [18., 92.25],
    [24., 93.25],
    [28., 94.25],
    [37., 95.25],
    [45., 96.5],
    [53.5, 96.5],
    [57., 97.5],
    [61., 98.5],
    [70., 99.5],
];
const X6_BULLET_RIGHT: Table = &[
    [0., 91.5],
    [10., 91.5],
    [13., 91.5],
    [18., 92.],
    [40., 96.5],
    [57., 98.5],
    [61., 100.],
];
const PC_TABLE: Table = &[
    [0., 90.5],
    [18., 90.5],
    [24., 91.],
    [28., 91.5],
    [37., 93.5],
    [45., 95.],
    [53.5, 96.],
    [57., 97.],
    [61., 98.],
    [79., 98.],
];
const ONE_R_LEFT: Table = &[
    [130., 98.],
    [128., 98.],
    [126., 97.],
    [120., 95.],
    [112., 94.5],
    [111., 95.],
    [100., 95.],
    [80., 95.],
    [65., 94.],
    [50., 95.],
    [42., 95.5],
    [37., 96.],
    [20., 98.],
];
const ONE_R_RIGHT: Table = &[
    [164., 98.],
    [146., 98.],
    [138., 97.],
    [132., 96.],
    [129., 95.],
    [118., 94.],
    [114., 94.5],
    [68., 94.5],
    [67., 93.],
    [50., 93.],
    [45., 94.],
    [40., 96.],
    [39., 98.],
];
const DM_LEFT: Table = &[
    [0., 91.],
    [10., 90.8],
    [30., 90.5],
    [48., 90.5],
    [55., 93.],
    [60., 98.],
    [115., 95.5],
    [130., 95.5],
    [180., 98.],
];
const DM_RIGHT: Table = &[
    [0., 97.],
    [15., 96.],
    [25., 94.],
    [40., 96.],
    [77., 96.],
    [90., 98.],
];
const DM_GUARD_LEFT: Table = &[
    [0., 91.],
    [10., 90.8],
    [30., 90.5],
    [48., 90.5],
    [73., 95.],
    [110., 95.],
    [115., 93.],
    [125., 93.],
    [130., 96.],
    [180., 98.],
];
const DM_GUARD_RIGHT: Table = &[
    [0., 97.],
    [16., 96.],
    [17., 92.],
    [42., 92.],
    [50., 95.],
    [90., 95.],
    [98., 98.],
];
const DM_LARGE_LEFT: Table = &[
    [0., 91.],
    [10., 90.8],
    [30., 90.5],
    [48., 90.5],
    [55., 93.],
    [80., 94.],
    [90., 96.],
    [98., 93.],
    [135., 94.],
    [180., 98.],
];
const DM_LARGE_RIGHT: Table = &[
    [0., 95.],
    [15., 92.],
    [25., 91.25],
    [40., 91.5],
    [77., 91.75],
    [90., 92.25],
    [98., 94.5],
    [115., 96.],
];
const DM_DONGLE_RIGHT: Table = &[
    [0., 92.],
    [15., 92.5],
    [25., 93.5],
    [40., 96.],
    [77., 96.],
    [90., 98.],
];
const AI_LEFT: Table = &[[0., 90.5], [11., 90.5], [17., 92.], [33., 95.]];
const AI_RIGHT: Table = &[[0., 91.5], [10., 91.5], [15., 92.], [20., 93.], [27., 94.]];
const LEFT: Table = &[[0., 90.5], [11., 90.5], [17., 92.], [33., 95.7], [57., 97.]];
const RIGHT: Table = &[
    [0., 91.5],
    [10., 91.5],
    [15., 92.],
    [20., 93.],
    [27., 94.],
    [40., 97.],
];
const GUARD_LEFT: Table = &[[0., 90.5], [11., 90.5], [17., 92.], [33., 94.]];
const GUARD_RIGHT: Table = &[[0., 91.5], [10., 91.5], [15., 92.], [18., 93.5], [23., 94.]];
const COOLING: Table = &[
    [0., 90.5],
    [11., 90.5],
    [17., 90.5],
    [20., 91.5],
    [24., 92.],
    [28., 92.5],
    [37., 93.],
    [45., 94.5],
    [53.5, 95.5],
    [57., 96.5],
    [61., 97.],
];
const GUARD_COOLING: Table = &[
    [0., 90.5],
    [11., 90.5],
    [17., 90.5],
    [20., 91.5],
    [24., 92.],
    [28., 92.5],
    [37., 93.],
    [45., 94.],
];
const DIVE: Table = &[[0., 90.5], [10., 90.5], [20., 91.8], [60., 94.]];
const DIVE_ALT: Table = &[[0., 91.], [10., 91.], [32., 92.5], [55., 93.5]];

fn tables(lens_type: i32, cooling: bool) -> Option<[Table; 2]> {
    Some(match lens_type {
        113 => [LEFT, if cooling { COOLING } else { RIGHT }],
        115 | 156 => [
            GUARD_LEFT,
            if cooling { GUARD_COOLING } else { GUARD_RIGHT },
        ],
        117 | 118 => [DIVE, DIVE],
        119 | 120 => [DIVE_ALT, DIVE_ALT],
        _ => return None,
    })
}

fn outline(lens: &Lens, table: Table, outer_deg: f64) -> Option<MaskPoly> {
    let intr = &lens.intrinsics;
    let outer = outer_deg.min(lens.fov_limit?.to_degrees());
    let base = radius_squared(intr, outer, 0.)?;
    let radii = table
        .iter()
        .map(|&[phi, theta]| radius_squared(intr, theta, phi))
        .collect::<Option<Vec<_>>>()?;
    // ErodeCircleMethord3 (0x2b693dc) works on the right half AFTER a 90-degree
    // CCW rotation. In the original lens image this is the BOTTOM, symmetric
    // about cx. Interpolate squared PIXEL radius, never theta or radius itself.
    // A half-degree step retains each table knot and has <0.03 px circle sag
    // at the X5's 5376-pixel calibration size. No per-frame polygon generation.
    let mut points = Vec::with_capacity(720);
    for step in 0..720 {
        let degrees = step as f64 * 0.5 - 180.;
        let phi = degrees.abs();
        let mut r2 = base;
        for (i, segment) in table.windows(2).enumerate() {
            let [a, b] = [segment[0][0], segment[1][0]];
            if phi >= a && phi < b {
                r2 = base.min(radii[i] + (radii[i + 1] - radii[i]) * (phi - a) / (b - a));
                break;
            }
        }
        let (sin, cos) = degrees.to_radians().sin_cos();
        points.push([
            intr.centre[0] + r2.sqrt() * sin,
            intr.centre[1] + r2.sqrt() * cos,
        ]);
    }
    Some(MaskPoly {
        polarity: MaskPolarity::Include,
        points,
    })
}

/// The vendor cancels its camera rotation before sampling the dewarp map in
/// calcFisheyeCoordFromFOV. Evaluate the equivalent local Mei projection directly.
/// Its output azimuth is used only to measure radius; the outline is symmetric.
fn radius_squared(intr: &Intrinsics, theta: f64, phi: f64) -> Option<f64> {
    let LensModel::Mei { xi, dist } = intr.model else {
        return None;
    };
    let (st, ct) = theta.to_radians().sin_cos();
    let (sp, cp) = phi.to_radians().sin_cos();
    let den = xi + ct;
    if den <= 0. {
        return None;
    }
    let (x, y) = (st * sp / den, st * cp / den);
    let r2 = x * x + y * y;
    let (xd, yd) = match dist {
        MeiDistortion::BrownV3 { k, p } => {
            let radial = 1. + r2 * (k[0] + r2 * (k[1] + r2 * k[2]));
            (
                x * radial + 2. * p[0] * x * y + p[1] * (r2 + 2. * x * x),
                y * radial + p[0] * (r2 + 2. * y * y) + 2. * p[1] * x * y,
            )
        }
        MeiDistortion::RadtanPro { d } => {
            let radial = 1. + r2 * (d[0] + r2 * (d[1] + r2 * (d[2] + r2 * (d[3] + r2 * d[4]))));
            let a = d[5] + d[7] * r2;
            let b = d[6] + d[8] * r2;
            (
                x * radial + (r2 + 2. * x * x) * a + 2. * x * y * b + d[9] * r2 + d[11] * r2 * r2,
                y * radial + (r2 + 2. * y * y) * b + 2. * x * y * a + d[10] * r2 + d[12] * r2 * r2,
            )
        }
    };
    let radius = (intr.focal[0] * xd).powi(2) + (intr.focal[1] * yd).powi(2);
    (radius.is_finite() && radius > 0.).then_some(radius)
}
