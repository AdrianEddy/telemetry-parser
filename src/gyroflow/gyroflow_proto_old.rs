// SPDX-License-Identifier: MIT OR Apache-2.0
//
// LEGACY Gyroflow Protobuf schema

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Main {
    #[prost(string, tag = "1")]
    pub magic_string: ::prost::alloc::string::String,
    #[prost(uint32, tag = "2")]
    pub protocol_version: u32,
    #[prost(message, optional, tag = "3")]
    pub header: ::core::option::Option<Header>,
    #[prost(message, optional, tag = "4")]
    pub frame: ::core::option::Option<FrameMetadata>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Header {
    #[prost(message, optional, tag = "1")]
    pub camera: ::core::option::Option<header::CameraMetadata>,
    #[prost(message, optional, tag = "2")]
    pub clip: ::core::option::Option<header::ClipMetadata>,
}
pub mod header {
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct CameraMetadata {
        #[prost(string, tag = "1")]
        pub camera_brand: ::prost::alloc::string::String,
        #[prost(string, tag = "2")]
        pub camera_model: ::prost::alloc::string::String,
        #[prost(string, optional, tag = "3")]
        pub camera_serial_number: ::core::option::Option<::prost::alloc::string::String>,
        #[prost(string, optional, tag = "4")]
        pub firmware_version: ::core::option::Option<::prost::alloc::string::String>,
        #[prost(string, tag = "5")]
        pub lens_brand: ::prost::alloc::string::String,
        #[prost(string, tag = "6")]
        pub lens_model: ::prost::alloc::string::String,
        /// Sensor pixel pitch in nanometers (single value; square pixels assumed).
        #[prost(uint32, tag = "7")]
        pub pixel_pitch_nm: u32,
        #[prost(uint32, tag = "8")]
        pub sensor_pixel_width: u32,
        #[prost(uint32, tag = "9")]
        pub sensor_pixel_height: u32,
        #[prost(float, optional, tag = "10")]
        pub crop_factor: ::core::option::Option<f32>,
        #[prost(string, optional, tag = "11")]
        pub lens_profile: ::core::option::Option<::prost::alloc::string::String>,
        #[prost(string, optional, tag = "12")]
        pub imu_orientation: ::core::option::Option<::prost::alloc::string::String>,
        #[prost(message, optional, tag = "13")]
        pub imu_rotation: ::core::option::Option<super::Quaternion>,
        #[prost(message, optional, tag = "14")]
        pub quats_rotation: ::core::option::Option<super::Quaternion>,
        #[prost(string, optional, tag = "15")]
        pub additional_data: ::core::option::Option<::prost::alloc::string::String>,
    }
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct ClipMetadata {
        #[prost(uint32, tag = "1")]
        pub frame_width: u32,
        #[prost(uint32, tag = "2")]
        pub frame_height: u32,
        /// Clip duration in microseconds (legacy: float).
        #[prost(float, tag = "3")]
        pub duration_us: f32,
        #[prost(float, tag = "4")]
        pub record_frame_rate: f32,
        #[prost(float, tag = "5")]
        pub sensor_frame_rate: f32,
        #[prost(float, tag = "6")]
        pub file_frame_rate: f32,
        #[prost(int32, tag = "7")]
        pub rotation_degrees: i32,
        #[prost(uint32, tag = "8")]
        pub imu_sample_rate: u32,
        #[prost(string, optional, tag = "9")]
        pub color_profile: ::core::option::Option<::prost::alloc::string::String>,
        #[prost(float, tag = "10")]
        pub pixel_aspect_ratio: f32,
        /// Frame readout time in microseconds (legacy: float).
        #[prost(float, tag = "11")]
        pub frame_readout_time_us: f32,
        #[prost(enumeration = "clip_metadata::ReadoutDirection", tag = "12")]
        pub frame_readout_direction: i32,
    }
    pub mod clip_metadata {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
        #[repr(i32)]
        pub enum ReadoutDirection {
            TopToBottom = 0,
            BottomToTop = 1,
            RightToLeft = 2,
            LeftToRight = 3,
        }
    }
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct FrameMetadata {
    #[prost(double, tag = "1")]
    pub start_timestamp_us: f64,
    #[prost(double, tag = "2")]
    pub end_timestamp_us: f64,
    #[prost(uint32, tag = "3")]
    pub frame_number: u32,
    #[prost(uint32, optional, tag = "4")]
    pub iso: ::core::option::Option<u32>,
    /// Actual exposure time in microseconds (legacy: float).
    #[prost(float, optional, tag = "5")]
    pub exposure_time_us: ::core::option::Option<f32>,
    #[prost(uint32, optional, tag = "6")]
    pub white_balance_kelvin: ::core::option::Option<u32>,
    #[prost(float, optional, tag = "7")]
    pub white_balance_tint: ::core::option::Option<f32>,
    #[prost(float, optional, tag = "8")]
    pub digital_zoom_ratio: ::core::option::Option<f32>,
    #[prost(int32, optional, tag = "9")]
    pub shutter_speed_numerator: ::core::option::Option<i32>,
    /// Renamed to `shutter_speed_denominator` in the current schema (same wire).
    #[prost(int32, optional, tag = "10")]
    pub shutter_speed_denumerator: ::core::option::Option<i32>,
    #[prost(float, optional, tag = "11")]
    pub shutter_angle_degrees: ::core::option::Option<f32>,
    #[prost(float, optional, tag = "12")]
    pub crop_x: ::core::option::Option<f32>,
    #[prost(float, optional, tag = "13")]
    pub crop_y: ::core::option::Option<f32>,
    #[prost(float, optional, tag = "14")]
    pub crop_width: ::core::option::Option<f32>,
    #[prost(float, optional, tag = "15")]
    pub crop_height: ::core::option::Option<f32>,
    #[prost(message, repeated, tag = "16")]
    pub lens: ::prost::alloc::vec::Vec<LensData>,
    #[prost(message, repeated, tag = "17")]
    pub imu: ::prost::alloc::vec::Vec<ImuData>,
    #[prost(message, repeated, tag = "18")]
    pub quaternions: ::prost::alloc::vec::Vec<QuaternionData>,
    #[prost(message, repeated, tag = "19")]
    pub ois: ::prost::alloc::vec::Vec<LensOisData>,
    #[prost(message, repeated, tag = "20")]
    pub ibis: ::prost::alloc::vec::Vec<IbisData>,
    #[prost(message, repeated, tag = "21")]
    pub eis: ::prost::alloc::vec::Vec<EisData>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct LensData {
    #[prost(enumeration = "lens_data::DistortionModel", tag = "1")]
    pub distortion_model: i32,
    #[prost(float, repeated, tag = "2")]
    pub distortion_coefficients: ::prost::alloc::vec::Vec<f32>,
    #[prost(float, repeated, tag = "3")]
    pub camera_intrinsic_matrix: ::prost::alloc::vec::Vec<f32>,
    #[prost(float, optional, tag = "4")]
    pub focal_length_mm: ::core::option::Option<f32>,
    #[prost(float, optional, tag = "5")]
    pub f_number: ::core::option::Option<f32>,
    #[prost(float, optional, tag = "6")]
    pub focus_distance_mm: ::core::option::Option<f32>,
}
pub mod lens_data {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum DistortionModel {
        OpenCvFisheye = 0,
        OpenCvStandard = 1,
        Poly3 = 2,
        Poly5 = 3,
        PtLens = 4,
        GenericPolynomial = 5,
    }
}
#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct ImuData {
    #[prost(double, tag = "1")]
    pub sample_timestamp_us: f64,
    #[prost(float, tag = "2")]
    pub gyroscope_x: f32,
    #[prost(float, tag = "3")]
    pub gyroscope_y: f32,
    #[prost(float, tag = "4")]
    pub gyroscope_z: f32,
    #[prost(float, tag = "5")]
    pub accelerometer_x: f32,
    #[prost(float, tag = "6")]
    pub accelerometer_y: f32,
    #[prost(float, tag = "7")]
    pub accelerometer_z: f32,
    #[prost(float, optional, tag = "8")]
    pub magnetometer_x: ::core::option::Option<f32>,
    #[prost(float, optional, tag = "9")]
    pub magnetometer_y: ::core::option::Option<f32>,
    #[prost(float, optional, tag = "10")]
    pub magnetometer_z: ::core::option::Option<f32>,
}
#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct Quaternion {
    #[prost(float, tag = "1")]
    pub w: f32,
    #[prost(float, tag = "2")]
    pub x: f32,
    #[prost(float, tag = "3")]
    pub y: f32,
    #[prost(float, tag = "4")]
    pub z: f32,
}
#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct QuaternionData {
    #[prost(double, tag = "1")]
    pub sample_timestamp_us: f64,
    #[prost(message, optional, tag = "2")]
    pub quat: ::core::option::Option<Quaternion>,
}
#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct LensOisData {
    #[prost(double, tag = "1")]
    pub sample_timestamp_us: f64,
    /// Optical element shift in the X axis, nanometers.
    #[prost(float, tag = "2")]
    pub x: f32,
    /// Optical element shift in the Y axis, nanometers.
    #[prost(float, tag = "3")]
    pub y: f32,
}
#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct IbisData {
    #[prost(double, tag = "1")]
    pub sample_timestamp_us: f64,
    #[prost(float, tag = "2")]
    pub shift_x: f32,
    #[prost(float, tag = "3")]
    pub shift_y: f32,
    #[prost(float, tag = "4")]
    pub roll_angle_degrees: f32,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct EisData {
    #[prost(double, optional, tag = "1")]
    pub sample_timestamp_us: ::core::option::Option<f64>,
    #[prost(enumeration = "eis_data::EisDataType", tag = "2")]
    pub r#type: i32,
    #[prost(message, optional, tag = "3")]
    pub quaternion: ::core::option::Option<Quaternion>,
    #[prost(message, optional, tag = "4")]
    pub mesh_warp: ::core::option::Option<MeshWarpData>,
    #[prost(float, repeated, tag = "5")]
    pub matrix_4x4: ::prost::alloc::vec::Vec<f32>,
}
pub mod eis_data {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum EisDataType {
        Quaternion = 0,
        MeshWarp = 1,
        Matrix4x4 = 2,
    }
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct MeshWarpData {
    #[prost(int32, tag = "1")]
    pub grid_width: i32,
    #[prost(int32, tag = "2")]
    pub grid_height: i32,
    /// New position of each grid coordinate (legacy layout; maps to `warped_xy`).
    #[prost(float, repeated, tag = "3")]
    pub values: ::prost::alloc::vec::Vec<f32>,
}

// ---------------------------------------------------------------------------
// Conversions to the current schema (`super::gyroflow_proto`). Every legacy
// message maps onto its current counterpart so the rest of the parser only
// deals with current-schema types.
// ---------------------------------------------------------------------------

use super::gyroflow_proto as pb;

impl From<Main> for pb::Main {
    fn from(m: Main) -> Self {
        Self {
            magic_string: m.magic_string,
            protocol_version: m.protocol_version,
            header: m.header.map(Into::into),
            frame: m.frame.map(Into::into),
        }
    }
}
impl From<Header> for pb::Header {
    fn from(h: Header) -> Self {
        Self { camera: h.camera.map(Into::into), clip: h.clip.map(Into::into) }
    }
}
impl From<header::CameraMetadata> for pb::header::CameraMetadata {
    fn from(c: header::CameraMetadata) -> Self {
        Self {
            camera_brand: c.camera_brand,
            camera_model: c.camera_model,
            camera_serial_number: c.camera_serial_number,
            firmware_version: c.firmware_version,
            lens_brand: c.lens_brand,
            lens_model: c.lens_model,
            // Legacy carried a single (square-pixel) pitch; mirror it onto both axes.
            pixel_pitch_x_nm: c.pixel_pitch_nm,
            pixel_pitch_y_nm: c.pixel_pitch_nm,
            sensor_pixel_width: c.sensor_pixel_width,
            sensor_pixel_height: c.sensor_pixel_height,
            crop_factor: c.crop_factor,
            lens_profile: c.lens_profile,
            imu_orientation: c.imu_orientation,
            imu_rotation: c.imu_rotation.map(Into::into),
            quats_rotation: c.quats_rotation.map(Into::into),
            additional_data: c.additional_data,
        }
    }
}
impl From<header::ClipMetadata> for pb::header::ClipMetadata {
    fn from(c: header::ClipMetadata) -> Self {
        Self {
            frame_width: c.frame_width,
            frame_height: c.frame_height,
            duration_us: c.duration_us as f64,
            record_frame_rate: c.record_frame_rate,
            sensor_frame_rate: c.sensor_frame_rate,
            file_frame_rate: c.file_frame_rate,
            rotation_degrees: c.rotation_degrees,
            imu_sample_rate: c.imu_sample_rate,
            color_profile: c.color_profile,
            pixel_aspect_ratio: c.pixel_aspect_ratio,
            frame_readout_time_us: c.frame_readout_time_us as f64,
            // ReadoutDirection has identical values in both schemas.
            frame_readout_direction: c.frame_readout_direction,
        }
    }
}
impl From<FrameMetadata> for pb::FrameMetadata {
    fn from(f: FrameMetadata) -> Self {
        Self {
            start_timestamp_us: f.start_timestamp_us,
            end_timestamp_us: f.end_timestamp_us,
            frame_number: f.frame_number,
            iso: f.iso,
            exposure_time_us: f.exposure_time_us.map(|v| v as f64),
            white_balance_kelvin: f.white_balance_kelvin,
            white_balance_tint: f.white_balance_tint,
            digital_zoom_ratio: f.digital_zoom_ratio,
            shutter_speed_numerator: f.shutter_speed_numerator,
            shutter_speed_denominator: f.shutter_speed_denumerator,
            shutter_angle_degrees: f.shutter_angle_degrees,
            crop_x: f.crop_x,
            crop_y: f.crop_y,
            crop_width: f.crop_width,
            crop_height: f.crop_height,
            lens: f.lens.into_iter().map(Into::into).collect(),
            imu: f.imu.into_iter().map(Into::into).collect(),
            quaternions: f.quaternions.into_iter().map(Into::into).collect(),
            ois: f.ois.into_iter().map(Into::into).collect(),
            ibis: f.ibis.into_iter().map(Into::into).collect(),
            eis: f.eis.into_iter().map(Into::into).collect(),
            gps: ::prost::alloc::vec::Vec::new(),
        }
    }
}
impl From<LensData> for pb::LensData {
    fn from(l: LensData) -> Self {
        use pb::lens_data::Distortion;
        let coeffs = l.distortion_coefficients;
        let model = lens_data::DistortionModel::try_from(l.distortion_model)
            .unwrap_or(lens_data::DistortionModel::OpenCvFisheye);
        let distortion = Some(match model {
            lens_data::DistortionModel::OpenCvFisheye     => Distortion::OpencvFisheye(pb::OpenCvFisheye { coefficients: coeffs }),
            lens_data::DistortionModel::OpenCvStandard    => Distortion::OpencvStandard(pb::OpenCvStandard { coefficients: coeffs }),
            lens_data::DistortionModel::Poly3             => Distortion::LensfunPoly3(pb::LensFunPoly3 { coefficients: coeffs }),
            lens_data::DistortionModel::Poly5             => Distortion::LensfunPoly5(pb::LensFunPoly5 { coefficients: coeffs }),
            lens_data::DistortionModel::PtLens            => Distortion::LensfunPtlens(pb::LensFunPtLens { coefficients: coeffs }),
            lens_data::DistortionModel::GenericPolynomial => Distortion::GenericPolynomial(pb::GenericPolynomial { coefficients: coeffs }),
        });
        Self {
            sample_timestamp_us: None,
            camera_intrinsic_matrix: l.camera_intrinsic_matrix,
            focal_length_mm: l.focal_length_mm,
            f_number: l.f_number,
            focus_distance_mm: l.focus_distance_mm,
            distortion,
        }
    }
}
impl From<ImuData> for pb::ImuData {
    fn from(s: ImuData) -> Self {
        Self {
            sample_timestamp_us: Some(s.sample_timestamp_us),
            gyroscope_x: s.gyroscope_x,
            gyroscope_y: s.gyroscope_y,
            gyroscope_z: s.gyroscope_z,
            accelerometer_x: s.accelerometer_x,
            accelerometer_y: s.accelerometer_y,
            accelerometer_z: s.accelerometer_z,
            magnetometer_x: s.magnetometer_x,
            magnetometer_y: s.magnetometer_y,
            magnetometer_z: s.magnetometer_z,
        }
    }
}
impl From<Quaternion> for pb::Quaternion {
    fn from(q: Quaternion) -> Self { Self { w: q.w, x: q.x, y: q.y, z: q.z } }
}
impl From<QuaternionData> for pb::QuaternionData {
    fn from(q: QuaternionData) -> Self {
        Self { sample_timestamp_us: Some(q.sample_timestamp_us), quat: q.quat.map(Into::into) }
    }
}
impl From<LensOisData> for pb::LensOisData {
    fn from(o: LensOisData) -> Self {
        Self { sample_timestamp_us: Some(o.sample_timestamp_us), shift_x_nm: o.x, shift_y_nm: o.y }
    }
}
impl From<IbisData> for pb::IbisData {
    fn from(b: IbisData) -> Self {
        Self {
            sample_timestamp_us: Some(b.sample_timestamp_us),
            shift_x_nm: b.shift_x,
            shift_y_nm: b.shift_y,
            roll_angle_degrees: b.roll_angle_degrees,
        }
    }
}
impl From<EisData> for pb::EisData {
    fn from(e: EisData) -> Self {
        use pb::eis_data::Data;
        let ty = eis_data::EisDataType::try_from(e.r#type)
            .unwrap_or(eis_data::EisDataType::Quaternion);
        let data = match ty {
            eis_data::EisDataType::Quaternion => e.quaternion.map(|q| Data::Quaternion(q.into())),
            eis_data::EisDataType::MeshWarp   => e.mesh_warp.map(|m| Data::MeshWarp(m.into())),
            eis_data::EisDataType::Matrix4x4  => (!e.matrix_4x4.is_empty())
                .then(|| Data::Matrix4x4(pb::Matrix4x4 { values: e.matrix_4x4 })),
        };
        Self { sample_timestamp_us: e.sample_timestamp_us, data }
    }
}
impl From<MeshWarpData> for pb::MeshWarpData {
    fn from(m: MeshWarpData) -> Self {
        // The legacy schema had no region_width/region_height: the warped grid
        // implicitly spanned the frame, and `values` carried the (x, y) target
        // positions (2 per anchor — the same layout as the current `warped_xy`).
        // Recover a region from the bounding box of those positions so the
        // consumer's anchor grid (uniform over [0, region]) isn't degenerate;
        // anchors start at 0, so the max coordinate is a faithful proxy for the
        // span. (If a producer instead wrote the ambiguous grid_width*grid_height
        // count, the consumer's `warped_xy.len() == 2*gw*gh` check drops it — see
        // build_mesh_correction_json — which is the safe outcome.)
        let mut region_width = 0.0f32;
        let mut region_height = 0.0f32;
        for xy in m.values.chunks_exact(2) {
            region_width  = region_width.max(xy[0]);
            region_height = region_height.max(xy[1]);
        }
        Self {
            grid_width: m.grid_width.max(0) as u32,
            grid_height: m.grid_height.max(0) as u32,
            region_width,
            region_height,
            warped_xy: m.values,
        }
    }
}
