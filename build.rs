use std::io::Result;
fn main() -> Result<()> {
    // prost_build::compile_protos(&["src/dji/dvtm_oq101.proto"], &["src/dji/"])?;
    // prost_build::compile_protos(&["src/dji/dvtm_wm169.proto"], &["src/dji/"])?;
    // prost_build::compile_protos(&["src/dji/dvtm_eagle4_wa530.proto"], &["src/dji/"])?;
    // prost_build::Config::new()
    //     .type_attribute(".", "#[derive(::serde::Serialize, ::serde::Deserialize)]")
    //     .message_attribute(".", "#[serde(default)]")
    //     .compile_protos(&["src/gyroflow/gyroflow.proto"], &["src/gyroflow/"])?;
    Ok(())
}