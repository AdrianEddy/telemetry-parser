// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2021 Adrian <adrian.eddy at gmail>

use ::prost::alloc::string::String;
use ::prost::alloc::vec::Vec;
use ::core::option::Option;

#[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
#[serde(default)]
pub struct ExtraMetadata {
    #[prost(string, tag="1")]  pub serial_number: String,
    #[prost(string, tag="2")]  pub camera_type: String,
    #[prost(string, tag="3")]  pub fw_version: String,
    #[prost(string, tag="4")]  pub file_type: String,
    #[prost(string, tag="5")]  pub offset: String,
    #[prost(string, tag="6")]  pub ip: String,
    #[prost(uint64, tag="7")]  pub creation_time: u64,
    #[prost(uint64, tag="8")]  pub export_time: u64,
    #[prost(uint64, tag="9")]  pub file_size: u64,
    #[prost(uint32, tag="10")] pub total_time: u32,

    #[serde(serialize_with="bytes_serializer")]
    #[prost(bytes="vec", tag="11")] pub gps: Vec<u8>,

    #[serde(serialize_with="bytes_serializer")]
    #[prost(bytes="vec", tag="12")] pub orientation: Vec<u8>,

    #[prost(message, optional, tag="13")] pub user_options: Option<extra_metadata::ExtraUserOptions>,

    #[serde(serialize_with="bytes_serializer")]
    #[prost(bytes="vec", tag="14")] pub gyro: Vec<u8>,

    #[serde(serialize_with="HdrState_serializer")]
    #[prost(enumeration="extra_metadata::HdrState", tag="15")] pub hdr_state: i32,

    #[prost(string, tag="16")] pub hdr_identifier: String,
    #[prost(string, tag="17")] pub original_offset: String,

    #[serde(serialize_with="TriggerSource_serializer")]
    #[prost(enumeration="extra_metadata::TriggerSource", tag="18")] pub trigger_source: i32,

    #[prost(message, optional, tag="19")] pub dimension: Option<extra_metadata::Vector2>,
    #[prost(int32,             tag="20")] pub frame_rate: i32,
    #[prost(message, optional, tag="21")] pub image_translate: Option<extra_metadata::Vector2>,
    #[prost(string,            tag="22")] pub gamma_mode: String,
    #[prost(message, optional, tag="23")] pub thumbnail_gyro_index: Option<extra_metadata::GyroIndex>,
    #[prost(int64,             tag="24")] pub first_frame_timestamp: i64,
    #[prost(double,            tag="25")] pub rolling_shutter_time: f64,
    #[prost(message, optional, tag="26")] pub file_group_info: Option<extra_metadata::FileGroupInfo>,
    #[prost(message, optional, tag="27")] pub window_crop_info: Option<extra_metadata::WindowCropInfo>,
    #[prost(double,            tag="28")] pub gyro_timestamp: f64,
    #[prost(bool,              tag="29")] pub is_has_gyro_timestamp: bool,
    #[prost(uint32,            tag="30")] pub timelapse_interval: u32,

    #[serde(serialize_with="bytes_serializer")]
    #[prost(bytes="vec", tag="31")] pub gyro_calib: Vec<u8>,

    #[serde(serialize_with="EvoStatusMode_serializer")]
    #[prost(enumeration="extra_metadata::EvoStatusMode", tag="32")] pub evo_status_mode: i32,

    #[prost(string, tag="33")] pub evo_status_id: String,
    #[prost(string, tag="34")] pub original_offset_3d: String,

    #[serde(serialize_with="GpsSource_serializer")]
    #[prost(enumeration="extra_metadata::GpsSource", repeated, tag="35")] pub gps_sources: Vec<i32>,

    #[prost(int64, tag="36")] pub first_gps_timestamp: i64,

    #[serde(serialize_with="bytes_serializer")]
    #[prost(bytes="vec", tag="37")] pub orientation_calib: Vec<u8>,

    #[prost(bool,              tag="38")] pub is_collected: bool,
    #[prost(uint64,            tag="39")] pub recycle_time: u64,
    #[prost(uint32,            tag="40")] pub total_frames: u32,
    #[prost(bool,              tag="41")] pub is_selfie: bool,
    #[prost(bool,              tag="42")] pub is_flowstate_online: bool,
    #[prost(bool,              tag="43")] pub is_dewarp: bool,
    #[prost(message, optional, tag="44")] pub resolution_size: Option<extra_metadata::Vector2>,

    #[serde(serialize_with="BatteryType_serializer")]
    #[prost(enumeration="extra_metadata::BatteryType", tag="45")] pub battery_type: i32,

    #[serde(serialize_with="CameraPosture_serializer")]
    #[prost(enumeration="extra_metadata::CameraPosture", tag="46")] pub cam_posture: i32,

    #[serde(serialize_with="ImageFovType_serializer")]
    #[prost(enumeration="extra_metadata::ImageFovType", tag="47")] pub fov_type: i32,

    #[prost(double, tag="48")] pub distance: f64,
    #[prost(double, tag="49")] pub fov: f64,

    #[serde(serialize_with="GyroFilterType_serializer")]
    #[prost(enumeration="extra_metadata::GyroFilterType", tag="50")] pub gyro_filter_type: i32,

    #[serde(serialize_with="GyroType_serializer")]
    #[prost(enumeration="extra_metadata::GyroType", tag="51")] pub gyro_type: i32,

    #[serde(serialize_with="VideoMediaDataRotateAngle_serializer")]
    #[prost(enumeration="extra_metadata::VideoMediaDataRotateAngle", tag="52")] pub media_data_rotate_angel: i32,

    #[prost(string, tag="53")] pub offset_v2: String,
    #[prost(string, tag="54")] pub offset_v3: String,
    #[prost(string, tag="55")] pub original_offset_v2: String,
    #[prost(string, tag="56")] pub original_offset_v3: String,

    #[serde(serialize_with="SensorDevice_serializer")]
    #[prost(enumeration="extra_metadata::SensorDevice", tag="57")] pub focus_sensor: i32,

    #[serde(serialize_with="ExpectOutputType_serializer")]
    #[prost(enumeration="extra_metadata::ExpectOutputType", tag="58")] pub expect_output_type: i32,

    #[prost(bool, tag="59")] pub timelapse_interval_in_millisecond: bool,
    #[prost(float, repeated, tag="60")] pub photo_rot: Vec<f32>,

    #[serde(serialize_with = "AudioModeType_serializer")]
    #[prost(enumeration="extra_metadata::AudioModeType", tag="61")] pub audio_mode: i32,

    #[prost(bool, tag="62")] pub is_raw_gyro: bool,

    #[prost(enumeration="extra_metadata::RawCaptureType", tag="63")] pub raw_capture_type: i32,
    #[prost(enumeration="extra_metadata::VideoPtsType", tag="64")] pub pts_type: i32,
    #[prost(message, optional, tag="65")] pub gyro_cfg_info: Option<extra_metadata::GyroConfigInfo>,

}

/// Nested message and enum types in `ExtraMetadata`.
pub mod extra_metadata {
    #[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
    pub struct ExtraUserOptions {
        #[prost(string, tag="1")] pub filter: String,
        #[prost(string, tag="2")] pub befilter: String,

        #[serde(serialize_with="super::bytes_serializer")]
        #[prost(bytes="vec", tag="3")] pub euler: Vec<u8>,

        #[prost(bool,   tag="4")] pub rm_purple: bool,
        #[prost(uint32, tag="5")] pub gyro_calibrate_mode: u32,
        #[prost(bool,   tag="6")] pub euler_enable: bool,

        #[serde(serialize_with="super::LogoType_serializer")]
        #[prost(enumeration="extra_user_options::LogoType", tag="7")] pub logo_type: i32,

        #[prost(string, tag="8")] pub adjust_filters: String,
        #[prost(string, tag="9")] pub lut_filter: String,

        #[serde(serialize_with="super::OffsetConvertState_serializer")]
        #[prost(enumeration="extra_user_options::OffsetConvertState", repeated, tag="10")] pub offset_convert_states: Vec<i32>,
    }
    /// Nested message and enum types in `ExtraUserOptions`.
    pub mod extra_user_options {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
        #[repr(i32)]
        pub enum LogoType {
            UnknownLogoType = 0,
            NoLogo          = 1,
            InstaLogo       = 2,
        }
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
        #[repr(i32)]
        pub enum OffsetConvertState {
            WaterProof         = 0,
            DivingWater        = 1,
            DivingAir          = 2,
            StitchOptimization = 3,
            Protect            = 4,
            SphereProtect      = 5,
            FpvProtect         = 6,
        }
    }
    #[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
    pub struct Vector2 {
        #[prost(int32, tag="1")] pub x: i32,
        #[prost(int32, tag="2")] pub y: i32,
    }
    #[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
    pub struct GyroIndex {
        #[prost(int32, tag="1")] pub index: i32,
        #[prost(int64, tag="2")] pub timestamp: i64,
    }
    #[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
    pub struct FileGroupInfo {
        #[serde(serialize_with="super::SubMediaType_serializer")]
        #[prost(enumeration="SubMediaType", tag="1")] pub r#type: i32,
        #[prost(uint32, tag="2")] pub index: u32,
        #[prost(string, tag="3")] pub identify: String,
        #[prost(uint32, tag="4")] pub total: u32,
    }
    #[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
    pub struct WindowCropInfo {
        #[prost(uint32, tag="1")] pub src_width: u32,
        #[prost(uint32, tag="2")] pub src_height: u32,
        #[prost(uint32, tag="3")] pub dst_width: u32,
        #[prost(uint32, tag="4")] pub dst_height: u32,
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum HdrState {
        NotHdr         = 0,
        WaitingProcess = 1,
        Processed      = 2,
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum TriggerSource {
        Unknown       = 0,
        CameraButton  = 1,
        RemoteControl = 2,
        Usb           = 3,
        BtRemote      = 4,
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum EvoStatusMode {
        UnknownStatusMode = 0,
        Degree180         = 1,
        Degree360         = 2,
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum GpsSource {
        Gsv       = 0,
        Dashboard = 1,
        Remote    = 2,
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum BatteryType {
        Thick    = 0,
        Thin     = 1,
        Vertical = 2,
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum CameraPosture {
        CameraRotate0   = 0,
        CameraRotate90  = 1,
        CameraRotate180 = 2,
        CameraRotate270 = 3,
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum ImageFovType {
        FovTypeUnknown          = 0,
        FovTypeWide             = 1,
        FovTypeLinear           = 2,
        FovTypeUltrawide        = 3,
        FovTypeNarrow           = 4,
        FovTypePov              = 5,
        FovTypeLinearPlus       = 6,
        FovTypeLinearHorizon    = 7,
        FovTypeFpv              = 8,
        FovTypeSuper            = 9,
        FovTypeTinyPlanet       = 10,
        FovTypeInfinityWide     = 11,
        FovType360LinearHorizon = 12,
        FovTypeMaxView          = 13,
        FovTypeDewarp           = 14,
        FovTypeMega             = 15,
        FovTypeBulletTime       = 16,
        FovTypeNum              = 17,
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum GyroFilterType {
        Unknown = 0,
        Brute   = 1,
        Akf     = 2,
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum GyroType {
        InsdevImuType20948 = 0,
        InsdevImuType40609 = 1,
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum VideoMediaDataRotateAngle {
        MediaDataRotateUnknown  = 0,
        MediaDataRotateAngle0   = 1,
        MediaDataRotateAngle180 = 3,
        MediaDataRotateAngle90  = 6,
        MediaDataRotateAngle270 = 8,
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum SensorDevice {
        Unknown = 0,
        Front   = 1,
        Rear    = 2,
        All     = 3,
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum ExpectOutputType {
        Default     = 0,
        InstaPano   = 1,
        MultiCamera = 2,
        OneTake     = 3,
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum AudioModeType {
        AudioModeUnknown = 0,
        AudioModeFocus   = 1,
        AudioModeStereo  = 2,
        AudioMode360     = 3,
        RsStereo         = 4,
        Reserve1         = 5,
        Reserve2         = 6,
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum SubMediaType {
        VideoNormal           = 0,
        VideoBullettime       = 1,
        VideoTimelapse        = 2,
        PhotoNormal           = 3,
        PhotoHdr              = 4,
        PhotoIntervalshooting = 5,
        VideoHdr              = 6,
        PhotoBurst            = 7,
        VideoStaticTimelapse  = 8,
        VideoTimeshift        = 9,
        PhotoAebNightMode     = 10,
        VideoSuperNormal      = 11,
        VideoLooprecording    = 12,
        PhotoStarlapse        = 13,
        PhotoPanoMode         = 14,
        VideoFpv              = 15,
        VideoMovie            = 16,
        VideoSlowmotion       = 17,
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum RawCaptureType {
        RawCaptureTypeOff      = 0,
        RawCaptureTypeDng      = 1,
        RawCaptureTypeRaw      = 2,
        RawCaptureTypePureshot = 3,
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum VideoPtsType {
        VideoPtsUnknown       = 0,
        VideoPtsMp4           = 1,
        VideoPtsEexposureFile = 2,
    }

    #[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
    pub struct GyroConfigInfo {
        #[prost(uint32, tag="1")] pub acc_range: u32,
        #[prost(uint32, tag="2")] pub gyro_range: u32,
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
#[repr(i32)]
pub enum ExtraType {
    All          = 0,
    Metadata     = 1,
    Thumbnail    = 2,
    Gyro         = 3,
    Exposure     = 4,
    ExtThumbnail = 5,
    FramePts     = 6,
    Gps          = 7,
    StarNum      = 8,
    AaaData      = 9,
    Highlight    = 10,
    AaaSim       = 11,
    ExposureSecondary = 12,
    Magnetic     = 13,
    Euler        = 14,
    SecGyro      = 15,
    Speed        = 16,
    TBox         = 17,
    Quaternions  = 18,
    TimeMap      = 128
}

// ----------------------------------------------------------------------------------------------------------------------

macro_rules! enum_serializer {
    ($name:ident, $type:ty) => {
        paste::paste! {
            #[allow(non_snake_case)]
            fn [<$name _serializer>]<S>(x: &i32, s: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
                match (*x).try_into().ok() as Option<$type> {
                    Some(v) => serde::ser::Serialize::serialize(&v, s),
                    None    => serde::ser::Serialize::serialize(x, s),
                }
            }
        }
    };
    (vec $name:ident, $type:ty) => {
        paste::paste! {
            #[allow(non_snake_case)]
            fn [<$name _serializer>]<S>(x: &[i32], s: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
                let mut copy = Vec::with_capacity(x.len());
                for v in x {
                    if let Some(v) = (*v).try_into().ok() as Option<$type> {
                        copy.push(v);
                    }
                }
                serde::ser::Serialize::serialize(&copy, s)
            }
        }
    };
}
enum_serializer!(HdrState,                  extra_metadata::HdrState);
enum_serializer!(TriggerSource,             extra_metadata::TriggerSource);
enum_serializer!(EvoStatusMode,             extra_metadata::EvoStatusMode);
enum_serializer!(vec GpsSource,             extra_metadata::GpsSource);
enum_serializer!(BatteryType,               extra_metadata::BatteryType);
enum_serializer!(CameraPosture,             extra_metadata::CameraPosture);
enum_serializer!(ImageFovType,              extra_metadata::ImageFovType);
enum_serializer!(GyroFilterType,            extra_metadata::GyroFilterType);
enum_serializer!(GyroType,                  extra_metadata::GyroType);
enum_serializer!(VideoMediaDataRotateAngle, extra_metadata::VideoMediaDataRotateAngle);
enum_serializer!(SensorDevice,              extra_metadata::SensorDevice);
enum_serializer!(ExpectOutputType,          extra_metadata::ExpectOutputType);
enum_serializer!(AudioModeType,             extra_metadata::AudioModeType);
enum_serializer!(SubMediaType,              extra_metadata::SubMediaType);
enum_serializer!(LogoType,                  extra_metadata::extra_user_options::LogoType);
enum_serializer!(vec OffsetConvertState,    extra_metadata::extra_user_options::OffsetConvertState);

fn bytes_serializer<S>(x: &[u8], s: S) -> Result<S::Ok, S::Error> where S: serde::Serializer {
    let mut ret = String::with_capacity(x.len() * 2);
    for b in x {
        ret.push_str(&format!("{:02x}", b));
    }
    s.serialize_str(&ret)
}

use byteorder::{ReadBytesExt, LittleEndian};
use std::io::{Cursor, ErrorKind};

pub fn parse_gyro_calib(data: &[u8]) -> std::io::Result<serde_json::Value> {
    let mut d = Cursor::new(data);
    let vec = (0..6).map(|_| d.read_f64::<LittleEndian>()).collect::<std::io::Result<Vec<f64>>>();
    let unix_timestamp = d.read_u64::<LittleEndian>()?;
    Ok(serde_json::json!({
        "numbers": vec?,
        "unix_timestamp": unix_timestamp
    }))
}

pub fn parse_gyro(data: &[u8]) -> std::io::Result<serde_json::Value> {
    let mut d = Cursor::new(data);
    let timestamp = d.read_u64::<LittleEndian>()?;
    let vec = (0..6).map(|_| d.read_f64::<LittleEndian>()).collect::<std::io::Result<Vec<f64>>>();
    Ok(serde_json::json!({
        "numbers": vec?,
        "timestamp": timestamp
    }))
}

pub fn parse_offset(data: &str) -> std::io::Result<serde_json::Value> {
    if data.is_empty() { return Err(ErrorKind::InvalidData.into()); }

    let vec: std::io::Result<Vec<f64>> = data.split('_')
                                             .map(|v| v.parse::<f64>().map_err(|_| ErrorKind::InvalidData.into()))
                                             .collect();
    Ok(vec?.into())
}