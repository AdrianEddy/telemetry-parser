// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2022 Adrian <adrian.eddy at gmail>

use std::io::*;
use std::sync::{ Arc, atomic::AtomicBool };

use crate::tags_impl::*;
use crate::*;
use byteorder::{ ReadBytesExt, LittleEndian, BigEndian };
use memchr::memmem;

// The rate the frames were captured at, frames per second as a numerator and a denominator (`CaptureRate::of`)
#[derive(Clone, Copy)]
struct CaptureRate {
    num: u64,
    den: u64,
}

impl CaptureRate {
    // The rate the frames were captured at, from the sensor rate they state (`srte`, a numerator and a denominator) and
    // the container's frame rate (`container_rate`): the sensor rate, but an integer one in a project of a 1000/1001 rate
    // (the container's is N × 1000/1001 frames per second for an integer N, as 23.976, 29.97 and 59.94 are) names that
    // rate × 1000/1001, as such a project's rates are named. The camera writes such a clip's frames at 32.970, 59.940 and
    // 119.879 a second of both its audio's and its IMU's time for a sensor rate of 33, 60 and 120 (at 23.976, 29.97 and
    // 23.976), an integer project's at its sensor rate, and states a sensor rate of 1000/1001 as one (30000/1001 at
    // 29.97). The Blackmagic RAW SDK gives the sensor rate as the frames state it. Without one, the frames are captured at
    // the container's rate
    fn of(sensor_rate: Option<(u64, u64)>, container: Option<(u64, u64)>) -> Option<Self> {
        let of_1000_1001 = |(num, den): (u64, u64)| {
            let (num, den) = (num as u128, den as u128);
            num % den != 0 && (num * 1001) % (1000 * den) == 0
        };
        match (sensor_rate, container) {
            (Some((num, den)), Some(container)) if num % den == 0 && of_1000_1001(container) => Some(Self { num: num / den * 1000, den: 1001 }),
            (Some((num, den)), _) | (None, Some((num, den))) => Some(Self { num, den }),
            (None, None) => None,
        }
    }
    fn fps(&self) -> f64 {
        self.num as f64 / self.den as f64
    }
}

#[derive(Default)]
pub struct BlackmagicBraw {
    pub model: Option<String>,
    frame_readout_time: Option<f64>
}

impl BlackmagicBraw {
    pub fn camera_type(&self) -> String {
        if self.model.is_some() {
            "Blackmagic".to_owned()
        } else {
            "Blackmagic RAW".to_owned()
        }
    }
    pub fn has_accurate_timestamps(&self) -> bool {
        match self.model.as_deref() {
            Some("Micro Studio Camera 4K G2") => false,
            _ => true
        }
    }
    pub fn possible_extensions() -> Vec<&'static str> {
        vec!["braw"]
    }
    pub fn frame_readout_time(&self) -> Option<f64> {
        self.frame_readout_time
    }
    pub fn normalize_imu_orientation(v: String) -> String {
        v
    }

    // A clip's `moov/meta` metadata: its manufacturer, and a key of Blackmagic RAW's own, `braw_codec_bitrate` or
    // `braw_compression_ratio` (every clip has the second; firmware 6.0's, the SDK's sample clip among them, not the first)
    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], _filepath: P, _options: &crate::InputOptions) -> Option<Self> {
        if memmem::find(buffer, b"Blackmagic Design").is_some() && (memmem::find(buffer, b"braw_codec_bitrate").is_some() || memmem::find(buffer, b"braw_compression_ratio").is_some()) {
            Some(Self::default())
        } else {
            None
        }
    }

    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, size: usize, progress_cb: F, cancel_flag: Arc<AtomicBool>, options: crate::InputOptions) -> Result<Vec<SampleInfo>> {
        let mut gyro = Vec::new();
        let mut accl = Vec::new();
        // The sample of each reading, its track's index and its index in the track, for the frame it was taken during
        // (`util::READING_FRAMES`)
        let (mut gyro_samples, mut accl_samples) = (Vec::new(), Vec::new());

        let mut map = GroupedTagMap::new();

        let mut samples = Vec::new();
        // The sensor rate as the frames state it, frames per second as a numerator and a denominator
        let mut sensor_rate = None;

        let mut firmware_version = String::new();
        // let mut crop_factor = 1.0;
        if let Ok(meta) = self.parse_meta(stream, size) {
            if let Some(cam) = meta.get("camera_type").and_then(|x| x.as_str()) {
                self.model = Some(cam.trim_start_matches("Blackmagic ").to_string());
            }
            if let Some(fw) = meta.get("firmware_version").and_then(|x| x.as_str()) {
                firmware_version = fw.to_string();
            }
            if let Some(v) = meta.get("crop_origin").and_then(|v| v.as_array()).and_then(|x| Some((x.get(0)?.as_f64()? as f32, x.get(1)?.as_f64()? as f32))) {
                util::insert_tag(&mut map, tag!(parsed GroupId::Imager, TagId::CaptureAreaOrigin, "Capture area origin", f32x2, |v| format!("{v:?}"), v, vec![]), &options);
            }
            // The sensor area the decoded picture shows (`sensor_area`), from `crop_origin`
            if let Some((w, h)) = Self::sensor_area(&meta) {
                util::insert_tag(&mut map, tag!(parsed GroupId::Imager, TagId::CaptureAreaSize, "Capture area size", f32x2, |v| format!("{v:?}"), (w as f32, h as f32), vec![]), &options);
            }
            match self.model.as_deref() {
                Some("Pocket Cinema Camera 6K Pro") |
                Some("Pocket Cinema Camera 6K G2") |
                Some("Pocket Cinema Camera 6K") => {
                    util::insert_tag(&mut map, tag!(parsed GroupId::Imager, TagId::PixelPitch, "Pixel pitch", u32x2, |v| format!("{v:?}"), (3759, 3759), vec![]), &options);
                    // crop_factor = 1.5;
                },
                Some("Pocket Cinema Camera 4K") => {
                    util::insert_tag(&mut map, tag!(parsed GroupId::Imager, TagId::PixelPitch, "Pixel pitch", u32x2, |v| format!("{v:?}"), (4628, 4628), vec![]), &options);
                    // crop_factor = 2.0;
                },
                Some("Micro Studio Camera 4K G2") => {
                    // TODO: this is not confirmed
                    util::insert_tag(&mut map, tag!(parsed GroupId::Imager, TagId::PixelPitch, "Pixel pitch", u32x2, |v| format!("{v:?}"), (4628, 4628), vec![]), &options);
                    // crop_factor = 2.0;
                },
                _ => { }
            }

            util::insert_tag(&mut map, tag!(parsed GroupId::Default, TagId::Metadata, "Metadata", Json, |v| serde_json::to_string(v).unwrap(), meta, vec![]), &options);
        }

        let video = util::get_track_samples(stream, size, mp4parse::TrackType::Video, true, Some(8192), |mut info: SampleInfo, data: &[u8], file_position: u64, _video_md: Option<&VideoMetadata>| {
            if size > 0 {
                progress_cb(file_position as f64 / size as f64 / 3.0);
            }
            if let Ok(md) = Self::parse_per_frame_meta(data) {
                let mut map = GroupedTagMap::new();

                if let Some(v) = md.get("sensor_rate").and_then(|v| v.as_array()) {
                    if v.len() == 2 {
                        sensor_rate = v[0].as_u64().zip(v[1].as_u64()).map(|(a, b)| (a, b.max(1)));
                    }
                }
                if let Some(v) = md.get("focal_length").and_then(|v| v.as_str()) {
                    let v = v.replace("mm", "");
                    if let Ok(v) = v.parse::<f32>() {
                        util::insert_tag(&mut map, tag!(parsed GroupId::Lens, TagId::FocalLength, "Focal length", f32, |v| format!("{v:.2} mm"), v, vec![]), &options);
                    }
                }

                util::insert_tag(&mut map, tag!(parsed GroupId::Default, TagId::Metadata, "Metadata", Json, |v| serde_json::to_string(v).unwrap(), md, vec![]), &options);
                info.tag_map = Some(map);
                samples.push(info);
                if options.probe_only {
                    cancel_flag.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
        }, cancel_flag.clone());

        // The readings are timed on the capture timeline, as the sensor captures the frames: frame k from k / the rate
        // they were captured at (`CaptureRate::of`), which is the clip's frame rate when its frames state a sensor rate.
        // Without a rate the frames aren't known
        let capture_rate = CaptureRate::of(sensor_rate, video.as_ref().ok().and_then(Self::container_rate));
        if let Some(fr) = capture_rate.filter(|_| sensor_rate.is_some()).map(|rate| rate.fps()) {
            util::insert_tag(&mut map, tag!(parsed GroupId::Default, TagId::FrameRate, "Frame rate", f64, |v| format!("{:?}", v), fr, vec![]), &options);
        }
        let cancel_flag2 = cancel_flag.clone();
        // A reading is timed by its sample, on the clock of the frames (the Blackmagic RAW SDK's motion sample i is at
        // i / its rate on the frames' timeline): a frame's time is when its first row is imaged, the middle of the row's
        // exposure whatever the shutter (gyroflow's sync on the frames the SDK decodes finds the readings within a few
        // ms of it, with no term of the exposure, from 2.5 to 20.8 ms on the Pocket Cinema Camera 6K Pro). The decoded
        // picture's middle row is imaged half its readout time later (`readout_time`), and that is the frame's time to
        // the stabilization, so the readings are moved back by it
        let ctx = util::get_metadata_track_samples(stream, size, false, |info: SampleInfo, data: &[u8], file_position: u64, _video_md: Option<&VideoMetadata>| {
            if size > 0 {
                progress_cb(((info.track_index as f64 - 1.0) + (file_position as f64 / size as f64)) / 3.0);
            }

            if data.len() >= 4+4+3*4 {
                let mut d = Cursor::new(data);
                crate::try_block!({
                    d.seek(SeekFrom::Start(8)).ok()?;
                    if &data[4..8] == b"mogy" {
                        gyro.push(TimeVector3 { t: (info.timestamp_ms - self.frame_readout_time.unwrap_or(0.0) / 2.0) / 1000.0,
                            x: d.read_f32::<LittleEndian>().ok()? as f64,
                            y: d.read_f32::<LittleEndian>().ok()? as f64,
                            z: d.read_f32::<LittleEndian>().ok()? as f64
                        });
                        gyro_samples.push((info.track_index, info.sample_index));
                    } else if &data[4..8] == b"moac" {
                        accl.push(TimeVector3 { t: (info.timestamp_ms - self.frame_readout_time.unwrap_or(0.0) / 2.0) / 1000.0,
                            x: -d.read_f32::<LittleEndian>().ok()? as f64,
                            y: -d.read_f32::<LittleEndian>().ok()? as f64,
                            z: -d.read_f32::<LittleEndian>().ok()? as f64
                        });
                        accl_samples.push((info.track_index, info.sample_index));
                    }
                });
            }
            if options.probe_only {
                cancel_flag2.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }, cancel_flag)?;


        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Data, "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), accl, vec![]), &options);
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Data, "Gyroscope data",     Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]), &options);
        if let Some(rate) = capture_rate {
            if let Some(accl_frames) = Self::reading_frames(&ctx, &accl_samples, rate) {
                util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Custom(util::READING_FRAMES.into()), "Frame of each reading", Vec_u64, |v| format!("{:?}", v), accl_frames, Vec::new()), &options);
            }
            if let Some(gyro_frames) = Self::reading_frames(&ctx, &gyro_samples, rate) {
                util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Custom(util::READING_FRAMES.into()), "Frame of each reading", Vec_u64, |v| format!("{:?}", v), gyro_frames, Vec::new()), &options);
            }
        }

        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Unit, "Accelerometer unit", String, |v| v.to_string(), "m/s²".into(),  Vec::new()), &options);
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Unit, "Gyroscope unit",     String, |v| v.to_string(), "rad/s".into(), Vec::new()), &options);

        let imu_orientation = match self.model.as_deref() {
            Some("Micro Studio Camera 4K G2") if firmware_version == "8.4" => "yXZ",
            _ => "yxz"
        };
        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()), &options);
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()), &options);

        samples.insert(0, SampleInfo { tag_map: Some(map), ..Default::default() });

        Ok(samples)
    }

    // The container's frame rate: its first video track's timescale over the nominal frame duration, the most common one
    // (the shorter of two as common), as `segment_table::video_frame_times` takes it, and not the average (an undercranked
    // clip's camera holds its last frame to the end of its sound)
    fn container_rate(ctx: &mp4parse::MediaContext) -> Option<(u64, u64)> {
        let track = ctx.tracks.iter().find(|t| t.track_type == mp4parse::TrackType::Video)?;
        let mut durations = std::collections::BTreeMap::<u32, u64>::new();
        for entry in track.stts.as_ref()?.samples.iter() {
            *durations.entry(entry.sample_delta).or_default() += entry.sample_count as u64;
        }
        let (nominal, _) = durations.iter().max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))?;
        let timescale = track.timescale?.0;
        (timescale > 0 && *nominal > 0).then_some((timescale, *nominal as u64))
    }

    // The frame each reading was taken during (`util::READING_FRAMES`), for the readings of `samples` (their track's index
    // in `ctx` and their index in the track): the frame being captured at the middle of the first period of its sample,
    // or of the whole sample when it is shorter, frame k being captured after k / `rate` up to (k + 1) / `rate` (a middle
    // exactly at a frame's start is the frame before's, frame 0's start frame 0's). The period is the track's sample
    // duration, the most common one (the shorter of two as common): each sample holds one reading of the IMU and lasts as
    // long, but a file cut from recordings can lengthen a segment's last one over a gap in its source's readings (its
    // reading is still of the time the sample starts) or shorten one at a junction. Exact, from the samples' times in
    // their track's ticks and the capture rate. `None` where a reading's sample isn't in its track
    fn reading_frames(ctx: &mp4parse::MediaContext, samples: &[(usize, u64)], rate: CaptureRate) -> Option<Vec<u64>> {
        let mut frames = vec![0u64; samples.len()];
        let mut tracks: Vec<usize> = samples.iter().map(|s| s.0).collect();
        tracks.sort_unstable();
        tracks.dedup();
        for index in tracks {
            let track = ctx.tracks.get(index)?;
            let ticks = track.timescale?.0 as i128;
            let table = mp4parse::unstable::create_sample_table(track, 0.into())?;
            let mut times = Vec::new();
            for (k, &(t, i)) in samples.iter().enumerate() {
                if t == index {
                    let s = table.get(usize::try_from(i).ok()?)?;
                    times.push((k, s.start_composition.0 as i128, (s.end_composition.0 - s.start_composition.0).max(0) as i128));
                }
            }
            drop(table);
            let mut durations: Vec<i128> = times.iter().map(|x| x.2).collect();
            durations.sort_unstable();
            let period = durations.chunk_by(|a, b| a == b).max_by(|a, b| a.len().cmp(&b.len()).then(b[0].cmp(&a[0])))?[0];
            for (k, start, duration) in times {
                // Twice the time of the middle, in ticks: its frame is the last one captured before it
                let middle = 2 * start + duration.min(period);
                frames[k] = (middle * rate.num as i128 - 1).div_euclid(2 * ticks * rate.den as i128).max(0) as u64;
            }
        }
        Some(frames)
    }

    // The picture the Blackmagic RAW SDK decodes the clip to (`IBlackmagicRawClip::GetWidth` and `GetHeight`, the processed
    // image at full resolution), in pixels and before the clip's rotation: the camera's crop of the sensor (`crop_size`,
    // from `crop_origin`) with each side rounded down to a multiple of 8, pixel for pixel. The SDK's resolutions are that
    // picture whole and a half, a quarter and an eighth of it (`IBlackmagicRawClipResolutions`), the eighth in whole
    // pixels: the Pocket Cinema Camera 6K's 2.8K 17:9 crop of 2868 × 1512 decodes to 2864 × 1512, the crop's first 2864
    // columns. Neither the sensor area captured (`sensor_area_captured`, larger than the crop where the camera scales it:
    // `sensor_area`) nor the track header (on some cameras the whole readout, the crop's border with it: 2912 × 1528 for
    // that 2.8K crop) is the picture. `None` without a crop
    pub fn picture_size(meta: &serde_json::Value) -> Option<(usize, usize)> {
        let size = meta.get("crop_size")?.as_array().filter(|x| x.len() == 2)?;
        let side = |v: &serde_json::Value| v.as_f64().filter(|v| v.is_finite() && *v >= 0.0).map(|v| v as usize / 8 * 8);
        Some((side(&size[0])?, side(&size[1])?))
    }

    // The area of the sensor the decoded picture shows, in photosites, from `crop_origin`: the picture (`picture_size`) of
    // a crop read photosite for photosite, scaled with the crop where the camera images a larger area of the sensor in it
    // (`sensor_area_captured`, larger than the crop by the same factor on both sides: 1.5 in the URSA Cine 17K 65's and
    // the PYXIS 12K's 8K open gate, their whole 17520 × 8040 and 12288 × 8040 photosites in 11680 × 5360 and 8192 × 5360
    // pixels). A sensor area captured as large as the crop is the crop read 1:1 (every other mode), and so is one that
    // isn't a scale of it: the URSA Mini Pro 4.6K's firmware 4.5 states its coded frame there, the crop and the 16-pixel
    // border around it (4640 × 2624 for 4608 × 2592), whose picture the SDK decodes from `crop_origin` pixel for pixel.
    // `None` without a crop
    pub fn sensor_area(meta: &serde_json::Value) -> Option<(f64, f64)> {
        let (w, h) = Self::picture_size(meta)?;
        let pair = |key: &str| -> Option<(f64, f64)> {
            let v = meta.get(key)?.as_array().filter(|x| x.len() == 2)?;
            Some((v[0].as_f64()?, v[1].as_f64()?)).filter(|(a, b)| a.is_finite() && b.is_finite() && *a > 0.0 && *b > 0.0)
        };
        // Integers held exactly: the two sides' factors are equal exactly when the products are
        let scale = match (pair("crop_size"), pair("sensor_area_captured")) {
            (Some(crop), Some(area)) if area.0 * crop.1 == area.1 * crop.0 => area.0 / crop.0,
            _ => 1.0
        };
        Some((w as f64 * scale, h as f64 * scale))
    }

    // The time the sensor takes to read the decoded picture (`picture_size`), in ms: its rows times the clip's line time
    // (`line_time`). The line time is the time from one row of the camera's crop (`crop_size`) to the next, and the
    // picture is the crop's first rows. Where the camera images a larger area of the sensor in the crop (`sensor_area`)
    // it is still a crop row's: the URSA Cine 17K 65's 8K open gate states 1.68 µs at 100 frames a second, its 5360 rows
    // read in 9.0 ms of the 10 ms frame, where the 8040 rows of the sensor area it images would take 13.5 ms. `None`
    // without a crop or a line time
    fn readout_time(meta: &serde_json::Value) -> Option<f64> {
        let (_, rows) = Self::picture_size(meta)?;
        Some(rows as f64 * Self::line_time(meta)? / 1000.0)
    }

    // The clip's line time, in µs: `sensor_line_time`, a per-clip metadata entry "for motion data usage" since Blackmagic
    // RAW 2.6, but half of it on the Pocket Cinema Camera 4K's firmware 7.9 (the clip's `camera_type` and
    // `firmware_version`), which states twice its line time: 15.015015 µs in every mode and at every frame rate, where
    // firmware 7.9.1 and later state 7.5075073 µs for the same modes (Blackmagic Camera 7.9.1: "Pocket Cinema Camera 4K:
    // Improved motion sensor data accuracy"), and 2160 rows read in 32.4 ms wouldn't fit in the 20 ms frame of the
    // 50 fps clips it records. The Pocket Cinema Camera 6K and 6K Pro state the same line times on 7.9 as later. `None`
    // without a positive line time (the URSA Cine 12K LF's firmware 9.0 states 0)
    fn line_time(meta: &serde_json::Value) -> Option<f64> {
        let line_time = meta.get("sensor_line_time")?.as_f64().filter(|v| v.is_finite() && *v > 0.0)?;
        let text = |key: &str| meta.get(key).and_then(|v| v.as_str());
        let doubled = text("camera_type") == Some("Blackmagic Pocket Cinema Camera 4K") && text("firmware_version") == Some("7.9");
        Some(if doubled { line_time / 2.0 } else { line_time })
    }

    // The clip's metadata: the movie's QuickTime metadata (QTFF, "Metadata atoms and types": the `meta` atom of `moov`
    // whose handler is `mdta`, with the keys `keys` and their values `ilst`), found by the file's structure, the top-level
    // boxes and then `moov`'s, as the Blackmagic RAW SDK reads it. Not by its bytes: the stale data of a card after a
    // clip's last box can hold an earlier recording's `moov`, its metadata among them, partly overwritten, and a clip's
    // own `moov` isn't always in the file's last megabytes
    pub fn parse_meta<T: Read + Seek>(&mut self, stream: &mut T, size: usize) -> Result<serde_json::Value> {
        let meta = Self::read_movie_meta(stream, size as u64)?;
        stream.seek(SeekFrom::Start(0))?;

        if let Some(meta) = meta.as_deref() {
            let mut keys = Vec::new();
            let mut md = serde_json::Map::<String, serde_json::Value>::new();
            Self::iter_boxes(meta, false, |name, d, _| {
                if name == "keys" {
                    // After its version, flags and count
                    Self::iter_boxes(d.get(8..).ok_or(ErrorKind::UnexpectedEof)?, false, |_, d, _| {
                        if let Ok(key) = std::str::from_utf8(&d) {
                            keys.push(key.to_string());
                        }
                        Ok(())
                    })?;
                }
                if name == "ilst" {
                    Self::iter_boxes(&d, true, |_, d, i| {
                        let typ = d.get(..4).ok_or(ErrorKind::UnexpectedEof)?.read_u32::<BigEndian>()?;
                        if let Some(key) = keys.get(i).cloned() {
                            // https://developer.apple.com/library/archive/documentation/QuickTime/QTFF/Metadata/Metadata.html#//apple_ref/doc/uid/TP40000939-CH1-SW35
                            let mut d = d.get(8..).ok_or(ErrorKind::UnexpectedEof)?; // after the type and the locale
                            let v = match typ {
                                1  => serde_json::to_value(std::str::from_utf8(d).unwrap_or(&"")),
                                23 => serde_json::to_value(d.read_f32::<BigEndian>()? as f64),
                                24 => serde_json::to_value(d.read_f64::<BigEndian>()?),
                                65 => serde_json::to_value(d.read_i8()?),
                                66 => serde_json::to_value(d.read_i16::<BigEndian>()?),
                                67 => serde_json::to_value(d.read_i32::<BigEndian>()?),
                                70 |
                                71 => serde_json::to_value([d.read_f32::<BigEndian>()? as f64, d.read_f32::<BigEndian>()? as f64]),
                                74 => serde_json::to_value(d.read_i64::<BigEndian>()?),
                                75 => serde_json::to_value(d.read_u8()?),
                                76 => serde_json::to_value(d.read_u16::<BigEndian>()?),
                                77 => serde_json::to_value(d.read_u32::<BigEndian>()?),
                                78 => serde_json::to_value(d.read_u64::<BigEndian>()?),
                                _ => {
                                    log::debug!("{}({}): {}", key, typ, pretty_hex::pretty_hex(&d[..128.min(d.len().saturating_sub(1))].to_vec()));
                                    Err(serde_json::Error::io(ErrorKind::InvalidData.into()))
                                }
                            };
                            if let Ok(v) = v {
                                md.insert(key, v);
                            }
                        }
                        Ok(())
                    })?;
                }
                Ok(())
            })?;

            let md = serde_json::Value::Object(md);
            self.frame_readout_time = Self::readout_time(&md);
            return Ok(md);
        }
        Err(ErrorKind::InvalidData.into())
    }

    // The data of `moov/meta` whose handler is `mdta` (`parse_meta`): its atoms, the handler first. `None` for a file
    // without one before a box that can't be stepped over
    fn read_movie_meta<T: Read + Seek>(stream: &mut T, size: u64) -> Result<Option<Vec<u8>>> {
        let Some((moov, moov_len)) = crate::segment_table::find_box(stream, 0, size, b"moov", true)? else { return Ok(None) };
        let (mut pos, end) = (moov, moov + moov_len);
        while let Some((at, len)) = crate::segment_table::find_box(stream, pos, end, b"meta", false)? {
            stream.seek(SeekFrom::Start(at))?;
            let meta = util::read_data(stream, len)?;
            // After the handler's version and flags and its predefined field (QTFF's component type)
            if util::find_box_data(&meta, b"hdlr").and_then(|hdlr| hdlr.get(8..12)) == Some(b"mdta") {
                return Ok(Some(meta));
            }
            pos = at + len;
        }
        Ok(None)
    }

    fn parse_per_frame_meta(data: &[u8]) -> Result<serde_json::Value> {
        if data.len() > 8 && &data[4..8] == b"bmdf" {
            let size = (&data[..8]).read_u32::<BigEndian>()? as usize;
            let meta = data.get(8..size).ok_or(ErrorKind::InvalidData)?;
            let mut md = serde_json::Map::<String, serde_json::Value>::new();
            // Each box is an item of its own: one that can't be read as its item (shorter than it, text that isn't
            // UTF-8) is left out, and the frame's other items are still read
            Self::iter_boxes(meta, false, |name, d, _| {
                if let Some((key, v)) = Self::per_frame_item(name, d) {
                    md.insert(key.to_string(), v);
                }
                Ok(())
            })?;
            return Ok(serde_json::Value::Object(md));
        }
        Err(ErrorKind::InvalidData.into())
    }

    // The item of a frame's `bmdf` box `name` whose contents are `d`, by the name and the type of the Blackmagic RAW SDK's
    // frame metadata item (`IBlackmagicRawFrame::GetMetadataIterator`). `None` for a box of another name, or one that
    // doesn't hold its item. The ISO is an unsigned integer as wide as its box: 4 bytes, or 2 as earlier firmware writes
    // it (the Pocket Cinema Camera 6K's 6.9.6, the URSA Mini Pro 4.6K's 6.0), which the SDK gives as the same `u32`. The
    // white balance tints are signed (`s16`)
    fn per_frame_item(name: &str, mut d: &[u8]) -> Option<(&'static str, serde_json::Value)> {
        let text = |d: &[u8]| std::str::from_utf8(d).ok().map(|s| serde_json::Value::from(s.trim_end_matches('\0')));
        Some(match name {
            "srte" => ("sensor_rate",          serde_json::to_value([d.read_u32::<BigEndian>().ok()?, d.read_u32::<BigEndian>().ok()?]).ok()?),
            "innd" => ("internal_nd",          (d.read_f32::<BigEndian>().ok()? as f64).into()),
            "agpf" => ("analog_gain",          (d.read_f32::<BigEndian>().ok()? as f64).into()),
            "expo" => ("exposure",             (d.read_f32::<BigEndian>().ok()? as f64).into()),
            "isoe" => ("iso",                  if d.len() == 2 { u32::from(d.read_u16::<BigEndian>().ok()?) } else { d.read_u32::<BigEndian>().ok()? }.into()),
            "wkel" => ("white_balance_kelvin", d.read_u32::<BigEndian>().ok()?.into()),
            "wtin" => ("white_balance_tint",   d.read_i16::<BigEndian>().ok()?.into()),
            "asct" => ("as_shot_kelvin",       d.read_u32::<BigEndian>().ok()?.into()),
            "asti" => ("as_shot_tint",         d.read_i16::<BigEndian>().ok()?.into()),
            "shtv" => ("shutter_value",        text(d)?),
            "aptr" => ("aperture",             text(d)?),
            "dsnc" => ("distance",             text(d)?),
            "fcln" => ("focal_length",         text(d)?),
            _ => return None
        })
    }

    // Calls `cb` with the name and contents of each box in `data`. The items of an array (an `ilst`) are an index from 1
    // and a `data` box, whose name and contents `cb` gets with the index from 0. A box of size 0 extends to the end of
    // `data`, and the walk stops at a box it can't step past: smaller than its header, or past the end of `data`
    fn iter_boxes<F: FnMut(&str, &[u8], usize) -> Result<()>>(data: &[u8], is_array: bool, mut cb: F) -> Result<()> {
        let mut offs = 0;
        while data.len().saturating_sub(offs) > 8 {
            let size = match (&data[offs..offs+4]).read_u32::<BigEndian>()? as usize {
                0 => data.len() - offs,
                size => size
            };
            let Some(d) = data[offs..].get(8..size) else { break };
            if is_array {
                let index = (&data[offs+4..offs+8]).read_u32::<BigEndian>()? as usize;
                let size2 = d.get(..4).map_or(0, |mut x| x.read_u32::<BigEndian>().unwrap_or_default() as usize);
                if let (Some(index), Some(d), Some(Ok(name))) = (index.checked_sub(1), d.get(8..size2), d.get(4..8).map(std::str::from_utf8)) {
                    cb(name, d, index)?;
                }
            } else {
                if let Ok(name) = std::str::from_utf8(&data[offs+4..offs+8]) {
                    cb(name, d, 0)?;
                }
            }
            offs += size;
        }
        Ok(())
    }
}
