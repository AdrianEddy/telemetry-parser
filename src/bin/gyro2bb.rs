use std::time::Instant;
use argh::FromArgs;

use telemetry_parser::*;
use telemetry_parser::tags_impl::*;

/** gyro2bb v0.1.0
Author: Adrian <adrian.eddy@gmail.com>

Extract gyro data from Sony, GoPro and Insta360 cameras to betaflight blackbox csv log
*/
#[derive(FromArgs)]
struct Opts {
    /// input file
    #[argh(positional)]
    input: String,

    /// dump all metadata
    #[argh(switch, short = 'd')]
    dump: bool,

    /// IMU orientation (XYZ, ZXY etc, lowercase is negative, eg. xZy)
    #[argh(option)]
    imuo: Option<String>,
}

#[derive(Default)]
struct IMUData {
    timestamp: f64,
    gyro: Vector3<f64>,
    accl: Vector3<f64>
}

fn main() {
    let opts: Opts = argh::from_env();
    let _time = Instant::now();

    let mut stream = std::fs::File::open(&opts.input).unwrap();
    let filesize = stream.metadata().unwrap().len() as usize;

    let input = Input::from_stream(&mut stream, filesize).unwrap();

    let mut i = 0;
    let mut timestamp = 0f64;
    println!("Detected camera: {} {}", input.camera_type(), input.camera_model().unwrap_or(&"".into()));

    let samples = input.samples.as_ref().unwrap();

    let mut csv = String::with_capacity(2*1024*1024);
    csv.push_str(r#""loopIteration","time","gyroADC[0]","gyroADC[1]","gyroADC[2]","accSmooth[0]","accSmooth[1]","accSmooth[2]""#);
    csv.push('\n');

    for info in samples {
        if info.tag_map.is_none() { continue; }

        let mut final_data = Vec::<IMUData>::with_capacity(10000);

        let grouped_tag_map = info.tag_map.as_ref().unwrap();

        // Insta360
        let first_frame_ts = try_block!(f64, {
            let obj = (grouped_tag_map.get(&GroupId::Default)?.get_t(TagId::Metadata) as Option<&serde_json::Value>)?.as_object()?;
            
            println!("{}", serde_json::to_string_pretty(obj).unwrap_or_default());

            obj.get("first_frame_timestamp")?
               .as_i64()? as f64 / 1000.0
        }).unwrap_or_default();

        for (group, map) in grouped_tag_map {
            if opts.dump {
                for (tagid, taginfo) in map {
                    println!("{: <25} {: <25} {: <50}: {}", format!("{}", group), format!("{}", tagid), taginfo.description, &taginfo.value.to_string());
                }
            }

            if group == &GroupId::Gyroscope || group == &GroupId::Accelerometer {
                let raw2unit = crate::try_block!(f64, {
                    match &map.get(&TagId::Scale)?.value {
                        TagValue::i16(v) => *v.get() as f64,
                        TagValue::f32(v) => *v.get() as f64,
                        _ => 1.0
                    }
                }).unwrap_or(1.0);

                let unit2deg = crate::try_block!(f64, {
                    match (map.get_t(TagId::Unit) as Option<&String>)?.as_str() {
                        "rad/s" => 180.0 / std::f64::consts::PI, // rad to deg
                        _ => 1.0
                    }
                }).unwrap_or(1.0);

                let mut imu_orientation = match map.get_t(TagId::Orientation) as Option<&String> {
                    Some(v) => v.clone(),
                    None => "XYZ".into()
                };
                if let Some(imuo) = &opts.imuo {
                    imu_orientation = imuo.clone();
                }

                let io = imu_orientation.as_bytes();
                if let Some(taginfo) = map.get(&TagId::Data) {
                    match &taginfo.value {
                        TagValue::Vec_Vector3_i16(arr) => {
                            let arr = arr.get();
                            let reading_duration = info.duration_ms / arr.len() as f64;
        
                            let mut j = 0;
                            for v in arr {
                                if final_data.len() <= j {
                                    final_data.resize_with(j + 1, Default::default);
                                    final_data[j].timestamp = timestamp;
                                    timestamp += reading_duration;
                                }
                                let itm = v.clone().into_scaled(&raw2unit, &unit2deg).orient(io);
                                     if group == &GroupId::Gyroscope     { final_data[j].gyro = itm; }
                                else if group == &GroupId::Accelerometer { final_data[j].accl = itm; }
                                
                                j += 1;
                            }
                        }, 
                        TagValue::Vec_TimeVector3_f64(arr) => {
                            println!("IMU orientation: {}", imu_orientation);
                            let mut j = 0;
                            for v in arr.get() {
                                if v.t < first_frame_ts { continue; } // Skip gyro readings before actual first frame
                                if final_data.len() <= j {
                                    final_data.resize_with(j + 1, Default::default);
                                    final_data[j].timestamp = (v.t - first_frame_ts) * 1000.0;
                                }
                                let itm = v.clone().into_scaled(&raw2unit, &unit2deg).orient(io);
                                     if group == &GroupId::Gyroscope     { final_data[j].gyro = itm; }
                                else if group == &GroupId::Accelerometer { final_data[j].accl = itm; }

                                j += 1;
                            }
                        },
                        _ => ()
                    }
                }
            }
        }

        for v in final_data {
            csv.push_str(&format!("{},{:.0},{},{},{},{},{},{}\n", i, (v.timestamp * 1000.0).round(), 
                -v.gyro.z, v.gyro.y, v.gyro.x,
                -v.accl.z, v.accl.y, v.accl.x
            ));
            i += 1;
        }
    }
    std::fs::write(&format!("{}.csv", std::path::Path::new(&opts.input).to_path_buf().to_string_lossy()), csv).unwrap();
    println!("Done in {:.3} ms", _time.elapsed().as_micros() as f64 / 1000.0);
}
