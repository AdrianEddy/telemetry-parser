use std::error::Error;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn Error>> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dji_dir = manifest_dir.join("..").join("..").join("src").join("dji");

    let args: Vec<String> = std::env::args().skip(1).collect();
    let protos: Vec<PathBuf> = if args.is_empty() {
        vec![dji_dir.join("dvtm_ow001.proto")]
    } else {
        args.iter().map(|p| dji_dir.join(p)).collect()
    };

    let mut config = prost_build::Config::new();
    config.out_dir(&dji_dir);
    config.compile_protos(&protos, &[dji_dir])?;
    Ok(())
}
