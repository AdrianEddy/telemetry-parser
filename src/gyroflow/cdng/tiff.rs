// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2026 Adrian <adrian.eddy at gmail>

//! Just enough of TIFF to read a DNG's metadata: the IFDs and their values, every read bounded by the file. Classic
//! TIFF only, the image data is never touched.

use std::io::*;
use byteorder::{ ByteOrder, BigEndian, LittleEndian };

/// IFD entries per IFD a DNG plausibly has (the fp writes ~70), past which the IFD is taken as broken
const MAX_IFD_ENTRIES: usize = 4096;
/// The largest value read through `Tiff::value`: enough for any tag read here (an opcode list is ~1 KB, the fp's
/// MakerNote ~70 KB), and keeps a broken count from allocating gigabytes
pub const MAX_VALUE_SIZE: usize = 4 << 20;

pub const IMAGE_WIDTH:          u16 = 0x0100;
pub const IMAGE_LENGTH:         u16 = 0x0101;
pub const MAKE:                 u16 = 0x010F;
pub const MODEL:                u16 = 0x0110;
pub const ORIENTATION:          u16 = 0x0112;
pub const SOFTWARE:             u16 = 0x0131;
pub const NEW_SUBFILE_TYPE:     u16 = 0x00FE;
pub const SUB_IFDS:             u16 = 0x014A;
pub const EXPOSURE_TIME:        u16 = 0x829A;
pub const F_NUMBER:             u16 = 0x829D;
pub const EXIF_IFD:             u16 = 0x8769;
pub const ISO:                  u16 = 0x8827;
pub const DATE_TIME_ORIGINAL:   u16 = 0x9003;
pub const SUBJECT_DISTANCE:     u16 = 0x9206;
pub const FOCAL_LENGTH:         u16 = 0x920A;
pub const MAKER_NOTE:           u16 = 0x927C;
pub const LENS_MAKE:            u16 = 0xA433;
pub const LENS_MODEL:           u16 = 0xA434;
pub const UNIQUE_CAMERA_MODEL:  u16 = 0xC614;
pub const DEFAULT_SCALE:        u16 = 0xC61E;
pub const DEFAULT_CROP_ORIGIN:  u16 = 0xC61F;
pub const DEFAULT_CROP_SIZE:    u16 = 0xC620;
pub const CAMERA_SERIAL_NUMBER: u16 = 0xC62F;
pub const ACTIVE_AREA:          u16 = 0xC68D;
pub const OPCODE_LIST3:         u16 = 0xC74E;
pub const TIME_CODES:           u16 = 0xC763;
pub const FRAME_RATE:           u16 = 0xC764;

pub struct Entry {
    pub tag: u16,
    pub kind: u16,
    pub count: u32,
    /// The value, or the offset of the value when it doesn't fit, as stored
    pub value: [u8; 4],
}

pub struct Tiff<'a, R: Read + Seek> {
    stream: &'a mut R,
    size: u64,
    big_endian: bool,
}

fn invalid(msg: &str) -> Error {
    Error::new(ErrorKind::InvalidData, msg.to_owned())
}

/// A classic TIFF (or DNG) header
pub fn is_tiff(buffer: &[u8]) -> bool {
    buffer.starts_with(b"II\x2A\0") || buffer.starts_with(b"MM\0\x2A")
}

/// Bytes of one value of a TIFF type
fn type_size(kind: u16) -> Option<usize> {
    match kind {
        1 | 2 | 6 | 7 => Some(1), // BYTE, ASCII, SBYTE, UNDEFINED
        3 | 8         => Some(2), // SHORT, SSHORT
        4 | 9 | 11 | 13 => Some(4), // LONG, SLONG, FLOAT, IFD
        5 | 10 | 12   => Some(8), // RATIONAL, SRATIONAL, DOUBLE
        _ => None
    }
}

impl<'a, R: Read + Seek> Tiff<'a, R> {
    pub fn new(stream: &'a mut R, size: u64) -> Result<Self> {
        let mut header = [0u8; 4];
        stream.seek(SeekFrom::Start(0))?;
        stream.read_exact(&mut header)?;
        let big_endian = match &header[..2] {
            b"II" => false,
            b"MM" => true,
            _ => return Err(invalid("Not a TIFF file"))
        };
        let tiff = Self { stream, size, big_endian };
        match tiff.u16(&header[2..]) {
            42 => Ok(tiff),
            43 => Err(invalid("BigTIFF is not supported")),
            _ => Err(invalid("Not a TIFF file"))
        }
    }

    fn u16(&self, x: &[u8]) -> u16 { if self.big_endian { BigEndian::read_u16(x) } else { LittleEndian::read_u16(x) } }
    fn u32(&self, x: &[u8]) -> u32 { if self.big_endian { BigEndian::read_u32(x) } else { LittleEndian::read_u32(x) } }
    fn u64(&self, x: &[u8]) -> u64 { if self.big_endian { BigEndian::read_u64(x) } else { LittleEndian::read_u64(x) } }

    pub fn read_at(&mut self, offset: u64, len: usize) -> Result<Vec<u8>> {
        if offset.checked_add(len as u64).is_none_or(|end| end > self.size) {
            return Err(invalid("TIFF value past the end of the file"));
        }
        let mut data = vec![0u8; len];
        self.stream.seek(SeekFrom::Start(offset))?;
        self.stream.read_exact(&mut data)?;
        Ok(data)
    }

    pub fn first_ifd(&mut self) -> Result<u64> {
        let x = self.read_at(4, 4)?;
        Ok(self.u32(&x) as u64)
    }

    pub fn ifd(&mut self, offset: u64) -> Result<Vec<Entry>> {
        let count = self.read_at(offset, 2)?;
        let count = self.u16(&count) as usize;
        if count > MAX_IFD_ENTRIES {
            return Err(invalid("TIFF IFD has too many entries"));
        }
        let data = self.read_at(offset + 2, count * 12)?;
        Ok(data.chunks_exact(12).map(|x| Entry {
            tag:   self.u16(&x[0..2]),
            kind:  self.u16(&x[2..4]),
            count: self.u32(&x[4..8]),
            value: x[8..12].try_into().unwrap(),
        }).collect())
    }

    /// The IFD a LONG or IFD entry points to
    pub fn sub_ifd(&mut self, entry: &Entry) -> Result<Vec<Entry>> {
        if !matches!(entry.kind, 4 | 13) || entry.count != 1 {
            return Err(invalid("TIFF IFD pointer is not a single offset"));
        }
        let offset = self.u32(&entry.value) as u64;
        self.ifd(offset)
    }

    /// The raw bytes of an entry's values
    pub fn value(&mut self, entry: &Entry) -> Result<Vec<u8>> {
        let size = type_size(entry.kind).ok_or_else(|| invalid("Unknown TIFF type"))?;
        let len = (entry.count as usize).checked_mul(size).filter(|x| *x <= MAX_VALUE_SIZE).ok_or_else(|| invalid("TIFF value too large"))?;
        if len <= 4 {
            Ok(entry.value[..len].to_vec())
        } else {
            let offset = self.u32(&entry.value) as u64;
            self.read_at(offset, len)
        }
    }

    /// Where an entry's values are in the file, when they don't fit in the entry
    pub fn value_offset(&self, entry: &Entry) -> Option<u64> {
        let len = (entry.count as usize).checked_mul(type_size(entry.kind)?)?;
        (len > 4).then(|| self.u32(&entry.value) as u64)
    }

    pub fn text(&mut self, entry: &Entry) -> Option<String> {
        if entry.kind != 2 { return None; }
        let data = self.value(entry).ok()?;
        let end = data.iter().position(|x| *x == 0).unwrap_or(data.len());
        Some(String::from_utf8_lossy(&data[..end]).trim().to_owned()).filter(|x| !x.is_empty())
    }

    /// The values of a numeric entry, rationals divided out (a zero denominator gives NaN)
    pub fn numbers(&mut self, entry: &Entry) -> Option<Vec<f64>> {
        let size = type_size(entry.kind)?;
        let data = self.value(entry).ok()?;
        Some(data.chunks_exact(size).map(|x| match entry.kind {
            1 | 7 => x[0] as f64,
            6     => x[0] as i8 as f64,
            3     => self.u16(x) as f64,
            8     => self.u16(x) as i16 as f64,
            4 | 13 => self.u32(x) as f64,
            9     => self.u32(x) as i32 as f64,
            5     => self.u32(&x[..4]) as f64 / self.u32(&x[4..]) as f64,
            10    => self.u32(&x[..4]) as i32 as f64 / self.u32(&x[4..]) as i32 as f64,
            11    => f32::from_bits(self.u32(x)) as f64,
            12    => f64::from_bits(self.u64(x)),
            _     => f64::NAN,
        }).collect())
    }
}

pub fn find(entries: &[Entry], tag: u16) -> Option<&Entry> {
    entries.iter().find(|x| x.tag == tag)
}

/// What a DNG frame tells about itself, from its TIFF and Exif tags. Missing or broken tags are `None`.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct DngInfo {
    pub make: Option<String>,
    pub model: Option<String>,
    pub unique_camera_model: Option<String>,
    pub software: Option<String>,
    pub serial_number: Option<String>,
    pub lens_make: Option<String>,
    pub lens_model: Option<String>,
    pub date_time_original: Option<String>,
    /// TIFF orientation, 1 = upright
    pub orientation: Option<u16>,
    /// The size of the full-resolution image
    pub image_size: Option<(u32, u32)>,
    /// top, left, bottom, right
    pub active_area: Option<[u32; 4]>,
    /// Relative to the active area, before the default scale
    pub default_crop_origin: Option<(f64, f64)>,
    pub default_crop_size: Option<(f64, f64)>,
    /// How the picture is to be resampled, horizontally and vertically (anamorphic or binned sensors)
    pub default_scale: Option<(f64, f64)>,
    pub frame_rate: Option<f64>,
    /// SMPTE timecode, `hh:mm:ss:ff`
    pub timecode: Option<String>,
    pub exposure_time_s: Option<f64>,
    pub iso: Option<u32>,
    pub f_number: Option<f64>,
    pub focal_length_mm: Option<f64>,
    pub subject_distance_m: Option<f64>,
    /// The WarpRectilinear opcode: the distortion the camera corrects, for this frame's focus
    pub warp_rectilinear: Option<super::lensfit::WarpRectilinear>,
}

/// The two pictures of a DNG frame. A decoder shows either: the whole image, its active area (FFmpeg), or the default
/// crop of it, the picture the DNG defines (Resolve, Adobe)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pictures {
    /// The active area's size, in pixels. It is also the image the opcodes in OpcodeList3 apply to
    pub image: (u32, u32),
    /// Where the default crop is in the active area
    pub crop_origin: (f64, f64),
    pub crop_size: (u32, u32),
}

impl Pictures {
    pub fn image_f64(&self) -> (f64, f64) { (self.image.0 as f64, self.image.1 as f64) }
    pub fn crop_f64(&self) -> (f64, f64) { (self.crop_size.0 as f64, self.crop_size.1 as f64) }
}

impl DngInfo {
    pub fn read<R: Read + Seek>(tiff: &mut Tiff<R>, ifd0: &[Entry], exif: &[Entry]) -> Self {
        let mut text = |ifd: &[Entry], tag| find(ifd, tag).and_then(|x| tiff.text(x));
        let make                = text(ifd0, MAKE);
        let model               = text(ifd0, MODEL);
        let unique_camera_model = text(ifd0, UNIQUE_CAMERA_MODEL);
        let software            = text(ifd0, SOFTWARE);
        let serial_number       = text(ifd0, CAMERA_SERIAL_NUMBER);
        let lens_make           = text(exif, LENS_MAKE);
        let lens_model          = text(exif, LENS_MODEL);
        let date_time_original  = text(exif, DATE_TIME_ORIGINAL);

        let mut numbers = |ifd: &[Entry], tag| find(ifd, tag).and_then(|x| tiff.numbers(x)).filter(|x| x.iter().all(|v| v.is_finite()));
        let positive = |x: Option<Vec<f64>>| x.and_then(|x| x.first().copied()).filter(|x| *x > 0.0);
        let pair = |x: Option<Vec<f64>>| x.filter(|x| x.len() == 2).map(|x| (x[0], x[1]));

        let mut image_size = pair(numbers(ifd0, IMAGE_WIDTH).zip(numbers(ifd0, IMAGE_LENGTH)).map(|(w, h)| [w, h].concat()));
        let active_area = numbers(ifd0, ACTIVE_AREA).filter(|x| x.len() == 4).map(|x| [x[0] as u32, x[1] as u32, x[2] as u32, x[3] as u32]);
        let default_crop_origin = pair(numbers(ifd0, DEFAULT_CROP_ORIGIN));
        let default_crop_size   = pair(numbers(ifd0, DEFAULT_CROP_SIZE));
        let default_scale       = pair(numbers(ifd0, DEFAULT_SCALE));
        let frame_rate          = positive(numbers(ifd0, FRAME_RATE));
        let orientation         = positive(numbers(ifd0, ORIENTATION)).map(|x| x as u16);
        let exposure_time_s     = positive(numbers(exif, EXPOSURE_TIME));
        let iso                 = positive(numbers(exif, ISO)).map(|x| x.round() as u32);
        let f_number            = positive(numbers(exif, F_NUMBER));
        let focal_length_mm     = positive(numbers(exif, FOCAL_LENGTH));
        let subject_distance_m  = positive(numbers(exif, SUBJECT_DISTANCE));

        // IFD0 of a CinemaDNG frame is the raw image; a DNG that keeps a preview there has the raw image in a SubIFD
        let is_preview = |ifd: &[Entry], tiff: &mut Tiff<R>| find(ifd, NEW_SUBFILE_TYPE).and_then(|x| tiff.numbers(x)).is_some_and(|x| x.first() == Some(&1.0));
        if is_preview(ifd0, tiff) {
            let sub_ifds = find(ifd0, SUB_IFDS).and_then(|x| tiff.numbers(x)).unwrap_or_default();
            for offset in sub_ifds {
                let Ok(ifd) = tiff.ifd(offset as u64) else { continue };
                if !is_preview(&ifd, tiff) {
                    let w = find(&ifd, IMAGE_WIDTH).and_then(|x| tiff.numbers(x)).and_then(|x| x.first().copied());
                    let h = find(&ifd, IMAGE_LENGTH).and_then(|x| tiff.numbers(x)).and_then(|x| x.first().copied());
                    image_size = w.zip(h);
                    break;
                }
            }
        }

        let timecode = find(ifd0, TIME_CODES).and_then(|x| tiff.value(x).ok()).and_then(|x| smpte_timecode(&x));
        let warp_rectilinear = find(ifd0, OPCODE_LIST3).and_then(|x| tiff.value(x).ok()).and_then(|x| super::lensfit::warp_rectilinear(&x));

        Self {
            make, model, unique_camera_model, software, serial_number, lens_make, lens_model, date_time_original, orientation,
            image_size: image_size.filter(|(w, h)| *w >= 1.0 && *h >= 1.0).map(|(w, h)| (w as u32, h as u32)),
            active_area, default_crop_origin, default_crop_size, default_scale, frame_rate, timecode, exposure_time_s, iso, f_number,
            focal_length_mm, subject_distance_m, warp_rectilinear,
        }
    }
}

impl DngInfo {
    /// The frame's pictures: its active area (the whole image without one) and its default crop of it (the whole active
    /// area without one). A default scale other than 1 resamples the crop, which isn't a crop of the image any more,
    /// so the picture is then taken as the active area alone
    pub fn pictures(&self) -> Option<Pictures> {
        let image = self.active_area
            .filter(|[t, l, b, r]| b > t && r > l && self.image_size.is_none_or(|(w, h)| *r <= w && *b <= h))
            .map(|[t, l, b, r]| (r - l, b - t))
            .or(self.image_size)?;
        let whole = Pictures { image, crop_origin: (0.0, 0.0), crop_size: image };
        if self.default_scale.is_some_and(|(x, y)| (x - 1.0).abs() > 1e-6 || (y - 1.0).abs() > 1e-6) {
            return Some(whole);
        }
        let Some((w, h)) = self.default_crop_size else { return Some(whole) };
        let (x, y) = self.default_crop_origin.unwrap_or((0.0, 0.0));
        let inside = x >= 0.0 && y >= 0.0 && w >= 1.0 && h >= 1.0 && x + w <= image.0 as f64 + 1e-6 && y + h <= image.1 as f64 + 1e-6;
        Some(if inside { Pictures { image, crop_origin: (x, y), crop_size: (w.round() as u32, h.round() as u32) } } else { whole })
    }
}

/// The first of the DNG TimeCodes (SMPTE 12M: frames, seconds, minutes, hours in BCD, then the user bits)
fn smpte_timecode(x: &[u8]) -> Option<String> {
    let bcd = |v: u8| (v >> 4) as u32 * 10 + (v & 0x0F) as u32;
    let x = x.get(..4)?;
    Some(format!("{:02}:{:02}:{:02}:{:02}", bcd(x[3] & 0x3F), bcd(x[2] & 0x7F), bcd(x[1] & 0x7F), bcd(x[0] & 0x3F)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timecode_is_bcd_with_flags_masked() {
        // 00:36:15:24 with the drop-frame and colour-frame flags set
        assert_eq!(smpte_timecode(&[0x24 | 0xC0, 0x15, 0x36, 0x00, 0, 0, 0, 0]).as_deref(), Some("00:36:15:24"));
        assert_eq!(smpte_timecode(&[0x01, 0x02]), None);
    }
}
