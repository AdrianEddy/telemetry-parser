// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2026 Adrian <adrian.eddy at gmail>

//! The GoPros from before GPMF: HERO3, HERO3+ and HERO4.
//!
//! Their files have no metadata track. What the camera was and how it was set
//! is in a handful of boxes of `moov/udta`, written first in it:
//!
//! | box    | content                                                        |
//! |--------|----------------------------------------------------------------|
//! | `FIRM` | the firmware version, text: `HD4.02.03.00.00`, `HD3.01.03.00`  |
//! | `LENS` | the lens's serial number, text (HERO4)                         |
//! | `CAME` | the camera's id, binary                                        |
//! | `SETT` | the recording settings, bit-packed: 12 bytes on a HERO4, 4 on a HERO3 |
//!
//! The firmware version names the model, and on a HERO4 one field of `SETT` is
//! the field of view the clip was recorded with, which is what tells one lens
//! profile from another at the same resolution: 2.7K Medium is a 1:1 crop of
//! the sensor, with a focal length 1.41 times that of 2.7K Wide.

use memchr::memmem;

/// What the `udta` boxes say about the clip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Atoms {
    /// `FIRM`, without its padding.
    pub firmware: String,
    /// `SETT`, verbatim.
    pub settings: Option<Vec<u8>>,
}

/// The boxes, found in the part of the file that was read for detecting it.
///
/// They are looked for by `FIRM` rather than by walking the file's boxes,
/// because the buffer is the file's two ends glued together and `moov` is at
/// either: at the start of a HERO3's file, at the end of a HERO4's.
pub(crate) fn find(buffer: &[u8]) -> Option<Atoms> {
    memmem::find_iter(buffer, b"FIRM")
        .filter_map(|pos| buffer.get(pos.checked_sub(4)?..))
        .find_map(read)
}

/// `FIRM` and the boxes after it, from `data` that starts at `FIRM`'s size.
fn read(data: &[u8]) -> Option<Atoms> {
    let mut atoms: Option<Atoms> = None;
    let mut pos = 0usize;
    // `SETT` is the fourth box of a HERO4's `udta` and the third of a HERO3's.
    for _ in 0..8 {
        let Some(header) = data.get(pos..pos + 8) else { break };
        let size = u32::from_be_bytes(header[..4].try_into().ok()?) as usize;
        let Some(payload) = (size >= 8).then(|| data.get(pos + 8..pos + size)).flatten() else { break };
        match (&header[4..], &mut atoms) {
            (b"FIRM", None) => atoms = Some(Atoms { firmware: firmware(payload)?, settings: None }),
            (_, None) => return None,
            (b"SETT", Some(atoms)) => {
                atoms.settings = Some(payload.to_vec());
                break;
            }
            _ => {}
        }
        pos += size;
    }
    atoms
}

/// The firmware version of a `FIRM` payload, or `None` for bytes that are not
/// one: `HD`, the camera generation, then groups of digits (`HD4.02.03.00.00`,
/// `HDX.01.02.00.00`).
fn firmware(payload: &[u8]) -> Option<String> {
    let end = payload.iter().position(|b| *b == 0).unwrap_or(payload.len());
    let text = std::str::from_utf8(&payload[..end]).ok()?;
    let rest = text.strip_prefix("HD")?;
    let mut chars = rest.chars();
    let valid = text.len() <= 32
        && chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.next() == Some('.')
        && rest.len() >= 4
        && chars.all(|c| c.is_ascii_digit() || c == '.');
    valid.then(|| text.to_owned())
}

/// The model a firmware version belongs to, as the cameras with GPMF name
/// themselves (`HERO5 Black`), by the first two groups of the version: every
/// GoPro model has its own firmware line.
///
/// Read from files: `HD4.02` (HERO4 Black, which also names itself in the
/// `GPRO` block at the start of `mdat`) and `HD3.01` (HERO3 White). The others
/// are the versions of GoPro's firmware updates for those models.
pub(crate) fn model(firmware: &str) -> Option<&'static str> {
    Some(match firmware.get(..6)? {
        "HD3.01" => "HERO3 White",
        "HD3.02" => "HERO3 Silver",
        "HD3.03" => "HERO3 Black",
        "HD3.10" => "HERO3+ Silver",
        "HD3.11" => "HERO3+ Black",
        "HD4.01" => "HERO4 Silver",
        "HD4.02" => "HERO4 Black",
        "HDX.01" => "HERO4 Session",
        _ => return None,
    })
}

/// The field of view a HERO4 Black or Silver recorded with, as GPMF's `VFOV`
/// has it (`W`, `M`), or `None` when the file doesn't say, or says it with a
/// code that isn't known.
///
/// It is the low four bits of the 8th byte of the 12-byte `SETT`: 0 is Wide, 2
/// is Medium. Both were established on HERO4 Black clips by measuring the lens
/// from the picture: at 2.7K, clips with 0 have the focal length of 4K (which
/// the camera records in Wide only) scaled to the frame, and clips with 2 have
/// it at the sensor's own scale, which is what the 1:1 crop of Medium is. 4
/// was seen at 1080p only and is probably Narrow (the camera has Wide, Medium
/// and Narrow there), but wasn't measured, so it isn't named here.
///
/// The other cameras aren't read: a HERO3's `SETT` is 4 bytes with another
/// layout, and no HERO4 Session's was looked at.
pub(crate) fn field_of_view(atoms: &Atoms) -> Option<&'static str> {
    if !atoms.firmware.starts_with("HD4.") {
        return None;
    }
    let settings = atoms.settings.as_deref().filter(|s| s.len() == 12)?;
    match settings[7] & 0x0f {
        0 => Some("W"),
        2 => Some("M"),
        code => {
            log::debug!("Unknown HERO4 field of view {code} in SETT {}", crate::util::to_hex(settings));
            None
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn atom(name: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut out = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(name);
        out.extend_from_slice(payload);
        out
    }

    fn padded(text: &str, len: usize) -> Vec<u8> {
        let mut out = text.as_bytes().to_vec();
        out.resize(len, 0);
        out
    }

    fn hex(text: &str) -> Vec<u8> {
        (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap()).collect()
    }

    /// The `udta` of a HERO4 Black clip (firmware HD4.02.03.00.00), box for
    /// box as the camera writes it, with `SETT` as given. The serial numbers
    /// (`LENS`, `CAME`, `MUID`) are made up, and the boxes after `MUID`
    /// (`HMMT`, `BCID`, `free`) are the zeroes they are in the file.
    pub(crate) fn hero4_udta(firmware: &str, settings: &str) -> Vec<u8> {
        let body = [
            atom(b"FIRM", firmware.as_bytes()),
            atom(b"LENS", &padded("LAT0000000000000", 48)),
            atom(b"CAME", &[0x11; 16]),
            atom(b"SETT", &hex(settings)),
            atom(b"AMBA", &hex(&format!("{:0<240}", "00040003010178000000000200003aa7000afc8001c9c38001c9c380"))),
            atom(b"MUID", &[0x11; 32]),
            atom(b"HMMT", &[0; 404]),
            atom(b"BCID", &[0; 36]),
            atom(b"free", &[0; 124]),
        ].concat();
        atom(b"udta", &body)
    }

    /// The `udta` of a HERO3 White clip (firmware HD3.01.03.00), likewise.
    pub(crate) fn hero3_udta(settings: &str) -> Vec<u8> {
        let body = [
            atom(b"FIRM", b"HD3.01.03.00"),
            atom(b"CAME", &hex("1111111100000000")),
            atom(b"SETT", &hex(settings)),
            atom(b"AMBA", &hex(&format!("{:0<240}", "0010000901010f0000000004000017760002bf2000e4e1c000e4e1c00000000001"))),
            atom(b"free", &[0; 192]),
        ].concat();
        atom(b"udta", &body)
    }

    /// The boxes as a file's ends are searched for them: after other bytes.
    fn found(udta: &[u8]) -> Option<Atoms> {
        find(&[&[0x5a; 100][..], udta].concat())
    }

    #[test]
    fn hero4_field_of_view_is_read_from_sett() {
        // The 4K Wide and the 2.7K Medium clip the mapping was measured on, and
        // the other values of the bytes around the field seen in 292 clips
        for (settings, expected) in [
            ("400000000000458000000000", Some("W")),
            ("400000000000458200000000", Some("M")),
            ("c00000000000459000000000", Some("W")),
            ("c00000000000459200000000", Some("M")),
            ("40000000000045a000000000", Some("W")),
            ("000000000000459000000000", Some("W")),
            // Probably Narrow, not measured
            ("400000000000458400000000", None),
            ("c00000000000459400000000", None),
        ] {
            let atoms = found(&hero4_udta("HD4.02.03.00.00", settings)).unwrap();
            assert_eq!(atoms.firmware, "HD4.02.03.00.00");
            assert_eq!(atoms.settings.as_deref(), Some(&hex(settings)[..]));
            assert_eq!(field_of_view(&atoms), expected, "{settings}");
            assert_eq!(model(&atoms.firmware), Some("HERO4 Black"));
        }
    }

    #[test]
    fn hero3_names_its_model_and_no_field_of_view() {
        for settings in ["03e00020", "03e00220"] {
            let atoms = found(&hero3_udta(settings)).unwrap();
            assert_eq!(atoms.firmware, "HD3.01.03.00");
            assert_eq!(model(&atoms.firmware), Some("HERO3 White"));
            assert_eq!(field_of_view(&atoms), None);
        }
    }

    #[test]
    fn models_by_firmware_line() {
        assert_eq!(model("HD4.01.05.00.00"), Some("HERO4 Silver"));
        assert_eq!(model("HD4.02.05.00.00"), Some("HERO4 Black"));
        assert_eq!(model("HDX.01.02.00.00"), Some("HERO4 Session"));
        assert_eq!(model("HD3.11.03.03"), Some("HERO3+ Black"));
        assert_eq!(model("HD3.21.02.00"), None);
        assert_eq!(model("HD5.0"), None);
    }

    #[test]
    fn other_bytes_are_not_these_boxes() {
        // The word in other data, at the buffer's start, and with a size that leaves the buffer
        assert_eq!(find(b"FIRM"), None);
        assert_eq!(find(b"....FIRMWARE 1.0 of some other camera"), None);
        assert_eq!(find(&[&[0, 0, 0, 16][..], b"FIRMnot one."].concat()), None);
        assert_eq!(find(&[&[0, 0, 1, 0][..], b"FIRMHD4.02.03.00.00"].concat()), None);
        // A SETT that is cut short: the camera is still named
        let udta = hero4_udta("HD4.02.03.00.00", "400000000000458200000000");
        let cut = found(&udta[..8 + 23 + 56 + 24 + 12]).unwrap();
        assert_eq!((cut.firmware.as_str(), cut.settings.as_ref(), field_of_view(&cut)), ("HD4.02.03.00.00", None, None));
        // A SETT of another length is another layout
        let short = found(&hero4_udta("HD4.02.03.00.00", "03e00220")).unwrap();
        assert_eq!(field_of_view(&short), None);
        // The Session's SETT wasn't looked at
        let session = found(&hero4_udta("HDX.01.02.00.00", "400000000000458200000000")).unwrap();
        assert_eq!(field_of_view(&session), None);
    }
}
