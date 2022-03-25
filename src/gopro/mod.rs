pub mod klv;

use std::io::*;
use byteorder::{ReadBytesExt, BigEndian};

use crate::tags_impl::*;
use crate::*;
use klv::KLV;
use memchr::memmem;

#[derive(Default)]
pub struct GoPro {
    pub model: Option<String>,
    extra_gpmf: Option<GroupedTagMap>
}

impl GoPro {
    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], _filepath: P) -> Option<Self> {
        let mut ret = None;
    
        if let Some(pos) = memmem::find(buffer, b"GPMFDEVC") {
            let mut obj = Self::default();
            let mut buf = &buffer[pos-4..];
            let len = buf.read_u32::<BigEndian>().unwrap() as usize;
            let gpmf_box = &buf[..len];
            
            if let Ok(map) = Self::parse_metadata(&gpmf_box[8+8..], GroupId::Default, true) {
                for v in map.values() {
                    if let Some(v) = v.get_t(TagId::Unknown(0x4D494E46/*MINF*/)) as Option<&String> {
                        obj.model = Some(v.clone());
                        break;
                    }
                }
                obj.extra_gpmf = Some(map);
            }
            ret = Some(obj);
        } else if memmem::find(buffer, b"GoPro MET").is_some() {
            ret = Some(Self::default());
        }
        
        if ret.is_none() || ret.as_ref().unwrap().model.is_none() {
            // Find model name in GPRO section in `mdat` at the beginning of the file
            if let Some(p1) = memmem::find(buffer, b"GPRO") {
                if let Some(model) = util::find_between_with_offset(&buffer[p1..(p1+1024).min(buffer.len())], b"HERO", b'\0', -4) {
                    if let Some(obj) = &mut ret {
                        obj.model = Some(model);
                    } else {
                        ret = Some(Self { model: Some(model), ..Default::default() });
                    }
                }
            }
        }
        ret
    }

    pub fn parse<T: Read + Seek>(&mut self, stream: &mut T, size: usize) -> Result<Vec<SampleInfo>> {
        let mut samples = Vec::new();
        if let Some(extra) = &self.extra_gpmf {
            samples.push(SampleInfo { index: 0, timestamp_ms: 0.0, duration_ms: 0.0, tag_map: Some(extra.clone()) });
        }
        let ctx = util::get_metadata_track_samples(stream, size, |mut info: SampleInfo, data: &[u8]| {
            if Self::detect_metadata(data) {
                if let Ok(mut map) = GoPro::parse_metadata(&data[8..], GroupId::Default, false) {
                    self.process_map(&mut map);
                    info.tag_map = Some(map);
                    samples.push(info);
                }
            }
        })?;
        let mut fps = None;
        if !ctx.tracks.is_empty() {
            fps = util::get_fps_from_track(&ctx.tracks[0]);
        }
        self.process_samples(&mut samples, fps);
        Ok(samples)
    }

    fn detect_metadata(data: &[u8]) -> bool {
        data.len() > 8 && &data[0..4] == b"DEVC"
    }

    fn parse_metadata(data: &[u8], group_id: GroupId, force_group: bool) -> Result<GroupedTagMap> {
        let mut slice = Cursor::new(data);
        let datalen = data.len() as u64;
        let mut map = GroupedTagMap::new();

        while slice.position() < datalen {
            let start_pos = slice.position() as usize;
            if datalen as i64 - start_pos as i64 >= 8 {
                let klv = KLV::parse_header(&mut slice)?;
                let pos = slice.position() as usize;

                let len = klv.data_len();
                if len == 0 { continue; }

                let full_tag_data = &data[start_pos..(pos + len)];
                let tag_data      = &data[pos      ..(pos + len)];

                slice.seek(SeekFrom::Current(klv.aligned_data_len() as i64))?;

                if klv.data_type == 0 { // Container
                    let container_group = if force_group { group_id.clone() } else { KLV::group_from_key(GoPro::get_last_klv(tag_data)?) };
                    for (g, v) in GoPro::parse_metadata(tag_data, container_group, force_group)? {
                        let group_map = map.entry(g).or_insert_with(TagMap::new);
                        group_map.extend(v);
                    }
                    continue;
                }

                util::insert_tag(&mut map, TagDescription {
                    group:       group_id.clone(),
                    id:          klv.tag_id(),
                    description: klv.key_as_string(),
                    value:       klv.parse_data(full_tag_data),
                    native_id:   Some((&klv.key[..]).read_u32::<BigEndian>()?)
                });
            } else {
                break;
            }
        }

        Ok(map)
    }

    fn process_map(&self, tag_map: &mut GroupedTagMap) {
        for (g, v) in tag_map.iter_mut() {
            // If we have ORIN and ORIO but not MTRX, construct MTRX from ORIN and ORIO and insert to the map
            if v.contains_key(&TagId::OrientationIn) && v.contains_key(&TagId::OrientationOut) && !v.contains_key(&TagId::Matrix) {
                crate::try_block!({
                    let m = KLV::orientations_to_matrix(
                        (v.get_t(TagId::OrientationIn)  as Option<&String>)?, 
                        (v.get_t(TagId::OrientationOut) as Option<&String>)?
                    )?;
                    v.insert(TagId::Matrix, 
                        crate::tag!(parsed g.clone(), TagId::Matrix, "MTRX", Vec_Vec_f32, |v| format!("{:?}", v), vec![m], Vec::new())
                    );
                });
            }

            // Convert MTRX to Orientation tag
            if g == &GroupId::Gyroscope || g == &GroupId::Accelerometer || g == &GroupId::Magnetometer {
                let mut imu_orientation = None;
                if let Some(m) = v.get_t(TagId::Matrix) as Option<&Vec<Vec<f32>>> {
                    if !m.is_empty() && !m[0].is_empty() {
                        imu_orientation = Some(GoPro::mtrx_to_orientation(&m[0]));
                    }
                } else if let Some(m) = &self.model {
                    if m.contains("HERO6") { imu_orientation = Some("ZyX".to_string()); }
                    // if m.contains("HERO5") { imu_orientation = Some("XYZ".to_string()); }
                }
                if let Some(o) = imu_orientation {
                    v.insert(TagId::Orientation, crate::tag!(parsed g.clone(), TagId::Orientation, "IMUO", String, |v| v.to_string(), o, Vec::new()));
                }
            }

        }
    }

    fn get_timestamp(info: &util::SampleInfo, group_id: &GroupId) -> Option<i64> {
        if let Some(ref grouped_tag_map) = info.tag_map {
            for (group, map) in grouped_tag_map {
                if group == group_id {
                    let mut tick = 0i64;
                    if let Some(t) = map.get_t(TagId::Unknown(0x5449434b /*TICK*/)) as Option<&u32> {
                        tick = (*t as i64) * 1000;
                    }
                    let timestamp_us = (map.get_t(TagId::TimestampUs) as Option<&u64>).map(|x| *x as i64).unwrap_or(tick);
                    return Some(timestamp_us);
                }
            }
        }
        None
    }

    fn process_samples(&self, samples: &mut Vec<SampleInfo>, fps: Option<f64>) {
        // Normalize quaternions
        let mut prev_increment = 0;
        let mut start_timestamp_us = None;
        let mut global_ts_cori: f64 = 0.0;
        let mut global_ts_iori: f64 = 0.0;
        let global_increment = fps.map(|x| 1000.0 / x);
        for i in 0..samples.len() {
            let info = &samples[i];
            if info.tag_map.is_none() { continue; }
        
            let grouped_tag_map = info.tag_map.as_ref().unwrap();
    
            let mut cori = Vec::new();
            let mut iori = Vec::new();
            for (group, map) in grouped_tag_map.iter() {
                if group == &GroupId::CameraOrientation || group == &GroupId::ImageOrientation {
                    let scale = *(map.get_t(TagId::Scale) as Option<&i16>).unwrap_or(&32767) as f64;
                    let mut timestamp_us = *(map.get_t(TagId::TimestampUs) as Option<&u64>).unwrap_or(&0) as i64;
                    // let start_count = *(map.get_t(TagId::Count) as Option<&u32>).unwrap_or(&0);
                    let next_timestamp_us = samples.get(i + 1).map(|x | Self::get_timestamp(x, &group)).unwrap_or(None);
                    if start_timestamp_us.is_none() {
                        start_timestamp_us = Some(timestamp_us);
                    }
                    // TODO https://github.com/gopro/gpmf-parser/blob/master/GPMF_utils.c
                    if let Some(arr) = map.get_t(TagId::Data) as Option<&Vec<Quaternion<i16>>> {
                        let sample_count = arr.len() as i64;
                        let increment = next_timestamp_us.map(|x| ((x - timestamp_us) / sample_count)).unwrap_or(prev_increment);
                        prev_increment = increment;
                        for v in arr.iter() {
                            let mut ts = timestamp_us - start_timestamp_us.unwrap();
                            if let Some(global_inc) = global_increment {
                                if group == &GroupId::CameraOrientation {
                                    ts = (global_ts_cori * 1000.0).round() as i64;
                                    global_ts_cori += global_inc;
                                } else {
                                    ts = (global_ts_iori * 1000.0).round() as i64;
                                    global_ts_iori += global_inc;
                                }
                            }
                            let aout = if group == &GroupId::CameraOrientation { &mut cori } else { &mut iori };
                            aout.push((
                                ts, 
                                Quaternion {
                                    w: v.w as f64 / scale, 
                                    x: -v.x as f64 / scale,
                                    y: v.y as f64 / scale,
                                    z: v.z as f64 / scale
                                }
                            ));
                            timestamp_us += increment;
                        }
                    }
                }
            }
            if !cori.is_empty() && cori.len() == iori.len() {
                // Multiply CORI * IORI
                let quat = cori.into_iter().zip(iori.into_iter()).map(|(c, i)| TimeQuaternion {
                    t: c.0 as f64 / 1000.0,
                    v: c.1 * i.1
                }).collect();
                
                let grouped_tag_map = samples[i].tag_map.as_mut().unwrap();
                util::insert_tag(grouped_tag_map, tag!(parsed GroupId::Quaternion, TagId::Data, "Quaternion data",  Vec_TimeQuaternion_f64, |v| format!("{:?}", v), quat, vec![]));
            }
        }
    }
    pub fn get_avg_sample_duration(samples: &Vec<SampleInfo>, group_id: &GroupId) -> Option<f64> {
        let mut total_duration_ms = 0.0;

        let mut first_tsus = None;
        let mut last_tsus = None;
        let mut count = 0;
        let mut last_len = 0;
        for info in samples {
            total_duration_ms += info.duration_ms;
            if info.tag_map.is_none() { continue; }
            for (group, map) in info.tag_map.as_ref().unwrap() {
                if group == group_id {
                    if let Some(t) = map.get_t(TagId::TimestampUs) as Option<&u64> {
                        if first_tsus.is_none() { first_tsus = Some(*t as i64); }
                        last_tsus = Some(*t as i64);
                    } else if let Some(t) = map.get_t(TagId::TimestampMs) as Option<&u64> {
                        if first_tsus.is_none() { first_tsus = Some((*t as i64) * 1000); }
                        last_tsus = Some((*t as i64) * 1000);
                    } else if let Some(t) = map.get_t(TagId::Unknown(0x5449434b /*TICK*/)) as Option<&u32> {
                        if first_tsus.is_none() { first_tsus = Some((*t as i64) * 1000); }
                        last_tsus = Some((*t as i64) * 1000);
                    }
                    if let Some(t) = map.get_t(TagId::Data) as Option<&Vec<Vector3<i16>>> {
                        count += t.len();
                        last_len = t.len();
                    } else if let Some(t) = map.get_t(TagId::Data) as Option<&Vec<Quaternion<i16>>> {
                        count += t.len();
                        last_len = t.len();
                    }
                }
            }
        }
        if first_tsus.is_some() && last_tsus.is_some() && count > 0 {
            Some((last_tsus.unwrap() as f64 - first_tsus.unwrap() as f64) / (count - last_len).max(1) as f64 / 1000.0)
        } else if count > 0 {
            Some(total_duration_ms / count as f64)
        } else {
            None
        }
    }

    fn get_last_klv(data: &[u8]) -> Result<&[u8]> {
        let mut slice = Cursor::new(data);

        let mut offset = 0;
        while slice.position() < data.len() as u64 {
            offset = slice.position() as usize;
            let klv = KLV::parse_header(&mut slice)?;
            slice.seek(SeekFrom::Current(klv.aligned_data_len() as i64))?;
        }
        Ok(&data[offset..])
    }

    fn mtrx_to_orientation(mtrx: &[f32]) -> String {
        assert!(mtrx.len() == 9);

        (0..3).map(|x| {
                 if mtrx[x * 3 + 0] > 0.5 { 'X' } else if mtrx[x * 3 + 0] < -0.5 { 'x' }
            else if mtrx[x * 3 + 1] > 0.5 { 'Y' } else if mtrx[x * 3 + 1] < -0.5 { 'y' }
            else if mtrx[x * 3 + 2] > 0.5 { 'Z' } else if mtrx[x * 3 + 2] < -0.5 { 'z' }
            else { panic!("Invalid MTRX {:?}", mtrx) }
        }).collect()
    }

    pub fn normalize_imu_orientation(v: String) -> String {
        v
    }
    
    pub fn camera_type(&self) -> String {
        "GoPro".to_owned()
    }
}
