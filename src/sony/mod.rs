pub mod rtmd_tags;

#[cfg(feature="sony-xml")]
pub mod xml_metadata;

use std::io::*;

use byteorder::{ReadBytesExt, BigEndian};
use rtmd_tags::*;
use crate::tags_impl::*;
use crate::*;
use memchr::memmem;

#[derive(Default)]
pub struct Sony {
    pub model: Option<String>
}
impl Sony {
    pub fn detect(buffer: &[u8]) -> Option<Self> {
        if let Some(p1) = memmem::find(&buffer, b"manufacturer=\"Sony\"") {
            return Some(Self {
                model: crate::try_block!(String, {
                    let p2 = memmem::find(&buffer[p1..], b"modelName=\"")?;
                    let e = memchr::memchr(b'"', &buffer[p1+p2+11..])?;
                    String::from_utf8_lossy(&buffer[p1+p2+11..p1+p2+11+e]).into_owned()
                }),
                ..Default::default()
            });
        }
        None
    }

    pub fn parse<T: Read + Seek>(&mut self, stream: &mut T, size: usize) -> Result<Vec<SampleInfo>> {
        let mut samples = Vec::new();
        util::get_metadata_track_samples(stream, size, |mut info: SampleInfo, data: &[u8]| {
            if Self::detect_metadata(&data) {
                if let Ok(map) = Sony::parse_metadata(&data[0x1C..]) {
                    info.tag_map = Some(map);
                    samples.push(info);
                }
            }
        })?;
        Ok(samples)
    }

    fn detect_metadata(data: &[u8]) -> bool {
        data.len() > 0x1C && data[0..2] == [0x00, 0x1C]
    }

    fn parse_metadata(data: &[u8]) -> Result<GroupedTagMap> {
        let mut slice = Cursor::new(data);
        let datalen = data.len() as usize;
        let mut map = GroupedTagMap::new();

        while slice.position() < datalen as u64 {
            let tag = slice.read_u16::<BigEndian>()?;
            if tag == 0x060e {
                /*let uuid = &data[slice.position() as usize - 2..slice.position() as usize + 14];
                println!("--- {} ---", match &uuid[..16] {
                    &hex_literal::hex!("060E2B34 02530101 0C020101 01010000") => "LensUnitMetadata",
                    &hex_literal::hex!("060E2B34 02530101 0C020101 02010000") => "CameraUnitMetadata",
                    &hex_literal::hex!("060E2B34 02530101 0C020101 7F010000") => "UserDefinedAcquisitionMetadata",
                    _ => "Unknown"
                });*/
                slice.seek(SeekFrom::Current(14))?;
                continue;
            }
            if tag == 0 || tag == 0xffff { break; }
            let len = slice.read_u16::<BigEndian>()? as usize;
            let pos = slice.position() as usize;
            if pos + len > datalen {
                //eprintln!("Tag: {:02x}, len: {}, Available: {:x?}", tag, len, datalen - pos);
                //println!("{}", crate::util::to_hex(&data[pos-4..]));
                break;
            }
            let tag_data = &data[pos..(pos + len)];
            slice.seek(SeekFrom::Current(len as i64))?;
            if tag == 0x8300 { // Container
                // Since there's a lot of containers, this code cen be made more efficient by taking the TagMap by parameter, instead of creating new one for each container
                // Benchmarking will be a good idea
                for (g, v) in Self::parse_metadata(tag_data)? {
                    let group_map = map.entry(g).or_insert_with(TagMap::new);
                    group_map.extend(v);
                }
                continue;
            }
            let mut tag_info = get_tag(tag, &tag_data);
            tag_info.native_id = Some(tag as u32);

            let group_map = map.entry(tag_info.group.clone()).or_insert_with(TagMap::new);
            group_map.insert(tag_info.id.clone(), tag_info);
        }
        Ok(map)
    }
}
