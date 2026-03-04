use std::error::Error;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn Error>> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dji_dir = manifest_dir.join("..").join("..").join("src").join("dji");
    let proto = dji_dir.join("dvtm_ow001.proto");

    let mut config = prost_build::Config::new();
    config.out_dir(&dji_dir);
    config.compile_protos(&[proto], &[dji_dir])?;
    Ok(())
}
