use ::prost::alloc::string::String;
use ::prost::alloc::vec::Vec;
use ::core::option::Option;

#[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
#[serde(default)]
pub struct DebugInfoMain {
    #[prost(message, optional, tag="1")] pub header: Option<Header>,
    #[prost(message, repeated, tag="2")] pub frames: Vec<PerFrameMsg>,
}

#[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
pub struct Header {
    #[prost(message, optional, tag="1")] pub general: Option<HeaderInner1>,
    #[prost(message, optional, tag="2")] pub sensor_info: Option<HeaderInner2>,
}

#[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
pub struct HeaderInner1 {
    #[prost(message, optional, tag="1")] pub timestamp: Option<Timestamp>,
    #[prost(message, optional, tag="2")] pub protobuf_filename: Option<Strings2>,
}

#[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
pub struct HeaderInner2 {
    #[prost(message, optional, tag="1")] pub sensor: Option<String1>,
}

#[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
pub struct Timestamp {
    #[prost(uint32, tag="1")] pub timestamp: u32,
}
#[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
pub struct TimestampAndIndex {
    #[prost(uint32, tag="2")] pub timestamp: u32,
    #[prost(uint32, tag="3")] pub index: u32,
}
#[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
pub struct Strings2 {
    #[prost(string, tag="1")] pub str1: String,
    #[prost(string, tag="2")] pub str2: String,
}
#[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
pub struct IntString {
    #[prost(uint32, tag="1")] pub num1: u32,
    #[prost(string, tag="2")] pub str2: String,
}
#[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
pub struct String1 {
    #[prost(string, tag="1")] pub data: String,
}

#[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
pub struct PerFrameMsg {
    #[prost(message, optional, tag="1")] pub inner: Option<PerFrameMsgInner>,
}

#[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
pub struct PerFrameMsgInner {
    #[prost(message, optional, tag="1")] pub timestamp: Option<TimestampAndIndex>,
    #[prost(message, optional, tag="2")] pub sensor_mode: Option<IntString>,
    #[prost(message, optional, tag="3")] pub record_mode: Option<IntString>,
    #[prost(message, optional, tag="4")] pub frame_data4: Option<FrameDataInner4>,
    #[prost(message, optional, tag="5")] pub frame_data5_imu: Option<FrameDataInner5>,
    // #[prost(message, optional, tag="6")] pub frame_data5: Option<FrameDataInner6>, // unknown/empty
    #[prost(message, optional, tag="7")] pub frame_data7: Option<FrameDataInner7>,
    #[prost(message, optional, tag="8")] pub frame_data8: Option<FrameDataInner8>,
    // #[prost(message, optional, tag="9")] pub frame_data9: Option<FrameDataInner9>, // unknown/empty
    // #[prost(message, optional, tag="10")] pub frame_data10: Option<FrameDataInner10>, // unknown/empty
}
#[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
pub struct FrameDataInner4 {
    #[prost(int32, tag="1")] pub unknown1: i32,
    #[prost(float, tag="2")] pub unknownf2: f32,
    #[prost(float, tag="3")] pub unknownf3: f32,
    #[prost(uint32, tag="4")] pub unknown4: u32,
    #[prost(uint32, tag="5")] pub unknown5: u32,
    #[prost(uint32, tag="8")] pub unknown6: u32,

    #[prost(float, tag="12")] pub unknownf12: f32,

    #[prost(uint32, tag="15")] pub unknown15: u32,
    #[prost(uint32, tag="16")] pub unknown16: u32,
    #[prost(uint32, tag="19")] pub unknown19: u32, // Probably ISO

    #[prost(float, tag="21")] pub frame_rate: f32,
    #[prost(float, tag="22")] pub unknownf22: f32,
    #[prost(float, tag="23")] pub unknownf23: f32,
    #[prost(float, tag="24")] pub unknownf24: f32,
    #[prost(float, tag="25")] pub unknownf25: f32,
    #[prost(uint32, tag="26")] pub unknown26: u32,

    #[prost(float, tag="27")] pub unknownf27: f32,
    #[prost(float, tag="30")] pub unknownf30: f32,
    #[prost(float, tag="31")] pub unknownf31: f32,
    #[prost(float, tag="33")] pub unknownf33: f32,
    #[prost(float, tag="34")] pub unknownf34: f32,
    #[prost(float, tag="35")] pub unknownf35: f32,
    #[prost(float, tag="36")] pub unknownf36: f32,
    #[prost(float, tag="37")] pub unknownf37: f32,
    #[prost(float, tag="38")] pub unknownf38: f32,
    #[prost(float, tag="39")] pub unknownf39: f32,
    #[prost(float, tag="40")] pub unknownf40: f32,
    #[prost(float, tag="41")] pub unknownf41: f32,
    #[prost(float, tag="42")] pub unknownf42: f32,
    #[prost(float, tag="43")] pub unknownf43: f32,
    #[prost(float, tag="44")] pub unknownf44: f32,
    #[prost(float, tag="45")] pub unknownf45: f32,
    #[prost(float, tag="46")] pub unknownf46: f32,
    #[prost(float, tag="47")] pub unknownf47: f32,
    #[prost(float, tag="48")] pub unknownf48: f32,
    #[prost(float, tag="50")] pub unknownf50: f32,
    #[prost(float, tag="51")] pub unknownf51: f32,
    #[prost(float, tag="52")] pub unknownf52: f32,
    #[prost(float, tag="53")] pub unknownf53: f32,
    #[prost(float, tag="58")] pub unknownf58: f32,
    #[prost(uint32, tag="61")] pub unknown61: u32,
    #[prost(uint32, tag="62")] pub unknown62: u32,
    #[prost(float, tag="63")] pub unknownf63: f32,
    #[prost(float, tag="64")] pub unknownf64: f32,
    #[prost(float, tag="65")] pub unknownf65: f32,
    #[prost(uint32, tag="67")] pub unknown67: u32,
    
    #[prost(float, tag="68")] pub unknownf68: f32,
    #[prost(uint32, tag="69")] pub unknown69: u32,
    #[prost(uint32, tag="70")] pub unknown70: u32,
    #[prost(uint32, tag="71")] pub unknown71: u32,

    #[serde(serialize_with="bytes_serializer")]
    #[prost(bytes="vec", tag="74")] pub unknown74_bin: Vec<u8>,
    
    #[prost(float, tag="75")] pub unknownf75: f32,
    #[prost(float, tag="77")] pub unknownf77: f32,
    #[prost(float, tag="78")] pub unknownf78: f32,
    #[prost(float, tag="79")] pub unknownf79: f32,
    #[prost(float, tag="80")] pub unknownf80: f32,
    #[prost(float, tag="81")] pub unknownf81: f32,
    #[prost(float, tag="85")] pub unknownf85: f32,
    #[prost(float, tag="86")] pub unknownf86: f32,

    #[prost(uint32, tag="87")] pub unknown_size1: u32,
    #[prost(uint32, tag="88")] pub unknown_size2: u32,
    #[serde(serialize_with="bytes_serializer")]
    #[prost(bytes="vec", tag="89")] pub floats32_bin1: Vec<u8>,

    #[prost(uint32, tag="90")] pub unknown_size3: u32,

    #[serde(serialize_with="bytes_serializer")] 
    #[prost(bytes="vec", tag="91")] pub floats32_bin2: Vec<u8>,

    #[serde(serialize_with="bytes_serializer")] 
    #[prost(bytes="vec", tag="92")] pub null_bin: Vec<u8>,
    
    #[prost(float, tag="93")] pub unknownf93: f32,

    #[prost(uint32, tag="96")] pub unknown96: u32,
    #[prost(uint32, tag="97")] pub unknown97: u32,

    #[prost(float, tag="111")] pub unknownf111: f32,
    #[prost(float, tag="112")] pub unknownf112: f32,
    #[prost(float, tag="113")] pub unknownf113: f32,
    #[prost(float, tag="114")] pub unknownf114: f32,
    #[prost(float, tag="115")] pub unknownf115: f32,
    #[prost(float, tag="116")] pub unknownf116: f32,
    #[prost(float, tag="117")] pub unknownf117: f32,
    #[prost(float, tag="118")] pub unknownf118: f32,
    #[prost(float, tag="119")] pub unknownf119: f32,
    #[prost(float, tag="120")] pub unknownf120: f32,
    #[prost(float, tag="121")] pub unknownf121: f32,
    #[prost(float, tag="122")] pub unknownf122: f32,
    #[prost(float, tag="123")] pub unknownf123: f32,
    #[prost(float, tag="124")] pub unknownf124: f32,
    #[prost(float, tag="125")] pub unknownf125: f32,
    #[prost(float, tag="126")] pub unknownf126: f32,
    #[prost(float, tag="127")] pub unknownf127: f32,
    #[prost(uint32, tag="129")] pub unknown129: u32,
    #[prost(float, tag="132")] pub unknownf132: f32,
    #[prost(float, tag="134")] pub unknownf134: f32,
    #[prost(float, tag="135")] pub unknownf135: f32,
    #[prost(float, tag="136")] pub unknownf136: f32,
    #[prost(float, tag="137")] pub unknownf137: f32,
}

#[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
pub struct FrameDataInner5 {
    #[serde(serialize_with="bytes_serializer")]
    #[prost(bytes="vec", tag="1")] pub data: Vec<u8>,
}

#[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
pub struct FrameDataInner7 {
    #[prost(uint32, tag="1")] pub unknown1: u32,
    #[serde(serialize_with="bytes_serializer")]
    #[prost(bytes="vec", tag="2")] pub unknown2_bin: Vec<u8>,

    #[prost(uint32, tag="3")] pub unknown3: u32,

    #[prost(uint32, tag="4")] pub unknown4: u32,
    #[serde(serialize_with="bytes_serializer")]
    #[prost(bytes="vec", tag="5")] pub unknown5_bin: Vec<u8>,

    #[prost(uint32, tag="6")] pub unknown6: u32,
    #[serde(serialize_with="bytes_serializer")]
    #[prost(bytes="vec", tag="7")] pub unknown7_bin: Vec<u8>,

    #[prost(message, optional, tag="8")] pub unknown8: Option<SingleF64>,

    #[prost(uint32, tag="9")] pub unknown9: u32,
    #[prost(uint32, tag="11")] pub unknown11: u32,
    #[prost(uint32, tag="12")] pub unknown12: u32,
    
//    [6a] 13 string: (64): 
//        [80 08] 128 varint: 1024 (0x400)
//        [80 08] 128 varint: 1024 (0x400)
//        ... repeated
//    [72] 14 string: (32): 
//        [00] 0 varint: 0 (0x0)
//        [00] 0 varint: 0 (0x0)
//        ... repeated

    #[serde(serialize_with="bytes_serializer")]
    #[prost(bytes="vec", tag="15")] pub unknown15_bin: Vec<u8>,

    #[prost(uint32, tag="17")] pub unknown17: u32,

    #[serde(serialize_with="bytes_serializer")]
    #[prost(bytes="vec", tag="18")] pub unknown18_bin: Vec<u8>,

    #[serde(serialize_with="bytes_serializer")]
    #[prost(bytes="vec", tag="19")] pub unknown19_bin: Vec<u8>,

    #[serde(serialize_with="bytes_serializer")]
    #[prost(bytes="vec", tag="20")] pub unknown20_bin: Vec<u8>,
}

#[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
pub struct FrameDataInner8 {
    #[prost(float, tag="1")] pub unknownf1: f32,
}

#[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
pub struct SingleF64 {
    #[prost(double, tag="19")] pub unknownd19: f64,
}


fn bytes_serializer<S>(x: &[u8], s: S) -> std::prelude::rust_2021::Result<S::Ok, S::Error> where S: serde::Serializer {
    let mut ret = String::with_capacity(x.len() * 2);
    for b in x {
        ret.push_str(&format!("{:02x}", b));
    }
    s.serialize_str(&ret)
}

use byteorder::{ReadBytesExt, LittleEndian};
use std::io::Cursor;

pub fn parse_floats(data: &[u8]) -> std::io::Result<serde_json::Value> {
    let mut d = Cursor::new(data);
    let datalen = data.len() as u64;
    let mut ret = Vec::new();
    while d.position() < datalen {
        ret.push(d.read_f32::<LittleEndian>()?);
    }

    Ok(serde_json::to_value(ret)?)
}
