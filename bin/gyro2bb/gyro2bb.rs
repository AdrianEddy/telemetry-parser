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

    if opts.dump {
        for info in samples {
            if info.tag_map.is_none() { continue; }
            let grouped_tag_map = info.tag_map.as_ref().unwrap();
    
            for (group, map) in grouped_tag_map {
                for (tagid, taginfo) in map {
                    println!("{: <25} {: <25} {: <50}: {}", format!("{}", group), format!("{}", tagid), taginfo.description, &taginfo.value.to_string());
                }
            }
        }
    }

    let imu_data = util::normalized_imu(&samples, opts.imuo).unwrap();

    let mut csv = String::with_capacity(2*1024*1024);
    csv.push_str(r#""loopIteration","time","gyroADC[0]","gyroADC[1]","gyroADC[2]","accSmooth[0]","accSmooth[1]","accSmooth[2]""#);
    csv.push('\n');
    for v in imu_data {
        csv.push_str(&format!("{},{:.0},{},{},{},{},{},{}\n", i, (v.timestamp * 1000.0).round(), 
            -v.gyro.z, v.gyro.y, v.gyro.x,
            -v.accl.z, v.accl.y, v.accl.x
        ));
        i += 1;
    }
    std::fs::write(&format!("{}.csv", std::path::Path::new(&opts.input).to_path_buf().to_string_lossy()), csv).unwrap();

    println!("Done in {:.3} ms", _time.elapsed().as_micros() as f64 / 1000.0);
}