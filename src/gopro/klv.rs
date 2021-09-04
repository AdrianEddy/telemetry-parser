use std::io::*;
use byteorder::{ReadBytesExt, BigEndian};

use crate::tags_impl::*;

#[derive(Default)]
pub struct KLV {
    pub key: [u8; 4],
    pub data_type: u8,
    pub size: usize,
    pub repeat: usize
}
impl KLV {
    pub fn parse_header(d: &mut Cursor<&[u8]>) -> Result<Self> {
        if d.get_ref().len() < 8 {
            return Err(ErrorKind::UnexpectedEof.into());
        }

        let mut klv: Self = Default::default();
        d.read_exact(&mut klv.key)?;
        klv.data_type = d.read_u8()?;
        klv.size      = d.read_u8()? as usize;
        klv.repeat    = d.read_u16::<BigEndian>()? as usize;

        if klv.data_len() > (d.get_ref().len() - d.position() as usize) {
            eprintln!("Tag: {}, len: {}, Available: {}", String::from_utf8_lossy(&klv.key), klv.data_len(), (d.get_ref().len() - d.position() as usize));
            return Err(ErrorKind::UnexpectedEof.into());
        }

        Ok(klv)
    }
    pub fn data_len(&self) -> usize {
        self.size * self.repeat
    }
    pub fn aligned_data_len(&self) -> usize { // Align to 4 bytes
        let mut len = self.data_len();
        if len % 4 != 0 {
            len += 4 - len % 4;
        }
        len
    }
    pub fn key_as_string(&self) -> String {
        String::from_utf8_lossy(&self.key).to_string()
    }
    fn get_repeat_count<T>(&self) -> (usize, usize) {
        (
            self.repeat,
            self.size / std::mem::size_of::<T>()
        )
    }
    pub fn parse_data(&self, tag_data: &[u8]) -> TagValue {
        macro_rules! types {
            ($($field:expr => ($type:ty, $body:expr)),*,) => {
                match self.data_type {
                    $($field => {
                        paste::paste! {
                            match self.get_repeat_count::<$type>() {
                                (1, 1) => TagValue::[<$type>]                 (ValueType::new(|d| Self::parse_single      ::<$type>(d, $body), |v| format!("{}",   v), tag_data.to_vec())),
                                (_, 1) => TagValue::[<Vec_ $type>]            (ValueType::new(|d| Self::parse_list        ::<$type>(d, $body), |v| format!("{:?}", v), tag_data.to_vec())),
                                (_, 3) => TagValue::[<Vec_Vector3_ $type>]    (ValueType::new(|d| Self::parse_vector3     ::<$type>(d, $body), |v| format!("{:?}", v), tag_data.to_vec())),
                                (_, 4) => TagValue::[<Vec_TimeVector3_ $type>](ValueType::new(|d| Self::parse_timevector3 ::<$type>(d, $body), |v| format!("{:?}", v), tag_data.to_vec())),
                                (_, _) => TagValue::[<Vec_Vec_ $type>]        (ValueType::new(|d| Self::parse_nested      ::<$type>(d, $body), |v| format!("{:?}", v), tag_data.to_vec()))
                            }
                        }
                    },)*
                    b'c' => TagValue::String(ValueType::new(|d| Self::parse_string(d),   |v| v.into(), tag_data.to_vec())),
                    b'F' => TagValue::String(ValueType::new(|d| Self::parse_string(d),   |v| v.into(), tag_data.to_vec())),
                    b'G' => TagValue::Uuid  (ValueType::new(|d| Self::parse_uuid(d),     |v| format!("{{{:08x}-{:08x}-{:08x}-{:08x}}}", v.0, v.1, v.2, v.3), tag_data.to_vec())),
                    b'U' => TagValue::u64   (ValueType::new(|d| Self::parse_utcdate(d),  |v| chrono::TimeZone::timestamp_millis(&chrono::Utc, *v as i64).to_string(), tag_data.to_vec())),
                    // b'?' => unimplemented!(),
                    _ => TagValue::Unknown(ValueType::new(|_| Ok(()), |_| "".into(), tag_data.to_vec()))
                }
            };
        }
        if self.data_type == b's' && (&tag_data[..4] == b"CORI" || &tag_data[..4] == b"IORI") { // Quaternions
            return TagValue::Vec_Quaternioni16(ValueType::new(|d| Self::parse_quaternion::<i16>(d, |d| d.read_i16::<BigEndian>()), |v| format!("{:?}", v), tag_data.to_vec()))
        }

        types! {
            b'b' => (i8,  |d| d.read_i8()),
            b'B' => (u8,  |d| d.read_u8()),
            b's' => (i16, |d| d.read_i16::<BigEndian>()),
            b'S' => (u16, |d| d.read_u16::<BigEndian>()),
            b'l' => (i32, |d| d.read_i32::<BigEndian>()),
            b'L' => (u32, |d| d.read_u32::<BigEndian>()),
            b'f' => (f32, |d| d.read_f32::<BigEndian>()),
            b'd' => (f64, |d| d.read_f64::<BigEndian>()),
            b'j' => (i64, |d| d.read_i64::<BigEndian>()),
            b'J' => (u64, |d| d.read_u64::<BigEndian>()),
            b'q' => (f32, |d| Ok(d.read_i16::<BigEndian>()? as f32 + (d.read_u16::<BigEndian>()? as f32 / 65536.0))),
            b'Q' => (f64, |d| Ok(d.read_i32::<BigEndian>()? as f64 + (d.read_u32::<BigEndian>()? as f64 / 4294967295.0))),
        }
    }

    pub fn tag_id(&self) -> TagId {
        match &self.key {
            b"GYRO" | b"ACCL" | b"GRAV" |
            b"WBAL" | b"ISOE" | b"SHUT" |
            b"MWET" | b"IORI" | b"CORI" |
            b"AALP" | b"WNDM" | b"UNIF" |
            b"WRGB" => TagId::Data,

            b"SIUN" | b"UNIT" => TagId::Unit,
            b"MTRX" => TagId::Matrix,
            b"SCAL" => TagId::Scale,
            b"STMP" => TagId::TimestampUs,
            b"STNM" => TagId::Name,
            b"DVNM" => TagId::Name,
            b"TMPC" => TagId::Temperature,
            b"TSMP" => TagId::Count,
            b"ORIN" => TagId::OrientationIn,
            b"ORIO" => TagId::OrientationOut,
            x => TagId::Unknown((&x[..]).read_u32::<BigEndian>().unwrap())
        }
    }
    pub fn group_from_key(k: &[u8]) -> GroupId {
        if k.is_empty() { return GroupId::UnknownGroup(0); }
        match &k[..4] {
            b"GYRO" => GroupId::Gyroscope,
            b"ACCL" => GroupId::Accelerometer,
            b"GRAV" => GroupId::GravityVector,
            b"CORI" => GroupId::CameraOrientation,
            b"IORI" => GroupId::ImageOrientation,
            b"SHUT" => GroupId::Exposure,
            b"MWET" => GroupId::Custom("MicrophoneWet".into()),
            b"AALP" => GroupId::Custom("AGCAudioLevel".into()),
            b"WNDM" => GroupId::Custom("WindProcessing".into()),
            b"UNIF" => GroupId::Custom("ImageUniformity".into()),
            b"WRGB" => GroupId::Custom("WhiteBalanceRGBGains".into()),
            b"WBAL" => GroupId::Custom("WhiteBalanceTemperature".into()),
            b"ISOE" => GroupId::Custom("SensorISO".into()),
            x => GroupId::Custom(x[..].iter().map(|&c| c as char).collect::<String>())
        }
    }

    // ---------- Value parsers ----------

    fn parse_string(d: &mut Cursor::<&[u8]>) -> Result<String> {
        Ok((&d.get_ref()[8..].iter().map(|&c| c as char).collect::<String>()).trim_end_matches(char::from(0)).to_string())
    }
    fn parse_utcdate(x: &mut Cursor::<&[u8]>) -> Result<u64> {
        let e = |_| -> Error { ErrorKind::InvalidData.into() };
        let data = std::str::from_utf8(&x.get_ref()[8..8+16]).map_err(|_| -> Error { ErrorKind::InvalidData.into() })?;

        let y  = 2000 + &data[0..2].parse::<i32>().map_err(e)?;
        let m  = data[2..4]  .parse::<u32>().map_err(e)?;
        let d  = data[4..6]  .parse::<u32>().map_err(e)?;
        let h  = data[6..8]  .parse::<u32>().map_err(e)?;
        let i  = data[8..10] .parse::<u32>().map_err(e)?;
        let s  = data[10..12].parse::<u32>().map_err(e)?;
        let ms = data[13..16].parse::<u32>().map_err(e)?;

        Ok(chrono::NaiveDate::from_ymd(y, m, d).and_hms_milli(h, i, s, ms).timestamp_millis() as u64)
    }
    fn parse_uuid(d: &mut Cursor::<&[u8]>) -> Result<(u32,u32,u32,u32)> {
        d.seek(SeekFrom::Current(8))?; // Skip header
        Ok((d.read_u32::<BigEndian>()?, d.read_u32::<BigEndian>()?, d.read_u32::<BigEndian>()?, d.read_u32::<BigEndian>()?))
    }
    fn parse_single<T>(d: &mut Cursor<&[u8]>, read_fn: fn(&mut Cursor<&[u8]>) -> Result<T>) -> Result<T> {
        d.seek(SeekFrom::Current(8))?; // Skip header
        read_fn(d)
    }
    fn parse_list<T>(d: &mut Cursor<&[u8]>, read_fn: fn(&mut Cursor<&[u8]>) -> Result<T>) -> Result<Vec<T>> {
        let repeat = Self::parse_header(d)?.repeat;

        (0..repeat).map(|_| read_fn(d)).collect()
    }
    fn parse_vector3<T>(d: &mut Cursor<&[u8]>, read_fn: fn(&mut Cursor<&[u8]>) -> Result<T>) -> Result<Vec<Vector3<T>>> {
        let repeat = Self::parse_header(d)?.repeat;

        (0..repeat).map(|_| Ok(Vector3 {
            x: read_fn(d)?,
            y: read_fn(d)?,
            z: read_fn(d)?
        })).collect()
    }
    fn parse_quaternion<T>(d: &mut Cursor<&[u8]>, read_fn: fn(&mut Cursor<&[u8]>) -> Result<T>) -> Result<Vec<Quaternion<T>>> {
        let repeat = Self::parse_header(d)?.repeat;
        
        (0..repeat).map(|_| Ok(Quaternion {
            w: read_fn(d)?,
            x: read_fn(d)?,
            y: read_fn(d)?,
            z: read_fn(d)?
        })).collect()
    }
    fn parse_timevector3<T>(d: &mut Cursor<&[u8]>, read_fn: fn(&mut Cursor<&[u8]>) -> Result<T>) -> Result<Vec<TimeVector3<T>>> {
        let repeat = Self::parse_header(d)?.repeat;

        (0..repeat).map(|_| Ok(TimeVector3 {
            t: read_fn(d)?,
            x: read_fn(d)?,
            y: read_fn(d)?,
            z: read_fn(d)?
        })).collect()
    }
    fn parse_nested<T>(d: &mut Cursor<&[u8]>, read_fn: fn(&mut Cursor<&[u8]>) -> Result<T>) -> Result<Vec<Vec<T>>> {
        let (repeat, items_in_chunk) = Self::parse_header(d)?.get_repeat_count::<T>();
        
        (0..repeat).map(|_| {
            (0..items_in_chunk).map(|_| read_fn(d)).collect()
        }).collect()
    }

    pub fn orientations_to_matrix(orin: &str, orio: &str) -> Option<Vec<f32>> {
        if orin.is_empty() || (orin.len() != orio.len()) { return None; }
    
        Some(orio.chars()
            .flat_map(|o| orin.chars().map(move |i|
                     if i == o                     {  1.0 }
                else if i.eq_ignore_ascii_case(&o) { -1.0 }
                else                               {  0.0 }
            ))
            .collect::<Vec<f32>>())
    }
}

impl std::fmt::Debug for KLV {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KLV")
        .field("key", &String::from_utf8_lossy(&self.key))
        .field("type", &(self.data_type as char))
        .field("size", &self.size)
        .field("repeat", &self.repeat)
        .field("data_len", &self.data_len())
        .field("aligned_data_len", &self.aligned_data_len())
        .finish()
    }
}
