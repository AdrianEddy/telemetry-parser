use std::io::*;
use std::path::Path;
use std::sync::{ Arc, atomic::AtomicBool };

use crate::tags_impl::*;
use crate::*;
use byteorder::{ ReadBytesExt, BigEndian };

#[derive(Default)]
pub struct RedR3d {
    pub model: Option<String>,
    all_parts: Vec<String>,
}

impl RedR3d {
    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], filepath: P) -> Option<Self> {
        if buffer.len() > 8 && &buffer[4..8] == b"RED2" {
            Some(Self {
                model: None,
                all_parts: Self::detect_all_parts(filepath.as_ref()).unwrap_or_default()
            })
        } else {
            None
        }
    }

    fn detect_all_parts(path: &Path) -> Result<Vec<String>> {
        let mut ret = Vec::new();
        if let Some(filename) = path.file_name().map(|x| x.to_string_lossy()) {
            if let Some(pos) = filename.rfind('_') {
                let filename_base = &filename[0..pos + 1];

                if let Some(parent) = path.parent() {
                    for x in parent.read_dir()? {
                        let x = x?;
                        let fname = x.file_name().to_string_lossy().to_string();
                        if fname.starts_with(filename_base) && fname.to_lowercase().ends_with(".r3d") {
                            if let Some(p) = x.path().to_str() {
                                ret.push(p.to_string());
                            }
                        }
                    }
                }
            }
        }
        ret.sort_by(|a, b| human_sort::compare(a, b));
        Ok(ret)
    }
    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, _stream: &mut T, _size: usize, progress_cb: F, cancel_flag: Arc<AtomicBool>) -> Result<Vec<SampleInfo>> {
        let mut gyro = Vec::new();
        let mut accl = Vec::new();
        let mut first_timestamp = None;

        let total_count = self.all_parts.len() as f64;

        for (i, file) in self.all_parts.iter().enumerate() {
            let stream = std::fs::File::open(file)?;
            let filesize = stream.metadata()?.len() as usize;

            let mut stream = std::io::BufReader::with_capacity(128*1024, stream);

            while let Ok(size) = stream.read_u32::<BigEndian>() {
                let mut name = [0u8; 4];
                stream.read_exact(&mut name)?;
                let aligned_size = ((size as f64 / 4096.0).ceil() * 4096.0) as usize;
                // println!("Name: {}{}{}{}, size: {}", name[0] as char, name[1] as char, name[2] as char, name[3] as char, aligned_size);
                if &name == b"RDX\x02" {
                    let mut data = Vec::with_capacity(aligned_size);
                    data.resize(aligned_size, 0);
                    stream.seek(SeekFrom::Current(-8))?;
                    stream.read_exact(&mut data)?;
                    if data.len() > 4096 {
                        let mut data = &data[4096..];
                        crate::try_block!({
                            while let Ok(mut timestamp) = data.read_u64::<BigEndian>() {
                                if timestamp > 0 {
                                    if first_timestamp.is_none() {
                                        first_timestamp = Some(timestamp);
                                    }
                                    timestamp -= first_timestamp.unwrap();
                                    accl.push(TimeVector3 { t: timestamp as f64 / 1000000.0,
                                        x: data.read_i16::<BigEndian>().ok()? as f64 / 1000.0,
                                        y: data.read_i16::<BigEndian>().ok()? as f64 / 1000.0,
                                        z: data.read_i16::<BigEndian>().ok()? as f64 / 1000.0
                                    });
                                    gyro.push(TimeVector3 { t: timestamp as f64 / 1000000.0,
                                        x: data.read_i16::<BigEndian>().ok()? as f64 / 10.0,
                                        y: data.read_i16::<BigEndian>().ok()? as f64 / 10.0,
                                        z: data.read_i16::<BigEndian>().ok()? as f64 / 10.0
                                    });
                                }
                            }
                        });
                    }
                } else {
                    stream.seek(SeekFrom::Current(aligned_size as i64 - 8))?;
                }

                if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) { break; }
                if filesize > 0 {
                    progress_cb((stream.stream_position()? as f64 / filesize as f64) * ((i as f64 + 1.0) / total_count));
                }
            }
        }

        let mut map = GroupedTagMap::new();

        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Data, "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), accl, vec![]));
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Data, "Gyroscope data",     Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]));

        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Unit, "Accelerometer unit", String, |v| v.to_string(), "m/s²".into(),  Vec::new()));
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Unit, "Gyroscope unit",     String, |v| v.to_string(), "deg/s".into(), Vec::new()));

        let imu_orientation = "zyx";
        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()));
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()));

        Ok(vec![
            SampleInfo { index: 0, timestamp_ms: 0.0, duration_ms: 0.0, tag_map: Some(map) }
        ])
    }

    pub fn normalize_imu_orientation(v: String) -> String {
        v
    }

    pub fn camera_type(&self) -> String {
        "RED RAW".to_owned()
    }

    pub fn frame_readout_time(&self) -> Option<f64> {
        None
    }
}
