// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2022 Adrian <adrian.eddy at gmail>

use std::io::*;
use memchr::memmem;
use std::sync::{ Arc, atomic::AtomicBool };

use crate::tags_impl::*;
use crate::*;

mod rig;
mod records;

#[derive(Default)]
pub struct Vuze {
    pub model: Option<String>,
    frame_readout_time: Option<f64>,
}

impl Vuze {
    pub fn camera_type(&self) -> String {
        "Vuze".to_owned()
    }
    pub fn has_accurate_timestamps(&self) -> bool {
        // bmdt timestamps share the video clock. In particular, the first
        // sensor reading need not coincide with video PTS zero.
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
        if memmem::find(buffer, b"bmdt").is_some() &&
           memmem::find(buffer, b"modl").is_some() &&
           memmem::find(buffer, b"slno").is_some() &&
           memmem::find(buffer, b"cali").is_some() {
            return Some(Self::default());
        }
        None
    }

    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, _size: usize, progress_cb: F, cancel_flag: Arc<AtomicBool>, options: crate::InputOptions) -> Result<Vec<SampleInfo>> {
        let mut gyro = Vec::new();
        let mut accl = Vec::new();
        let mut exposure = Vec::new();
        let mut processing_mode = None;
        let mut capture_mode = None;

        let mut map = GroupedTagMap::new();

        let last_timestamp;
        // The per-camera crops and the calibration document, collected as the
        // walk finds them rather than consumed where they are found: the two
        // crops are what turn the calibration into a picture map, and nothing
        // in the container promises which box comes first.
        let mut crops: [Option<rig::Crop>; 2] = [None; 2];
        let mut calibration = None;

        while let Ok((typ, _offs, size, header_size)) = util::read_box(stream) {
            if size == 0 || typ == 0 { break; }
            let org_pos = stream.stream_position()?;

            if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) { break; }

            if typ == fourcc("moov") || typ == fourcc("udta") {
                continue; // go inside these boxes
            } else {
                if typ == fourcc("modl") { // Model
                    let mut buf = vec![0u8; size as usize - header_size as usize];
                    stream.read_exact(&mut buf)?;
                    self.model = Some(String::from_utf8_lossy(&buf).trim_start_matches("Vuze").to_string());
                }
                if typ == fourcc("pmod") || typ == fourcc("scfg") {
                    let mut buf = vec![0u8; size as usize - header_size as usize];
                    stream.read_exact(&mut buf)?;
                    let value = String::from_utf8_lossy(&buf).trim().to_owned();
                    if typ == fourcc("pmod") { processing_mode = Some(value); }
                    else { capture_mode = Some(value); }
                }
                // The two cameras' crops, in the order their tracks are
                // written. `rcrp` belongs to `CAM_0` and `lcrp` to `CAM_1`,
                // which is both the file's own order and what a measurement of
                // the two recorded image circles says.
                for (i, key) in ["rcrp", "lcrp"].iter().enumerate() {
                    if typ == fourcc(key) {
                        let mut buf = vec![0u8; size as usize - header_size as usize];
                        stream.read_exact(&mut buf)?;
                        crops[i] = rig::Crop::parse(&String::from_utf8_lossy(&buf));
                    }
                }
                if typ == fourcc("cali") { // Calibration YAML
                    let mut buf = vec![0u8; size as usize - header_size as usize];
                    stream.read_exact(&mut buf)?;
                    let calib = String::from_utf8_lossy(&buf).to_string().replace("%YAML:1.0", "");

                    match serde_yaml::from_str(calib.trim()) as serde_yaml::Result<serde_json::Value> {
                        Ok(calib) if calib.get("CamModel_V2_Set").is_some() => {
                            util::insert_tag(&mut map, tag!(parsed GroupId::Default, TagId::Metadata, "Calibration", Json, |v| serde_json::to_string(v).unwrap(), calib.clone(), vec![]), &options);
                            calibration = Some(calib);
                        },
                        Err(e) => log::warn!("Failed to parse YAML: {}\n{}", e, &calib),
                        _ => log::warn!("Failed to parse YAML: {}", &calib)
                    }
                }
                if typ == fourcc("bmdt") { // IMU data
                    let buflen = size as usize - header_size as usize;
                    let mut buf = vec![0u8; buflen];
                    stream.read_exact(&mut buf)?;

                    let data = records::parse(&buf, &cancel_flag, &progress_cb);
                    self.frame_readout_time = data.readout_ms;
                    gyro.extend(data.gyro);
                    accl.extend(data.accl);
                    exposure.extend(data.exposure);
                }

                stream.seek(SeekFrom::Start(org_pos + size - header_size as u64))?;
            }
        }

        // Studio compensates processing latency only for camera-processed
        // images. Raw fisheyes already use the sensor clock. Older metadata
        // fixtures omit pmod, so absence alone is not evidence of processing.
        if processing_mode.as_deref().is_some_and(|v| v != "raw") {
            let delay = if capture_mode.as_deref() == Some("360") { 0.066733 } else { 0.033366 };
            for v in gyro.iter_mut().chain(accl.iter_mut()) { v.t += delay; }
        }
        last_timestamp = gyro.last().map_or(0.0, |v| v.t * 1000.0);

        if let (Some(calib), [Some(right), Some(left)]) = (&calibration, crops) {
            if let Some(profile) = self.get_lens_profile(&calib["CamModel_V2_Set"], right) {
                util::insert_tag(&mut map, tag!(parsed GroupId::Lens, TagId::Data, "Lens profile", Json, |v| serde_json::to_string(v).unwrap(), profile, vec![]), &options);
            } else {
                log::warn!("Failed to get lens profile");
            }
            // The rig goes at its own tag and never at `Lens/Data`: that one is
            // the single-lens JSON profile above, tags are last-wins per (group,
            // id), and a rig-shaped object there would deserialise into an empty
            // lens profile downstream and then suppress the lens-database lookup.
            if let Some(mut camera_rig) = rig::camera_rig(self.model.as_deref().unwrap_or_default(), calib, [right, left]) {
                if let crate::rig::geometry::ProjectionKind::Rig { cameras, .. } = &mut camera_rig.projection.kind {
                    for camera in cameras { camera.readout = crate::rig::readout(self.frame_readout_time); }
                }
                util::insert_tag(&mut map, tag!(parsed GroupId::Lens, TagId::Rig, "Camera rig", CameraRig, |v: &crate::rig::CameraRig| serde_json::to_string(v).unwrap_or_default(), camera_rig, vec![]), &options);
            }
        }

        util::insert_tag(&mut map, tag!(parsed GroupId::Exposure, TagId::Data, "Exposure duration", Vec_TimeScalar_f64, |v| format!("{:?}", v), exposure, vec![]), &options);
        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Data, "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), accl, vec![]), &options);
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Data, "Gyroscope data",     Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]), &options);

        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Unit, "Accelerometer unit", String, |v| v.to_string(), "g".into(), Vec::new()), &options);
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Unit, "Gyroscope unit",     String, |v| v.to_string(), "deg/s".into(), Vec::new()), &options);

        let imu_orientation = "xYz";
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()), &options);
        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()), &options);

        Ok(vec![
            SampleInfo { timestamp_ms: 0.0, duration_ms: last_timestamp, tag_map: Some(map), ..Default::default() }
        ])
    }

    /// The single-lens JSON profile a lens database deserialises, from `CAM_0`.
    ///
    /// The camera matrix is carried into the RECORDED picture's own pixels —
    /// scaled by the camera's measured ratio and shifted by the crop's origin,
    /// the rig's own map — because that is the only space the profile format
    /// can express: it states one frame size and assumes the calibration is in
    /// it. It used to state the crop as the frame size and leave `K` in the
    /// ~3040-px calibration space, which makes the focal length 38 % too short
    /// and the principal point 450 px off centre.
    fn get_lens_profile(&self, data: &serde_json::Value, crop: rig::Crop) -> Option<serde_json::Value> {
        let model = self.model.clone()?;
        let cam = data.get("CAM_0")?;
        let m = cam.get("K")?.get("data")?.as_array()?.iter().filter_map(|x| x.as_f64()).collect::<Vec<f64>>();
        let coeffs = cam.get("DistortionCoeffs")?.as_array()?.iter().filter_map(|x| x.as_f64()).collect::<Vec<f64>>();
        let radius = cam.get("ImageCircleRadius")?.as_f64()?;
        if m.len() != 9 { return None; }
        if coeffs.len() < 4 { return None; }
        if !(radius.is_finite() && radius > 0.0) { return None; }

        let s = rig::picture_scale(crop);
        let (ox, oy) = (crop.x as f64, crop.y as f64);

        Some(serde_json::json!({
            "calibrated_by": "Vuze",
            "camera_brand": "Vuze",
            "camera_model": model,
            "calib_dimension": { "w": crop.width, "h": crop.height },
            "orig_dimension":  { "w": crop.width, "h": crop.height },
            "frame_readout_time": 0.0,
            "official": true,
            "fisheye_params": {
              "camera_matrix": [
                [ s * m[0], s * m[1], s * m[2] - ox ],
                [ s * m[3], s * m[4], s * m[5] - oy ],
                [ m[6],     m[7],     m[8]          ]
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
        }))
    }
}
