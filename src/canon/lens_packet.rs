// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2025 Adrian <adrian.eddy at gmail>

//! The lens correction data of a frame: E3.00 to E3.06, which no specification describes. They are the parts of one block,
//! whose header states its layout, and which a CRM holds too as its lens packet (tag 0x4015 of item 0x10009). What its values
//! mean is how Canon's CRM SDK uses that packet.
//!
//! Each part ends in 10 unused bytes. The radial positions are image heights, 4095 being 21.635 mm: the half-diagonal of a
//! 36 x 24 mm frame

use byteorder::{ LittleEndian, ReadBytesExt };
use serde_json::{ json, Value };

use crate::tags_impl::*;
use crate::{ tag, util };
use super::cndm_tags::{ self, FRAME_METADATA };

pub const FIRST: u16 = 0xe300;
pub const LAST: u16 = 0xe306;
pub const ITEMS: usize = (LAST - FIRST + 1) as usize;

const SIZE: u16 = 1710;
/// The sizes of the sections, as the header states them. A block of other sizes has a layout that isn't known
const SECTIONS: [u16; 10] = [32, 96, 582, 580, 144, 26, 96, 16, 16, 122];

const PARAMETERS: usize = 32;
const PARAMETER_WORDS: usize = 24;
const PERIPHERAL: usize = 128;
const PERIPHERAL_VALUES: usize = 210;
const PERIPHERAL_POSITIONS: usize = 460;
const PERIPHERAL_POINTS: usize = 115;
const SHADING: usize = 710;
/// Where the gains of the shading register are, and how many, as they run on past the unused bytes of 3 parts. Together 256
const SHADING_GAINS: [(usize, usize); 3] = [(714, 118), (960, 120), (1210, 18)];
const CHROMATIC: usize = 1290;
const DISTORTION: usize = 1460;
/// The most points of the tables of the chromatic aberration and the distortion
const POINTS: usize = 23;
const BREATHING: usize = 1588;

/// The image height of a position, in mm
fn image_height(position: u16) -> f64 {
    position as f64 / 4095.0 * 21.635
}

fn is_known_layout(block: &[u8]) -> bool {
    block.len() == SIZE as usize
        && u16::from_le_bytes([block[2], block[3]]) == SIZE
        && block[4..24].chunks_exact(2).map(|x| u16::from_le_bytes([x[0], x[1]])).eq(SECTIONS)
}

/// Adds the tags of the parts of a block, in their order. What isn't a known block stays as it is, unknown tags
pub fn insert_tags(items: [Option<&[u8]>; ITEMS], options: &crate::InputOptions, map: &mut GroupedTagMap) {
    let block = items.iter().flatten().copied().collect::<Vec<&[u8]>>().concat();
    if !is_known_layout(&block) {
        for (tag, data) in (FIRST..).zip(items) {
            if let Some(data) = data {
                let mut tag_info = cndm_tags::get_tag(tag, data);
                tag_info.native_id = Some(tag as u32);
                util::insert_tag(map, tag_info, options);
            }
        }
        return;
    }

    let mut correction = tag!(FRAME_METADATA, TagId::Custom("LensCorrection".into()), "Lens Correction", Json, |v| v.to_string(), |d| Ok(decode(d.get_ref())), block);
    correction.native_id = Some(FIRST as u32);
    util::insert_tag(map, correction, options);

    // How much the correction of focus breathing enlarges the picture about its center, the most at the closest focus, where
    // the field of view is the widest: `recorded = center + (corrected - center) / scale`. A picture that had it corrected
    // ("Enabled") doesn't breathe any more
    let breathing = &block[BREATHING..BREATHING + 4];
    if i32::from_le_bytes([breathing[0], breathing[1], breathing[2], breathing[3]]) > 0 {
        let mut scale = tag!(GroupId::LensBreathing, TagId::Scale, "Focus Breathing Scale", f64, "{:.5}", |d| Ok(65536.0 / d.read_i32::<LittleEndian>()? as f64), breathing);
        scale.native_id = Some(LAST as u32);
        util::insert_tag(map, scale, options);
    }
}

fn decode(block: &[u8]) -> Value {
    let word = |offset: usize| u16::from_le_bytes([block[offset], block[offset + 1]]);
    let words = |offset: usize, count: usize| (0..count).map(|i| word(offset + i * 2)).collect::<Vec<u16>>();
    let heights = |offset: usize, count: usize| words(offset, count).into_iter().map(image_height).collect::<Vec<f64>>();
    let parameters = words(PARAMETERS, PARAMETER_WORDS);
    let enabled = |index: usize| parameters[index] != 0;
    let strength = |index: usize| parameters[index] as f64 / 100.0;

    // Illumination relative to the center. The gain that corrects it is `strength / illumination + 1 - strength`
    let peripheral_points = (word(PERIPHERAL) as usize).min(PERIPHERAL_POINTS);
    let illumination = words(PERIPHERAL_VALUES, peripheral_points).into_iter().map(|x| x as f64 / 8191.0).collect::<Vec<f64>>();

    // The gain at a distance of r pixels from the center is at index `r * r / 2^(shift + 4) * scale`, 512 being a gain of 1
    let gains = SHADING_GAINS.iter().flat_map(|&(offset, count)| words(offset, count)).map(|x| x as f64 / 512.0).collect::<Vec<f64>>();

    // In mm of radial shift, in planes 0 and 2 of the SDK's demosaicing, which are the red and the blue going by their order
    let chromatic_points = (word(CHROMATIC) as usize).min(POINTS);
    let shift = |offset: usize| words(offset, chromatic_points).into_iter().map(|x| if x == 0x1000 { 0.0 } else { (x as f64 - 16384.0) * 0.2 / 16384.0 }).collect::<Vec<f64>>();

    // Which radius of the recorded picture a radius of the corrected picture comes from, as the radial part of OpenCV's model:
    // `recorded = corrected * (1 + strength * (scale - 1))`
    let distortion_points = (word(DISTORTION) as usize).min(POINTS);
    let scale = words(DISTORTION + 2, distortion_points).into_iter().map(|x| 1.0 + 0.8 * (x.min(16384) as f64 - 16384.0) / 16383.0).collect::<Vec<f64>>();

    json!({
        "Version": word(0),
        "PixelPitch": parameters[0] as f64 / 1000.0,
        "SensorSize": [parameters[1], parameters[2]],
        "Enabled": {
            "PeripheralIllumination": enabled(3),
            "Distortion": enabled(4),
            "ChromaticAberration": enabled(5),
            "FocusBreathing": enabled(6)
        },
        "Strength": {
            "PeripheralIllumination": strength(19),
            "Distortion": strength(20),
            "ChromaticAberration": strength(21)
        },
        "PeripheralIllumination": {
            "ImageHeight": heights(PERIPHERAL_POSITIONS, peripheral_points),
            "Illumination": illumination
        },
        "ShadingGain": {
            "Gain": gains,
            "IndexShift": word(SHADING + 2),
            "IndexScale": if word(SHADING) == 1 { 0.75 } else { 1.0 }
        },
        "ChromaticAberration": {
            "ImageHeight": heights(CHROMATIC + 2 + 2 * 2 * POINTS, chromatic_points),
            "RedShift": shift(CHROMATIC + 2),
            "BlueShift": shift(CHROMATIC + 2 + 2 * POINTS)
        },
        "Distortion": {
            "ImageHeight": heights(DISTORTION + 2 + 2 * POINTS, distortion_points),
            "Scale": scale
        },
        "Parameters": parameters
    })
}
