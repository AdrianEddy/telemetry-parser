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

    #[serde(serialize_with="Lens283MType_serializer")]
    #[prost(enumeration="extra_metadata::Lens283MType", tag="66")] pub lens_283_m_type1: i32,

    #[serde(serialize_with="Lens283MType_serializer")]
    #[prost(enumeration="extra_metadata::Lens283MType", tag="67")] pub lens_283_m_type2: i32,

    /// What the camera recorded its lenses as wearing: lens guards, an ND
    /// filter, a dive case — or nothing (`Common`, also an absent field).
    #[serde(serialize_with="OffsetState_serializer")]
    #[prost(enumeration="OffsetState", tag="68")] pub offset_states: i32,
    #[prost(float, repeated, tag="69")] pub q_i_uav: Vec<f32>,
    #[prost(float, repeated, tag="70")] pub r_uav_i: Vec<f32>,

    #[prost(message, optional, tag="71")] pub auto_clip_para: Option<extra_metadata::AutoClipParam>,

    #[prost(message, optional, tag="72")] pub meta_track_para: Option<extra_metadata::MetaTrackParam>,

    #[serde(serialize_with="TimeScale_serializer")]
    #[prost(enumeration="extra_metadata::TimeScale", tag="73")] pub time_scale: i32,

    #[serde(serialize_with="HdrProcessType_serializer")]
    #[prost(enumeration="extra_metadata::HdrProcessType", tag="74")] pub hdr_process_type: i32,

    #[serde(serialize_with="ThumbnailType_serializer")]
    #[prost(enumeration="extra_metadata::ThumbnailType", tag="75")] pub thumbnail_type: i32,

    #[serde(serialize_with="PureShotProcessType_serializer")]
    #[prost(enumeration="extra_metadata::PureShotProcessType", tag="76")] pub pureshot_process_type: i32,
    #[prost(bool, tag="77")] pub pre_rec_time_flag: bool,

    #[prost(message, optional, tag="78")] pub frame_rate_av_rational: Option<extra_metadata::FrameRateAvRational>,

    #[serde(serialize_with="PanoRecordType_serializer")]
    #[prost(enumeration="extra_metadata::PanoRecordType", tag="79")] pub pano_record_type: i32,
    /// Which of a two-track file's tracks holds which lens. The one statement
    /// the vendor binds a track to a lens by; `stream_type` says nothing to it.
    #[serde(serialize_with="PanoRecordMultiTrackOrder_serializer")]
    #[prost(enumeration="extra_metadata::PanoRecordMultiTrackOrder", tag="80")] pub pano_record_multi_track_order: i32,

    #[serde(serialize_with="ThumbnailStitchType_serializer")]
    #[prost(enumeration="extra_metadata::ThumbnailStitchType", tag="81")] pub thumbnail_stitch_type: i32,

    #[serde(serialize_with="ImageStitchType_serializer")]
    #[prost(enumeration="extra_metadata::ImageStitchType", tag="82")] pub image_stitch_type: i32,

    #[serde(serialize_with="SceneType_serializer")]
    #[prost(enumeration="extra_metadata::SceneType", tag="83")] pub scene_type: i32,
    #[prost(bool, tag="84")] pub is_cutting_thumbnail: bool,

    #[serde(serialize_with="ReprocessingType_serializer")]
    #[prost(enumeration="extra_metadata::ReprocessingType", tag="85")] pub reprocessing_type: i32,

    #[serde(serialize_with="OnlineFlowstateLevel_serializer")]
    #[prost(enumeration="extra_metadata::OnlineFlowstateLevel", tag="86")] pub online_flowstate_level: i32,
    #[prost(bool, tag="87")] pub b_high_light_eidted: bool,

    #[serde(serialize_with="FileSplitType_serializer")]
    #[prost(enumeration="extra_metadata::FileSplitType", tag="88")] pub file_split_type: i32,
    #[prost(bool, tag="89")] pub b_favorite: bool,
    #[prost(uint32, tag="90")] pub edit_version: u32,
    #[prost(double, repeated, tag="91")] pub q_i_wnew: Vec<f64>,
    #[prost(bool, tag="92")] pub b_inside_editor_export: bool,
    #[prost(uint32, tag="93")] pub total_time_ms: u32,
    #[prost(bool, tag="94")] pub b_continue_record: bool,
    #[prost(bool, tag="95")] pub b_paused_record: bool,
    #[prost(bool, tag="96")] pub b_torchmode: bool,

    #[serde(serialize_with="HdrModeType_serializer")]
    #[prost(enumeration="extra_metadata::HdrModeType", tag="97")] pub hdr_mode_type: i32,

    #[prost(message, repeated, tag="98")] pub continue_record_info: Vec<extra_metadata::ContinueRecordInfo>,
    #[prost(bool, tag="99")] pub b_drop_frame_occur: bool,
    #[prost(uint32, tag="100")] pub photo_group_num: u32,
    #[prost(bool, tag="101")] pub recovery: bool,
    #[prost(bool, tag="102")] pub has_pb_resolution: bool,

    #[prost(message, optional, tag="103")] pub pb_resolution_size: Option<extra_metadata::Vector2>,

    #[serde(serialize_with="GuardDetectedType_serializer")]
    #[prost(enumeration="extra_metadata::GuardDetectedType", tag="104")] pub guard_detected_type: i32,

    /// Whether the cooling shell (the Thermo Grip Cover) was on: set by the
    /// user, or left to the camera's own detection.
    #[serde(serialize_with="CoolingShellSelectedState_serializer")]
    #[prost(enumeration="extra_metadata::CoolingShellSelectedState", tag="105")] pub cooling_shell_selected_state: i32,
    /// What the camera's own detection of the cooling shell found.
    #[serde(serialize_with="CoolingShellAlgoDetectedState_serializer")]
    #[prost(enumeration="extra_metadata::CoolingShellAlgoDetectedState", tag="106")] pub cooling_shell_algo_state: i32,

    #[serde(serialize_with="AiSwitchState_serializer")]
    #[prost(enumeration="extra_metadata::AiSwitchState", tag="107")] pub ai_switch_state: i32,
    #[prost(string, tag="108")] pub pureshot_image_debug: String,

    #[prost(message, optional, tag="109")] pub material_3a_info: Option<extra_metadata::Material3AInfo>,

    #[serde(serialize_with="ResolutiuonType_serializer")]
    #[prost(enumeration="extra_metadata::ResolutiuonType", tag="110")] pub resolution_type: i32,

    #[prost(string, tag="111")] pub offset_v6: String,
    #[prost(string, tag="112")] pub original_offset_v6: String,
    #[prost(uint32, tag="113")] pub interval_photo_interval_time: u32,
    #[prost(uint32, tag="114")] pub flowstate_level: u32,

    #[prost(message, optional, tag="115")] pub folder_info: Option<extra_metadata::FolderInfo>,
    #[prost(bool, tag="116")] pub horizon_enable: bool,
    #[prost(int32, tag="117")] pub time_zone_seconds_from_gmt: i32,

    #[serde(serialize_with="UnderWaterModeType_serializer")]
    #[prost(enumeration="extra_metadata::UnderWaterModeType", tag="118")] pub underwater_mode_type: i32,

    #[serde(serialize_with="WaterMarkType_serializer")]
    #[prost(enumeration="extra_metadata::WaterMarkType", tag="119")] pub water_mark_type: i32,
    #[prost(uint32, tag="120")] pub audio_track_num: u32,

    #[prost(message, repeated, tag="121")] pub audio_track_info: Vec<extra_metadata::AudioTrackInfo>,

    #[serde(serialize_with="CloudVideoType_serializer")]
    #[prost(enumeration="extra_metadata::CloudVideoType", tag="122")] pub cloud_video_type: i32,

    #[serde(serialize_with="ExtremeLowPowerModeType_serializer")]
    #[prost(enumeration="extra_metadata::ExtremeLowPowerModeType", tag="123")] pub extreme_lowpower_mode_type: i32,

    #[serde(serialize_with="LowLightStabilizationModeType_serializer")]
    #[prost(enumeration="extra_metadata::LowLightStabilizationModeType", tag="124")] pub lowlight_stabilization_mode_type: i32,

    #[serde(serialize_with="bytes_serializer")]
    #[prost(bytes="vec", tag="125")] pub cloud_video_info: Vec<u8>,

    #[serde(serialize_with="LensModuleType_serializer")]
    #[prost(enumeration="extra_metadata::LensModuleType", tag="126")] pub lens_module_type: i32,
    #[prost(int32, tag="127")] pub file_duration_tailor_time: i32,
    #[prost(int32, tag="128")] pub blend_angle: i32,

    #[serde(serialize_with="FileCategory_serializer")]
    #[prost(enumeration="extra_metadata::FileCategory", tag="129")] pub file_category: i32,

    #[serde(serialize_with="FileRotate_serializer")]
    #[prost(enumeration="extra_metadata::FileRotate", tag="130")] pub file_rotate: i32,

    /// How the camera's pictures are spread over files and tracks — including
    /// whether the two lenses' tracks are in lens order or reversed.
    #[serde(serialize_with="StreamType_serializer")]
    #[prost(enumeration="extra_metadata::StreamType", tag="131")] pub stream_type: i32,

    #[serde(serialize_with="CodecType_serializer")]
    #[prost(enumeration="extra_metadata::CodecType", tag="132")] pub codec_type: i32,
    #[prost(uint64, tag="133")] pub timelapse_interval_ms: u64,
    #[prost(bool, tag="134")] pub is_pre_record: bool,
    #[prost(uint64, tag="135")] pub file_expect_bitrate: u64,

    /// Which offset string the camera wrote last. **Advisory** — the enum
    /// value is not the version number (`OFFSET_V6 = 4`), and the vendor's own
    /// player ignores it in favour of a fixed v6 > v3 > v2 > v1 preference.
    #[serde(serialize_with="OffsetVersion_serializer")]
    #[prost(enumeration="extra_metadata::OffsetVersion", tag="136")] pub capture_offset_version: i32,

    #[prost(message, optional, tag="148")] pub expected_pano_ratio: Option<extra_metadata::Vector2>,

    #[serde(serialize_with="MctfProcessType_serializer")]
    #[prost(enumeration="extra_metadata::MctfProcessType", tag="149")] pub mctf_process_type: i32,

    #[serde(serialize_with="TimelapsStorageType_serializer")]
    #[prost(enumeration="extra_metadata::TimelapsStorageType", tag="150")] pub timelaps_storage_type: i32,

    #[serde(serialize_with="DirectionLockType_serializer")]
    #[prost(enumeration="extra_metadata::DirectionLockType", tag="151")] pub direction_lock_type: i32,

    #[serde(serialize_with="DeWarpType_serializer")]
    #[prost(enumeration="extra_metadata::DeWarpType", tag="152")] pub dewarp_type: i32,

    #[serde(serialize_with="UavCameraMode_serializer")]
    #[prost(enumeration="extra_metadata::UavCameraMode", tag="153")] pub uav_camera_mode: i32,
    #[prost(uint32, tag="154")] pub trim_start: u32,
    #[prost(uint32, tag="155")] pub trim_end: u32,

    #[serde(serialize_with="RemovePurpleFringing_serializer")]
    #[prost(enumeration="extra_metadata::RemovePurpleFringing", tag="156")] pub remove_purple_fringing: i32,

    #[serde(serialize_with="CaptureRecommendStitchType_serializer")]
    #[prost(enumeration="extra_metadata::CaptureRecommendStitchType", tag="157")] pub capture_recommend_stitch_type: i32,

    #[prost(message, optional, tag="158")] pub takeover_info: Option<extra_metadata::IntervaRecTakeoverInfo>,

    #[serde(serialize_with="PrsestType_serializer")]
    #[prost(enumeration="extra_metadata::PrsestType", tag="159")] pub preset_type: i32,

    #[prost(message, optional, tag="160")] pub file_scene_info: Option<extra_metadata::FileSceneInfo>,

    #[serde(serialize_with="EvMapLevel_serializer")]
    #[prost(enumeration="extra_metadata::EvMapLevel", tag="161")] pub evmap_level: i32,
    #[prost(uint32, tag="162")] pub ae_state: u32,

    #[serde(serialize_with="SharpenState_serializer")]
    #[prost(enumeration="extra_metadata::SharpenState", tag="163")] pub sharpen_state: i32,

    #[serde(serialize_with="AeFlickerType_serializer")]
    #[prost(enumeration="extra_metadata::AeFlickerType", tag="164")] pub aeflicker_type: i32,
    #[prost(uint32, tag="165")] pub uav_pos_data_version: u32,

    #[prost(message, optional, tag="166")] pub mag_calib_dongle: Option<extra_metadata::MagCalib>,

    #[prost(message, optional, tag="167")] pub mag_calib_tm: Option<extra_metadata::MagCalib>,

    #[prost(message, optional, tag="168")] pub q_i_mag_dongle: Option<extra_metadata::MagQi>,

    #[prost(message, optional, tag="169")] pub q_i_mag_tm: Option<extra_metadata::MagQi>,

    #[prost(message, optional, tag="170")] pub q_b_tm: Option<extra_metadata::MagQi>,
    #[prost(uint32, tag="171")] pub uav_datatype_version: u32,
    #[prost(uint32, tag="172")] pub pre_rec_frano_sec: u32,
    #[prost(bool, tag="173")] pub b_playback_cache: bool,
    #[prost(bool, tag="174")] pub is_horizontal_state: bool,

    #[serde(serialize_with="PropellerGuardStatusType_serializer")]
    #[prost(enumeration="extra_metadata::PropellerGuardStatusType", tag="175")] pub propeller_guard_status: i32,
    #[prost(bool, tag="176")] pub is_af_lens: bool,

    #[prost(message, optional, tag="177")] pub af_lens_info: Option<AfLensInfo>,

    #[prost(message, optional, tag="178")] pub camera_calib_additional_info: Option<extra_metadata::CameraCalibAdditionalInfo>,

    #[serde(serialize_with="HighLightSource_serializer")]
    #[prost(enumeration="extra_metadata::HighLightSource", tag="179")] pub high_light_source: i32,
    #[prost(bool, tag="180")] pub is_capture_beauty_mode_on: bool,

    #[prost(message, optional, tag="181")] pub beauty_info: Option<CaptureBeautyParams>,

    #[serde(serialize_with="ColorCorrectionType_serializer")]
    #[prost(enumeration="extra_metadata::ColorCorrectionType", tag="182")] pub color_correction: i32,

    #[prost(message, optional, tag="183")] pub capture_image_quality_info: Option<extra_metadata::CaptureImageQualityInfo>,
    #[prost(bool, tag="184")] pub is_capture_beauty_support: bool,
    #[prost(int64, tag="185")] pub record_start_timestamp_ms: i64,

    #[serde(serialize_with="LensAccessoryType_serializer")]
    #[prost(enumeration="extra_metadata::LensAccessoryType", tag="186")] pub lens_accessory_type: i32,
    #[prost(string, tag="187")] pub lens_accessory_offset: String,

    #[serde(serialize_with="CameraScreenOrientation_serializer")]
    #[prost(enumeration="extra_metadata::CameraScreenOrientation", tag="188")] pub screen_orientation: i32,

    #[serde(serialize_with="CaptureFilpType_serializer")]
    #[prost(enumeration="extra_metadata::CaptureFilpType", tag="189")] pub filp_type: i32,

    #[serde(serialize_with="ShootingAccessoriesType_serializer")]
    #[prost(enumeration="extra_metadata::ShootingAccessoriesType", tag="190")] pub shooting_accessories: i32,

    #[serde(serialize_with="StreetPhotoHorizCorrection_serializer")]
    #[prost(enumeration="extra_metadata::StreetPhotoHorizCorrection", tag="191")] pub street_photo_horizon_correction: i32,

    #[prost(message, optional, tag="192")] pub offset_calib: Option<extra_metadata::CameraOffsetCalib>,
    #[prost(bool, tag="193")] pub p3_fake_on: bool,

    #[serde(serialize_with="WaterMarkStyle_serializer")]
    #[prost(enumeration="extra_metadata::WaterMarkStyle", tag="194")] pub watermark_style: i32,

    #[prost(message, optional, tag="195")] pub origin_framerate: Option<extra_metadata::FrameRateAvRational>,

    #[prost(message, optional, tag="196")] pub final_framerate: Option<extra_metadata::FrameRateAvRational>,
    #[prost(string, tag="197")] pub offset_v8: String,
    #[prost(uint64, tag="198")] pub first_tracking_frame_number: u64,

    #[serde(serialize_with="RepairedType_serializer")]
    #[prost(enumeration="extra_metadata::RepairedType", tag="199")] pub repaired_type: i32,
    #[prost(string, tag="200")] pub aperture_value: String,
    #[prost(string, tag="201")] pub extern_filter: String,
    #[prost(bool, tag="202")] pub is_3a_description_supported: bool,

    #[serde(serialize_with="VideoBitDepth_serializer")]
    #[prost(enumeration="extra_metadata::VideoBitDepth", tag="203")] pub main_video_bitdepth: i32,

    #[serde(serialize_with="VideoBitDepth_serializer")]
    #[prost(enumeration="extra_metadata::VideoBitDepth", tag="204")] pub optional_video_bitdepth: i32,

    #[serde(serialize_with="VideoColorSpace_serializer")]
    #[prost(enumeration="extra_metadata::VideoColorSpace", tag="205")] pub video_color_space: i32,

    #[prost(message, repeated, tag="206")] pub offset_info: Vec<OffsetInfo>,
    #[prost(float, tag="207")] pub sensor_module_temper: f32,
    #[prost(bool, tag="208")] pub is_ultra_hdr_on: bool,

    #[prost(message, repeated, tag="209")] pub views: Vec<extra_metadata::BigboomTracks>,
    #[prost(float, repeated, tag="210")] pub photo_still_rot: Vec<f32>,

    #[prost(message, optional, tag="211")] pub aigc_info: Option<extra_metadata::AigcInfo>,

    #[prost(message, optional, tag="212")] pub shooting_param_info: Option<extra_metadata::ShootingParamInfo>,

    #[prost(message, optional, tag="213")] pub thumbnail_hero_rot_quat: Option<extra_metadata::ThumbnailHeroRotQuatData>,
    #[prost(float, tag="214")] pub fac_calib_temper: f32,
    #[prost(bool, tag="215")] pub is_vfr_record: bool,

    #[serde(serialize_with="UavControlType_serializer")]
    #[prost(enumeration="extra_metadata::UavControlType", tag="216")] pub uav_control_type: i32,

    #[serde(serialize_with="UavDisplayType_serializer")]
    #[prost(enumeration="extra_metadata::UavDisplayType", tag="217")] pub uav_display_type: i32,

    #[serde(serialize_with="AvSynthStatus_serializer")]
    #[prost(enumeration="extra_metadata::AvSynthStatus", tag="218")] pub av_synth_status: i32,
    #[prost(string, tag="219")] pub simulation_path: String,

    #[serde(serialize_with="AudioConcertSceneType_serializer")]
    #[prost(enumeration="extra_metadata::AudioConcertSceneType", tag="220")] pub audio_concert_scene: i32,

    #[serde(serialize_with="ThumbnailType_serializer")]
    #[prost(enumeration="extra_metadata::ThumbnailType", tag="221")] pub small_thumbnail_type: i32,
}

/// What the camera recorded its lenses as wearing — `insta360.messages.OffsetState`,
/// under the vendor's own names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize, ::serde::Deserialize)]
#[repr(i32)]
pub enum OffsetState {
    Common         = 0,
    SphereProtect  = 1,
    DivingWater    = 2,
    DivingWater23  = 3,
    ProtectA       = 4,
    ProtectS       = 5,
    ProtectAuto    = 6,
    FilterNd16     = 7,
    FilterNd32     = 8,
    FilterNd64     = 9,
    DivingWaterPro = 10,
    DivingAirPro   = 11,
    FilterNd128    = 12,
    FilterJoystick = 13,
    G01Protect     = 14,
}

/// `insta360.messages.AFLensInfo`
#[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
pub struct AfLensInfo {
    #[prost(int32, tag="1")] pub range_near: i32,
    #[prost(int32, tag="2")] pub range_far: i32,
}
/// `insta360.messages.CaptureBeautyParams`
#[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
pub struct CaptureBeautyParams {
    #[prost(float, tag="1")] pub skin_care_strength: f32,
    #[prost(float, tag="2")] pub blemish_removal_strength: f32,
    #[prost(uint32, repeated, tag="3")] pub rsv: Vec<u32>,
    #[prost(bool, tag="4")] pub one_click_beauty_switch: bool,
}
/// `insta360.messages.OffsetInfo`
#[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
pub struct OffsetInfo {
    #[prost(int32, tag="1")] pub offset_version: i32,
    #[prost(string, tag="2")] pub offset: String,
}

/// Nested message and enum types in `ExtraMetadata`.
pub mod extra_metadata {
    /// Whether the cooling shell was on (`CoolingShellSelectedState`): stated,
    /// or left to the camera's detection (`Auto`).
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize, ::serde::Deserialize)]
    #[repr(i32)]
    pub enum CoolingShellSelectedState {
        Unknown = 0,
        Auto    = 1,
        On      = 2,
        Off     = 3,
    }
    /// What the camera's detection of the cooling shell found
    /// (`CoolingShellAlgoDetectedState`).
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize, ::serde::Deserialize)]
    #[repr(i32)]
    pub enum CoolingShellAlgoDetectedState {
        None = 0,
        On   = 1,
        Off  = 2,
    }

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
        #[prost(int32, tag="5")] pub crop_offset_x: i32,
        #[prost(int32, tag="6")] pub crop_offset_y: i32,
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
        FovTypeMosquito         = 17,
        FovTypeSuperMicro = 18,
        FovTypeUnderwater = 19,
        FovTypeNum        = 20,
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
        AudioModeWindreduction = 2,
        AudioMode360     = 3,
        AudioModeStereo  = 4,
        AudioModeExtmicdev = 5,
        AudioModeAirpoddev = 6,
        AudioModeInsalgoFocus                    = 7,
        AudioModeAdaptiveDenoise                 = 8,
        AudioModeAiWindreduction                 = 9,
        AudioModeAmbisonic                       = 10,
        AudioModeSpeechEnhance                   = 11,
        AudioModeAirpoddevFocus                  = 12,
        AudioModeAirpoddevWindreduction          = 13,
        AudioModeAirpoddevStereo                 = 14,
        AudioModeAirpoddevExtmicdev              = 15,
        AudioModeAirpoddevAiWindreduction        = 16,
        AudioModeAirpoddevTraditionWindreduction = 17,
        AudioModeAirpoddevSpeechEnhance          = 18,
        AudioModeAirpoddevSpeechAmbisonic        = 19,
        AudioModeAiWindreductionInner            = 20,
        AudioModeSpeechEnhanceInner              = 21,
        AudioModeAirpoddevAiWindreductionInner   = 22,
        AudioModeAirpoddevSpeechEnhanceInner     = 23,
        AudioModeHifi                            = 24,
        AudioModeEngineSoundEnhance              = 25,
        AudioModeMultimodal                      = 26,
        AudioModeConcert                         = 27,
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
        VideoSelfie         = 18,
        PhotoPureshotplus   = 19,
        VideoPure           = 20,
        VideoStarlapse      = 21,
        PhotoStartrail      = 22,
        VideoDashCam        = 23,
        VideoVirPtz         = 24,
        VideoOrdinary       = 25,
        PhotoOrdinary       = 26,
        VideoIev            = 27,
        PhotoAeb            = 28,
        LongExp             = 29,
        Photo360Pano        = 30,
        PhotoSuperPano      = 31,
        PhotoGimbalPanoMode = 32,
        VideoConcert        = 33,
        VideoDualScene      = 34,
        PhotoPortrait       = 35,
        VideoPortrait       = 36,
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum RawCaptureType {
        RawCaptureTypeOff      = 0,
        RawCaptureTypeDng      = 1,
        RawCaptureTypeRaw      = 2,
        RawCaptureTypePureshot = 3,
        PureshotRaw = 4,
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

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum PanoRecordType {
        Unknown    = 0,
        SplitFile  = 1,
        MultiTrack = 2,
    }
    /// The vendor's own names: track 0 holds the camera's stream `10`, or its
    /// stream `00`. Lens 0 is stream `00`, so the first is the reversed order.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum PanoRecordMultiTrackOrder {
        Unknown           = 0,
        Track0Stream10    = 1,
        Track0Stream00    = 2,
    }
    /// Where the camera put each lens's pictures.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum StreamType {
        Unknown             = 0,
        SingleStreamFile    = 1,
        /// The two lenses are in two separate files.
        DualStreamFile      = 2,
        /// Two video tracks of this file, lens order == track order.
        DualStreamTrack     = 3,
        /// Two video tracks of this file, in the opposite order.
        DualStreamTrackReverse = 4,
    }
    /// The offset-string version the camera declares it wrote. The value is an
    /// enum ordinal and **not** the version number: `V6` is 4, and there is no
    /// `v4` or `v5` string.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum OffsetVersion {
        Unknown = 0,
        V1      = 1,
        V2      = 2,
        V3      = 3,
        V6      = 4,
        V8      = 5,
    }

    /// `insta360.messages.ExtraMetadata.Lens283MType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum Lens283MType {
        LensUnknow           = 0,
        LensLowTransparency  = 1,
        LensHighTransparency = 2,
    }
    /// `insta360.messages.ExtraMetadata.AutoClipParam`
    #[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
    pub struct AutoClipParam {
        #[prost(message, optional, tag="1")] pub version: Option<AutoClipVersion>,
        #[prost(float, tag="2")] pub sample_rate: f32,
        #[prost(uint32, tag="3")] pub img_w: u32,
        #[prost(uint32, tag="4")] pub img_h: u32,
        #[prost(uint32, tag="5")] pub img_pitch: u32,
        #[serde(serialize_with="super::AutoClipDataStorage_serializer")]
        #[prost(enumeration="AutoClipDataStorage", tag="6")] pub autoclip_data_storage: i32,
        #[prost(uint32, tag="7")] pub fps: u32,
        #[prost(uint32, tag="8")] pub seq_diff: u32,
        #[prost(uint32, tag="9")] pub pts_diff: u32,
        #[prost(uint32, tag="10")] pub total_frame: u32,
        #[prost(uint32, tag="11")] pub resume_num: u32,
        #[prost(int32, repeated, tag="12")] pub last_frame_id: Vec<i32>,
        #[prost(uint32, tag="13")] pub direction_seq_diff: u32,
        #[prost(int64, tag="14")] pub first_cap_pts: i64,
        #[prost(uint32, tag="15")] pub apei_fps: u32,
    }
    /// `insta360.messages.ExtraMetadata.MetaTrackParam`
    #[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
    pub struct MetaTrackParam {
        #[prost(uint64, tag="1")] pub imu_data_size: u64,
        #[prost(uint64, tag="2")] pub aaa_data_size: u64,
        #[prost(uint32, tag="3")] pub compress: u32,
    }
    /// `insta360.messages.ExtraMetadata.TimeScale`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum TimeScale {
        Timescale1X  = 0,
        Timescale4X  = 1,
        Timescale8X  = 2,
        Timescale14X = 3,
        Timescale18X = 4,
    }
    /// `insta360.messages.ExtraMetadata.HdrProcessType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum HdrProcessType {
        HdrProcessExternal         = 0,
        HdrProcessInner            = 1,
        NoHdr                      = 2,
        HdrProcessExternalVersion2 = 3,
    }
    /// `insta360.messages.ExtraMetadata.ThumbnailType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum ThumbnailType {
        Unknown = 0,
        Yuv     = 1,
        Jpeg    = 2,
        H264    = 3,
    }
    /// `insta360.messages.ExtraMetadata.PureShotProcessType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum PureShotProcessType {
        PureshotProcessExternal = 0,
        PureshotProcessInner    = 1,
    }
    /// `insta360.messages.ExtraMetadata.FrameRateAVRational`
    #[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
    pub struct FrameRateAvRational {
        #[prost(uint32, tag="1")] pub num: u32,
        #[prost(uint32, tag="2")] pub den: u32,
    }
    /// `insta360.messages.ExtraMetadata.ThumbnailStitchType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum ThumbnailStitchType {
        Unknown  = 0,
        Original = 1,
        Stitched = 2,
    }
    /// `insta360.messages.ExtraMetadata.ImageStitchType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum ImageStitchType {
        Unknown  = 0,
        Original = 1,
        Stitched = 2,
    }
    /// `insta360.messages.ExtraMetadata.SceneType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum SceneType {
        Unknow     = 0,
        UnderWater = 1,
    }
    /// `insta360.messages.ExtraMetadata.ReprocessingType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum ReprocessingType {
        Unknown = 0,
        Inside  = 1,
    }
    /// `insta360.messages.ExtraMetadata.OnlineFlowstateLevel`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum OnlineFlowstateLevel {
        Unknow   = 0,
        Standard = 1,
        High     = 2,
        Low      = 3,
        Off      = 4,
    }
    /// `insta360.messages.ExtraMetadata.FileSplitType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum FileSplitType {
        Unknow   = 0,
        NotSplit = 1,
        Splitted = 2,
    }
    /// `insta360.messages.ExtraMetadata.HdrModeType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum HdrModeType {
        HdrModeUnknow = 0,
        HdrModeOff    = 1,
        HdrModeOn     = 2,
    }
    /// `insta360.messages.ExtraMetadata.ContinueRecordInfo`
    #[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
    pub struct ContinueRecordInfo {
        #[prost(int64, tag="1")] pub first_frame_timestamp: i64,
        #[prost(int64, tag="2")] pub end_frame_timestamp: i64,
        #[prost(int64, tag="3")] pub first_gps_timestamp: i64,
        #[prost(int64, tag="4")] pub end_gps_timestamp: i64,
        #[prost(int64, tag="5")] pub total_frames: i64,
        #[serde(serialize_with="super::TimeScale_serializer")]
        #[prost(enumeration="TimeScale", tag="6")] pub time_scale: i32,
        #[prost(int32, tag="7")] pub submediatype: i32,
    }
    /// `insta360.messages.ExtraMetadata.GuardDetectedType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum GuardDetectedType {
        Unknow = 0,
        A      = 1,
        S      = 2,
        Off    = 3,
        AS     = 4,
        Nd16   = 5,
        Nd32   = 6,
        Nd64   = 7,
        Nd128  = 8,
    }
    /// `insta360.messages.ExtraMetadata.AISwitchState`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum AiSwitchState {
        AiSwitchUnknow = 0,
        AiSwitchOff    = 1,
        AiSwitchOn     = 2,
    }
    /// `insta360.messages.ExtraMetadata.Material3AInfo`
    #[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
    pub struct Material3AInfo {
        #[prost(float, tag="1")] pub ev: f32,
        #[prost(uint32, tag="2")] pub wb: u32,
        #[prost(uint32, tag="3")] pub iso: u32,
        #[prost(float, tag="4")] pub shutter: f32,
    }
    /// `insta360.messages.ExtraMetadata.ResolutiuonType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum ResolutiuonType {
        InsResUnknow = 0,
        InsResPlus   = 1,
    }
    /// `insta360.messages.ExtraMetadata.FolderInfo`
    #[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
    pub struct FolderInfo {
        #[serde(serialize_with="super::FolderType_serializer")]
        #[prost(enumeration="FolderType", tag="1")] pub r#type: i32,
        #[prost(uint32, tag="2")] pub index: u32,
        #[prost(string, tag="3")] pub identify: String,
        #[prost(uint32, tag="4")] pub max_num: u32,
    }
    /// `insta360.messages.ExtraMetadata.UnderWaterModeType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum UnderWaterModeType {
        UnderWaterModeUnknow = 0,
        UnderWaterModeOff    = 1,
        UnderWaterModeOn     = 2,
    }
    /// `insta360.messages.ExtraMetadata.WaterMarkType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum WaterMarkType {
        Unknow = 0,
        Off    = 1,
        On     = 2,
        PanoOn = 3,
        EisOn  = 4,
    }
    /// `insta360.messages.ExtraMetadata.AudioTrackInfo`
    #[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
    pub struct AudioTrackInfo {
        #[prost(int32, tag="1")] pub volume: i32,
        #[prost(int32, tag="2")] pub delay_time: i32,
        #[serde(serialize_with="super::AudioChannelMode_serializer")]
        #[prost(enumeration="AudioChannelMode", tag="3")] pub channel_mode: i32,
        #[serde(serialize_with="super::AudioFocusMode_serializer")]
        #[prost(enumeration="AudioFocusMode", tag="4")] pub focus_mode: i32,
    }
    /// `insta360.messages.ExtraMetadata.CloudVideoType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum CloudVideoType {
        Original      = 0,
        Agent         = 1,
        StitchedPano  = 2,
        Eacubemap     = 3,
        SingleFisheye = 4,
    }
    /// `insta360.messages.ExtraMetadata.ExtremeLowPowerModeType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum ExtremeLowPowerModeType {
        ExtremeLowpoaerModeUnknow = 0,
        ExtremeLowpoaerModeOff    = 1,
        ExtremeLowpoaerModeOn     = 2,
    }
    /// `insta360.messages.ExtraMetadata.LowLightStabilizationModeType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum LowLightStabilizationModeType {
        LowlightStabilizationModeUnknow             = 0,
        LowlightStabilizationModeJitterBlurRedution = 1,
        LowlightStabilizationModeAntiFlicker        = 2,
    }
    /// `insta360.messages.ExtraMetadata.LensModuleType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum LensModuleType {
        LensModuleUnkown   = 0,
        LensModuleShunyu   = 1,
        LensModuleHongjing = 2,
    }
    /// `insta360.messages.ExtraMetadata.FileCategory`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum FileCategory {
        Unknow                = 0,
        Plane                 = 1,
        DoubleFishEyePano     = 2,
        FishEye               = 3,
        WideAngle             = 4,
        DoubleHalfFishEyePano = 5,
        EquirectangularPano   = 6,
    }
    /// `insta360.messages.ExtraMetadata.FileRotate`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum FileRotate {
        FileRotateUnknow = 0,
        FileRotate0      = 1,
        FileRotate90     = 2,
        FileRotate180    = 3,
        FileRotate270    = 4,
    }
    /// `insta360.messages.ExtraMetadata.CodecType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum CodecType {
        VideoCodecTypeUnknow    = 0,
        VideoCodecTypeH264      = 1,
        VideoCodecTypeH265      = 2,
        VideoCodecTypeMjpeg     = 3,
        VideoCodecTypeH264Intra = 4,
        VideoCodecTypeH265Intra = 5,
    }
    /// `insta360.messages.ExtraMetadata.MCTFProcessType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum MctfProcessType {
        MctfProcessTypeUnknow    = 0,
        MctfProcessTypeOff       = 1,
        MctfProcessTypeAmba      = 2,
        MctfProcessTypeInsta     = 3,
        MctfProcessTypePurevideo = 4,
        MctfProcessTypeAmbaInsta = 5,
        MctfProcessTypeExternal  = 6,
    }
    /// `insta360.messages.ExtraMetadata.TimelapsStorageType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum TimelapsStorageType {
        Unknow     = 0,
        Gyro       = 1,
        Quaternion = 2,
    }
    /// `insta360.messages.ExtraMetadata.DirectionLockType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum DirectionLockType {
        Unknow    = 0,
        Off       = 1,
        Panofpvon = 2,
    }
    /// `insta360.messages.ExtraMetadata.DeWarpType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum DeWarpType {
        DewarpTypeUnknown = 0,
        DewarpTypeOff     = 1,
        DewarpTypeOn      = 2,
    }
    /// `insta360.messages.ExtraMetadata.UavCameraMode`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum UavCameraMode {
        UavCamModeRecord        = 0,
        UavCamModePhoto         = 1,
        UavCamModeTimelapse     = 2,
        UavCamModeHdrRecord     = 3,
        UavCamModeSlowmotion    = 4,
        UavCamModePhotoHdr      = 5,
        UavCamModePhotoBurst    = 6,
        UavCamModePhotoAeb      = 7,
        UavCamModePhotoInterval = 8,
        UavCamModePhotoStack    = 9,
        UavCamModePlayback      = 10,
    }
    /// `insta360.messages.ExtraMetadata.RemovePurpleFringing`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum RemovePurpleFringing {
        RemovrPurpleFringingNotSupport = 0,
        RemovrPurpleFringingInner      = 1,
        RemovrPurpleFringingExternal   = 2,
    }
    /// `insta360.messages.ExtraMetadata.CaptureRecommendStitchType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum CaptureRecommendStitchType {
        Unknown = 0,
        Ai2     = 1,
    }
    /// `insta360.messages.ExtraMetadata.IntervaRecTakeoverInfo`
    #[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
    pub struct IntervaRecTakeoverInfo {
        #[prost(uint64, tag="1")] pub start_timestamp: u64,
        #[prost(uint64, tag="2")] pub end_timestamp: u64,
    }
    /// `insta360.messages.ExtraMetadata.PrsestType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum PrsestType {
        PresetTypeBase        = 0,
        PresetTypeVlog        = 1,
        PresetTypeCycling     = 2,
        PresetTypeRunning     = 3,
        PresetTypeParentChild = 4,
        PresetTypeOutdoor     = 5,
        PresetTypeSkiing      = 6,
        PresetTypeSurfing     = 7,
        PresetTypeStarfield   = 8,
        PresetTypeDiy         = 9,
    }
    /// `insta360.messages.ExtraMetadata.FileSceneInfo`
    #[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
    pub struct FileSceneInfo {
        #[prost(uint32, tag="1")] pub r#type: u32,
        #[prost(string, tag="2")] pub identity: String,
        #[prost(uint64, tag="3")] pub caputre_timestamp: u64,
        #[serde(serialize_with="super::LiveSdkVersion_serializer")]
        #[prost(enumeration="LiveSdkVersion", tag="4")] pub live_sdk_version: i32,
        #[serde(serialize_with="super::LiveModeType_serializer")]
        #[prost(enumeration="LiveModeType", tag="5")] pub live_mode_type: i32,
        #[serde(serialize_with="super::LiveSceneType_serializer")]
        #[prost(enumeration="LiveSceneType", tag="6")] pub live_scene_type: i32,
        #[prost(uint32, tag="7")] pub select_frame_num: u32,
    }
    /// `insta360.messages.ExtraMetadata.EvMapLevel`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum EvMapLevel {
        EvmapLevelOff  = 0,
        EvmapLevelLow  = 1,
        EvmapLevelHigh = 2,
    }
    /// `insta360.messages.ExtraMetadata.SharpenState`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum SharpenState {
        SharpenExternalOff = 0,
        SharpenExternalOn  = 1,
    }
    /// `insta360.messages.ExtraMetadata.AeFlickerType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum AeFlickerType {
        AeflickerTypeUncomplete = 0,
        AeflickerTypeNone       = 1,
        AeflickerTypeUnknow     = 2,
        AeflickerType50Hz       = 3,
        AeflickerType60Hz       = 4,
    }
    /// `insta360.messages.ExtraMetadata.MagCalib`
    #[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
    pub struct MagCalib {
        #[prost(float, repeated, tag="1")] pub bias: Vec<f32>,
        #[prost(float, repeated, tag="2")] pub scale: Vec<f32>,
        #[prost(float, repeated, tag="3")] pub cross: Vec<f32>,
    }
    /// `insta360.messages.ExtraMetadata.MagQI`
    #[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
    pub struct MagQi {
        #[prost(float, repeated, tag="1")] pub q_i_mag: Vec<f32>,
    }
    /// `insta360.messages.ExtraMetadata.PropellerGuardStatusType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum PropellerGuardStatusType {
        PropellerGuardStatusNone       = 0,
        PropellerGuardStatusWear       = 1,
        PropellerGuardStatusWearLarge  = 2,
        PropellerGuardStatusWearDongle = 3,
    }
    /// `insta360.messages.ExtraMetadata.CameraCalibAdditionalInfo`
    #[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
    pub struct CameraCalibAdditionalInfo {
        #[prost(float, tag="1")] pub sensor0_temper: f32,
        #[prost(float, tag="2")] pub sensor1_temper: f32,
        #[prost(float, tag="3")] pub env_temper: f32,
    }
    /// `insta360.messages.ExtraMetadata.HighLightSource`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum HighLightSource {
        HighlightsourceNone   = 0,
        HighlightsourceSelfie = 1,
        HighlightsourceTm1    = 2,
        HighlightsourceGaze   = 3,
    }
    /// `insta360.messages.ExtraMetadata.ColorCorrectionType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum ColorCorrectionType {
        Unknow     = 0,
        None       = 1,
        StaticMap  = 2,
        DynamicMap = 3,
    }
    /// `insta360.messages.ExtraMetadata.CaptureImageQualityInfo`
    #[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
    pub struct CaptureImageQualityInfo {
        #[prost(int32, tag="1")] pub image_luminance_value_index: i32,
        #[prost(uint32, tag="2")] pub image_correlated_color_temperature: u32,
    }
    /// `insta360.messages.ExtraMetadata.LensAccessoryType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum LensAccessoryType {
        LensAccessoriesTypeUnkown                 = 0,
        LensAccessoriesTypeStandard               = 1,
        LensAccessoriesTypeBlackMistFilter        = 2,
        LensAccessoriesTypeWideAngleLens          = 3,
        LensAccessoriesTypeAdjustableMacroLens    = 4,
        LensAccessoriesTypeAnamorphicLens         = 5,
        LensAccessoriesTypeStarFilter             = 6,
        LensAccessoriesTypeNdFilter               = 7,
        LensAccessoriesTypeMiceoLens              = 8,
        LensAccessoriesTypeMiceoLens5             = 9,
        LensAccessoriesTypeMiceoLens11P6          = 10,
        LensAccessoriesTypeMiceoLens21P5          = 11,
        LensAccessoriesTypeMiceoLens38            = 12,
        LensAccessoriesTypeTelePhotoLens50Mm      = 13,
        LensAccessoriesTypeWideAngleLens180       = 14,
        LensAccessoriesTypeTelePhotoLens85Mm      = 15,
        LensAccessoriesTypeNdFilterNd16           = 16,
        LensAccessoriesTypeNdFilterNd32           = 17,
        LensAccessoriesTypeNdFilterNd64           = 18,
        LensAccessoriesTypeTelePhotoLens200Mm     = 19,
        LensAccessoriesTypeUvFilter               = 20,
        LensAccessoriesTypeDivingHousing          = 21,
        LensAccessoriesTypeWideAngleSoftFocus     = 22,
        LensAccessoriesTypeWideAngleNd16          = 23,
        LensAccessoriesTypeWideAngleNd32          = 24,
        LensAccessoriesTypeWideAngleNd64          = 25,
        LensAccessoriesTypeWideAngleDivingHousing = 26,
        LensAccessoriesTypeUvFilterSoftFocus      = 27,
        LensAccessoriesTypeUvFilterNd16           = 28,
        LensAccessoriesTypeUvFilterNd32           = 29,
        LensAccessoriesTypeUvFilterNd64           = 30,
        LensAccessoriesTypeUvFilterDivingHousing  = 31,
    }
    /// `insta360.messages.ExtraMetadata.Camera_Screen_Orientation`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum CameraScreenOrientation {
        CamScreenUnknown = 0,
        CamScreenVerti   = 1,
        CamScreenHoriz   = 2,
    }
    /// `insta360.messages.ExtraMetadata.CaptureFilpType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum CaptureFilpType {
        Null          = 0,
        V0HorzFlip    = 1,
        V180HorzFlip  = 2,
        PhotoHorzFlip = 3,
    }
    /// `insta360.messages.ExtraMetadata.ShootingAccessoriesType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum ShootingAccessoriesType {
        AccessoriesNone   = 0,
        AccessoriesStreet = 1,
    }
    /// `insta360.messages.ExtraMetadata.StreetPhotoHorizCorrection`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum StreetPhotoHorizCorrection {
        HorizCorrectionUnknown = 0,
        HorizCorrectionOff     = 1,
        HorizCorrectionOn      = 2,
    }
    /// `insta360.messages.ExtraMetadata.CameraOffsetCalib`
    #[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
    pub struct CameraOffsetCalib {
        #[prost(string, tag="1")] pub offset_v3_dual: String,
        #[prost(string, tag="2")] pub offset_v3_cam0: String,
        #[prost(string, tag="3")] pub offset_v3_cam1: String,
        #[prost(string, tag="4")] pub offset_v3_cam0_refined: String,
        #[prost(string, tag="5")] pub offset_v3_cam1_refined: String,
        #[prost(string, tag="6")] pub offset_v7_cam0_refined: String,
        #[prost(string, tag="7")] pub offset_v7_cam1_refined: String,
        #[prost(string, tag="8")] pub offset_v3_dual_refined: String,
        #[prost(string, tag="9")] pub offset_v7_dual_refined: String,
    }
    /// `insta360.messages.ExtraMetadata.WaterMarkStyle`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum WaterMarkStyle {
        WatermarkStyleUnknown       = 0,
        StreetPhotoWatermarkSimple  = 1,
        StreetPhotoWatermarkTexture = 2,
        StreetPhotoWatermarkVinyl   = 3,
    }
    /// `insta360.messages.ExtraMetadata.RepairedType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum RepairedType {
        Unknown            = 0,
        FileRepairedAssist = 1,
    }
    /// `insta360.messages.ExtraMetadata.VideoBitDepth`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum VideoBitDepth {
        VideoBitdepthUnknown = 0,
        VideoBitdepth8       = 1,
        VideoBitdepth10      = 2,
        VideoBitdepth12      = 3,
        VideoBitdepth16      = 4,
    }
    /// `insta360.messages.ExtraMetadata.VideoColorSpace`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum VideoColorSpace {
        VideoNamedColorSpaceUnknown       = 0,
        VideoNamedColorSpaceBt709         = 1,
        VideoNamedColorSpaceDisplayP3     = 2,
        VideoNamedColorSpaceBt2020Hlg     = 3,
        VideoNamedColorSpaceDolbyVision84 = 4,
    }
    /// `insta360.messages.ExtraMetadata.BigboomTracks`
    #[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
    pub struct BigboomTracks {
        #[prost(double, repeated, tag="1")] pub sfov: Vec<f64>,
        #[prost(double, repeated, tag="2")] pub attrs: Vec<f64>,
        #[prost(int32, repeated, tag="3")] pub attrs_index: Vec<i32>,
        #[prost(double, tag="4")] pub score: f64,
        #[prost(double, tag="5")] pub view_score: f64,
        #[prost(bool, tag="6")] pub is_asteroid: bool,
        #[prost(bool, tag="7")] pub best_view: bool,
        #[prost(int32, tag="8")] pub aspect: i32,
    }
    /// `insta360.messages.ExtraMetadata.AIGCInfo`
    #[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
    pub struct AigcInfo {
        #[prost(bool, tag="1")] pub is_aigc: bool,
        #[prost(string, tag="2")] pub aigc_token: String,
        #[prost(bool, tag="3")] pub is_aigc_hd: bool,
    }
    /// `insta360.messages.ExtraMetadata.ShootingParamInfo`
    #[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
    pub struct ShootingParamInfo {
        #[prost(uint32, tag="1")] pub contrast_level: u32,
        #[prost(uint32, tag="2")] pub saturation_level: u32,
        #[serde(serialize_with="super::SharpnessLevel_serializer")]
        #[prost(enumeration="shooting_param_info::SharpnessLevel", tag="3")] pub sharpness_level: i32,
        #[serde(serialize_with="super::MeteringMode_serializer")]
        #[prost(enumeration="shooting_param_info::MeteringMode", tag="4")] pub metering_mode: i32,
        #[serde(serialize_with="super::FocusMode_serializer")]
        #[prost(enumeration="shooting_param_info::FocusMode", tag="5")] pub focus_mode: i32,
        #[serde(serialize_with="super::FovcFlag_serializer")]
        #[prost(enumeration="shooting_param_info::FovcFlag", tag="6")] pub fovc_flag: i32,
        #[prost(string, tag="7")] pub lut_type: String,
        #[serde(serialize_with="super::ColorMode_serializer")]
        #[prost(enumeration="shooting_param_info::ColorMode", tag="8")] pub color_mode: i32,
        #[prost(uint32, tag="9")] pub lut_level: u32,
    }
    /// `insta360.messages.ExtraMetadata.ThumbnailHeroRotQuatData`
    #[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
    pub struct ThumbnailHeroRotQuatData {
        #[prost(message, optional, tag="1")] pub still_rot_quat: Option<ThumbnailRotQuat>,
        #[prost(message, optional, tag="2")] pub hero_offset_rot_quat: Option<ThumbnailRotQuat>,
        #[prost(int64, tag="3")] pub thumbnail_timestamp: i64,
    }
    /// `insta360.messages.ExtraMetadata.UavControlType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum UavControlType {
        Unknown = 0,
        Src     = 1,
        PacAir  = 2,
        Pac     = 3,
        Bvc     = 4,
    }
    /// `insta360.messages.ExtraMetadata.UavDisplayType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum UavDisplayType {
        Unknown = 0,
        Dgs     = 1,
        Dgs2    = 2,
        Pac     = 3,
        Bvc     = 4,
    }
    /// `insta360.messages.ExtraMetadata.AvSynthStatus`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum AvSynthStatus {
        AvSynthUnspecified  = 0,
        AvSynthNoTask       = 1,
        AvSynthSynthesizing = 2,
        AvSynthCompleted    = 3,
        AvSynthFailed       = 4,
    }
    /// `insta360.messages.ExtraMetadata.AudioConcertSceneType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum AudioConcertSceneType {
        AudioConcertSceneUnknown = 0,
        AudioConcertSceneStage   = 1,
        AudioConcertSceneVenue   = 2,
    }
    /// `insta360.messages.ExtraMetadata.AutoClipVersion`
    #[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
    pub struct AutoClipVersion {
        #[serde(serialize_with="super::AiPlatform_serializer")]
        #[prost(enumeration="AiPlatform", tag="1")] pub platform: i32,
        #[serde(serialize_with="super::AiAlgorithm_serializer")]
        #[prost(enumeration="AiAlgorithm", tag="2")] pub algorithm: i32,
        #[prost(message, optional, tag="3")] pub model_ver: Option<AiLibVersion>,
        #[prost(message, optional, tag="4")] pub svc_ver: Option<AiLibVersion>,
    }
    /// `insta360.messages.ExtraMetadata.AutoClipDataStorage`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum AutoClipDataStorage {
        StorageTypeNormal = 0,
        StorageTypeLoop   = 1,
    }
    /// `insta360.messages.ExtraMetadata.FolderType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum FolderType {
        FoldertypeUnknown       = 0,
        FoldertypeIntervalVideo = 1,
    }
    /// `insta360.messages.ExtraMetadata.AudioChannelMode`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum AudioChannelMode {
        Unknown = 0,
        Mono    = 1,
        Dual    = 2,
        Stereo  = 3,
    }
    /// `insta360.messages.ExtraMetadata.AudioFocusMode`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum AudioFocusMode {
        Unknown         = 0,
        Omnidirectional = 1,
        Forward         = 2,
        Backward        = 3,
        ForwardBackward = 4,
    }
    /// `insta360.messages.ExtraMetadata.LiveSDKVersion`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum LiveSdkVersion {
        LiveSdkVersionInvalid = 0,
        LiveSdkVersionNormal  = 1,
        LiveSdkVersionZ03V1   = 2,
    }
    /// `insta360.messages.ExtraMetadata.LiveModeType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum LiveModeType {
        Invalid = 0,
        Photo   = 1,
        Video   = 2,
    }
    /// `insta360.messages.ExtraMetadata.LiveSceneType`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum LiveSceneType {
        Invalid        = 0,
        FirstCapture   = 1,
        RaiseLowerHand = 2,
        DynamicFps     = 3,
    }
    /// `insta360.messages.ExtraMetadata.ThumbnailRotQuat`
    #[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
    pub struct ThumbnailRotQuat {
        #[prost(float, tag="1")] pub w: f32,
        #[prost(float, tag="2")] pub x: f32,
        #[prost(float, tag="3")] pub y: f32,
        #[prost(float, tag="4")] pub z: f32,
    }
    /// `insta360.messages.ExtraMetadata.AiPlatform`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum AiPlatform {
        Cv5  = 0,
        Cv22 = 1,
        H22  = 2,
    }
    /// `insta360.messages.ExtraMetadata.AiAlgorithm`
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
    #[repr(i32)]
    pub enum AiAlgorithm {
        Single = 0,
        Pano   = 1,
    }
    /// `insta360.messages.ExtraMetadata.AiLibVersion`
    #[derive(Clone, PartialEq, ::prost::Message, ::serde::Serialize)]
    pub struct AiLibVersion {
        #[prost(uint32, tag="1")] pub major: u32,
        #[prost(uint32, tag="2")] pub minor: u32,
        #[prost(uint32, tag="3")] pub patch: u32,
    }
    /// Nested message and enum types in `shooting_param_info`.
    pub mod shooting_param_info {
        /// `insta360.messages.ExtraMetadata.ShootingParamInfo.SharpnessLevel`
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
        #[repr(i32)]
        pub enum SharpnessLevel {
            Unknown = 0,
            Low     = 1,
            Medium  = 2,
            High    = 3,
        }
        /// `insta360.messages.ExtraMetadata.ShootingParamInfo.MeteringMode`
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
        #[repr(i32)]
        pub enum MeteringMode {
            Unknown = 0,
            Average = 1,
            Face    = 2,
        }
        /// `insta360.messages.ExtraMetadata.ShootingParamInfo.FocusMode`
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
        #[repr(i32)]
        pub enum FocusMode {
            Unknown    = 0,
            Manual     = 1,
            SingleAuto = 2,
            Macro      = 3,
            CafVideo   = 4,
            CafPicture = 5,
            Edof       = 6,
            Show       = 7,
        }
        /// `insta360.messages.ExtraMetadata.ShootingParamInfo.FovcFlag`
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
        #[repr(i32)]
        pub enum FovcFlag {
            Unknown = 0,
            Disable = 1,
            Enable  = 2,
        }
        /// `insta360.messages.ExtraMetadata.ShootingParamInfo.ColorMode`
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration, ::serde::Serialize)]
        #[repr(i32)]
        pub enum ColorMode {
            Unknown  = 0,
            Standard = 1,
            Ilog     = 2,
            Dolby    = 3,
        }
    }
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
enum_serializer!(PanoRecordType,            extra_metadata::PanoRecordType);
enum_serializer!(PanoRecordMultiTrackOrder, extra_metadata::PanoRecordMultiTrackOrder);
enum_serializer!(SubMediaType,              extra_metadata::SubMediaType);
enum_serializer!(LogoType,                  extra_metadata::extra_user_options::LogoType);
enum_serializer!(vec OffsetConvertState,    extra_metadata::extra_user_options::OffsetConvertState);
enum_serializer!(StreamType,                extra_metadata::StreamType);
enum_serializer!(OffsetState,               OffsetState);
enum_serializer!(CoolingShellSelectedState, extra_metadata::CoolingShellSelectedState);
enum_serializer!(CoolingShellAlgoDetectedState, extra_metadata::CoolingShellAlgoDetectedState);
enum_serializer!(OffsetVersion,             extra_metadata::OffsetVersion);

enum_serializer!(AeFlickerType,                     extra_metadata::AeFlickerType);
enum_serializer!(AiAlgorithm,                       extra_metadata::AiAlgorithm);
enum_serializer!(AiPlatform,                        extra_metadata::AiPlatform);
enum_serializer!(AiSwitchState,                     extra_metadata::AiSwitchState);
enum_serializer!(AudioChannelMode,                  extra_metadata::AudioChannelMode);
enum_serializer!(AudioConcertSceneType,             extra_metadata::AudioConcertSceneType);
enum_serializer!(AudioFocusMode,                    extra_metadata::AudioFocusMode);
enum_serializer!(AutoClipDataStorage,               extra_metadata::AutoClipDataStorage);
enum_serializer!(AvSynthStatus,                     extra_metadata::AvSynthStatus);
enum_serializer!(CameraScreenOrientation,           extra_metadata::CameraScreenOrientation);
enum_serializer!(CaptureFilpType,                   extra_metadata::CaptureFilpType);
enum_serializer!(CaptureRecommendStitchType,        extra_metadata::CaptureRecommendStitchType);
enum_serializer!(CloudVideoType,                    extra_metadata::CloudVideoType);
enum_serializer!(CodecType,                         extra_metadata::CodecType);
enum_serializer!(ColorCorrectionType,               extra_metadata::ColorCorrectionType);
enum_serializer!(ColorMode,                         extra_metadata::shooting_param_info::ColorMode);
enum_serializer!(DeWarpType,                        extra_metadata::DeWarpType);
enum_serializer!(DirectionLockType,                 extra_metadata::DirectionLockType);
enum_serializer!(EvMapLevel,                        extra_metadata::EvMapLevel);
enum_serializer!(ExtremeLowPowerModeType,           extra_metadata::ExtremeLowPowerModeType);
enum_serializer!(FileCategory,                      extra_metadata::FileCategory);
enum_serializer!(FileRotate,                        extra_metadata::FileRotate);
enum_serializer!(FileSplitType,                     extra_metadata::FileSplitType);
enum_serializer!(FocusMode,                         extra_metadata::shooting_param_info::FocusMode);
enum_serializer!(FolderType,                        extra_metadata::FolderType);
enum_serializer!(FovcFlag,                          extra_metadata::shooting_param_info::FovcFlag);
enum_serializer!(GuardDetectedType,                 extra_metadata::GuardDetectedType);
enum_serializer!(HdrModeType,                       extra_metadata::HdrModeType);
enum_serializer!(HdrProcessType,                    extra_metadata::HdrProcessType);
enum_serializer!(HighLightSource,                   extra_metadata::HighLightSource);
enum_serializer!(ImageStitchType,                   extra_metadata::ImageStitchType);
enum_serializer!(Lens283MType,                      extra_metadata::Lens283MType);
enum_serializer!(LensAccessoryType,                 extra_metadata::LensAccessoryType);
enum_serializer!(LensModuleType,                    extra_metadata::LensModuleType);
enum_serializer!(LiveModeType,                      extra_metadata::LiveModeType);
enum_serializer!(LiveSceneType,                     extra_metadata::LiveSceneType);
enum_serializer!(LiveSdkVersion,                    extra_metadata::LiveSdkVersion);
enum_serializer!(LowLightStabilizationModeType,     extra_metadata::LowLightStabilizationModeType);
enum_serializer!(MctfProcessType,                   extra_metadata::MctfProcessType);
enum_serializer!(MeteringMode,                      extra_metadata::shooting_param_info::MeteringMode);
enum_serializer!(OnlineFlowstateLevel,              extra_metadata::OnlineFlowstateLevel);
enum_serializer!(PropellerGuardStatusType,          extra_metadata::PropellerGuardStatusType);
enum_serializer!(PrsestType,                        extra_metadata::PrsestType);
enum_serializer!(PureShotProcessType,               extra_metadata::PureShotProcessType);
enum_serializer!(RemovePurpleFringing,              extra_metadata::RemovePurpleFringing);
enum_serializer!(RepairedType,                      extra_metadata::RepairedType);
enum_serializer!(ReprocessingType,                  extra_metadata::ReprocessingType);
enum_serializer!(ResolutiuonType,                   extra_metadata::ResolutiuonType);
enum_serializer!(SceneType,                         extra_metadata::SceneType);
enum_serializer!(SharpenState,                      extra_metadata::SharpenState);
enum_serializer!(SharpnessLevel,                    extra_metadata::shooting_param_info::SharpnessLevel);
enum_serializer!(ShootingAccessoriesType,           extra_metadata::ShootingAccessoriesType);
enum_serializer!(StreetPhotoHorizCorrection,        extra_metadata::StreetPhotoHorizCorrection);
enum_serializer!(ThumbnailStitchType,               extra_metadata::ThumbnailStitchType);
enum_serializer!(ThumbnailType,                     extra_metadata::ThumbnailType);
enum_serializer!(TimeScale,                         extra_metadata::TimeScale);
enum_serializer!(TimelapsStorageType,               extra_metadata::TimelapsStorageType);
enum_serializer!(UavCameraMode,                     extra_metadata::UavCameraMode);
enum_serializer!(UavControlType,                    extra_metadata::UavControlType);
enum_serializer!(UavDisplayType,                    extra_metadata::UavDisplayType);
enum_serializer!(UnderWaterModeType,                extra_metadata::UnderWaterModeType);
enum_serializer!(VideoBitDepth,                     extra_metadata::VideoBitDepth);
enum_serializer!(VideoColorSpace,                   extra_metadata::VideoColorSpace);
enum_serializer!(WaterMarkStyle,                    extra_metadata::WaterMarkStyle);
enum_serializer!(WaterMarkType,                     extra_metadata::WaterMarkType);

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