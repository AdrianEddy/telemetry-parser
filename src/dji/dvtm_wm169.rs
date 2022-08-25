//*
// About the detailed definition of some messages, please refer to the websites as follow:
// DNG Specification: <https://www.adobe.com/content/dam/acom/en/products/photoshop/pdfs/dng_spec_1.5.0.0.pdf>
// Standard Exif Specification: <https://www.cipa.jp/std/documents/e/DC-008-2012_E.pdf>
// TIFF Specification: <https://www.adobe.io/content/dam/udp/en/open/standards/tiff/TIFF6.pdf>
//
// About the length of the repeated message and string, please refer to the dvtm_library.options file.

///*
/// One clip can be part of the video file or part of the remote transfering content. It would include some essential
/// messages which are used to describe the basic information and to distinguish different clips. About the detailed
/// properties of the clip, we shall put them in a clip metadata message after ClipMetaHeader and shall not put them in
/// the header part. Note that this message shall be included in the ClipMeta.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct ClipMetaHeader {
    ///* The name of the product proto file. The max string length of
    ///it is 32 bytes (including ending symbol).
    #[prost(string, tag="1")]
    pub proto_file_name: ::prost::alloc::string::String,
    ///* The version of the library proto file. The max string length
    ///of it is 32 bytes (including ending symbol).
    #[prost(string, tag="2")]
    pub library_proto_version: ::prost::alloc::string::String,
    ///* The version of the product proto file. The max string length
    ///of it is 32 bytes (including ending symbol).
    #[prost(string, tag="3")]
    pub product_proto_version: ::prost::alloc::string::String,
    ///* The serial number of the product producing this clip. The
    ///max string length of it is 32 bytes (including ending
    ///symbol).
    #[prost(string, tag="5")]
    pub product_sn: ::prost::alloc::string::String,
    ///* The firmware version of the product producing this clip. The
    ///max string length of it is 32 bytes (including ending
    ///symbol).
    #[prost(string, tag="6")]
    pub product_firmware_version: ::prost::alloc::string::String,
    ///* The encryption type for encrypting metadata messages.
    #[prost(enumeration="clip_meta_header::MetaEncryptionType", tag="7")]
    pub meta_encryption_type: i32,
    ///* The compression type for compressing metadata messages.
    #[prost(enumeration="clip_meta_header::MetaCompressionType", tag="8")]
    pub meta_compression_type: i32,
    ///* The timestamp is the duration starting from the power-up
    ///time to the time that the first frame of this clip comes.
    ///Unit: micro-second.
    #[prost(uint64, tag="9")]
    pub clip_timestamp: u64,
    ///* The name of this product. The max string length of it is 64
    ///bytes (including ending symbol).
    #[prost(string, tag="10")]
    pub product_name: ::prost::alloc::string::String,
}
/// Nested message and enum types in `ClipMetaHeader`.
pub mod clip_meta_header {
    #[derive(::serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum MetaEncryptionType {
        ///* No encryption on metadata messages.
        None = 0,
    }
    #[derive(::serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum MetaCompressionType {
        ///* No compression on metadata messages.
        None = 0,
    }
}
///*
/// It would include some essential messages which are used to describe the basic information and to distinguish
/// different streams. About the detailed properties of the stream, we shall put them in a stream metadata message after
/// StreamMetaHeader and shall not put them in the header part. Note that this message shall be included in the
/// StreamMeta.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct StreamMetaHeader {
    ///* The unique ID which is used to distinguish different streams
    ///having the same stream type.
    #[prost(uint32, tag="1")]
    pub stream_id: u32,
    ///* The type of this stream.
    #[prost(enumeration="stream_meta_header::StreamType", tag="2")]
    pub stream_type: i32,
    ///* The stream alias name which is used to distinguish different
    ///streams intuitively for people. The max string length of it
    ///is 32 bytes (including ending symbol).
    #[prost(string, tag="3")]
    pub stream_name: ::prost::alloc::string::String,
}
/// Nested message and enum types in `StreamMetaHeader`.
pub mod stream_meta_header {
    #[derive(::serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum StreamType {
        ///* The stream type is video.
        Video = 0,
        ///* The stream type is audio.
        Audio = 1,
    }
}
///*
/// It would include some essential messages which are used to describe the basic information and to distinguish
/// different frames. About the detailed properties of the frame, we shall put them in a frame metadata message after
/// FrameMetaHeader and shall not put them in the header part. Note that this message shall be included in the FrameMeta.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct FrameMetaHeader {
    ///* The frame sequence number which is used to distinguish the
    ///different frame in a stream.
    #[prost(uint64, tag="1")]
    pub frame_seq_num: u64,
    ///* The timestamp is the duration starting from the power-up
    ///time to the time that the this frame comes.
    ///Unit: micro-second.
    #[prost(uint64, tag="2")]
    pub frame_timestamp: u64,
    ///* The stream id which this frame belongs to, to avoid the same
    ///frame sequence number in different stream.
    #[prost(uint32, tag="3")]
    pub stream_id: u32,
    ///* The check code for metadata messages is enable or not.
    #[prost(bool, tag="4")]
    pub check_code_enable: bool,
    ///* The check code type for checking metadata messages.
    #[prost(enumeration="frame_meta_header::CheckCodeType", tag="5")]
    pub check_code_type: i32,
    ///* The check code of this frame which does not include messages
    ///in FrameMetaHeader. When the type is CRC32, the CRC32 value
    ///will be filled in the low 32-bits and the high 32-bits is
    ///zero.
    #[prost(uint64, tag="6")]
    pub check_code: u64,
}
/// Nested message and enum types in `FrameMetaHeader`.
pub mod frame_meta_header {
    #[derive(::serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum CheckCodeType {
        ///* No check code is used.
        None = 0,
        ///* The check code type is CRC32 (32-bit Cyclic Redundancy
        ///Check).
        Crc32 = 1,
    }
}
///*
/// It would include some essential messages which are used to distinguish the different devices. About the device
/// properties, we shall put them in a message after the metadata header of device and shall not put them in the header
/// part.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct MetaHeaderOfDevice {
    ///* The unique ID for distinguishing the different device.
    #[prost(uint32, tag="1")]
    pub device_id: u32,
    ///* The type of this device is physical or virtual.
    #[prost(enumeration="meta_header_of_device::DeviceType", tag="2")]
    pub device_type: i32,
    ///* The specific type of this device such as camera body and
    ///drone.
    #[prost(enumeration="meta_header_of_device::DeviceSubType", tag="3")]
    pub device_sub_type: i32,
    ///* The device alias name which is used to distinguish different
    ///devices intuitively for people. The max string length of it
    ///is 32 bytes (including ending symbol).
    #[prost(string, tag="4")]
    pub device_name: ::prost::alloc::string::String,
    ///* The metadata generated frequency of this device.
    ///Unit: Hz.
    #[prost(float, tag="5")]
    pub device_frequency: f32,
    ///* The timestamp is the duration starting from the power-up
    ///time to the time that the current frame included in this
    ///device comes. Unit: micro-second.
    #[prost(uint64, tag="6")]
    pub device_timestamp: u64,
}
/// Nested message and enum types in `MetaHeaderOfDevice`.
pub mod meta_header_of_device {
    #[derive(::serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum DeviceType {
        ///* The device type is undefined.
        Undefined = 0,
        ///* The device type is physical.
        Physical = 1,
        ///* The device type is virtual.
        Virtual = 2,
    }
    #[derive(::serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum DeviceSubType {
        ///* The device sub-type is undefined.
        Undefined = 0,
        ///* The device sub-type is camera body.
        CameraBody = 1,
        ///* The device sub-type is drone.
        Drone = 2,
        ///* The device sub-type is gimbal.
        Gimbal = 3,
        ///* The device sub-type is gimbal z-axis.
        GimbalZaxis = 4,
        ///* The device sub-type is recorder.
        Recorder = 5,
        ///* The device sub-type is laser ranging.
        LaserRanging = 6,
    }
}
///*
/// It would include some essential messages which are used to distinguish the different sub-devices. About the
/// sub-device properties, we shall put them in a message after the metadata header of sub-device and shall not put them
/// in the header part.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct MetaHeaderOfSubDevice {
    ///* The unique ID for distinguishing the different
    ///sub-devices.
    #[prost(uint32, tag="1")]
    pub sub_dev_id: u32,
    ///* The type of this sub-device is physical or virtual.
    #[prost(enumeration="meta_header_of_sub_device::SubDeviceType", tag="2")]
    pub sub_device_type: i32,
    ///* The specific type of this sub-device such as lens.
    #[prost(enumeration="meta_header_of_sub_device::SubDeviceSubType", tag="3")]
    pub sub_device_sub_type: i32,
    ///* The sub-device alias name which is used to distinguish
    ///different sub-devices intuitively for people. The max string
    ///length of it is 32 bytes (including ending symbol).
    #[prost(string, tag="4")]
    pub sub_device_name: ::prost::alloc::string::String,
    ///* The metadata generated frequency of this sub-device.
    ///Unit: Hz.
    #[prost(float, tag="5")]
    pub sub_device_frequency: f32,
    ///* The timestamp is the duration starting from the power-up
    ///time to the time that the current frame included in this
    ///sub-device comes. Unit: micro-second.
    #[prost(uint64, tag="6")]
    pub sub_device_timestamp: u64,
}
/// Nested message and enum types in `MetaHeaderOfSubDevice`.
pub mod meta_header_of_sub_device {
    #[derive(::serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum SubDeviceType {
        ///* The sub-device type is undefined.
        Undefined = 0,
        ///* The sub-device type is physical.
        Physical = 1,
        ///* The sub-device type is virtual.
        Virtual = 2,
    }
    #[derive(::serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum SubDeviceSubType {
        ///* The sub-device sub-type is undefined.
        Undefined = 0,
        ///* The sub-device sub-type is lens.
        Lens = 1,
    }
}
///*
/// The fixed feature of the specified video stream.
/// If it has been filled, it always follows the message StreamMetaHeader.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct VideoStreamMeta {
    ///* The width of the resolution. Unit: pixel.
    #[prost(uint32, tag="1")]
    pub resolution_width: u32,
    ///* The height of the resolution. Unit: pixel.
    #[prost(uint32, tag="2")]
    pub resolution_height: u32,
    ///* The rate of video data sampling. Unit: frame/second.
    #[prost(float, tag="3")]
    pub framerate: f32,
    ///* The number of bits used for each color component.
    #[prost(uint32, tag="5")]
    pub bit_depth: u32,
    ///* The bit format used for each color component.
    #[prost(enumeration="video_stream_meta::BitFormatType", tag="6")]
    pub bit_format: i32,
    ///* The user-specified type of this video stream such as slow
    ///motion and quick movie.
    #[prost(enumeration="video_stream_meta::VideoStreamType", tag="7")]
    pub video_stream_type: i32,
    ///* The compression format used for this video stream.
    #[prost(enumeration="video_stream_meta::VideoCodecType", tag="8")]
    pub video_codec_type: i32,
}
/// Nested message and enum types in `VideoStreamMeta`.
pub mod video_stream_meta {
    #[derive(::serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum BitFormatType {
        ///* The bit format is unknown.
        Unknown = 0,
        ///* The bit format is raw.
        Raw = 1,
        ///* The bit format is RGB.
        Rgb = 2,
        ///* The bit format is RGBA.
        Rgba = 3,
        ///* The bit format is YUV420.
        Yuv420 = 4,
        ///* The bit format is YUV422.
        Yuv422 = 5,
        ///* The bit format is YUV444.
        Yuv444 = 6,
    }
    #[derive(::serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum VideoStreamType {
        ///* The video stream type is normal.
        Normal = 0,
        ///* The video stream type is delay.
        Delay = 1,
        ///* The video stream type is slow motion.
        SlowMotion = 2,
        ///* The video stream type is quick movie.
        QuickMovie = 3,
        ///* The video stream type is timeslapse.
        Timelapse = 4,
        ///* The video stream type is motionlapse.
        Motionlapse = 5,
        ///* The video stream type is hyperlapse.
        Hyperlapse = 6,
        ///* The video stream type is HDR.
        Hdr = 7,
        ///* The video stream type is loop record.
        LoopRecord = 8,
    }
    #[derive(::serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum VideoCodecType {
        ///* The video codec type is H264.
        H264 = 0,
        ///* The video codec type is H265.
        H265 = 1,
        ///* The video codec type is prores.
        Prores = 2,
        ///* The video codec type is prores raw.
        Proresraw = 3,
        ///* The video codec type is JPEG.
        Jpeg = 4,
        ///* The video codec type is JPEG 2000.
        Jpeg2000 = 5,
        ///* The video codec type is JPEG lossless.
        JpegLossless = 6,
    }
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct ImageProcessingQuality {
    ///* It is defined by DJI, which indicates different image
    ///processing in the ISP (Image Signal Processor)
    ///pipeline that represents different image quality.
    #[prost(enumeration="image_processing_quality::ImageProcessingQualityType", tag="1")]
    pub image_processing_quality: i32,
}
/// Nested message and enum types in `ImageProcessingQuality`.
pub mod image_processing_quality {
    #[derive(::serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum ImageProcessingQualityType {
        ///* The image processing quality type is normal.
        Normal = 0,
        ///* The image processing quality type is high.
        High = 1,
    }
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct ProresCodecQuality {
    ///* The value will be filled only when the video codec type is
    ///prores or prores raw.
    #[prost(enumeration="prores_codec_quality::ProresCodecQualityType", tag="1")]
    pub prores_codec_quality: i32,
}
/// Nested message and enum types in `ProresCodecQuality`.
pub mod prores_codec_quality {
    #[derive(::serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum ProresCodecQualityType {
        ///* The prores codec quality is undefined.
        Undefined = 0,
        ///* The prores codec quality is proxy.
        Proxy = 1,
        ///* The prores codec quality is LT.
        Lt = 2,
        ///* The prores codec quality is standard.
        Sd = 3,
        ///* The prores codec quality is HQ.
        Hq = 4,
        ///* The prores codec quality is XQ.
        Xq = 5,
    }
}
///*
/// The multiple streams description in one clip.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct ClipStreamsMeta {
    ///* The number of video stream track in this clip.
    #[prost(uint32, tag="1")]
    pub video_stream_num: u32,
    ///* The number of audio stream track in this clip.
    #[prost(uint32, tag="2")]
    pub audio_stream_num: u32,
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct DeviceSn {
    ///* The serial number of this device. The max string length of
    ///it is 32 bytes (including ending symbol).
    #[prost(string, tag="1")]
    pub device_sn: ::prost::alloc::string::String,
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct DeviceVersion {
    ///* The hardware version of this device. The max string length
    ///of it is 32 bytes (including ending symbol).
    #[prost(string, tag="1")]
    pub device_hw_version: ::prost::alloc::string::String,
    ///* The software version of this device. The max string length
    ///of it is 32 bytes (including ending symbol).
    #[prost(string, tag="2")]
    pub device_sw_version: ::prost::alloc::string::String,
}
///*
/// The user-specified cinema production information.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct CinemaProductionInfo {
    ///* The user-specified name of the production. The max string
    ///length of it is 64 bytes (including ending symbol).
    #[prost(string, tag="1")]
    pub production: ::prost::alloc::string::String,
    ///* The user-specified name of the company producing the
    ///content. The max string length of it is 64 bytes (including
    ///ending symbol).
    #[prost(string, tag="2")]
    pub production_company: ::prost::alloc::string::String,
    ///* The user-specified name of the director of the
    ///production. The max string length of it is 64 bytes
    ///(including ending symbol).
    #[prost(string, tag="3")]
    pub director: ::prost::alloc::string::String,
    ///* The user-specified name of the cinematographer directing the
    ///recording. The max string length of it is 64 bytes
    ///(including ending symbol).
    #[prost(string, tag="4")]
    pub cinematographer: ::prost::alloc::string::String,
    ///* The user-specified name of the camera operator. The max
    ///string length of it is 64 bytes (including ending
    ///symbol).
    #[prost(string, tag="5")]
    pub cinema_operator: ::prost::alloc::string::String,
    ///* The user-specified name of the capturing location. The max
    ///string length of it is 64 bytes (including ending
    ///symbol).
    #[prost(string, tag="6")]
    pub location: ::prost::alloc::string::String,
    ///* The user-specified number of the scene being captured.
    #[prost(uint32, tag="7")]
    pub scene: u32,
    ///* The user-specified number of the take being captured.
    #[prost(uint32, tag="8")]
    pub take: u32,
}
///*
/// The cinema naming information of this clip.
/// For example, there is a video file A001C0001_190101_ABCD_001.MOV included in the directory A001_ABCD:
///     Camera index is A
///     Reel name is A001_ABCD
///     Camera ID is ABCD
///     Clip name is A001C0001_190101_ABCD.MOV
///     Spin name is A001C0001_190101_ABCD_001.MOV
/// Refer to each field of this message for more details.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct CinemaClipNaming {
    ///* The user-specified camera index for identifying the
    ///individual camera unit (A - Z). The max string length of it
    ///is 8 bytes (including ending symbol).
    #[prost(string, tag="1")]
    pub camera_index: ::prost::alloc::string::String,
    ///* The name of the virtual reel which this clip belongs to. The
    ///max string length of it is 16 bytes (including ending
    ///symbol).
    #[prost(string, tag="2")]
    pub reel_name: ::prost::alloc::string::String,
    ///* The name of the camera ID (or unique code) indicates that
    ///this clip is recoded by which camera. The max string length
    ///of it is 8 bytes (including ending symbol).
    #[prost(string, tag="3")]
    pub camera_id: ::prost::alloc::string::String,
    ///* The name of this clip. The max string length of it is 32
    ///bytes (including ending symbol).
    #[prost(string, tag="4")]
    pub clip_name: ::prost::alloc::string::String,
    ///* If a clip file is divided into multiple files, the spin name
    ///can indicate the different divided files. It will be the
    ///same as the clip name when there is no division. The max
    ///string length of it is 32 bytes (including ending
    ///symbol).
    #[prost(string, tag="5")]
    pub spin_name: ::prost::alloc::string::String,
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct ProjectFps {
    ///* The user-specified framerate which the recoding project
    ///used. Unit: frame/second.
    #[prost(float, tag="1")]
    pub project_fps: f32,
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct ImageSizeType {
    ///* The user-specified image size which is the maximum area of a
    ///sample that the camera can image.
    #[prost(enumeration="image_size_type::ImageSizeType", tag="1")]
    pub image_size_type: i32,
}
/// Nested message and enum types in `ImageSizeType`.
pub mod image_size_type {
    #[derive(::serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum ImageSizeType {
        ///* The user-specified image size is default.
        Default = 0,
        ///* The user-specified image size is open gate.
        OpenGate = 1,
        ///* The user-specified image size is full frame.
        FullFrame = 2,
        ///* The user-specified image size is super 35.
        S35 = 3,
        ///* The user-specified image size is 4/3 inches.
        ImageSizeType43 = 4,
    }
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct FovType {
    ///* The user-specified FOV (Field of View), which is the maximum
    ///area of a sample that the camera can image.
    #[prost(enumeration="fov_type::FovType", tag="1")]
    pub fov_type: i32,
}
/// Nested message and enum types in `FOVType`.
pub mod fov_type {
    #[derive(::serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum FovType {
        ///* The user-specified FOV is default.
        Default = 0,
        ///* The user-specified FOV is normal.
        Normal = 1,
        ///* The user-specified FOV is narrow.
        Narrow = 2,
        ///* The user-specified FOV is wide.
        Wide = 3,
        ///* The user-specified FOV is snarrow.
        Snarrow = 4,
    }
}
///*
/// It will be filled only when the video format is raw.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct ImageArea {
    ///* A rectangle zone of valid pixel in photosite data array used
    ///in recording, composed of the left-top location (horizonal
    ///first), width and height of the area in order.
    #[prost(uint32, repeated, tag="1")]
    pub active_image_area: ::prost::alloc::vec::Vec<u32>,
    ///* A rectangle zone of whole photosite data array used in
    ///recording, including active image area and any extra
    ///photosite data, composed of the left-top location (horizonal
    ///first), width and height of the area in order.
    #[prost(uint32, repeated, tag="2")]
    pub full_image_area: ::prost::alloc::vec::Vec<u32>,
}
///*
/// It will be filled only when the video format is raw.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct CfaPattern {
    ///* The Bayer arrangement of color filters on a square grid of
    ///photosensors.
    #[prost(enumeration="cfa_pattern::CfaPatternType", tag="1")]
    pub cfa_pattern: i32,
}
/// Nested message and enum types in `CFAPattern`.
pub mod cfa_pattern {
    #[derive(::serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum CfaPatternType {
        ///* The CFA pattern is RGGB.
        Rggb = 0,
        ///* The CFA pattern is GRBG.
        Grbg = 1,
        ///* The CFA pattern is BGGR.
        Bggr = 2,
        ///* The CFA pattern is GBRG.
        Gbrg = 3,
    }
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct BayerGreenSplit {
    ///* The difference between the values of the green pixels in the
    ///blue/green rows and the values of the green pixels in the
    ///red/green rows. Usually it only be available for CFA images
    ///from the Bayer pattern filter array. Refer to the DNG
    ///specification for more details, and only the type is
    ///different (type is long in the DNG specification) for higher
    ///precision.
    #[prost(float, tag="1")]
    pub bayer_green_split: f32,
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct ColorSpace {
    ///* The specific transformation of image data will be used in
    ///the nominal processing algorithm.
    #[prost(enumeration="color_space::ColorSpaceType", tag="1")]
    pub color_space: i32,
}
/// Nested message and enum types in `ColorSpace`.
pub mod color_space {
    #[derive(::serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum ColorSpaceType {
        ///* The color space is default.
        Default = 0,
        ///* The color space is D-Gamut.
        Dgamut = 1,
        ///* The color space is REC709.
        Rec709 = 2,
        ///* The color space is BT2020.
        Bt2020 = 3,
        ///* The color space is BT2100.
        Bt2100 = 4,
    }
}
///*
/// This is a sub-message only quoted by the message ColorMatrix.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct ColorMatrixBox {
    ///* It stores 3x3 matrix in row-major order. Refer to the
    ///message ColorMatrix for more detail about the definition of
    ///the color matrix.
    #[prost(float, repeated, tag="1")]
    pub color_matrix: ::prost::alloc::vec::Vec<f32>,
}
///*
/// It will contains the message ColorMatrixBox.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct ColorMatrix {
    ///* Each box will contain a color matrix, which transforms
    ///linear RGB pixel values in the camera native color space to
    ///CIE 1931 XYZ values relative to the D65 illuminant under the
    ///specified illuminant described in the message
    ///CalibrationIlluminant.
    #[prost(message, repeated, tag="1")]
    pub color_matrix_box: ::prost::alloc::vec::Vec<ColorMatrixBox>,
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct CalibrationIlluminant {
    ///* The illuminants used in color calibration with the message
    ///ColorMatrix in order. Unit: Kelvin.
    #[prost(int32, repeated, tag="1")]
    pub calibration_illuminant: ::prost::alloc::vec::Vec<i32>,
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct NoiseReductionApplied {
    ///* The denoise strength applied on the image with range from
    ///-10.0 to +10.0. 0 is the default strength.
    #[prost(float, tag="1")]
    pub noise_reduction_applied: f32,
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct Saturation {
    ///* The factor by which the saturation of the image is altered
    ///in the conversion to the target color space with range from
    ///-10.0 to +10.0. 0 is the default strength. For raw image, it
    ///will be only applied on liveview or while for yuv image, it
    ///will be applied on image data.
    #[prost(float, tag="1")]
    pub saturation: f32,
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct Sharpness {
    ///* The factor by which the sharpness of the image is altered in
    ///the conversion to the target color space with range from
    ///-10.0 to +10.0. 0 is the default strength. For raw image, it
    ///will be only applied on liveview or while for yuv image, it
    ///will be applied on image data.
    #[prost(float, tag="1")]
    pub sharpness: f32,
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct PixelAspectRatio {
    ///* The factor be used to stretch reconstructed pixel data
    ///horizontally to compensate for anamorphic distortion.
    #[prost(float, tag="1")]
    pub pixel_aspect_ratio: f32,
}
///*
/// It stores the specific or custom three-dimensional look up table (3D LUT) file.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct LookUpTable3DFile {
    ///* The name of the 3D-LUT file. The max string length of it is
    ///256 bytes (including ending symbol).
    #[prost(string, tag="1")]
    pub lut3d_file_name: ::prost::alloc::string::String,
    ///* The data of the 3D-LUT file. The actual value type of it is
    ///32-bits float, so you should convert from byte array to
    ///float array. For example, if you use the functions
    ///ParseFromString() and MessageToDict() in python for decoding
    ///protobuf, you should do b64decode() first and then convert
    ///four bytes to one float by the little-endian mode.
    #[prost(bytes="vec", tag="2")]
    pub lut3d_file_data: ::prost::alloc::vec::Vec<u8>,
}
///*
/// It will be filled only when the video format is raw.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct ColorProcessingVersion {
    ///* The color processing version of reference image processing
    ///flows, which describes specific op groups shoule be applied
    ///at the appropriate stages in the image processing pipeline.
    ///Each op group can include several ops which should be
    ///processed sequentially. For version 1.0.0.0, there are 4 op
    ///groups defined.
    #[prost(string, tag="1")]
    pub color_processing_version: ::prost::alloc::string::String,
}
///*
/// Once op(operation) which contains type and data in the op group.
/// This is a sub-message only quoted by the message OpGroup.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct OpBox {
    ///* The type of this operation which is used.
    #[prost(enumeration="op_box::OpType", tag="1")]
    pub r#type: i32,
    ///* The data of this operation which is used. The actual value
    ///type of it is 32-bits float, so you should convert from byte
    ///array to float array. For example, if you use the functions
    ///ParseFromString() and MessageToDict() in python for decoding
    ///protobuf, you should do b64decode() first and then convert
    ///four bytes to one float by the little-endian mode.
    #[prost(bytes="vec", tag="2")]
    pub data: ::prost::alloc::vec::Vec<u8>,
}
/// Nested message and enum types in `OpBox`.
pub mod op_box {
    #[derive(::serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum OpType {
        ///* The operation type is the warp rectilinear.
        WarpRectilinear = 0,
        ///* The operation type is the warp fisheye.
        WarpFisheye = 1,
        ///* The operation type is the fix vignette radial.
        FixVignetteRadial = 2,
        ///* The operation type is the trim bounds.
        TrimBounds = 3,
        ///* The operation type is the map table.
        MapTable = 4,
        ///* The operation type is the map polynomial.
        MapPolynomial = 5,
        ///* The operation type is the gain map.
        GainMap = 6,
        ///* The operation type is the delta per row.
        DeltaPerRow = 7,
        ///* The operation type is the delta per column.
        DeltaPerColumn = 8,
        ///* The operation type is the scale per row.
        ScalePerRow = 9,
        ///* The operation type is the scale per column.
        ScalePerColumn = 10,
    }
}
///*
/// Every op(operation) group defines several ops should be applied in specific locations of normal image processing
/// pipeline sequentially.
/// It will contains the message OpBox.
/// It will be filled only when the video format is raw.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct OpGroup {
    ///* The op group1 includes operation(op)s that should applied
    ///sequentially to the raw image as read directly from the
    ///file.
    #[prost(message, repeated, tag="1")]
    pub op_group1: ::prost::alloc::vec::Vec<OpBox>,
    ///* The op group2 includes operation(op)s that should applied
    ///sequentially to the raw image after linear mapping (always
    ///before the demosaicing).
    #[prost(message, repeated, tag="2")]
    pub op_group2: ::prost::alloc::vec::Vec<OpBox>,
    ///* The op group3 includes operation(op)s that should applied
    ///sequentially to image just after demosaicing process.
    #[prost(message, repeated, tag="3")]
    pub op_group3: ::prost::alloc::vec::Vec<OpBox>,
    ///* The op group4 includes operation(op)s that should applied
    ///sequentially to image after normal image processing (always
    ///after tone mapping and in yuv domain).
    #[prost(message, repeated, tag="4")]
    pub op_group4: ::prost::alloc::vec::Vec<OpBox>,
}
///*
/// It will be filled only when the video format is raw.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct NoiseProfile {
    ///* The amount of noise in a raw image at time of capture.
    #[prost(double, repeated, tag="1")]
    pub noise_profile: ::prost::alloc::vec::Vec<f64>,
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct ImageDataSize {
    ///* The size in bytes of stored frame data.
    #[prost(uint32, tag="1")]
    pub image_data_size: u32,
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct ExposureIndex {
    ///* The index of effective exposure selected on camera at time
    ///of image data capture.
    #[prost(float, tag="1")]
    pub exposure_index: f32,
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct Iso {
    ///* The sensitivity (the signal gain) of the camera system.
    #[prost(float, tag="1")]
    pub iso: f32,
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct ExposureTime {
    ///* The exposure time (or the shutter speed) is the length of
    ///time that the camera sensor is exposed to light. Its value
    ///will be described as rational type, so for example, when
    ///exposure time is 1/50, its value will be [1, 50]. It is the
    ///real value and its denominator is rounded to the integer, so
    ///it has slightly different from the user-specified value.
    ///When the message ExposureTime and ShutterAngle are both
    ///filled and user-specified shutter unit is shutter angle, its
    ///value can be calculated by the formula:
    ///ExposureTime = ShutterAngle / (framerate * 360).
    ///Unit: second.
    #[prost(int32, repeated, tag="1")]
    pub exposure_time: ::prost::alloc::vec::Vec<i32>,
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct FNumber {
    ///* The ratio of the focal length to the aperture in an optical
    ///system. Its value will be described as rational type, so for
    ///example, when F-number is F2.8, its value will be [28, 10].
    ///[0, 0] means the F-number is invalid or unknown. It is the
    ///real value so it has slightly different from the
    ///user-specified value.
    #[prost(uint32, repeated, tag="1")]
    pub f_number: ::prost::alloc::vec::Vec<u32>,
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct ApertureValue {
    ///* The lens aperture in the APEX (Additive System of
    ///Photographic Exposure) value unit. The relation of the
    ///aperture value to F-number follows the formula:
    ///ApertureValue = 2 * log2(FNumber).
    ///Its value will be described as rational type, so for
    ///example, when aperture value is 3.61, its value will be
    ///[361, 100].
    #[prost(uint32, repeated, tag="1")]
    pub aperture_value: ::prost::alloc::vec::Vec<u32>,
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct ShutterAngle {
    ///* The shutter angle is exposure period expressed as an angle
    ///in seconds. When the message ExposureTime and ShutterAngle
    ///are both filled and user-specified shutter unit is exposure
    ///time, its value can be calculated by the formula:
    ///ShutterAngle = (framerate * 360) * ExposureTime.
    ///Unit: degree.
    #[prost(float, tag="1")]
    pub shutter_angle: f32,
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct FocusDistance {
    ///* The specified unit for the focus distance.
    #[prost(enumeration="focus_distance::FocusUnit", tag="1")]
    pub focus_unit: i32,
    ///* The focus distance in the specified focus unit, which
    ///indicates current distance at that the lens focuses. 0 means
    ///the focus distance is invalid or unknown.
    #[prost(int32, tag="2")]
    pub focus_distance: i32,
}
/// Nested message and enum types in `FocusDistance`.
pub mod focus_distance {
    #[derive(::serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum FocusUnit {
        ///* The focus distance unit is in thousandths of an inch.
        FocusUnit1000Inch = 0,
        ///* The focus distance unit is millimeter.
        Millimetre = 1,
    }
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct FocalLength {
    ///* The distance from the center of the lens to the focal points
    ///of the lens. 0 means the focal length is invalid or unknown.
    ///Its value will be described as rational type, so for
    ///example, when focal length is 35, its value will be
    ///[35000, 1000]. Unit: milli-meter.
    #[prost(int32, repeated, tag="1")]
    pub focal_length: ::prost::alloc::vec::Vec<i32>,
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct DigitalZoomRatio {
    ///* The digital zoom ratio of current frame.
    #[prost(float, tag="1")]
    pub digital_zoom_ratio: f32,
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct WhiteBalanceCct {
    ///* The white balance color temperature (CCT), selected at the
    ///time of capture. Unit: Kelvin.
    #[prost(uint32, tag="1")]
    pub white_balance_cct: u32,
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct WhiteBalanceTint {
    ///* The deviation from blackbody radiator, with range from -99.0
    ///to +99.0.
    #[prost(float, tag="1")]
    pub white_balance_tint: f32,
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct AsShotNeutral {
    ///* The white balance in the normalized coordinates of a
    ///perfectly neutral color in linear reference space values.
    #[prost(float, repeated, tag="1")]
    pub as_shot_neutral: ::prost::alloc::vec::Vec<f32>,
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct NdFilter {
    ///* The flag indicates whether the ND filter is enable.
    #[prost(bool, tag="1")]
    pub nd_filter_enable: bool,
    ///* The optical density of ND filter. Unit: reciprocal of the
    ///attenuation ratio. 1.0 means a clear filter.
    #[prost(float, tag="2")]
    pub nd_density: f32,
}
///*
/// It will be filled only when the video format is raw.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct BlackLevel {
    ///* The offset of the raw sample values.
    #[prost(float, repeated, tag="1")]
    pub black_level: ::prost::alloc::vec::Vec<f32>,
}
///*
/// It will be filled only when the video format is raw.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct WhiteLevel {
    ///* The fully saturated encoding level for the raw sample
    ///value.
    #[prost(float, tag="1")]
    pub white_level: f32,
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct TimeCode {
    ///* The user-specified run mode of the timecode which includes
    ///free run mode and record run mode.
    #[prost(enumeration="time_code::TimecodeRunMode", tag="1")]
    pub timecode_run_mode: i32,
    ///* A sequence of numeric codes generated at regular intervals
    ///by a timing synchronization system. The max string length of
    ///it is 12 bytes (including ending symbol).
    #[prost(string, tag="2")]
    pub timecode: ::prost::alloc::string::String,
    ///* The zero-based count of frame within the current second’s
    ///worth of timecode, in support of frame rates higher than
    ///those which can be encoded in SMPTE timecode.
    #[prost(uint32, tag="3")]
    pub sub_second_frame_count: u32,
}
/// Nested message and enum types in `TimeCode`.
pub mod time_code {
    #[derive(::serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum TimecodeRunMode {
        ///* The timecode run mode is free run.
        Free = 0,
        ///* The timecode run mode is record run.
        Record = 1,
    }
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct Orientation {
    ///* Any top-to-bottom reversals (flips) or left-to-right
    ///reversals (flops) that are performed on image data for
    ///evaluative viewing. Note that it just effects reference
    ///display method.
    #[prost(enumeration="orientation::OrientationType", tag="1")]
    pub orientation: i32,
}
/// Nested message and enum types in `Orientation`.
pub mod orientation {
    #[derive(::serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum OrientationType {
        ///* No flipping or flopping is desired.
        OrientationNoReverse = 0,
        ///* Any displayed image are flopped (horizontally reversed
        ///relative to original scene).
        OrientationHReverse = 1,
        ///* Any displayed image are flipped (vertically reversed
        ///relative to original scene).
        OrientationVReverse = 2,
        ///* Any displayed image are rotated 180° (that is, vertically
        ///and horizontally reversed relative to original scene).
        OrientationHvReverse = 3,
    }
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct ColorMode {
    ///* The user-specified color profile for video recording.
    #[prost(enumeration="color_mode::ColorModeType", tag="1")]
    pub color_mode: i32,
}
/// Nested message and enum types in `ColorMode`.
pub mod color_mode {
    #[derive(::serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum ColorModeType {
        ///* The color mode is default.
        ColorModeDefault = 0,
        ///* The color mode is D-Cinelike.
        ColorModeDCinelike = 1,
        ///* The color mode is D-Log.
        ColorModeDLog = 2,
        ///* The color mode is film A.
        ColorModeFilmA = 3,
        ///* The color mode is film B.
        ColorModeFilmB = 4,
        ///* The color mode is film C.
        ColorModeFilmC = 5,
        ///* The color mode is film D.
        ColorModeFilmD = 6,
        ///* The color mode is film E.
        ColorModeFilmE = 7,
        ///* The color mode is film F.
        ColorModeFilmF = 8,
        ///* The color mode is HLG.
        ColorModeHlg = 9,
        ///* The color mode is art.
        ColorModeArt = 10,
        ///* The color mode is black & white.
        ColorModeBw = 11,
        ///* The color mode is vivid.
        ColorModeVivid = 12,
        ///* The color mode is beach.
        ColorModeBeach = 13,
        ///* The color mode is dream.
        ColorModeDream = 14,
        ///* The color mode is sRGB.
        ColorModeSrgb = 15,
        ///* The color mode is adobe RGB.
        ColorModeAdobergb = 16,
        ///* The color mode is IR cut.
        ColorModeIrCut = 17,
        ///* The color mode is racing.
        ColorModeRacing = 18,
    }
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct ExposureIndexMode {
    ///* If the exposure index mode is on, it means that the exposure
    ///index value is valid.
    #[prost(enumeration="exposure_index_mode::ExposureIndexModeType", tag="1")]
    pub exposure_index_mode: i32,
}
/// Nested message and enum types in `ExposureIndexMode`.
pub mod exposure_index_mode {
    #[derive(::serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum ExposureIndexModeType {
        ///* The exposure index mode is off.
        EiModeOff = 0,
        ///* The exposure index mode is on.
        EiModeOn = 1,
    }
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct GainMode {
    ///* The user-specified gain mode which will determining the
    ///sensor conversion gain mode.
    #[prost(enumeration="gain_mode::GainModeType", tag="1")]
    pub gain_mode: i32,
}
/// Nested message and enum types in `GainMode`.
pub mod gain_mode {
    #[derive(::serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum GainModeType {
        ///* The gain mode is auto.
        Auto = 0,
        ///* The gain mode is low gain.
        LowGain = 1,
        ///* The gain mode is high gain.
        HighGain = 2,
    }
}
///*
/// It will be filled only when the video format is raw.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct BaselineExposure {
    ///* The amount in EV units to move the zero point for exposure
    ///compensation
    #[prost(float, tag="1")]
    pub baseline_exposure: f32,
}
///*
/// The quaternion provides a convenient mathematical notation for representing spatial orientations and rotations of
/// elements in three dimensional space. It can be converted to the Euler angle.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct Quaternion {
    ///* The w value of quaternion.
    #[prost(float, tag="1")]
    pub quaternion_w: f32,
    ///* The x value of quaternion.
    #[prost(float, tag="2")]
    pub quaternion_x: f32,
    ///* The y value of quaternion.
    #[prost(float, tag="3")]
    pub quaternion_y: f32,
    ///* The z value of quaternion.
    #[prost(float, tag="4")]
    pub quaternion_z: f32,
}
///*
/// The velocity of the device on the XYZ.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct Velocity {
    ///* The velocity value on the X-axis. Unit: meter/second.
    #[prost(float, tag="1")]
    pub velocity_x: f32,
    ///* The velocity value on the Y-axis. Unit: meter/second.
    #[prost(float, tag="2")]
    pub velocity_y: f32,
    ///* The velocity value on the Z-axis. Unit: meter/second.
    #[prost(float, tag="3")]
    pub velocity_z: f32,
}
///*
/// The position of the device on the XYZ. The definition of origins may be different when this message is included in
/// different device, so refer to the specific product proto file for more details.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct Position {
    ///* The position value on the X-axis. Unit: meter.
    #[prost(float, tag="1")]
    pub position_x: f32,
    ///* The position value on the Y-axis. Unit: meter.
    #[prost(float, tag="2")]
    pub position_y: f32,
    ///* The position value on the Z-axis. Unit: meter.
    #[prost(float, tag="3")]
    pub position_z: f32,
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct AbsoluteAltitude {
    ///* The absolute altitude of this device. It may come from the
    ///visual odometer. Unit: meter.
    #[prost(float, tag="1")]
    pub absolute_altitude: f32,
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct RelativeAltitude {
    ///* The relative altitude of this device. It may come from the
    ///visual odometer. Unit: millimeter.
    #[prost(float, tag="1")]
    pub relative_altitude: f32,
    ///* The flag to indicate whether the relative altitude value is
    ///valid.
    #[prost(bool, tag="2")]
    pub is_relative_altitude_valid: bool,
}
///*
/// Represents the relative distance.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct RelativeDistance {
    ///* The relative distance between two points. Unit: millimeter
    #[prost(int32, tag="1")]
    pub relative_distance: i32,
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct GimbalInstallPosition {
    ///* The install position of the gimbal which means the gimbal
    ///can be installed reversed or normally.
    #[prost(enumeration="gimbal_install_position::GimbalInstallPositionType", tag="1")]
    pub gimbal_install_position: i32,
}
/// Nested message and enum types in `GimbalInstallPosition`.
pub mod gimbal_install_position {
    #[derive(::serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum GimbalInstallPositionType {
        ///* The install position of gimbal is normal.
        Normal = 0,
        ///* The install position of gimbal is reversed.
        Reverse = 1,
    }
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct GimbalMode {
    ///* The user-specified gimbal mode.
    #[prost(enumeration="gimbal_mode::GimbalModeType", tag="1")]
    pub gimbal_mode: i32,
}
/// Nested message and enum types in `GimbalMode`.
pub mod gimbal_mode {
    #[derive(::serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum GimbalModeType {
        ///* The gimbal mode is off.
        GimbalModeOff = 0,
        ///* The gimbal mode is lock (or called free).
        GimbalModeLock = 1,
        ///* The gimbal mode is follow. If the product supports to set
        ///pan/tilt/roll follow mode separately, refer to the message
        ///GimbalModeFollowSubStatus for more details.
        GimbalModeFollow = 2,
    }
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct GimbalModeFollowSubStatus {
    ///* The flag to indicate whether the pan follow is on.
    #[prost(bool, tag="1")]
    pub pan_follow: bool,
    ///* The flag to indicate whether the tilt follow is on.
    #[prost(bool, tag="2")]
    pub tilt_follow: bool,
    ///* The flag to indicate whether the roll follow is on.
    #[prost(bool, tag="3")]
    pub roll_follow: bool,
}
///*
/// The Euler angles of gimbal relative to the NED (North, East, Down) coordinate system. Rotation sequence of the Euler
/// angle is ZXY (yaw, roll, pitch), intrinsic. For upward gimbal, the Euler angles translate from the real quaternion
/// of gimbal after rotate 180 degree around the X axis of moving body.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct EulerAngle {
    ///* The Euler angles of Pitch. Unit: deci-degree.
    #[prost(int32, tag="1")]
    pub pitch_decidegree: i32,
    ///* The Euler angles of Roll. Unit: deci-degree.
    #[prost(int32, tag="2")]
    pub roll_decidegree: i32,
    ///* The Euler angles of Yaw. Unit: deci-degree.
    #[prost(int32, tag="3")]
    pub yaw_decidegree: i32,
}
///*
/// Represents the positioning coordinates expressed in latitude and longitude.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct PositionCoord {
    ///* The unit of positioning coordinates.
    #[prost(enumeration="position_coord::PositionCoordUnit", tag="1")]
    pub position_coord_unit: i32,
    ///* The latitude, WGS-84 coordinate system.
    #[prost(double, tag="2")]
    pub latitude: f64,
    ///* The longitude, WGS-84 coordinate system.
    #[prost(double, tag="3")]
    pub longitude: f64,
}
/// Nested message and enum types in `PositionCoord`.
pub mod position_coord {
    #[derive(::serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum PositionCoordUnit {
        ///* The unit of positioning coordinates is radian.
        UnitRad = 0,
        ///* The unit of positioning coordinates is degree.
        UnitDeg = 1,
    }
}
///*
/// Represents the basic information for GPS.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct GpsBasic {
    ///* The GPS latitude and longitude coordinates.
    #[prost(message, optional, tag="1")]
    pub gps_coordinates: ::core::option::Option<PositionCoord>,
    ///* The GPS altitude, unit: mm, refer to gps_altitude_type for
    ///details.
    #[prost(int32, tag="2")]
    pub gps_altitude_mm: i32,
    ///* The GPS status.
    #[prost(enumeration="gps_basic::GpsStatus", tag="3")]
    pub gps_status: i32,
    ///* The GPS altitude type.
    #[prost(enumeration="gps_basic::GpsAltType", tag="4")]
    pub gps_altitude_type: i32,
}
/// Nested message and enum types in `GpsBasic`.
pub mod gps_basic {
    #[derive(::serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum GpsStatus {
        ///* The GPS status is normal.
        GpsNormal = 0,
        ///* The GPS status is invalid.
        GpsInvalid = 1,
        ///* The GPS status is RTK.
        GpsRtk = 2,
    }
    #[derive(::serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum GpsAltType {
        ///* The altitude is provided by barometer which is not
        ///ellipsoidal height.
        PressureAltitude = 0,
        ///* Fuse GPS and barometer height, which based on ellipsoidal
        ///coordinate.
        GpsFusionAltitude = 1,
        ///* The altitude is ellipsoidal height (WGS-84) provided by
        ///RTK.
        RtkAltitude = 2,
    }
}
///*
/// Represents the accelerometer of IMU.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct Accelerometer {
    ///* The timestamp of each frame, unit: nano-seconds.
    #[prost(uint64, tag="1")]
    pub msg_timestamp: u64,
    ///* The accelerometer in X direction, unit: 0.1 degree.
    #[prost(float, tag="2")]
    pub accelerometer_x: f32,
    ///* The accelerometer in Y direction, unit: 0.1 degree.
    #[prost(float, tag="3")]
    pub accelerometer_y: f32,
    ///* The accelerometer in Z direction, unit: 0.1 degree.
    #[prost(float, tag="4")]
    pub accelerometer_z: f32,
}
///*
/// Represents the gyroscope of IMU.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct Gyroscope {
    ///* The timestamp of each frame, unit: nano-seconds.
    #[prost(uint64, tag="1")]
    pub msg_timestamp: u64,
    ///* The gyroscope in X direction, unit: 0.1 degree.
    #[prost(float, tag="2")]
    pub gyroscope_x: f32,
    ///* The gyroscope in Y direction, unit: 0.1 degree.
    #[prost(float, tag="3")]
    pub gyroscope_y: f32,
    ///* The gyroscope in Z direction, unit: 0.1 degree.
    #[prost(float, tag="4")]
    pub gyroscope_z: f32,
}
///*
/// Represents the status of laser ranging finder.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct LaserStatus {
    #[prost(enumeration="laser_status::LaserStatusType", tag="1")]
    pub laser_status: i32,
}
/// Nested message and enum types in `LaserStatus`.
pub mod laser_status {
    #[derive(::serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum LaserStatusType {
        ///* The laser ranging finder works fine.
        LaserNormal = 0,
        ///* The target distance is less than minimum range of finder.
        LaserTooClose = 1,
        ///* The target distance is larger than maximum range of finder.
        LaserTooFar = 2,
        ///* The laser module is closed.
        LaserClosed = 3,
    }
}
///*
/// Represents the raw sensor data of laser ranging finder.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct LaserRawData {
    ///* unit: millimeter
    #[prost(uint32, repeated, tag="1")]
    pub distance: ::prost::alloc::vec::Vec<u32>,
    ///* The signal intensity, range: 0~255.
    #[prost(uint32, repeated, tag="2")]
    pub intensity: ::prost::alloc::vec::Vec<u32>,
}
///*
/// Represents the status of ranging function.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct RangingStatus {
    #[prost(enumeration="ranging_status::RangingStatusType", tag="1")]
    pub ranging_status: i32,
}
/// Nested message and enum types in `RangingStatus`.
pub mod ranging_status {
    #[derive(::serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum RangingStatusType {
        ///* The ranging function is off.
        RangingOff = 0,
        ///* The ranging function is on.
        RangingOn = 1,
    }
}
///*
/// Represents the target offset in screen.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct ScreenOffset {
    ///* The target offset on horizontal direction of screen in permillage.
    #[prost(uint32, tag="1")]
    pub screen_offset_x: u32,
    ///* The target offset on vertical direction of screen in permillage.
    #[prost(uint32, tag="2")]
    pub screen_offset_y: u32,
}
///*
/// Represents the scene mode of infrared camera.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct InfraredSceneMode {
    ///* The scene mode of infrared camera.
    #[prost(enumeration="infrared_scene_mode::InfraredSceneModeType", tag="1")]
    pub infrared_scene_mode: i32,
    ///* The DDE (digital detail enhance) value in percent, which is valid only in manual mode.
    #[prost(uint32, tag="2")]
    pub infrared_dde_percent: u32,
    ///* The contrast value in percent, which is valid only in manual mode.
    #[prost(uint32, tag="3")]
    pub infrared_contrast_percent: u32,
    ///* The brightness value in percent, which is valid only in manual mode.
    #[prost(uint32, tag="4")]
    pub infrared_brightness_percent: u32,
}
/// Nested message and enum types in `InfraredSceneMode`.
pub mod infrared_scene_mode {
    #[derive(::serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum InfraredSceneModeType {
        ///* The scene mode is manual. The dde, contrast and brightness are valid only in manual mode
        SceneModeManual = 0,
        ///* The scene mode is common.
        SceneModeCommon = 1,
        ///* The scene mode is inspection.
        SceneModeInspection = 2,
    }
}
///*
/// Represents the pseudo color of infrared camera.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct InfraredPseudoColor {
    ///* The infrared pseudo color.
    #[prost(enumeration="infrared_pseudo_color::InfraredPseudoColorType", tag="1")]
    pub infrared_pseudo_color: i32,
}
/// Nested message and enum types in `InfraredPseudoColor`.
pub mod infrared_pseudo_color {
    #[derive(::serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum InfraredPseudoColorType {
        ///* The infrared pseudo color is whitehot.
        InfraredPseudoColorWhitehot = 0,
        ///* The infrared pseudo color is lava.
        InfraredPseudoColorLava = 1,
        ///* The infrared pseudo color is iron red.
        InfraredPseudoColorIronRed = 2,
        ///* The infrared pseudo color is hotiron.
        InfraredPseudoColorHotiron = 3,
        ///* The infrared pseudo color is medicine.
        InfraredPseudoColorMedicine = 4,
        ///* The infrared pseudo color is northpole.
        InfraredPseudoColorNorthpole = 5,
        ///* The infrared pseudo color is rainbow1.
        InfraredPseudoColorRainbow1 = 6,
        ///* The infrared pseudo color is rainbow2.
        InfraredPseudoColorRainbow2 = 7,
        ///* The infrared pseudo color is tracered.
        InfraredPseudoColorTracered = 8,
        ///* The infrared pseudo color is blackhot.
        InfraredPseudoColorBlackhot = 9,
    }
}
///*
/// Represents the isotherm of infrared camera.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct InfraredIsotherm {
    ///* The infrared isotherm mode.
    #[prost(enumeration="infrared_isotherm::InfraredIsothermModeType", tag="1")]
    pub infrared_isotherm_mode: i32,
    ///* The high threshold of infrared isotherm.
    #[prost(int32, tag="2")]
    pub infrared_isotherm_high_threshold: i32,
    ///* The low threshold of infrared isotherm.
    #[prost(int32, tag="3")]
    pub infrared_isotherm_low_threshold: i32,
}
/// Nested message and enum types in `InfraredIsotherm`.
pub mod infrared_isotherm {
    #[derive(::serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum InfraredIsothermModeType {
        ///* The infrared isotherm mode is off.
        InfraredIsothermModeOff = 0,
        ///* The infrared isotherm mode is on.
        InfraredIsothermModeOn = 1,
    }
}
///*
/// Represents the gain mode of infrared camera.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct InfraredGainMode {
    ///* The user-specified gain mode of infrared camera.
    #[prost(enumeration="infrared_gain_mode::InfraredGainModeType", tag="1")]
    pub infrared_gain_mode: i32,
}
/// Nested message and enum types in `InfraredGainMode`.
pub mod infrared_gain_mode {
    #[derive(::serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum InfraredGainModeType {
        ///* high gain mode which has narrow temperature measurement range but high precision.
        InfraredGainModeHigh = 0,
        ///* low gain mode which has wide temperature measurement range but low precision.
        InfraredGainModeLow = 1,
    }
}
/// Attitude of a specific device
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct DeviceAttitude {
    /// timestamp of the first sample in the attidue array.
    #[prost(uint32, tag="1")]
    pub timestamp: u32,
    /// sensor vsync signal timestamp of first sample.
    #[prost(uint32, tag="2")]
    pub vsync: u32,
    /// array containing all fusioned quaternions belong to certain vsync cnt, like 200/fps
    #[prost(message, repeated, tag="3")]
    pub attitude: ::prost::alloc::vec::Vec<Quaternion>,
    /// time offset between first row of sensor exposure and first sample of device
    #[prost(float, tag="4")]
    pub offset: f32,
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct SensorFrameReadOutTime {
    /// read out time per frame of a certain mode.
    #[prost(uint64, tag="1")]
    pub readout_time: u64,
}
/// TODO: comment
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct SensorReadDirection {
    #[prost(enumeration="sensor_read_direction::SenorReadDirectionType", tag="1")]
    pub direction: i32,
}
/// Nested message and enum types in `SensorReadDirection`.
pub mod sensor_read_direction {
    #[derive(::serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum SenorReadDirectionType {
        TopLeft = 0,
        TopRight = 1,
        BottomRight = 2,
        BottomLeft = 3,
        LeftTop = 4,
        RightTop = 5,
        RightBottom = 6,
        LeftBottom = 7,
    }
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct SensorFrameRate {
    #[prost(float, tag="1")]
    pub sensor_frame_rate: f32,
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct ImuSamplingRate {
    #[prost(uint32, tag="1")]
    pub imu_sampling_rate: u32,
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct DigitalFocalLength {
    /// fx in camera intrinsic matrix, in our model, fx equals to fy.
    #[prost(float, tag="1")]
    pub focal_length: f32,
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct LensDistortionCoefficients {
    /// 1*4 array containing distortion coefficients (k1, k2, k3, k4) of an OpenCV fisheye model.
    #[prost(float, repeated, tag="1")]
    pub coeffients: ::prost::alloc::vec::Vec<f32>,
}
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct EisStatus {
    #[prost(enumeration="eis_status::EisStatusType", tag="1")]
    pub status: i32,
}
/// Nested message and enum types in `EisStatus`.
pub mod eis_status {
    #[derive(::serde::Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum EisStatusType {
        EisOff = 0,
        EisRockSteady = 1,
        EisHorizonSteady = 2,
    }
}
///*
/// Represents the proto entry when we do the encoding or decoding.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct ProductMeta {
    #[prost(message, optional, tag="1")]
    pub clip_meta: ::core::option::Option<ClipMeta>,
    #[prost(message, optional, tag="2")]
    pub stream_meta: ::core::option::Option<StreamMeta>,
    #[prost(message, optional, tag="3")]
    pub frame_meta: ::core::option::Option<FrameMeta>,
}
///*
/// Represents the metadata about video clip.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct ClipMeta {
    #[prost(message, optional, tag="1")]
    pub clip_meta_header: ::core::option::Option<ClipMetaHeader>,
    #[prost(message, optional, tag="2")]
    pub clip_streams_meta: ::core::option::Option<ClipStreamsMeta>,
    #[prost(message, optional, tag="3")]
    pub distortion_coefficients: ::core::option::Option<LensDistortionCoefficients>,
    #[prost(message, optional, tag="4")]
    pub sensor_readout_time: ::core::option::Option<SensorFrameReadOutTime>,
    #[prost(message, optional, tag="5")]
    pub sensor_read_direction: ::core::option::Option<SensorReadDirection>,
    #[prost(message, optional, tag="8")]
    pub digital_focal_length: ::core::option::Option<DigitalFocalLength>,
    #[prost(message, optional, tag="9")]
    pub eis_status: ::core::option::Option<EisStatus>,
    #[prost(message, optional, tag="10")]
    pub imu_sampling_rate: ::core::option::Option<ImuSamplingRate>,
    #[prost(message, optional, tag="11")]
    pub sensor_fps: ::core::option::Option<SensorFrameRate>,
}
///*
/// Represents the metadata about video stream.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct StreamMeta {
    #[prost(message, optional, tag="1")]
    pub stream_meta_header: ::core::option::Option<StreamMetaHeader>,
    #[prost(message, optional, tag="3")]
    pub video_stream_meta: ::core::option::Option<VideoStreamMeta>,
}
///*
/// Represents the metadata about video frame.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct FrameMeta {
    #[prost(message, optional, tag="1")]
    pub frame_meta_header: ::core::option::Option<FrameMetaHeader>,
    #[prost(message, optional, tag="2")]
    pub camera_frame_meta: ::core::option::Option<FrameMetaOfCamera>,
    #[prost(message, optional, tag="3")]
    pub imu_frame_meta: ::core::option::Option<FrameMetaOfImu>,
}
///*
/// Represents the frame metadata of camera device.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct FrameMetaOfCamera {
    #[prost(message, optional, tag="1")]
    pub camera_dev_header: ::core::option::Option<MetaHeaderOfDevice>,
    #[prost(message, optional, tag="2")]
    pub exposure_index: ::core::option::Option<ExposureIndex>,
    #[prost(message, optional, tag="3")]
    pub iso: ::core::option::Option<Iso>,
    #[prost(message, optional, tag="4")]
    pub exposure_time: ::core::option::Option<ExposureTime>,
    #[prost(message, optional, tag="5")]
    pub digital_zoom_ratio: ::core::option::Option<DigitalZoomRatio>,
    #[prost(message, optional, tag="6")]
    pub white_balance_cct: ::core::option::Option<WhiteBalanceCct>,
    #[prost(message, optional, tag="7")]
    pub orientation: ::core::option::Option<Orientation>,
}
///*
/// Represents the frame metadata of IMU device.
#[derive(::serde::Serialize, Clone, PartialEq, ::prost::Message)]
pub struct FrameMetaOfImu {
    #[prost(message, optional, tag="1")]
    pub imu_dev_header: ::core::option::Option<MetaHeaderOfDevice>,
    /// camera attitude samples within video frame interval. processed after motion estimation
    #[prost(message, optional, tag="2")]
    pub imu_attitude_after_fusion: ::core::option::Option<DeviceAttitude>,
}
