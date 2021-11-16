use std::io::*;
use byteorder::ReadBytesExt;

use crate::tags_impl::*;
use crate::*;
use memchr::memmem;

pub fn detect(buffer: &[u8], _filename: &str) -> bool {
    let hdr = &buffer[..200.min(buffer.len() - 1)];

    memmem::find(hdr, b"Time").is_some() &&
    memmem::find(hdr, b"Xg").is_some() &&
    memmem::find(hdr, b"Yg").is_some() &&
    memmem::find(hdr, b"Zg").is_some() &&
    memmem::find(hdr, b"Pitch").is_some() &&
    memmem::find(hdr, b"Roll").is_some() &&
    memmem::find(hdr, b"Yaw").is_some()
}

pub fn parse<T: Read + Seek>(stream: &mut T, size: usize) -> Result<Vec<SampleInfo>> {
    let mut gyro = Vec::new();
    let mut accl = Vec::new();
    
    let mut last_timestamp = 0.0;
    let mut first_timestamp = 0.0;
    
    // Replace all repeating whitespace with a single space
    let mut buffer = Vec::with_capacity(size);
    let mut prev_chr = '\0';
    while (stream.stream_position()? as usize) < size {
        let chr = stream.read_u8()? as char;
        if !(prev_chr.is_ascii_whitespace() && chr.is_ascii_whitespace()) || chr == '\n' {
            buffer.push(chr as u8);
            prev_chr = chr;
        }
    }
    let d = Cursor::new(&buffer[..]);

    let mut csv = csv::ReaderBuilder::new()
        .has_headers(true)
        .trim(csv::Trim::All)
        .delimiter(b' ')
        .from_reader(d);
    
    let h = csv.headers()?.clone();
    for row in csv.records() {
        let row = row?;
        let map = util::create_csv_map_hdr(&row, &h);

        let mut ts = map.get("Time").unwrap_or(&"0.0").parse::<f64>().unwrap_or(0.0);
        if first_timestamp == 0.0 {
            first_timestamp = ts;
        }
        last_timestamp = ts;
        ts -= first_timestamp;

        crate::try_block!({
            accl.push(TimeVector3 {
                t: ts as f64,
                x: map.get("Xg")?.parse::<f64>().ok()?,
                y: map.get("Yg")?.parse::<f64>().ok()?,
                z: map.get("Zg")?.parse::<f64>().ok()?
            });
        });
        crate::try_block!({
            gyro.push(TimeVector3 {
                t: ts as f64,
                x: map.get("Pitch")?.parse::<f64>().ok()?,
                y: map.get("Roll") ?.parse::<f64>().ok()?,
                z: map.get("Yaw")  ?.parse::<f64>().ok()?
            });
        });
    }

    let mut map = GroupedTagMap::new();

    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Data, "Accelerometer data", Vec_TimeVector3_f64, |v| format!("{:?}", v), accl, vec![]));
    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Unit, "Accelerometer unit", String, |v| v.to_string(), "m/s²".into(),  Vec::new()));

    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Data, "Gyroscope data",     Vec_TimeVector3_f64, |v| format!("{:?}", v), gyro, vec![]));
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Unit, "Gyroscope unit",     String, |v| v.to_string(), "rad/s".into(), Vec::new()));

    let imu_orientation = "XYZ"; // TODO
    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()));
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Orientation, "IMU orientation", String, |v| v.to_string(), imu_orientation.into(), Vec::new()));

    Ok(vec![
        SampleInfo { index: 0, timestamp_ms: first_timestamp as f64, duration_ms: (last_timestamp - first_timestamp) as f64, tag_map: Some(map) }
    ])
}
