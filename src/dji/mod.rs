pub mod dbgi;

use std::io::*;
use std::sync::{ Arc, atomic::AtomicBool };

use crate::tags_impl::*;
use crate::*;
use crate::util::insert_tag;
use memchr::memmem;
use prost::Message;

#[derive(Default)]
pub struct Dji {
    pub model: Option<String>,
}

impl Dji {
    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], _filepath: P) -> Option<Self> {
        if memmem::find(buffer, b"dbginfo").is_some() && memmem::find(buffer, b"IMX686").is_some() {
            Some(Self {
                model: Some("Action 2".to_string())
            })
        } else {
            None
        }
    }

    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, size: usize, _progress_cb: F, cancel_flag: Arc<AtomicBool>) -> Result<Vec<SampleInfo>> {
        let mut samples = Vec::new();
        util::get_other_track_samples(stream, size, |mut info: SampleInfo, data: &[u8], file_position: u64| {
            if let Ok(parsed) = dbgi::DebugInfoMain::decode(data) {
                if let Some (frame) = parsed.frames.first() {

                    let mut tag_map = GroupedTagMap::new();

                    let frame_data = frame.inner.as_ref().unwrap();
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
                    
                    info.tag_map = Some(tag_map);

                    samples.push(info);
                }
            }
        }, cancel_flag)?;

        Ok(samples)
    }

    pub fn normalize_imu_orientation(v: String) -> String {
        v
    }
    
    pub fn camera_type(&self) -> String {
        "DJI".to_owned()
    }
    
    pub fn frame_readout_time(&self) -> Option<f64> {
        None
    }
}
