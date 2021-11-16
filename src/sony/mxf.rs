
use std::io::*;
use byteorder::{ReadBytesExt, BigEndian};

use crate::*;
use crate::tags_impl::*;

pub fn parse<T: Read + Seek>(stream: &mut T, _size: usize) -> Result<Vec<SampleInfo>> {
    let mut stream = std::io::BufReader::new(stream);
    let mut samples = Vec::new();

    let mut index = 0;
    let mut id = [0u8; 16];
    while let Ok(_) = stream.read_exact(&mut id) {
        let length = read_ber(&mut stream)?;

        // println!("{}: {}", util::to_hex(&id), length);
        
        if id == [0x06, 0x0e, 0x2b, 0x34, 0x01, 0x02, 0x01, 0x01, 0x0d, 0x01, 0x03, 0x01, 0x17, 0x01, 0x02, 0x01] { // Metadata, Ancillary, SMPTE ST 436
            let mut data = vec![0; length];
            stream.read_exact(&mut data)?;
            let data = parse_ancillary(&data)?;
            
            if let Ok(map) = super::Sony::parse_metadata(&data) {
                let mut frame_rate = 25.0; // Probably wrong assumption, but it's better than 0 (at least we'll have some timestamps)
                if let Some(group) = map.get(&GroupId::Default) {
                    if let Some(val) = group.get(&TagId::FrameRate) {
                        match &val.value {
                            TagValue::f32(vv) => frame_rate = *vv.get() as f64,
                            TagValue::f64(vv) => frame_rate = *vv.get(),
                            _ => {}
                        }
                    }
                }
                let duration_ms = 1000.0 / frame_rate;

                // println!("Index: {}, Duration: {}, Frame rate: {}, Timestamp: {}", index, duration_ms, frame_rate, index as f64 * duration_ms);

                samples.push(SampleInfo {
                    index, 
                    duration_ms,
                    timestamp_ms: index as f64 * duration_ms, 
                    tag_map: Some(map)
                });
                index += 1;
            }
        } else {
            stream.seek(SeekFrom::Current(length as i64))?;
        }
    }

    Ok(samples)
}

fn read_ber<T: Read + Seek>(stream: &mut T) -> Result<usize> {
    let mut size = stream.read_u8()? as usize;

    if size & 0x80 != 0 {
        let bytes = size & 0x7f;
        assert!(bytes <= 8);
        size = 0;
        for _ in 0..bytes {
            size = size << 8 | (stream.read_u8()? as usize);
        }
    }
    Ok(size)
}

fn parse_ancillary(buffer: &[u8]) -> Result<Vec<u8>> {
    let mut slice = Cursor::new(&buffer);

    let count = slice.read_u16::<BigEndian>()?; // number of lines
    assert!(count as usize * 14 <= buffer.len());

    let mut full_data = Vec::with_capacity(buffer.len());
    for _ in 0..count {
        let _line_number = slice.read_u16::<BigEndian>()?;
        let _wrapping_type = slice.read_u8()?;
        let _payload_sample_coding = slice.read_u8()?;
        let sample_count = slice.read_u16::<BigEndian>()?;
        let array_count = slice.read_u32::<BigEndian>()?;
        let array_length = slice.read_u32::<BigEndian>()?;

        let pos = slice.position() as usize;

        let array_size = (array_count * array_length) as usize;

        let parsing_size = (sample_count as usize)
            .min(buffer.len() - pos)
            .min(array_size);

        let array_data = &buffer[pos..pos+parsing_size];
        if array_data[0] == 0x43 && array_data[1] == 0x05 {
            // let size = array_data[2] as usize;
            // let idx = array_data[3];
            let payload = &array_data[4..];
            
            full_data.extend_from_slice(&payload);
        }
        slice.seek(SeekFrom::Current(array_size as i64))?;
    }
    Ok(full_data)
}
