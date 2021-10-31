pub mod dbgi;

use std::io::*;

use crate::tags_impl::*;
use crate::*;
use crate::util::insert_tag;
use memchr::memmem;

#[derive(Default)]
pub struct Dji {
    pub model: Option<String>,
}

impl Dji {
    pub fn detect(buffer: &[u8], _filename: &str) -> Option<Self> {
        if memmem::find(buffer, b"dbginfo").is_some() && memmem::find(buffer, b"IMX686").is_some() {
            Some(Self {
                model: Some("Action 2".to_string())
            })
        } else {
            None
        }
    }

    pub fn parse<T: Read + Seek>(&mut self, stream: &mut T, size: usize) -> Result<Vec<SampleInfo>> {
        let mut samples = Vec::new();
        let mut full_dbgi = Vec::with_capacity(size / 20);
        util::get_other_track_samples(stream, size, |info: SampleInfo, data: &[u8]| {
            full_dbgi.extend_from_slice(data);
            samples.push(info);
        })?;

        use prost::Message;
        let parsed = dbgi::DebugInfoMain::decode(full_dbgi.as_slice())?;
        let mut i = 0;
        for x in &parsed.frames {
            let mut tag_map = GroupedTagMap::new();

            let frame_data = x.inner.as_ref().unwrap();
            let imu_data = frame_data.frame_data5_imu.as_ref().unwrap();

            insert_tag(&mut tag_map, tag!(parsed GroupId::Gyroscope, TagId::Data, "Gyroscope data",  Vec_u8, |v| format!("{}", v.len()), imu_data.data.to_vec(), vec![]));
            
            let mut v = serde_json::to_value(&frame_data).map_err(|_| Error::new(ErrorKind::Other, "Serialize error"));
            if let Ok(vv) = &mut v {
                if let Some(obj) = vv.as_object_mut() {
                    if let Ok(x) = dbgi::parse_floats(&frame_data.frame_data4.as_ref().unwrap().floats32_bin1) { obj["frame_data4"]["floats32_bin1"] = x; }
                    if let Ok(x) = dbgi::parse_floats(&frame_data.frame_data4.as_ref().unwrap().floats32_bin2) { obj["frame_data4"]["floats32_bin2"] = x; }
                }
            }
            if let Ok(vv) = v {
                insert_tag(&mut tag_map, tag!(parsed GroupId::Default, TagId::Metadata, "Metadata", Json, |v| serde_json::to_string(v).unwrap(), vv, vec![]));
            }
            
            samples[i].tag_map = Some(tag_map);
            i += 1;
        }

        Ok(samples)
    }

    pub fn normalize_imu_orientation(v: String) -> String {
        v
    }
    
    pub fn camera_type(&self) -> String {
        "DJI".to_owned()
    }
}
