use std::io::*;
use crate::tags_impl::*;
use byteorder::{ReadBytesExt, BigEndian};
use std::collections::BTreeMap;
use memchr::memmem;

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
        let mut all = read_beginning_and_end(stream, size, 2*1024*1024)?;
        if let Some(pos) = memchr::memmem::find(&all, b"mdat") {
            let how_much_less = (size - all.len()) as u64;
            let mut len = (&all[pos-4..]).read_u32::<BigEndian>()? as u64;
            if len == 1 { // Large box
                len = (&all[pos+4..]).read_u64::<BigEndian>()? - how_much_less;
                all[pos+4..pos+12].copy_from_slice(&len.to_be_bytes());
            } else {
                len -= how_much_less;
                all[pos-4..pos].copy_from_slice(&(len as u32).to_be_bytes());
            }
            let mut c = std::io::Cursor::new(&all);
            return mp4parse::read_mp4(&mut c);
        }
    }
    mp4parse::read_mp4(stream)
}

fn get_track_samples<F, T: Read + Seek>(stream: &mut T, size: usize, typ: mp4parse::TrackType, mut callback: F) -> Result<()>
    where F: FnMut(SampleInfo, &[u8])
{

    let ctx = parse_mp4(stream, size).or_else(|_| mp4parse::read_mp4(stream))?;

    let mut index = 0u64;
    // let mut sample_delta = 0u32;
    // let mut timestamp_ms = 0f64;

    for x in ctx.tracks {
        if x.track_type == typ {
            // if let Some(timescale) = x.timescale {
                // if let Some(ref stts) = x.stts {
                //     sample_delta = stts.samples[0].sample_delta;
                // }
                // let duration_ms = sample_delta as f64 * 1000.0 / timescale.0 as f64;

                if let Some(samples) = mp4parse::unstable::create_sample_table(&x, 0.into()) {
                    let mut sample_data = Vec::new();
                    for x in samples {
                        let sample_size = (x.end_offset.0 - x.start_offset.0) as usize;
                        let sample_timestamp_ms = x.start_composition.0 as f64 / 1000.0;
                        let sample_duration_ms = (x.end_composition.0 - x.start_composition.0) as f64 / 1000.0;
                        if sample_size > 4 {
                            if sample_data.len() != sample_size {
                                sample_data.resize(sample_size, 0u8);
                            }

                            stream.seek(SeekFrom::Start(x.start_offset.0 as u64))?;
                            stream.read_exact(&mut sample_data[..])?;

                            callback(SampleInfo { index, timestamp_ms: sample_timestamp_ms, duration_ms: sample_duration_ms, tag_map: None }, &sample_data);
                        
                            //timestamp_ms += duration_ms;
                            index += 1;
                        }
                    }
                }
            // }
        }
    }
    Ok(())
}

pub fn get_metadata_track_samples<F, T: Read + Seek>(stream: &mut T, size: usize, callback: F) -> Result<()>
    where F: FnMut(SampleInfo, &[u8])
{
    get_track_samples(stream, size, mp4parse::TrackType::Metadata, callback)
}
pub fn get_other_track_samples<F, T: Read + Seek>(stream: &mut T, size: usize, callback: F) -> Result<()>
    where F: FnMut(SampleInfo, &[u8])
{
    get_track_samples(stream, size, mp4parse::TrackType::Unknown, callback)
}

pub fn read_beginning_and_end<T: Read + Seek>(stream: &mut T, stream_size: usize, read_size: usize) -> Result<Vec<u8>> {
    let mut all = vec![0u8; read_size*2];

    stream.seek(SeekFrom::Start(0))?;

    if stream_size > read_size * 2 {
        let read1 = stream.read(&mut all[..read_size])?;
    
        stream.seek(SeekFrom::End(-(read_size as i64)))?;
        let read2 = stream.read(&mut all[read1..])?;

        all.resize(read1+read2, 0);
    } else {
        let read = stream.read(&mut all)?;
        all.resize(read, 0);
    }

    stream.seek(SeekFrom::Start(0))?;

    Ok(all)
}

#[derive(Default, serde::Serialize, Clone, Debug)]
pub struct IMUData {
    pub timestamp_ms: f64,
    pub gyro: Option<[f64; 3]>,
    pub accl: Option<[f64; 3]>,
    pub magn: Option<[f64; 3]>
}

// TODO: interpolate if gyro and accel have different rates
pub fn normalized_imu(input: &crate::Input, orientation: Option<String>) -> Result<Vec<IMUData>> {
    let mut timestamp = 0f64;

    let mut final_data = Vec::<IMUData>::with_capacity(10000);
    let mut data_index = 0;

    for info in input.samples.as_ref().unwrap() {
        if info.tag_map.is_none() { continue; }

        let grouped_tag_map = info.tag_map.as_ref().unwrap();

        // Insta360
        let first_frame_ts = crate::try_block!(f64, {
            (grouped_tag_map.get(&GroupId::Default)?.get_t(TagId::Metadata) as Option<&serde_json::Value>)?
                .as_object()?
                .get("first_frame_timestamp")?
                .as_i64()? as f64 / 1000.0
        }).unwrap_or_default();
        let is_insta360_raw_gyro = crate::try_block!(bool, {
            (grouped_tag_map.get(&GroupId::Default)?.get_t(TagId::Metadata) as Option<&serde_json::Value>)?
                .as_object()?
                .get("is_raw_gyro")?
                .as_bool()?
        }).unwrap_or_default();

        for (group, map) in grouped_tag_map {
            if group == &GroupId::Gyroscope || group == &GroupId::Accelerometer {
                let raw2unit = crate::try_block!(f64, {
                    match &map.get(&TagId::Scale)?.value {
                        TagValue::i16(v) => *v.get() as f64,
                        TagValue::f32(v) => *v.get() as f64,
                        TagValue::f64(v) => *v.get(),
                        _ => 1.0
                    }
                }).unwrap_or(1.0);

                let unit2deg = crate::try_block!(f64, {
                    match (map.get_t(TagId::Unit) as Option<&String>)?.as_str() {
                        "rad/s" => 180.0 / std::f64::consts::PI, // rad to deg
                        _ => 1.0
                    }
                }).unwrap_or(1.0);

                let mut io = match map.get_t(TagId::Orientation) as Option<&String> {
                    Some(v) => v.clone(),
                    None => "XYZ".into()
                };
                io = input.normalize_imu_orientation(io);
                if let Some(imuo) = &orientation {
                    io = imuo.clone();
                }
                let io = io.as_bytes();

                if let Some(taginfo) = map.get(&TagId::Data) {
                    match &taginfo.value {
                        // Sony and GoPro
                        TagValue::Vec_Vector3_i16(arr) => {
                            let arr = arr.get();
                            let reading_duration = info.duration_ms / arr.len() as f64;
        
                            for (j, v) in arr.iter().enumerate() {
                                if final_data.len() <= data_index + j {
                                    final_data.resize_with(data_index + j + 1, Default::default);
                                    final_data[data_index + j].timestamp_ms = timestamp;
                                    timestamp += reading_duration;
                                }
                                let itm = v.clone().into_scaled(&raw2unit, &unit2deg).orient(io);
                                     if group == &GroupId::Gyroscope     { final_data[data_index + j].gyro = Some([ itm.x, itm.y, itm.z ]); }
                                else if group == &GroupId::Accelerometer { final_data[data_index + j].accl = Some([ itm.x, itm.y, itm.z ]); }
                                else if group == &GroupId::Magnetometer  { final_data[data_index + j].magn = Some([ itm.x, itm.y, itm.z ]); }
                            }
                        }, 
                        // Insta360
                        TagValue::Vec_TimeVector3_f64(arr) => {
                            for (j, v) in arr.get().iter().enumerate() {
                                if v.t < first_frame_ts { continue; } // Skip gyro readings before actual first frame
                                if final_data.len() <= data_index + j {
                                    final_data.resize_with(data_index + j + 1, Default::default);
                                    let timestamp_multiplier = if is_insta360_raw_gyro { 1.0 } else { 1000.0 };
                                    final_data[data_index + j].timestamp_ms = (v.t - first_frame_ts) * timestamp_multiplier;
                                }
                                let itm = v.clone().into_scaled(&raw2unit, &unit2deg).orient(io);
                                     if group == &GroupId::Gyroscope     { final_data[data_index + j].gyro = Some([ itm.x, itm.y, itm.z ]); }
                                else if group == &GroupId::Accelerometer { final_data[data_index + j].accl = Some([ itm.x, itm.y, itm.z ]); }
                                else if group == &GroupId::Magnetometer  { final_data[data_index + j].magn = Some([ itm.x, itm.y, itm.z ]); }
                            }
                        },
                        _ => ()
                    }
                }
            }
        }
        data_index = final_data.len();
    }
    Ok(final_data)
}

// TODO: move to their respective modules
pub fn frame_readout_time(input: &crate::Input) -> Option<f64> {
    for info in input.samples.as_ref().unwrap() {
        if info.tag_map.is_none() { continue; }

        let grouped_tag_map = info.tag_map.as_ref().unwrap();

        // Insta360
        if let Some(val) = crate::try_block!(f64, {
            (grouped_tag_map.get(&GroupId::Default)?.get_t(TagId::Metadata) as Option<&serde_json::Value>)?
                .as_object()?
                .get("rolling_shutter_time")?
                .as_f64()?
            }) {
            return Some(val);
        }
            
        // GoPro
        if let Some(val) = crate::try_block!(f32, { *(grouped_tag_map.get(&GroupId::Default)?.get_t(TagId::Unknown(0x53524F54 /*SROT*/)) as Option<&f32>)? }) {
            return Some(val as f64);
        }
    }
    None
}

pub fn find_between_with_offset(buffer: &[u8], from: &[u8], to: u8, offset: i32) -> Option<String> {
    let pos = memmem::find(buffer, from)?;
    let end = memchr::memchr(to, &buffer[pos+from.len()..])?;
    Some(String::from_utf8_lossy(&buffer[(pos as i32 + from.len() as i32 + offset) as usize..pos+from.len()+end]).into())
}

pub fn find_between(buffer: &[u8], from: &[u8], to: u8) -> Option<String> {
    find_between_with_offset(buffer, from, to, 0)
}

pub fn insert_tag(map: &mut GroupedTagMap, tag: TagDescription) {
    let group_map = map.entry(tag.group.clone()).or_insert_with(TagMap::new);
    group_map.insert(tag.id.clone(), tag);
}

pub fn create_csv_map<'a, 'b>(row: &'b csv::StringRecord, headers: &'a Vec<String>) -> BTreeMap<&'a str, &'b str> {
    headers.iter().zip(row).map(|(a, b)| (&a[..], b.trim())).collect()
}
pub fn create_csv_map_hdr<'a, 'b>(row: &'b csv::StringRecord, headers: &'a csv::StringRecord) -> BTreeMap<&'a str, &'b str> {
    headers.iter().zip(row).map(|(a, b)| (a, b)).collect()
}

#[macro_export]
macro_rules! try_block {
    ($type:ty, $body:block) => {
        (|| -> Option<$type> {
            Some($body)
        }())
    };
    ($body:block) => {
        (|| -> Option<()> {
            $body
            Some(())
        }())
    };
}