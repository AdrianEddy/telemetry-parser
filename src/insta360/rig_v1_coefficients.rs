// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2026 Adrian <adrian.eddy at gmail>

//! The degree polynomial Insta360 Studio renders a V1 calibration string with,
//! for the lens types it was verified on.
//!
//! A V1 string states an image radius and no coefficients, so the vendor
//! evaluates a built-in table: these entries agree between its optical
//! `GetDegreeCoeffs` (0x2e368b0) and its mask `Lens::getCoeff` (0x4c5e48) in
//! Studio 6.0.2 arm64, transcribed by `docs/360/oracle/insta_mask_coefficients.py`
//! in oxivideo. Type 42's optical table differs from its mask table and is
//! stated where it is read (`rig::lens_model`); any other type has no verified
//! optics and is not rendered from a V1 string.

/// `r(θ) = c₀ + c₁θ + c₂θ² + c₃θ³ + c₄θ⁴`, θ in degrees, up to the scale the
/// string's image radius sets.
pub(super) fn polynomial(lens_type: i32) -> Option<[f64; 5]> {
    Some(match lens_type {
        19 | 33 => [0.0, 0.024134, -7.4557e-05, 2.0846e-06, -1.4576e-08],
        27 => [0.0, 0.023693, -5.9584e-05, 1.8402e-06, -1.4809e-08],
        38 => [0.0, 0.024336, -7.55532e-05, 2.13171e-06, -1.5040305e-08],
        39 => [0.0, 0.0236785, -5.894477e-05, 1.810191e-06, -1.4521993e-08],
        40 => [0.0, 0.0259398, -7.3377467e-05, 2.2925e-06, -1.7776969e-08],
        41 => [0.0, 0.02413266, -7.451592e-05, 2.0839511e-06, -1.4573211e-08],
        43 => [0.0, 0.02586, -8.1e-05, 2.41e-06, -1.819e-08],
        51 => [0.0, 0.024446313655691, -7.9118706055e-05, 2.24180908e-06, -1.5675326e-08],
        70 => [0.0, 0.02235, -5.382e-05, 2.001e-06, -1.392e-08],
        71 => [0.0, 0.02249, -5.79e-05, 1.932e-06, -1.295e-08],
        83 => [0.0, 0.024, -6.316e-05, 2.013e-06, -1.456e-08],
        _ => return None,
    })
}
