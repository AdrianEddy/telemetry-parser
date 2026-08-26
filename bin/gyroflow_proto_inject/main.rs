// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2026 Adrian <adrian.eddy at gmail>
//
// gyroflow_proto_inject — mux Gyroflow protobuf telemetry into an MP4 as a
// `data` stream, tagged so that telemetry-parser's
// `gyroflow::binary::GyroflowProtobuf` detects and parses it back.
//
// The telemetry may be given in either representation — a .jsonl stream or a
// folder of per-frame .bin protobufs — since both carry the same
// `gyroflow.proto::Main` messages. Source video is stream-copied; nothing is
// re-encoded.

use std::collections::HashMap;
use std::path::{ Path, PathBuf };

use argh::FromArgs;
use prost::Message;
use telemetry_parser::gyroflow::gyroflow_proto as proto;
use telemetry_parser::gyroflow::proto_json;

use ffmpeg_next::{ format, media, Packet, Rational };

/** gyroflow_proto_inject

Mux Gyroflow protobuf telemetry into an MP4 metadata track. Telemetry is either
a .jsonl stream or a folder of per-frame .bin protobufs; the video is
stream-copied from --video.
*/
#[derive(FromArgs)]
struct Opts {
    /// telemetry to inject: a .jsonl file, or a folder of frame_NNNNNN.bin files
    #[argh(positional)]
    telemetry: String,

    /// source MP4 / MOV whose video stream is copied into the output
    #[argh(option, short = 'v')]
    video: String,

    /// output MP4 path
    #[argh(option, short = 'o')]
    output: String,
}

fn main() {
    let opts: Opts = argh::from_env();

    let _ = simplelog::TermLogger::init(
        simplelog::LevelFilter::Info,
        simplelog::Config::default(),
        simplelog::TerminalMode::Mixed,
        simplelog::ColorChoice::Auto,
    );

    let telemetry = PathBuf::from(&opts.telemetry);
    let messages = match load_messages(&telemetry) {
        Ok(m) => m,
        Err(e) => { log::error!("{e}"); std::process::exit(1); }
    };
    if messages.is_empty() {
        log::error!("no Gyroflow protobuf messages found in {}", telemetry.display());
        std::process::exit(1);
    }

    let (payloads, frame_pts_us, frame_dur_us) = match build_packets(&messages) {
        Ok(x) => x,
        Err(e) => { log::error!("{e}"); std::process::exit(1); }
    };
    log::info!("Injecting {} messages into {}", payloads.len(), opts.output);

    match mux_with_ffmpeg_next(
        Path::new(&opts.video),
        Path::new(&opts.output),
        &payloads,
        &frame_pts_us,
        &frame_dur_us,
    ) {
        Ok(()) => log::info!("Wrote muxed output to {}", opts.output),
        Err(e) => {
            log::error!("ffmpeg mux failed: {e}");
            std::process::exit(2);
        }
    }
}

/// Reads the messages from whichever representation was handed to us.
fn load_messages(path: &Path) -> std::io::Result<Vec<proto::Main>> {
    if path.is_file() && path.extension().is_some_and(|e| e.eq_ignore_ascii_case("jsonl")) {
        let reader = std::io::BufReader::with_capacity(256 * 1024, std::fs::File::open(path)?);
        return Ok(proto_json::MessageReader::new(reader).collect());
    }

    // `frame_NNNNNN.bin` is zero-padded, so a lexicographic sort is frame order.
    let mut files: Vec<PathBuf> = if path.is_file() {
        vec![path.to_path_buf()]
    } else {
        std::fs::read_dir(path)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("bin")))
            .collect()
    };
    files.sort();

    let mut out = Vec::with_capacity(files.len());
    for file in &files {
        let buf = std::fs::read(file)?;
        match proto::Main::decode(buf.as_slice()) {
            Ok(main) => out.push(main),
            Err(e) => log::warn!("Skipping {}: not a Gyroflow protobuf message: {e}", file.display()),
        }
    }
    Ok(out)
}

/// Encodes each message and gives it the presentation time the video frame it
/// describes will have.
///
/// The metadata track's timeline has to be the *video's*, not the camera clock
/// the messages carry: the reader recovers the offset between the two as
/// `FirstFrameTimestamp = start_timestamp_us - packet PTS`, which is what
/// gyroflow adds back onto a video timestamp to land on the camera clock. So the
/// PTS here is the ideal CFR position of the frame — frame index over the header
/// frame rate — and each frame's own clock jitter survives in the difference,
/// exactly as it does when the same messages are read from JSONL.
fn build_packets(messages: &[proto::Main]) -> std::io::Result<(Vec<Vec<u8>>, Vec<i64>, Vec<i64>)> {
    // Same frame rate and same frame-index derivation the JSONL reader uses, so
    // a clip muxed here and the JSONL it came from land on identical timestamps.
    let clip = messages.iter().find_map(|m| m.header.as_ref()?.clip.as_ref());
    let fps = telemetry_parser::gyroflow::file_frame_rate(clip);
    if fps <= 0.0 {
        return Err(std::io::Error::other(
            "the telemetry header carries no frame rate, so per-frame presentation times can't be derived"
        ));
    }

    let mut payloads = Vec::with_capacity(messages.len());
    let mut pts_us   = Vec::with_capacity(messages.len());
    let mut dur_us   = Vec::with_capacity(messages.len());

    let mut frame_index = telemetry_parser::gyroflow::FrameIndex::default();
    let mut pending_header: Option<proto::Header> = None;

    for main in messages {
        // A header-only message — the natural first line of a JSONL stream — has
        // no frame of its own to be timed against. Carry its header onto the next
        // message that does have one, which is where the binary stream puts it,
        // rather than emitting a packet with no frame.
        let Some(ref frame) = main.frame else {
            if main.header.is_some() { pending_header = main.header.clone(); }
            continue;
        };
        let index = frame_index.index_of(frame) as f64;

        let mut buf = Vec::with_capacity(main.encoded_len());
        match pending_header.take() {
            // Clones at most once per stream, on the frame that adopts the header.
            Some(header) if main.header.is_none() => {
                let mut main = main.clone();
                main.header = Some(header);
                main.encode(&mut buf).map_err(std::io::Error::other)?;
            }
            _ => main.encode(&mut buf).map_err(std::io::Error::other)?,
        }
        payloads.push(buf);
        pts_us.push((index * 1_000_000.0 / fps).round() as i64);
        dur_us.push((1_000_000.0 / fps).round() as i64);
    }

    Ok((payloads, pts_us, dur_us))
}

// ---------------------------------------------------------------------------
// libavformat (ffmpeg-next) mux
// ---------------------------------------------------------------------------

/// Programmatic mux. Stream-copies every video & audio track from the source
/// MP4 and adds one `data` stream that carries our per-frame protobufs as
/// timestamped packets. We can't go through the ffmpeg CLI's concat demuxer
/// because it requires format-recognized inputs (raw protobuf bytes aren't
/// recognized — that's what the user hit with "Impossible to open
/// frame_000001.bin"). With libavformat we just create a `data` stream
/// (codec_type=AVMEDIA_TYPE_DATA, codec_id=AV_CODEC_ID_BIN_DATA) and feed
/// each frame as its own AVPacket with explicit PTS / DTS / duration.
///
/// Implementation closely mirrors the proven C++ reference at
/// https://github.com/5337-tyree/my-test-protobuf-convert: we mutate each
/// stream's `codecpar` in place via raw FFI (the wrapper's
/// `set_parameters` ↔ `parameters().as_mut_ptr()` round-trip went through
/// avcodec_parameters_copy with subtle wrapper-ownership semantics that
/// were producing an stsd with no sample entry — "invalid size 0 in stsd"
/// at demux). We also leave `codec_tag` unset on the data stream and let
/// the MOV muxer auto-resolve it via `ff_codec_movdata_tags` (which maps
/// AV_CODEC_ID_BIN_DATA → 'gpmd', dispatching to mov_write_gpmd_tag and
/// producing a valid 17-byte SampleEntry).
fn mux_with_ffmpeg_next(
    input_path: &Path,
    output_path: &Path,
    payloads: &[Vec<u8>],
    frame_pts_us: &[i64],
    frame_dur_us: &[i64],
) -> Result<(), ffmpeg_next::Error> {
    use ffmpeg_next::ffi;

    ffmpeg_next::init()?;

    let mut ictx = format::input(&input_path.to_path_buf())?;
    let mut octx = format::output(&output_path.to_path_buf())?;

    // ---- Map the source's video stream to the output ---------------------
    // We deliberately stream-copy ONLY video, mirroring the proven C++
    // reference at https://github.com/5337-tyree/my-test-protobuf-convert.
    // Audio is skipped because:
    //   * In ffmpeg 8.x the MP4 muxer rejects audio streams whose
    //     `ch_layout.order` is UNSPEC (the demuxer often produces this for
    //     AAC sourced from Sony MP4s), with "unsupported channel layout 2
    //     channels". Working around it requires per-codec layout fixups
    //     and isn't germane to this tool's goal: emit a valid MP4 carrying
    //     the gyroflow protobuf telemetry track for round-trip testing.
    //   * The point of this tool is gyro/IMU round-trip; if the user wants
    //     audio in the output they can mux it in separately.
    let mut stream_map: HashMap<usize, usize> = HashMap::new();
    let in_stream_descs: Vec<(usize, *const ffi::AVCodecParameters, ffi::AVRational, media::Type)> = ictx
        .streams()
        .map(|s| unsafe {
            let par = (*s.as_ptr()).codecpar as *const ffi::AVCodecParameters;
            let tb  = (*s.as_ptr()).time_base;
            (s.index(), par, tb, s.parameters().medium())
        })
        .collect();
    for (src_idx, src_par, src_tb, medium) in &in_stream_descs {
        if *medium != media::Type::Video { continue; }
        let dst_idx = unsafe {
            let octx_ptr = octx.as_mut_ptr();
            let new_stream = ffi::avformat_new_stream(octx_ptr, std::ptr::null());
            if new_stream.is_null() {
                return Err(ffmpeg_next::Error::Unknown);
            }
            let ret = ffi::avcodec_parameters_copy((*new_stream).codecpar, *src_par);
            if ret < 0 { return Err(ffmpeg_next::Error::from(ret)); }
            (*(*new_stream).codecpar).codec_tag = 0;
            (*new_stream).time_base = *src_tb;
            (*new_stream).id = (*octx_ptr).nb_streams as i32 - 1;
            (*new_stream).index as usize
        };
        stream_map.insert(*src_idx, dst_idx);
    }
    drop(in_stream_descs);

    // ---- Add the protobuf metadata `data` stream --------------------------
    // Time base 1/1_000_000 → PTS values are microseconds, matching the
    // values we computed in Phase 1 from `info.timestamp_ms * 1000`.
    //
    // We do NOT set codec_tag explicitly. The MOV muxer's
    // `ff_codec_movdata_tags` table maps AV_CODEC_ID_BIN_DATA → 'gpmd',
    // and `mov_init` writes that into `par->codec_tag` during write_header.
    // Then `mov_write_stsd_tag` dispatches on codec_tag=='gpmd' to
    // `mov_write_gpmd_tag` which emits the proper 17-byte SampleEntry.
    // (Setting codec_tag manually here ALSO works in principle, but the
    // C++ reference at 5337-tyree/my-test-protobuf-convert proves the
    // implicit path produces a valid file, so we use it.)
    //
    // Setting handler_name="GyroflowTelemetry" matches the reference and
    // makes the track easier to identify in `ffprobe`.
    let metadata_idx = unsafe {
        let octx_ptr = octx.as_mut_ptr();
        let new_stream = ffi::avformat_new_stream(octx_ptr, std::ptr::null());
        if new_stream.is_null() {
            return Err(ffmpeg_next::Error::Unknown);
        }
        let codecpar = (*new_stream).codecpar;
        (*codecpar).codec_type = ffi::AVMediaType::AVMEDIA_TYPE_DATA;
        (*codecpar).codec_id   = ffi::AVCodecID::AV_CODEC_ID_BIN_DATA;
        // codec_tag intentionally left 0 — see comment above.
        (*new_stream).time_base = ffi::AVRational { num: 1, den: 1_000_000 };
        (*new_stream).id        = (*octx_ptr).nb_streams as i32 - 1;

        let key  = std::ffi::CString::new("handler_name").unwrap();
        let name = std::ffi::CString::new("GyroflowTelemetry").unwrap();
        ffi::av_dict_set(
            &mut (*new_stream).metadata,
            key.as_ptr(),
            name.as_ptr(),
            0,
        );

        (*new_stream).index as usize
    };

    octx.write_header()?;

    // Snapshot the per-output-stream timebases up front so we don't need to
    // borrow octx immutably inside the packet loop (which holds an
    // implicit &mut on octx via write_interleaved).
    let out_tb: HashMap<usize, Rational> = octx.streams()
        .map(|s| (s.index(), s.time_base()))
        .collect();
    let metadata_tb = out_tb.get(&metadata_idx).copied()
        .unwrap_or(Rational::new(1, 1_000_000));
    let metadata_src_tb = Rational::new(1, 1_000_000);

    // Each message → one AVPacket. PTS/DTS/duration are computed in source
    // microseconds, then rescaled to whatever time_base the muxer actually wrote
    // into the data stream (MOV may rewrite the timescale at write_header time,
    // so trusting our pre-header 1_000_000 is unsafe).
    let write_metadata = |octx: &mut format::context::Output, i: usize| -> Result<(), ffmpeg_next::Error> {
        let mut pkt = Packet::copy(&payloads[i]);
        pkt.set_stream(metadata_idx);
        pkt.set_pts(Some(frame_pts_us[i]));
        pkt.set_dts(Some(frame_pts_us[i]));
        pkt.set_duration(frame_dur_us[i]);
        pkt.set_position(-1);
        pkt.set_flags(ffmpeg_next::packet::Flags::KEY);
        pkt.rescale_ts(metadata_src_tb, metadata_tb);
        pkt.write_interleaved(octx)
    };

    // ---- Copy source packets, merging the metadata in by timestamp ----
    // Writing every video packet and only then the metadata would hand the
    // interleaver a whole stream that is already in the past, and it would land
    // the telemetry wherever it happened to flush — in a long clip, more than
    // half way into the file. Emitting each metadata packet when the video
    // reaches its timestamp keeps the samples in timeline order, which is both
    // what demuxers expect and what keeps the telemetry near the start of the
    // file, where format detection looks for it.
    let mut next_metadata = 0usize;
    for (in_stream, mut packet) in ictx.packets() {
        if let Some(&dst_idx) = stream_map.get(&in_stream.index()) {
            let in_tb  = in_stream.time_base();
            let dst_tb = out_tb.get(&dst_idx).copied().unwrap_or(in_tb);

            // A packet with no timestamp at all says nothing about where we are
            // on the timeline, so it advances the metadata stream by nothing —
            // treating it as +∞ would flush every remaining telemetry packet at
            // once, which is exactly the lump this interleaving exists to avoid.
            let packet_us = packet.dts().or_else(|| packet.pts()).map(|t| {
                t as f64 * in_tb.numerator() as f64 / in_tb.denominator() as f64 * 1_000_000.0
            });
            if let Some(packet_us) = packet_us {
                while next_metadata < payloads.len() && (frame_pts_us[next_metadata] as f64) <= packet_us {
                    write_metadata(&mut octx, next_metadata)?;
                    next_metadata += 1;
                }
            }

            packet.rescale_ts(in_tb, dst_tb);
            packet.set_stream(dst_idx);
            packet.set_position(-1);
            packet.write_interleaved(&mut octx)?;
        }
    }
    // Anything past the end of the video — a telemetry stream longer than the
    // clip it was paired with.
    while next_metadata < payloads.len() {
        write_metadata(&mut octx, next_metadata)?;
        next_metadata += 1;
    }

    octx.write_trailer()?;
    Ok(())
}
