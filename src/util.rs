use std::io::*;
use crate::tags_impl::*;
use byteorder::{ReadBytesExt, BigEndian};

pub fn to_hex(data: &[u8]) -> String {
    let mut ret = String::with_capacity(data.len() * 3);
    for b in data {
        ret.push_str(&format!("{:02x} ", b));
    }
    ret
}

#[derive(Debug, Clone)]
pub struct SampleInfo {
    pub index: u64,
    pub timestamp_ms: f64,
    pub duration_ms: f64,
    pub tag_map: Option<GroupedTagMap>
}

fn parse_mp4<T: Read + Seek>(stream: &mut T, size: usize) -> mp4parse::Result<mp4parse::MediaContext> {
    if size > 10*1024*1024 {
        // With large files we can save a lot of time by only parsing actual MP4 box structure, skipping track data ifself.
        // We do that by reading 2 MB from each end of the file, then patching `mdat` box to make the 4 MB buffer a correct MP4 file.
        // This is hacky, but it's worth a try and if we fail we fallback to full parsing anyway.
        let mut all = read_beginning_and_end(stream, 2*1024*1024)?;
        if let Some(pos) = memchr::memmem::find(&all, b"mdat") {
            let how_much_less = (size - all.len()) as u64;
            let mut len = (&all[pos-4..]).read_u32::<BigEndian>()? as u64;
            if len == 1 { // Large box
                len = (&all[pos+4..]).read_u64::<BigEndian>()? - how_much_less;
                &all[pos+4..pos+12].copy_from_slice(&len.to_be_bytes());
            } else {
                len -= how_much_less;
                &all[pos-4..pos].copy_from_slice(&(len as u32).to_be_bytes());
            }
            let mut c = std::io::Cursor::new(&all);
            return mp4parse::read_mp4(&mut c);
        }
    }
    mp4parse::read_mp4(stream)
}

pub fn get_metadata_track_samples<F, T: Read + Seek>(stream: &mut T, size: usize, mut callback: F) -> Result<()>
    where F: FnMut(SampleInfo, &[u8])
{

    let ctx = parse_mp4(stream, size).or_else(|_| mp4parse::read_mp4(stream))?;

    let mut index = 0u64;
    let mut sample_delta = 0u32;
    let mut timestamp_ms = 0f64;

    for x in ctx.tracks {
        if x.track_type == mp4parse::TrackType::Metadata {
            if let Some(timescale) = x.timescale {
                if let Some(ref stts) = x.stts {
                    sample_delta = stts.samples[0].sample_delta;
                }
                let duration_ms = sample_delta as f64 * 1000.0 / timescale.0 as f64;

                if let Some(samples) = mp4parse::unstable::create_sample_table(&x, 0.into()) {
                    let mut sample_data = Vec::new();
                    for x in samples {
                        let sample_size = (x.end_offset.0 - x.start_offset.0) as usize;
                        if sample_size > 4 {
                            if sample_data.len() != sample_size {
                                sample_data.resize(sample_size, 0u8);
                            }

                            stream.seek(SeekFrom::Start(x.start_offset.0 as u64))?;
                            stream.read_exact(&mut sample_data[..])?;

                            callback(SampleInfo { index, timestamp_ms, duration_ms, tag_map: None }, &sample_data);
                        
                            timestamp_ms += duration_ms;
                            index += 1;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

pub fn read_beginning_and_end<T: Read + Seek>(stream: &mut T, size: usize) -> Result<Vec<u8>> {
    let mut all = vec![0u8; size*2];

    stream.seek(SeekFrom::Start(0))?;
    let read1 = stream.read(&mut all[..size])?;

    stream.seek(SeekFrom::End(-(size as i64)))?;
    let read2 = stream.read(&mut all[read1..])?;

    stream.seek(SeekFrom::Start(0))?;

    all.resize(read1+read2, 0);
    Ok(all)
}

#[macro_export]
macro_rules! try_block {
    ($type:ty, $body:block) => {
        (|| -> Option<$type> {
            Some($body)
        }());
    };
    ($body:block) => {
        (|| -> Option<()> {
            $body
            Some(())
        }());
    };
}