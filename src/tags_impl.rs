use once_cell::unsync::OnceCell;
use serde::Serialize;
use std::collections::*;

macro_rules! declare_groups {
    ($($field:ident),*,) => {
        #[allow(dead_code)]
        #[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Debug)]
        pub enum GroupId {
            $($field,)*
            UnknownGroup(u32),
            Custom(String),
            Any // For filtering, shouldn't be used directly
        }
        impl Serialize for GroupId {
            fn serialize<S>(&self, s: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
                match self {
                    GroupId::UnknownGroup(x) => s.serialize_str(&format!("0x{:x}", x)),
                    GroupId::Custom(x)       => s.serialize_str(x),
                    GroupId::Any             => s.serialize_str("*"),
                    $(GroupId::$field        => s.serialize_str(stringify!($field)),)*
                }
            }
        }
        impl std::fmt::Display for GroupId {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    GroupId::UnknownGroup(x) => f.write_str(&format!("0x{:x}", x)),
                    GroupId::Custom(x)       => f.write_str(x),
                    GroupId::Any             => f.write_str("*"),
                    $(GroupId::$field        => f.write_str(stringify!($field)),)*
                }
            }
        }
        impl std::str::FromStr for GroupId {
            type Err = std::num::ParseIntError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Ok(match s {
                    $(stringify!($field) => GroupId::$field,)*
                    "*" => GroupId::Any,
                    _ if s.starts_with("0x") => GroupId::UnknownGroup(u32::from_str_radix(&s[2..], 16)?),
                    _ => GroupId::Custom(s.to_string())
                })
            }
        }
    }
}

macro_rules! declare_ids {
    ($($field:ident),*,) => {
        #[allow(dead_code)]
        #[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Debug)]
        pub enum TagId {
            $($field,)*
            Unknown(u32),
            File(String),
            Custom(String),
            Any // For filtering, shouldn't be used directly
        }
        impl Serialize for TagId {
            fn serialize<S>(&self, s: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
                match self {
                    TagId::Unknown(x)      => s.serialize_str(&format!("0x{:x}", x)),
                    TagId::Custom(x)       => s.serialize_str(x),
                    TagId::File(x)         => s.serialize_str(x),
                    TagId::Any             => s.serialize_str("*"),
                    $(TagId::$field        => s.serialize_str(stringify!($field)),)*
                }
            }
        }
        impl std::fmt::Display for TagId {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    TagId::Unknown(x)      => f.write_str(&format!("0x{:x}", x)),
                    TagId::Custom(x)       => f.write_str(x),
                    TagId::File(x)         => f.write_str(x),
                    TagId::Any             => f.write_str("*"),
                    $(TagId::$field        => f.write_str(stringify!($field)),)*
                }
            }
        }
        impl std::str::FromStr for TagId {
            type Err = std::num::ParseIntError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Ok(match s {
                    $(stringify!($field) => TagId::$field,)*
                    "*" => TagId::Any,
                    _ if s.starts_with("0x") => TagId::Unknown(s.parse::<u32>()?),
                    _ => TagId::Custom(s.to_string())
                })
            }
        }
    }
}

macro_rules! declare_types {
    ($($field:ident:$type:ty),*,) => {
        #[allow(non_camel_case_types)]
        #[allow(dead_code)]
        #[derive(Clone)]
        pub enum TagValue {
            $($field(ValueType<$type>),)*
            Unknown(ValueType<()>),
        }
        impl ToString for TagValue {
            fn to_string(&self) -> String {
                match &self {
                    $(TagValue::$field(t) => (t.format_fn)(t.get()),)*
                    TagValue::Unknown(t) => format!("{} bytes: {}", t.raw_data.len(), crate::util::to_hex(&t.raw_data[..])),
                }
            }
        }
        impl Serialize for TagValue {
            fn serialize<S>(&self, s: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
                match &self {
                    $(TagValue::$field(t) => serde::Serialize::serialize(t.get(), s),)*
                    TagValue::Unknown(t) => s.serialize_bytes(&t.raw_data),
                }
            }
        }

        /*impl<T> std::convert::TryInto<ValueType<T>> for TagValue {
            type Error = &'static str;
            fn try_into(self) -> Result<ValueType<T>, Self::Error> {
                match self {
                    $(TagValue::$field(t) => Ok(t),)*
                    TagValue::Unknown(t) => Err("Unknown TagValue"),
                    _ => Err("Unknown TagValue")
                }
            }
        }*/
        pub trait GetWithType<T> { fn get_t(&self, k: TagId) -> Option<&T>; }
        $(
            impl std::convert::TryInto<$type> for TagValue {
                type Error = &'static str;
                fn try_into(self) -> Result<$type, Self::Error> {
                    if let TagValue::$field(t) = self {
                        return Ok(t.get().clone());
                    }
                    Err("Unknown TagValue")
                }
            }
            impl GetWithType<$type> for TagMap {
                fn get_t(&self, k: TagId) -> Option<&$type> {
                    if let Some(v) = self.get(&k) {
                        if let TagValue::$field(vv) = &v.value {
                            return Some(vv.get());
                        }
                    }
                    None
                }
            }
        )*
        impl std::fmt::Debug for TagValue {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match &self {
                    $(TagValue::$field(t) => f.write_fmt(format_args!("TagValue(\n\tType: {}\n\tValue: {:?}\n\tFormatted value: {}\n)", stringify!($field), &t.get(), self.to_string())),)*
                    TagValue::Unknown(_)  => f.write_fmt(format_args!("TagValue(\n\tType: Unknown\n\tValue: {}\n)", self.to_string()))
                }
            }
        }
    };
}

include!("tags.rs");

#[derive(Debug, Clone)]
pub struct TagDescription {
    pub group: GroupId,
    pub id: TagId,
    pub native_id: Option<u32>,
    pub description: String,
    pub value: TagValue,
}

type ParseFn<T> = fn(&mut std::io::Cursor::<&[u8]>) -> std::io::Result<T>;

#[derive(Clone)]
pub struct ValueType<T> {
    parse_fn: Option<ParseFn<T>>,
    format_fn: fn(&T) -> String,
    parsed_value: OnceCell<T>,
    pub raw_data: Vec<u8>
}
impl<T> ValueType<T> {
    pub fn new(parse_fn: ParseFn<T>, format_fn: fn(&T) -> String, raw_data: Vec<u8>) -> ValueType<T> {
        ValueType {
            parse_fn: Some(parse_fn),
            format_fn,
            raw_data,
            parsed_value: once_cell::unsync::OnceCell::new()
        }
    }
    pub fn new_parsed(format_fn: fn(&T) -> String, parsed_value: T, raw_data: Vec<u8>) -> ValueType<T> {
        let v = once_cell::unsync::OnceCell::new();
        let _ = v.set(parsed_value);
        ValueType {
            parse_fn: None,
            format_fn,
            raw_data,
            parsed_value: v
        }
    }
    pub fn get(&self) -> &T {
        self.parsed_value.get_or_init(|| {
            let mut tag_slice = std::io::Cursor::new(&self.raw_data[..]);
            (self.parse_fn.expect("value not parsed"))(&mut tag_slice).unwrap()
        })
    }
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Vector3<T> {
    pub x: T,
    pub y: T, 
    pub z: T,
}
impl<T: std::convert::Into<f64>> Vector3<T> {
    pub fn into_scaled(self, raw2unit: &f64, unit2deg: &f64) -> Vector3<f64> {
        Vector3 {
            x: self.x.into() / raw2unit * unit2deg,
            y: self.y.into() / raw2unit * unit2deg,
            z: self.z.into() / raw2unit * unit2deg,
        }
    }
}
impl Vector3<f64> {
    pub fn orient(&self, io: &[u8]) -> Vector3<f64> {
        let map = |o: u8| -> f64 {
            match o as char {
                'X' => self.x, 'x' => -self.x,
                'Y' => self.y, 'y' => -self.y,
                'Z' => self.z, 'z' => -self.z, 
                err => { panic!("Invalid orientation {}", err); }
            }
        };
        Vector3 { x: map(io[0]), y: map(io[1]), z: map(io[2]) }
    }
}
#[derive(Debug, Clone, Serialize, Default)]
pub struct TimeVector3<T> {
    pub t: T,
    pub x: T,
    pub y: T, 
    pub z: T,
}
impl<T: std::convert::Into<f64>> TimeVector3<T> {
    pub fn into_scaled(self, raw2unit: &f64, unit2deg: &f64) -> Vector3<f64> {
        Vector3 {
            x: self.x.into() / raw2unit * unit2deg,
            y: self.y.into() / raw2unit * unit2deg,
            z: self.z.into() / raw2unit * unit2deg,
        }
    }
}
#[derive(Debug, Clone, Serialize, Default)]
pub struct TimeScalar<T> {
    pub t: f64,
    pub v: T
}
#[derive(Debug, Clone, Serialize, Default)]
pub struct Quaternion<T> {
    pub w: T,
    pub x: T,
    pub y: T, 
    pub z: T,
}
#[derive(Debug, Clone, Serialize, Default)]
pub struct GpsData {
    pub is_acquired: bool,
    pub unix_timestamp: f64,
    pub lat: f64,
    pub lon: f64,
    pub speed: f64, // in km/h
    pub track: f64,
    pub altitude: f64, // in m
}

#[macro_export]
macro_rules! tag {
    ($group:expr, $id:expr, $name:literal, $type:ident, $format:literal, $body:expr, $tag_data:expr) => {
        TagDescription { group: $group, id: $id, description: $name.to_owned(), value: TagValue::$type(ValueType::new($body, |v| format!($format, v), $tag_data.to_vec())), native_id: None }
    };
    ($group:expr, $id:expr, $name:literal, $type:ident, $format:expr, $body:expr, $tag_data:expr) => {
        TagDescription { group: $group, id: $id, description: $name.to_owned(), value: TagValue::$type(ValueType::new($body, $format, $tag_data.to_vec())), native_id: None }
    };
    (parsed $group:expr, $id:expr, $name:literal, $type:ident, $format:expr, $val:expr, $tag_data:expr) => {
        TagDescription { group: $group, id: $id, description: $name.to_owned(), value: TagValue::$type(ValueType::new_parsed($format, $val, $tag_data.to_vec())), native_id: None }
    };
    ($group:expr, $id:expr, $name:literal, $tag_data:expr) => {
        TagDescription { group: $group, id: $id, description: $name.to_owned(), value: TagValue::Unknown(ValueType::new(|_| Ok(()), |_| "".into(), $tag_data.to_vec())), native_id: None }
    };
}

pub type TagMap = BTreeMap<TagId, TagDescription>;
pub type GroupedTagMap = BTreeMap<GroupId, TagMap>;
