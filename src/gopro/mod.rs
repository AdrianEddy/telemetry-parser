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
    pub fn detect(buffer: &[u8]) -> Option<Self> {
        let mut ret = None;
    
        if let Some(pos) = memmem::find(&buffer, b"GPMFDEVC") {
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
        }
        if ret.is_none() || ret.as_ref().unwrap().model.is_none() {
            // Find model name in GPRO section in `mdat` at the beginning of the file
            if let Some(p1) = memmem::find(&buffer, b"GPRO") {
                ret = Some(Self {
                    model: crate::try_block!(String, {
                        let p2 = memmem::find(&buffer[p1..p1+1024], b"HERO")?;
                        let end = memchr::memchr(0, &buffer[p1+p2..p1+p2+64])?;
                        String::from_utf8_lossy(&buffer[p1+p2..p1+p2+end]).into_owned()
                    }),
                    ..Default::default()
                });
            }
        }
        ret
    }

    pub fn parse<T: Read + Seek>(&mut self, stream: &mut T, size: usize) -> Result<Vec<SampleInfo>> {
        let mut samples = Vec::new();
        if let Some(extra) = &self.extra_gpmf {
            samples.push(SampleInfo { index: 0, timestamp_ms: 0.0, duration_ms: 0.0, tag_map: Some(extra.clone()) });
        }
        util::get_metadata_track_samples(stream, size, |mut info: SampleInfo, data: &[u8]| {
            if Self::detect_metadata(&data) {
                if let Ok(mut map) = GoPro::parse_metadata(&data[8..], GroupId::Default, false) {
                    self.process_map(&mut map);
                    info.tag_map = Some(map);
                    samples.push(info);
                }
            }
        })?;
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
                    let container_group = if force_group { group_id.clone() } else { KLV::group_from_key(GoPro::get_last_klv(&tag_data)?) };
                    for (g, v) in GoPro::parse_metadata(tag_data, container_group, force_group)? {
                        let group_map = map.entry(g).or_insert_with(TagMap::new);
                        group_map.extend(v);
                    }
                    continue;
                }

                let tag_info = TagDescription {
                    group:       group_id.clone(),
                    id:          klv.tag_id(),
                    description: klv.key_as_string(),
                    value:       klv.parse_data(&full_tag_data),
                    native_id:   Some((&klv.key[..]).read_u32::<BigEndian>()?)
                };

                let group_map = map.entry(tag_info.group.clone()).or_insert_with(TagMap::new);
                group_map.insert(tag_info.id.clone(), tag_info);
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
            if g == &GroupId::Gyroscope || g == &GroupId::Accelerometer/* || g == &GroupId::CustomAlloc("MAGN")*/ {
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
}
