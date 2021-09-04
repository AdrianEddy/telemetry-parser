pub mod extra_info;
pub mod record;

use std::io::*;
use byteorder::{ReadBytesExt, LittleEndian};

use crate::{try_block, tag, tags_impl::*};
use crate::tags_impl::{GroupId::*, TagId::*};

const HEADER_SIZE: usize = 32 + 4 + 4 + 32; // padding(32), size(4), version(4), magic(32)
const MAGIC: &[u8] = b"8db42d694ccc418790edff439fe026bf";

use crate::util::*;

#[derive(Default)]
pub struct Insta360 {
    pub model: Option<String>
}

impl Insta360 {
    pub fn detect(buffer: &[u8]) -> Option<Self> {
        if buffer.len() > MAGIC.len() && &buffer[buffer.len()-MAGIC.len()..] == MAGIC {
            return Some(Insta360::default());
        }
        None
    }

    pub fn parse<T: Read + Seek>(&mut self, stream: &mut T, _size: usize) -> Result<Vec<SampleInfo>> {
        let mut tag_map = Self::parse_file(stream)?;
        self.process_map(&mut tag_map);
        Ok(vec![SampleInfo { index: 0, timestamp_ms: 0.0, duration_ms: 0.0, tag_map: Some(tag_map) }])
    }

    fn parse_file<T: Read + Seek>(stream: &mut T) -> Result<GroupedTagMap> {
        let mut buf = vec![0u8; HEADER_SIZE];
        stream.seek(SeekFrom::End(-(HEADER_SIZE as i64)))?;
        stream.read_exact(&mut buf)?;
        if &buf[HEADER_SIZE-32..] == MAGIC {
            let mut map = GroupedTagMap::new();
    
            let extra_size = (&buf[32..]).read_u32::<LittleEndian>()? as i64;
            let _version   = (&buf[36..]).read_u32::<LittleEndian>()?;
    
            let mut offset = (HEADER_SIZE + 4+1+1) as i64;
            while offset < extra_size {
                stream.seek(SeekFrom::End(-offset))?;
    
                let format = stream.read_u8()?;
                let id     = stream.read_u8()?;
                let size   = stream.read_u32::<LittleEndian>()? as i64;
    
                buf.resize(size as usize, 0);
    
                stream.seek(SeekFrom::End(-offset - size))?;
                stream.read_exact(&mut buf)?;
                
                for (g, v) in record::parse(id, format, &buf)? {
                    let group_map = map.entry(g).or_insert_with(TagMap::new);
                    group_map.extend(v);
                }
    
                offset += size + 4+1+1;
            }
            return Ok(map);
        }
        Err(ErrorKind::NotFound.into())
    }
    
    fn process_map(&mut self, tag_map: &mut GroupedTagMap) {
        if let Some(x) = tag_map.get(&GroupId::Default) {
            self.model = try_block!(String, {
                (x.get_t(TagId::Metadata) as Option<&serde_json::Value>)?.as_object()?.get("camera_type")?.as_str()?.to_owned()
            });
        }
    
        let imu_orientation = match self.model.as_deref() {
            Some("Insta360 GO 2") => "YxZ",
            Some("Insta360 OneR") => "yXZ",
            _                     => "yXZ"
        };
    
        if let Some(x) = tag_map.get_mut(&GroupId::Gyroscope) {
            x.insert(Orientation, tag!(parsed Gyroscope,     Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.to_string(), Vec::new()));
        }
        if let Some(x) = tag_map.get_mut(&GroupId::Accelerometer) {
            x.insert(Orientation, tag!(parsed Accelerometer, Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.to_string(), Vec::new()));
        }
    }
}
