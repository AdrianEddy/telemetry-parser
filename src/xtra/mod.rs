// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2025 Adrian <adrian.eddy at gmail>

pub mod dvtm_ae2503;
pub mod dvtm_ae2504;

use std::io::*;
use std::sync::{ Arc, atomic::AtomicBool };

use crate::tags_impl::*;
use crate::*;
use crate::util::insert_tag;
use crate::dji::timing::{ self, HeldQuaternions };
use byteorder::{ BigEndian, ReadBytesExt };
use memchr::memmem;
use prost::Message;

#[derive(Default)]
pub struct Xtra {
    pub model: Option<String>,
    pub frame_readout_time: Option<f64>
}

/// The product schema (product proto file) a clip's metadata is written in
#[derive(PartialEq, Debug, Clone, Copy)]
enum DeviceProtobuf {
    Ae2503,
    Ae2504,
}
impl DeviceProtobuf {
    /// The schema of a product proto file, which is named `dvtm_<product>.proto` (XTRA white paper §2.1.9)
    fn from_proto_file(name: &str) -> Option<Self> {
        let product = name.trim();
        let product = product.strip_prefix("dvtm_").unwrap_or(product);
        let product = product.strip_suffix(".proto").unwrap_or(product);
        match product.to_ascii_lowercase().as_str() {
            "ae2503" => Some(Self::Ae2503),
            "ae2504" => Some(Self::Ae2504),
            _ => None
        }
    }
}

/// What the parser reads of a sample before it knows the product schema: the clip metadata header, which only a
/// clip's first sample has (XTRA white paper §4.1). It is at the same fields in every product schema, from the shared
/// library (dvtm_library.proto)
#[derive(Clone, PartialEq, ::prost::Message)]
struct SampleProbe {
    #[prost(message, optional, tag = "1")]
    clip_meta: Option<ClipMetaProbe>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
struct ClipMetaProbe {
    #[prost(message, optional, tag = "1")]
    clip_meta_header: Option<ClipMetaHeaderProbe>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
struct ClipMetaHeaderProbe {
    #[prost(string, tag = "1")]
    proto_file_name: String,
}

/// The longest entry of the `Xtra` box that is read for the product schema it names: its text tags are a few hundred
/// bytes, but Windows can write a picture tag there too
const MAX_XTRA_ENTRY: u64 = 1 << 16;

impl Xtra {
    pub fn camera_type(&self) -> String {
        "Xtra".to_owned()
    }
    pub fn has_accurate_timestamps(&self) -> bool {
        true
    }
    pub fn possible_extensions() -> Vec<&'static str> {
        vec!["mp4", "mov"]
    }
    pub fn frame_readout_time(&self) -> Option<f64> {
        self.frame_readout_time
    }
    pub fn normalize_imu_orientation(v: String) -> String {
        v
    }

    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], _filepath: P, _options: &crate::InputOptions) -> Option<Self> {
        if memmem::find(buffer, b"xtmd").is_some() && (memmem::find(buffer, b"CAM meta").is_some()) {
            Some(Self {
                model: None,
                frame_readout_time: None
            })
        } else {
            None
        }
    }

    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, size: usize, progress_cb: F, cancel_flag: Arc<AtomicBool>, options: crate::InputOptions) -> Result<Vec<SampleInfo>> {
        // The product schema the whole file declares, for a clip whose first sample doesn't
        let declared_proto_file = Self::declared_proto_file(stream, size);
        stream.seek(SeekFrom::Start(0))?;

        let mut samples = Vec::new();
        // Every frame's sample, its time in the video and its camera timestamp from the first one's: a clip without the
        // clip metadata is timed at the rate of this clock (`timing::sensor_rate`)
        let mut frames: Vec<timing::Frame> = Vec::new();
        // The frames' quaternions, timed once the whole track is read and the frame rates are known
        let mut held_quaternions: Vec<HeldQuaternions> = Vec::new();
        // The first frame's camera timestamp and its time in the video, which the frames are timed from
        let mut first_timestamp: Option<(i64, f64)> = None;

        let mut focal_length = None;
        let mut distortion_coeffs = None;
        let mut fps = 59.94;
        let mut sensor_fps = 59.969295501708984;
        // Only the first sample of the track has the clip metadata (frame rates, readout time, lens)
        let mut clip_meta_read = false;
        // let mut global_quat_i = 0;

        // let mut first_vsync = 0;
        let mut prev_quat: Option<Quaternion<f64>> = None;
        let mut inv = false;

        // Chosen at the track's first sample: the schema it declares, else the file's (`declared_proto_file`)
        let mut which_proto: Option<DeviceProtobuf> = None;
        let mut first_sample = true;
        // The video's frame rate as the track's reader gives it, for a clip without the clip metadata
        let mut video_fps = None;

        let cancel_flag2 = cancel_flag.clone();

        macro_rules! handle_parsed {
            ($info:ident, $parsed:expr, $field:tt) => {
                let mut tag_map = GroupedTagMap::new();

                if let Some(ref clip) = $parsed.clip_meta {
                    clip_meta_read = true;
                    self.model              = clip.clip_meta_header       .as_ref().map(|h| h.product_name.replace("XTRA ", "").replace("Xtra ", ""));
                    self.frame_readout_time = clip.sensor_readout_time    .as_ref().map(|h| h.readout_time as f64 / 1000_000.0);
                    focal_length            = clip.digital_focal_length   .as_ref().map(|h| h.focal_length as f64);
                    distortion_coeffs       = clip.distortion_coefficients.as_ref().map(|h| h.coeffients.clone());

                    if let Some(v) = clip.sensor_fps.as_ref().map(|h| h.sensor_frame_rate as f64) {
                        sensor_fps = v;
                    }

                    let v = serde_json::to_value(&clip).map_err(|_| Error::new(ErrorKind::Other, "Serialize error"));
                    if let Ok(vv) = v {
                        log::debug!("Metadata: {:?}", &vv);
                        insert_tag(&mut tag_map, tag!(parsed GroupId::Default, TagId::Metadata, "Metadata", Json, |v| serde_json::to_string(v).unwrap(), vv, vec![]), &options);
                    }
                    if let Some(ref stream) = $parsed.stream_meta {
                        if let Some(ref meta) = stream.video_stream_meta {
                            fps = meta.framerate as f64;
                        }
                    }
                    if let Some(ref mut v) = self.frame_readout_time {
                        *v /= fps / sensor_fps;
                    }
                }

                // A frame without its header has no timestamp to be timed by
                if let Some((frame, header)) = $parsed.frame_meta.as_ref().and_then(|frame| Some((frame, frame.frame_meta_header.as_ref()?))) {
                    let frame_ts = header.frame_timestamp as i64;
                    // The frames are timed from the first one, whichever entry of the track holds it, which is at its own
                    // time in the video: after a first entry the track didn't deliver, the frames aren't a frame early
                    let (first_frame_ts, _) = *first_timestamp.get_or_insert((frame_ts, $info.timestamp_ms));
                    let frame_relative_ts = frame_ts - first_frame_ts;
                    frames.push(timing::Frame { sample: $info.sample_index, ms: $info.timestamp_ms, us: frame_relative_ts });

                    if let Some(ref imu) = frame.imu_frame_meta {
                        if let Some(ref attitude) = imu.$field {
                            let len = attitude.attitude.len() as f64;
                            let mut readings = Vec::new();

                            for (i, q) in attitude.attitude.iter().enumerate() {
                                // The reading's time in the frame, in frames
                                let in_frame = (i as f64 - attitude.offset as f64) / len;

                                if q.quaternion_w.is_nan() || q.quaternion_x.is_nan() || q.quaternion_y.is_nan() || q.quaternion_z.is_nan() {
                                    continue;
                                }

                                let quat = util::multiply_quats(
                                    (q.quaternion_w as f64,
                                    q.quaternion_x as f64,
                                    q.quaternion_y as f64,
                                    q.quaternion_z as f64),
                                    (0.5, -0.5, -0.5, 0.5),
                                );
                                // Rotate Y axis 180 deg for horizon lock
                                let quat = util::multiply_quats((0.0, 0.0, 1.0, 0.0), (quat.w, quat.x, quat.y, quat.z));

                                if quat.w == 0.0 && quat.x == 0.0 && quat.y == 0.0 && quat.z == 0.0 {
                                    continue;
                                }

                                if prev_quat.is_some() && (prev_quat.unwrap() - quat).norm_squared().sqrt() > 1.5 {
                                    inv = !inv;
                                }
                                prev_quat = Some(quat.clone());

                                readings.push((in_frame, if inv { -quat } else { quat }));
                            }
                            held_quaternions.push(HeldQuaternions { sample: samples.len(), frame_relative_us: frame_relative_ts, readings });
                        }
                    }
                }

                $info.tag_map = Some(tag_map);

                samples.push($info);

                if options.probe_only {
                    cancel_flag2.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            };
        }

        // A sample in the clip's product schema
        macro_rules! handle_sample {
            ($info:ident, $data:expr) => {
                match which_proto {
                    None => { },
                    Some(DeviceProtobuf::Ae2503) => match dvtm_ae2503::ProductMeta::decode($data) {
                        Ok(parsed) => { handle_parsed!($info, parsed, imu_attitude_after_fusion); },
                        Err(e) => { log::warn!("Failed to parse protobuf: {:?}", e); }
                    },
                    Some(DeviceProtobuf::Ae2504) => match dvtm_ae2504::ProductMeta::decode($data) {
                        Ok(parsed) => { handle_parsed!($info, parsed, imu_attitude_after_fusion); },
                        Err(e) => { log::warn!("Failed to parse protobuf: {:?}", e); }
                    },
                }
            };
        }

        let ctx = util::get_metadata_track_samples(stream, size, true, |mut info: SampleInfo, data: &[u8], file_position: u64, video_md: Option<&VideoMetadata>| {
            if size > 0 {
                progress_cb(file_position as f64 / size as f64);
            }

            if std::mem::take(&mut first_sample) {
                // The schema the clip metadata header of the first sample declares, the only place in the track that
                // says it, else the one the file declares
                let probe = SampleProbe::decode(data).ok();
                let clip_meta = probe.as_ref().and_then(|p| p.clip_meta.as_ref());
                let proto_file = clip_meta.and_then(|c| c.clip_meta_header.as_ref()).map(|h| h.proto_file_name.clone()).filter(|name| !name.is_empty())
                    .or_else(|| declared_proto_file.clone());
                which_proto = proto_file.as_deref().and_then(DeviceProtobuf::from_proto_file);
                match (&proto_file, which_proto) {
                    (_, Some(proto)) => log::debug!("Using device protobuf: {proto:?}"),
                    (Some(name), None) => log::error!("XTRA: the clip's metadata is written in {name}, a product schema this version can't read"),
                    (None, None) => log::error!("XTRA: the clip doesn't say which product schema its metadata is written in: its first metadata sample has no clip metadata, and the file no Xtra box naming the schema. Its metadata can't be read"),
                }
                video_fps = video_md.map(|v| v.fps).filter(|fps| *fps > 0.0);
            }
            handle_sample!(info, data);
        }, cancel_flag)?;

        // The frames are timed at the frame rates the clip metadata states: the video's, and the sensor's, the rate of the
        // frames' own timestamps on the camera clock. Without it, at the video's exact frame rate and the rate the frames'
        // timestamps fit (`timing::sensor_rate`), in which neither a frame the track didn't deliver nor a timestamp off the
        // camera clock can pull the rate, and which doesn't count the frames by their sequence numbers
        if !clip_meta_read && which_proto.is_some() {
            let video = crate::dji::video_frame_rate(&ctx).or(video_fps);
            if let Some(v) = video { fps = v; }
            let measured_sensor_fps = timing::sensor_rate(&frames, 1000.0 / fps, None);
            if let Some(v) = measured_sensor_fps { sensor_fps = v; }
            let video = match video {
                Some(v) => format!("the video's {v:.3} fps"),
                None => format!("{fps:.3} fps, as the clip has no video frame rate either"),
            };
            let sensor = match measured_sensor_fps {
                Some(v) => format!("the sensor's {v:.6} fps, measured from the frames' timestamps"),
                None => format!("a sensor at {sensor_fps:.6} fps, as the frames' timestamps don't give its rate"),
            };
            log::warn!("XTRA: the clip metadata, which only the first sample of the track has, is missing (the sample is empty or damaged): there's no lens profile, sensor readout time or camera model. The frames are timed at {video} and {sensor}");
        }
        let first_frame_ms = first_timestamp.map_or(0.0, |(_, ms)| ms);
        for (k, h) in held_quaternions.into_iter().enumerate() {
            let Some(tag_map) = samples.get_mut(h.sample).and_then(|s| s.tag_map.as_mut()) else { continue };
            let quats = h.timed(first_frame_ms, sensor_fps, fps);
            if k == 0 { log::debug!("Quaternions: {:?}", &quats); }
            insert_tag(tag_map, tag!(parsed GroupId::Quaternion, TagId::Data, "Quaternion data", Vec_TimeQuaternion_f64, |v| format!("{:?}", v), quats, vec![]), &options);
        }

        match (samples.first_mut(), focal_length, distortion_coeffs) {
            (Some(sample), Some(focal_length), Some(coeffs)) if coeffs.len() >= 4 => {
                if let Some(tkhd) = ctx.tracks.iter().filter(|x| x.track_type == mp4parse::TrackType::Video).filter_map(|x| x.tkhd.as_ref()).next() {
                    let (w, h) = (tkhd.width >> 16, tkhd.height >> 16);

                    let profile = self.get_lens_profile(w, h, focal_length, &coeffs);
                    if let Some(ref mut tag_map) = sample.tag_map {
                        insert_tag(tag_map, tag!(parsed GroupId::Lens, TagId::Data, "Lens profile", Json, |v| serde_json::to_string(v).unwrap(), profile, vec![]), &options);
                    }
                }
            },
            _ => { }
        }

        Ok(samples)
    }

    /// The product proto file the clip's metadata is written in, as the file declares it in its `moov/udta/Xtra` box,
    /// apart from the clip metadata in the first sample of the track (XTRA white paper §4.1, §5.3.2): a Microsoft
    /// `Xtra` box whose `WM/Category` reads `pb_file:dvtm_<product>.proto;model_name:…;pb_version:…;pb_lib_version:…;`
    ///
    /// The box's entries are read one at a time (`xtra_strings`), and one longer than `MAX_XTRA_ENTRY` is skipped: a
    /// damaged size of the box, or of the boxes it is in, can't make it read more than one entry of text
    fn declared_proto_file<T: Read + Seek>(stream: &mut T, size: usize) -> Option<String> {
        let moov = find_box(stream, 0, size as u64, util::fourcc("moov"))?;
        let udta = find_box(stream, moov.0, moov.1, util::fourcc("udta"))?;
        let (mut pos, end) = find_box(stream, udta.0, udta.1, util::fourcc("Xtra"))?;
        while end.saturating_sub(pos) >= 4 {
            stream.seek(SeekFrom::Start(pos)).ok()?;
            let entry_size = u64::from(stream.read_u32::<BigEndian>().ok()?);
            if entry_size < 4 || entry_size > end - pos {
                return None;
            }
            if entry_size <= MAX_XTRA_ENTRY {
                stream.seek(SeekFrom::Start(pos)).ok()?;
                let entry = util::read_data(stream, entry_size).ok()?;
                let proto_file = xtra_strings(&entry).iter().flat_map(|value| value.split(';')).find_map(|item| item.trim().strip_prefix("pb_file:").map(str::to_owned));
                if proto_file.is_some() {
                    return proto_file;
                }
            }
            pos += entry_size;
        }
        None
    }

    fn get_lens_profile(&self, width: u32, height: u32, focal_length: f64, coeffs: &[f32]) -> serde_json::Value {
        let model = self.model.clone().unwrap_or_default();
        let half_width = width as f64 / 2.0;
        let half_height = height as f64 / 2.0;
        let output_size = Self::get_output_size(width, height);
        serde_json::json!({
            "calibrated_by": "Xtra",
            "camera_brand": "Xtra",
            "camera_model": model,
            "calib_dimension":  { "w": width, "h": height },
            "orig_dimension":   { "w": width, "h": height },
            "output_dimension": { "w": output_size.0, "h": output_size.1 },
            "frame_readout_time": self.frame_readout_time,
            "official": true,
            "fisheye_params": {
              "camera_matrix": [
                [ focal_length, 0.0, half_width ],
                [ 0.0, focal_length, half_height ],
                [ 0.0, 0.0, 1.0 ]
              ],
              "distortion_coeffs": coeffs
            },
            "sync_settings": {
              "initial_offset": 0,
              "initial_offset_inv": false,
              "search_size": 0.5,
              "max_sync_points": 5,
              "every_nth_frame": 1,
              "time_per_syncpoint": 0.6,
              "do_autosync": false
            },
            "calibrator_version": "---"
        })
    }

    fn get_output_size(width: u32, height: u32) -> (u32, u32) {
        let aspect = (width as f64 / height as f64 * 100.0) as u32;
        match aspect {
            133 => (width, (width as f64 / 1.7777777777777).round() as u32), // 4:3 -> 16:9
            _   => (width, height)
        }
    }
}

/// The contents of the first box of type `typ` among the boxes from `start` to `end` of the stream
fn find_box<T: Read + Seek>(stream: &mut T, start: u64, end: u64, typ: u32) -> Option<(u64, u64)> {
    let mut pos = start;
    while pos + 8 <= end {
        stream.seek(SeekFrom::Start(pos)).ok()?;
        let (box_typ, _, size, header_size) = util::read_box(stream).ok()?;
        // A size of 0 is a box up to the end of the one it is in
        let size = if size == 0 { end - pos } else { size };
        if size < header_size as u64 || size > end - pos {
            return None;
        }
        if box_typ == typ {
            return Some((pos + header_size as u64, pos + size));
        }
        pos += size;
    }
    None
}

/// The string values of a Microsoft `Xtra` box. It is a list of entries: the entry's size (counting itself), the
/// length of its name, the name and the number of values, each value its size (counting itself and the type), its
/// type and its data. Type 8 is a NUL-terminated UTF-16LE string
fn xtra_strings(data: &[u8]) -> Vec<String> {
    let mut strings = Vec::new();
    let mut entries = data;
    while let Some(size) = entries.get(..4).map(|mut size| size.read_u32::<BigEndian>().unwrap_or_default() as usize) {
        if size < 4 || size > entries.len() {
            break;
        }
        let (entry, rest) = entries.split_at(size);
        entries = rest;
        let _ = (|| -> Option<()> {
            let mut entry = &entry[4..];
            let name_len = entry.read_u32::<BigEndian>().ok()? as usize;
            entry = entry.get(name_len..)?;
            let values = entry.read_u32::<BigEndian>().ok()?;
            for _ in 0..values {
                let value_size = entry.read_u32::<BigEndian>().ok()? as usize;
                let value_type = entry.read_u16::<BigEndian>().ok()?;
                let value = entry.get(..value_size.checked_sub(6)?)?;
                entry = &entry[value.len()..];
                if value_type == 8 {
                    let units: Vec<u16> = value.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
                    strings.push(String::from_utf16_lossy(&units).trim_end_matches('\0').to_owned());
                }
            }
            Some(())
        })();
    }
    strings
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An entry of a Microsoft `Xtra` box: its name, and one value of type `typ`
    fn entry(name: &str, typ: u16, value: &[u8]) -> Vec<u8> {
        let mut body = (name.len() as u32).to_be_bytes().to_vec();
        body.extend(name.as_bytes());
        body.extend(1u32.to_be_bytes());
        body.extend((value.len() as u32 + 6).to_be_bytes());
        body.extend(typ.to_be_bytes());
        body.extend(value);
        [&(body.len() as u32 + 4).to_be_bytes()[..], &body].concat()
    }
    fn boxed(typ: &[u8; 4], body: &[u8]) -> Vec<u8> {
        [&(body.len() as u32 + 8).to_be_bytes()[..], typ, body].concat()
    }

    #[test]
    fn the_declared_schema_is_read_entry_by_entry() {
        let text: Vec<u8> = "pb_file:dvtm_ae2504.proto;model_name:XTRA EDGE;pb_version:1;".encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
        let category = entry("WM/Category", 8, &text);
        // A picture tag too long for an entry of text, which isn't read
        let picture = entry("WM/Picture", 1, &vec![0x5A; MAX_XTRA_ENTRY as usize]);
        let read = |data: Vec<u8>| { let size = data.len(); Xtra::declared_proto_file(&mut Cursor::new(data), size) };
        let file = boxed(b"moov", &boxed(b"udta", &boxed(b"Xtra", &[picture, category.clone()].concat())));
        let declared = read(file);
        assert_eq!(declared.as_deref(), Some("dvtm_ae2504.proto"));
        assert_eq!(declared.as_deref().and_then(DeviceProtobuf::from_proto_file), Some(DeviceProtobuf::Ae2504));

        // Damaged sizes of the boxes, which take in the rest of the file (its media data, zeros here): the entries are
        // read up to the first one that isn't one
        let mut file = boxed(b"moov", &boxed(b"udta", &boxed(b"Xtra", &category)));
        let tail = 1 << 20;
        for at in [0, 8, 16] {
            let size = u32::from_be_bytes(file[at..at + 4].try_into().unwrap()) + tail as u32;
            file[at..at + 4].copy_from_slice(&size.to_be_bytes());
        }
        file.resize(file.len() + tail, 0);
        assert_eq!(read(file.clone()).as_deref(), Some("dvtm_ae2504.proto"));
        let without = file.len() - tail - category.len();
        file.drain(without..without + category.len());
        file.resize(file.len() + category.len(), 0);
        assert_eq!(read(file), None);
    }
}
