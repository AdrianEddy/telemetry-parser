// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2026 Adrian <adrian.eddy at gmail>
//
// gyroflow_proto_convert — convert Gyroflow protobuf telemetry between its
// representations. All three carry exactly the same `gyroflow.proto::Main`
// messages:
//
//   * binary — one length-free protobuf per frame, as frame_NNNNNN.bin in a folder
//   * JSONL  — one message per line in the proto3 canonical JSON mapping
//   * MP4    — the same binary messages in the video's metadata track
//
// MP4 is read-only here: extracting needs nothing but the mp4 parser, while
// writing one needs libavformat, so injecting is `gyroflow_proto_inject`.

use std::io::Write;
use std::path::{ Path, PathBuf };
use std::sync::{ Arc, atomic::AtomicBool };

use argh::FromArgs;
use prost::Message;
use telemetry_parser::gyroflow::gyroflow_proto as proto;
use telemetry_parser::gyroflow::gyroflow_proto_old;
use telemetry_parser::gyroflow::proto_json;

/** gyroflow_proto_convert

Convert Gyroflow protobuf telemetry between per-frame binary files, a JSONL
stream, and an MP4's metadata track. The output form follows the input, except
for an MP4 — which both other forms are opposite to, so there it follows the
--output extension instead: .jsonl for JSONL, anything else for a folder of
.bin files.
*/
#[derive(FromArgs)]
struct Opts {
    /// input: an .mp4 / .mov carrying the telemetry track, a .jsonl file, a
    /// folder of frame_NNNNNN.bin files, or a single .bin
    #[argh(positional)]
    input: String,

    /// output: the .jsonl file to write, or the folder to write .bin files into
    /// (created if missing). Defaults to <input>.jsonl or <input>_bin/.
    #[argh(option, short = 'o')]
    output: Option<String>,
}

/// Which representation to write.
#[derive(PartialEq)]
enum Form { Jsonl, Binary }

fn main() {
    let opts: Opts = argh::from_env();

    let _ = simplelog::TermLogger::init(
        simplelog::LevelFilter::Info,
        simplelog::Config::default(),
        simplelog::TerminalMode::Mixed,
        simplelog::ColorChoice::Auto,
    );

    let input = PathBuf::from(&opts.input);
    if !input.exists() {
        log::error!("{} does not exist", input.display());
        std::process::exit(1);
    }
    let output = opts.output.map(PathBuf::from);

    // A .jsonl converts to binary and anything else to JSONL — except an MP4,
    // which is opposite to both, so its output form comes from --output.
    let form = if has_extension(&input, &["mp4", "mov"]) {
        match output.as_deref() {
            Some(out) if !has_extension(out, &["jsonl"]) => Form::Binary,
            _ => Form::Jsonl,
        }
    } else if input.is_file() && has_extension(&input, &["jsonl"]) {
        Form::Binary
    } else {
        Form::Jsonl
    };

    let out = output.unwrap_or_else(|| match form {
        Form::Jsonl  => input.with_file_name(format!("{}.jsonl", stem(&input))),
        Form::Binary => input.with_file_name(format!("{}_bin", stem(&input))),
    });

    if let Err(e) = convert(&input, &out, form) {
        log::error!("{e}");
        std::process::exit(2);
    }
}

fn stem(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "telemetry".into())
}

fn has_extension(path: &Path, wanted: &[&str]) -> bool {
    path.extension().is_some_and(|e| wanted.iter().any(|w| e.eq_ignore_ascii_case(w)))
}

fn convert(input: &Path, output: &Path, form: Form) -> std::io::Result<()> {
    let messages = load(input)?;
    if messages.is_empty() {
        return Err(std::io::Error::other(format!("no Gyroflow protobuf messages found in {}", input.display())));
    }

    match form {
        Form::Jsonl => {
            let mut out = std::io::BufWriter::new(std::fs::File::create(output)?);
            for main in &messages {
                writeln!(out, "{}", proto_json::to_jsonl_line(main))?;
            }
            out.flush()?;
            log::info!("Wrote {} JSONL messages to {}", messages.len(), output.display());
        }
        Form::Binary => {
            std::fs::create_dir_all(output)?;
            for (i, main) in messages.iter().enumerate() {
                let mut buf = Vec::with_capacity(main.encoded_len());
                main.encode(&mut buf).map_err(std::io::Error::other)?;
                std::fs::write(output.join(format!("frame_{:06}.bin", i + 1)), &buf)?;
            }
            log::info!("Wrote {} binary protobuf files to {}", messages.len(), output.display());
        }
    }
    Ok(())
}

/// Reads the messages out of whichever representation was handed to us.
fn load(input: &Path) -> std::io::Result<Vec<proto::Main>> {
    if has_extension(input, &["mp4", "mov"]) { return load_mp4(input); }

    if input.is_file() && has_extension(input, &["jsonl"]) {
        let reader = std::io::BufReader::with_capacity(256 * 1024, std::fs::File::open(input)?);
        return Ok(proto_json::MessageReader::new(reader).collect());
    }

    // `frame_NNNNNN.bin` is zero-padded, so a lexicographic sort is frame order.
    let mut files: Vec<PathBuf> = if input.is_file() {
        vec![input.to_path_buf()]
    } else {
        std::fs::read_dir(input)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_file() && has_extension(p, &["bin"]))
            .collect()
    };
    files.sort();

    let mut out = Vec::with_capacity(files.len());
    for file in &files {
        match decode(&std::fs::read(file)?) {
            Some(main) => out.push(main),
            None => log::warn!("Skipping {}: not a Gyroflow protobuf message", file.display()),
        }
    }
    Ok(out)
}

/// Reads the telemetry track of an MP4. Only the mp4 parser is involved — the
/// samples in that track are already the binary messages we want.
fn load_mp4(input: &Path) -> std::io::Result<Vec<proto::Main>> {
    let mut stream = std::fs::File::open(input)?;
    let size = stream.metadata()?.len() as usize;

    let mut out = Vec::new();
    let mut skipped = 0usize;
    telemetry_parser::util::get_metadata_track_samples(
        &mut stream, size, false,
        |_info, data: &[u8], _pos, _video_md| {
            match decode(data) {
                Some(main) => out.push(main),
                None => skipped += 1,
            }
        },
        Arc::new(AtomicBool::new(false)),
    )?;

    if skipped > 0 {
        log::warn!("Skipped {skipped} metadata samples that aren't Gyroflow protobuf messages");
    }
    Ok(out)
}

/// Decodes one message, accepting the pre-v1 schema and converting it forward so
/// everything downstream only ever sees the current one.
fn decode(data: &[u8]) -> Option<proto::Main> {
    proto::Main::decode(data).ok()
        .or_else(|| gyroflow_proto_old::Main::decode(data).ok().map(Into::into))
}
