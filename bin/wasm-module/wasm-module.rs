// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2021 Adrian <adrian.eddy at gmail>

// The parser for the browser. A file is read either from memory (`new Parser(bytes, name)`) or streamed
// (`Parser.from_reader(size, name, read)`): `read(offset, length)` returns those bytes of the file as an `Uint8Array`,
// synchronously, which a Web Worker can do for a `File` of any size with `FileReaderSync` and `Blob.slice`. The parsers
// only seek and read the parts of a file they need, so a 100 GB video costs the reads of its boxes and its metadata
// samples, not of the whole file.

use wasm_bindgen::prelude::*;
use js_sys::{ Array, Float32Array, Float64Array, Function, Object, Reflect, Uint32Array, Uint8Array };
use serde::Serialize;
use std::cell::{ Cell, RefCell };
use std::collections::{ BTreeMap, HashMap };
use std::io::{ Error, ErrorKind, Read, Seek, SeekFrom };
use std::sync::{ Arc, atomic::AtomicBool };

use telemetry_parser::*;
use telemetry_parser::tags_impl::*;

// ---------------------------------------------------------------------------------------------------------------------
// Logging and panics: the parsers report through `log`, which has nowhere to go in a browser. Warnings and errors are
// kept for the page to show (`take_log`), and a panic's message is kept for the call that trapped (`take_panic`)

thread_local! {
    static LOG: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
    static PANIC: RefCell<Option<String>> = const { RefCell::new(None) };
}
const MAX_LOG_LINES: usize = 2000;

struct Logger;
impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool { metadata.level() <= log::Level::Warn }
    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) { return; }
        let _ = LOG.try_with(|l| if let Ok(mut l) = l.try_borrow_mut() {
            if l.len() < MAX_LOG_LINES {
                l.push((record.level().as_str().to_ascii_lowercase(), record.args().to_string()));
            }
        });
    }
    fn flush(&self) { }
}
static LOGGER: Logger = Logger;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console, js_name = error)]
    fn console_error(s: &str);
}

#[wasm_bindgen(start)]
pub fn start() {
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Warn);
    }
    std::panic::set_hook(Box::new(|info| {
        let msg = info.to_string();
        console_error(&msg);
        let _ = PANIC.try_with(|p| if let Ok(mut p) = p.try_borrow_mut() { *p = Some(msg); });
    }));
    // The parsers' files are the page's (`set_files`)
    unsafe {
        filesystem::set_filesystem_functions(filesystem::FilesystemFunctions {
            get_filename: vfs_filename,
            get_folder: vfs_folder,
            list_folder: vfs_list,
            open_file: vfs_open,
        });
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The files the page gave (`set_files`), as the parsers' filesystem (`telemetry_parser::filesystem`): a clip can be more
// than one file, which its parser opens by name next to the one it was given (a RED clip's parts and RMD sidecar, a
// CinemaDNG sequence's frames, a video's motion log). Each is streamed as the file given is (`JsReader`)

thread_local! {
    static FILES: RefCell<BTreeMap<String, (Function, u64)>> = const { RefCell::new(BTreeMap::new()) };
    /// The reads from JavaScript since the file was opened, of all its files: calls and bytes
    static READS: Cell<(u64, u64)> = const { Cell::new((0, 0)) };
}

/// The files a parser may open, `[{ path, size, read }]`: `read(offset, length)` returns those bytes of the file as an
/// `Uint8Array`. Paths are `/`-separated; a folder's files are the ones whose path is in it
#[wasm_bindgen]
pub fn set_files(files: Array) -> Result<(), JsValue> {
    let mut map = BTreeMap::new();
    for item in files.iter() {
        let field = |k: &str| Reflect::get(&item, &JsValue::from_str(k));
        let path = field("path")?.as_string().ok_or_else(|| JsValue::from(js_sys::Error::new("A file without a path")))?;
        let size = field("size")?.as_f64().filter(|s| *s >= 0.0).ok_or_else(|| JsValue::from(js_sys::Error::new("A file without a size")))? as u64;
        let read = field("read")?.dyn_into::<Function>()?;
        map.insert(path, (read, size));
    }
    FILES.with(|f| *f.borrow_mut() = map);
    Ok(())
}

fn vfs_filename(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_owned()
}
fn vfs_folder(path: &str) -> String {
    if path.ends_with(['/', '\\']) { return path.to_owned(); }
    path.rfind(['/', '\\']).map_or(String::new(), |p| path[..p].to_owned())
}
fn vfs_list(folder: &str) -> Vec<(String, String)> {
    let folder = folder.trim_end_matches(['/', '\\']);
    FILES.with(|f| f.borrow().keys().filter(|p| vfs_folder(p) == folder).map(|p| (vfs_filename(p), p.clone())).collect())
}
fn vfs_open(path: &str) -> std::io::Result<filesystem::FileWrapper> {
    let (read, size) = FILES.with(|f| f.borrow().get(path).cloned())
        .ok_or_else(|| Error::new(ErrorKind::NotFound, format!("{path} isn't one of the files opened")))?;
    Ok(filesystem::FileWrapper { file: Box::new(JsReader::new(read, size)), size })
}

/// The warnings and errors the parsers logged since the last call, as `[{ level, message }]`
#[wasm_bindgen]
pub fn take_log() -> JsValue {
    let lines = LOG.with(|l| std::mem::take(&mut *l.borrow_mut()));
    let out = Array::new();
    for (level, message) in lines {
        let o = Object::new();
        set(&o, "level", &level.into());
        set(&o, "message", &message.into());
        out.push(&o);
    }
    out.into()
}

/// The message of the last panic, which the call that trapped (a `RuntimeError: unreachable`) doesn't carry
#[wasm_bindgen]
pub fn take_panic() -> Option<String> {
    PANIC.with(|p| p.borrow_mut().take())
}

// ---------------------------------------------------------------------------------------------------------------------
// The streamed file

/// How much a read from JavaScript reads at least: small reads (box headers, the parts of a sample its parser reads one
/// after another) come from a window of this size, larger ones go to JavaScript as they are. It adapts to how the file
/// is read: it doubles while the reads go on from the window's end (walking boxes), and halves while most of a window
/// goes unused before the next read lands elsewhere (metadata samples scattered between video frames, where a fixed
/// 64 KiB read 8 times what a DJI clip's 8 KB frames needed)
const READ_AHEAD: usize = 64 * 1024;
const MIN_READ_AHEAD: usize = 4 * 1024;
const MAX_READ_AHEAD: usize = 1024 * 1024;

struct JsReader {
    read_fn: Function,
    size: u64,
    pos: u64,
    window: Vec<u8>,
    window_start: u64,
    /// The bytes of the window read from it, and whether the window was judged yet (when the first read it couldn't
    /// serve came)
    window_used: usize,
    judged: bool,
    ahead: usize,
    calls: u64,
    bytes: u64,
}

impl JsReader {
    fn new(read_fn: Function, size: u64) -> Self {
        Self { read_fn, size, pos: 0, window: Vec::new(), window_start: 0, window_used: 0, judged: true, ahead: READ_AHEAD, calls: 0, bytes: 0 }
    }

    /// `len` bytes from `offset`, fewer at the end of the file
    fn fetch(&mut self, offset: u64, len: usize) -> std::io::Result<Uint8Array> {
        let v = self.read_fn.call2(&JsValue::NULL, &JsValue::from_f64(offset as f64), &JsValue::from_f64(len as f64))
            .map_err(|e| Error::new(ErrorKind::Other, format!("Reading {len} bytes at {offset} failed: {}", js_error_text(&e))))?;
        let arr = if v.is_instance_of::<Uint8Array>() {
            v.unchecked_into::<Uint8Array>()
        } else if v.is_instance_of::<js_sys::ArrayBuffer>() {
            Uint8Array::new(&v)
        } else {
            return Err(Error::new(ErrorKind::Other, "The read callback must return an Uint8Array or an ArrayBuffer"));
        };
        self.calls += 1;
        self.bytes += arr.length() as u64;
        let _ = READS.try_with(|r| { let (c, b) = r.get(); r.set((c + 1, b + arr.length() as u64)); });
        Ok(arr)
    }
}

impl Read for JsReader {
    /// Reads all of `out` that is in the file: `read_beginning_and_end` reads the ends of a file with a single `read`
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let want = (out.len() as u64).min(self.size.saturating_sub(self.pos)) as usize;
        let mut done = 0;
        while done < want {
            let window_end = self.window_start + self.window.len() as u64;
            if self.pos >= self.window_start && self.pos < window_end {
                let from = (self.pos - self.window_start) as usize;
                let n = (want - done).min(self.window.len() - from);
                out[done..done + n].copy_from_slice(&self.window[from..from + n]);
                done += n;
                self.pos += n as u64;
                self.window_used += n;
                continue;
            }
            if !self.judged {
                self.judged = true;
                if self.pos == window_end {
                    self.ahead = (self.ahead * 2).min(MAX_READ_AHEAD);
                } else if self.window_used * 2 < self.window.len() {
                    self.ahead = (self.ahead / 2).max(MIN_READ_AHEAD);
                }
            }
            let left = want - done;
            if left >= self.ahead {
                let arr = self.fetch(self.pos, left)?;
                let n = (arr.length() as usize).min(left);
                if n == 0 { break; }
                arr.subarray(0, n as u32).copy_to(&mut out[done..done + n]);
                done += n;
                self.pos += n as u64;
                if n < left { break; } // The file ended sooner than its size said
            } else {
                let len = (self.ahead as u64).min(self.size - self.pos) as usize;
                let arr = self.fetch(self.pos, len)?;
                let n = (arr.length() as usize).min(len);
                if n == 0 { break; }
                self.window.resize(n, 0);
                arr.subarray(0, n as u32).copy_to(&mut self.window);
                self.window_start = self.pos;
                self.window_used = 0;
                self.judged = false;
            }
        }
        Ok(done)
    }
}

impl Seek for JsReader {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let to = match pos {
            SeekFrom::Start(p)   => Some(p),
            SeekFrom::End(d)     => self.size.checked_add_signed(d),
            SeekFrom::Current(d) => self.pos.checked_add_signed(d),
        }.ok_or_else(|| Error::new(ErrorKind::InvalidInput, "Seek to a negative or overflowing position"))?;
        self.pos = to;
        Ok(to)
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Helpers

fn set(o: &Object, key: &str, value: &JsValue) {
    let _ = Reflect::set(o, &JsValue::from_str(key), value);
}

fn js_error_text(e: &JsValue) -> String {
    if let Some(s) = e.as_string() { return s; }
    if let Some(e) = e.dyn_ref::<js_sys::Error>() { return String::from(e.message()); }
    format!("{e:?}")
}

fn io_err(e: std::io::Error) -> JsValue {
    JsValue::from(js_sys::Error::new(&e.to_string()))
}

fn to_js<T: Serialize + ?Sized>(v: &T) -> JsValue {
    v.serialize(&serde_wasm_bindgen::Serializer::json_compatible()).unwrap_or(JsValue::NULL)
}

/// `s` cut to at most `max` bytes, at a character boundary
fn truncate(s: &mut String, max: usize) -> bool {
    if s.len() <= max { return false; }
    let mut end = max;
    while !s.is_char_boundary(end) { end -= 1; }
    s.truncate(end);
    true
}

/// The value types that are one reading of a few numbers, which serialize as an array like the lists of readings do
fn is_tuple(v: &TagValue) -> bool {
    matches!(v, TagValue::u32x2(_) | TagValue::f32x2(_) | TagValue::f64x3(_) | TagValue::Uuid(_))
}

/// What a value is, for showing and plotting it: "bytes" (a tag the parser doesn't decode), "scalar", "tuple",
/// "struct", "readings" (a list of them, spread over the sample's duration), "text", "list" (of something else) or
/// "empty"
fn kind_of(v: &TagValue, json: &serde_json::Value) -> &'static str {
    use serde_json::Value;
    if matches!(v, TagValue::Unknown(_)) { return "bytes"; }
    if is_tuple(v) { return "tuple"; }
    match json {
        Value::Null => "empty",
        Value::Bool(_) | Value::Number(_) => "scalar",
        Value::String(_) => "text",
        Value::Object(_) => "struct",
        Value::Array(a) => match a.first() {
            None => "list",
            Some(Value::Number(_) | Value::Bool(_) | Value::Array(_) | Value::Object(_)) => "readings",
            Some(_) => "list",
        },
    }
}

const MAX_CHANNELS: usize = 32;

/// The numbers in `v`, by their path: the fields of objects and the items of short arrays, three levels deep
fn leaves(v: &serde_json::Value, path: &mut String, out: &mut Vec<(String, f64)>, depth: usize) {
    use serde_json::Value;
    if out.len() >= MAX_CHANNELS { return; }
    match v {
        Value::Number(n) => out.push((if path.is_empty() { "value".into() } else { path.clone() }, n.as_f64().unwrap_or(f64::NAN))),
        Value::Bool(b) => out.push((if path.is_empty() { "value".into() } else { path.clone() }, if *b { 1.0 } else { 0.0 })),
        Value::Object(m) if depth < 3 => {
            for (k, x) in m {
                // A reading's own timestamp (`TimeVector3::t` and the like) isn't one of its values
                if depth == 0 && k == "t" { continue; }
                let l = path.len();
                if !path.is_empty() { path.push('.'); }
                path.push_str(k);
                leaves(x, path, out, depth + 1);
                path.truncate(l);
            }
        }
        Value::Array(a) if depth < 3 && a.len() <= 16 => {
            for (i, x) in a.iter().enumerate() {
                let l = path.len();
                if !path.is_empty() { path.push('.'); }
                path.push_str(&i.to_string());
                leaves(x, path, out, depth + 1);
                path.truncate(l);
            }
        }
        _ => { }
    }
}

/// The readings of a value: one for a single value, each item of a list of readings
fn readings<'a>(v: &TagValue, json: &'a serde_json::Value) -> (bool, Vec<&'a serde_json::Value>) {
    match json {
        serde_json::Value::Array(a) if !is_tuple(v) => (true, a.iter().collect()),
        _ => (false, vec![json]),
    }
}

/// The time of a reading's own timestamp field is in a different unit and origin in every format, so readings are
/// placed evenly over their sample, as Gyroflow does for the formats without timestamps of their own
fn reading_time(sample: &util::SampleInfo, i: usize, n: usize) -> f64 {
    if n <= 1 || sample.duration_ms <= 0.0 { sample.timestamp_ms } else { sample.timestamp_ms + sample.duration_ms * i as f64 / n as f64 }
}

fn find_tag<'a>(sample: &'a util::SampleInfo, group: &str, tag: &str) -> Option<&'a TagDescription> {
    let map = sample.tag_map.as_ref()?;
    let (_, tags) = map.iter().find(|(g, _)| g.to_string() == group)?;
    tags.iter().find(|(t, _)| t.to_string() == tag).map(|(_, d)| d)
}

/// The most numbers a series holds, its readings by its channels (`series`): 96 MB of them
const MAX_SERIES_VALUES: usize = 12_000_000;

// ---------------------------------------------------------------------------------------------------------------------

#[wasm_bindgen]
pub struct Parser {
    camera: Option<String>,
    model: Option<String>,
    input: Input,
    reader: Option<JsReader>,
    size: u64,
}

#[wasm_bindgen]
impl Parser {
    /// Parses a file that is all in memory
    #[wasm_bindgen(constructor)]
    pub fn new(data: &[u8], filename: &str) -> Result<Parser, JsValue> {
        let mut stream = std::io::Cursor::new(&data);
        let input = Input::from_stream(&mut stream, data.len() as u64, filename, |_|(), Arc::new(AtomicBool::new(false))).map_err(io_err)?;
        Ok(Self {
            camera: Some(input.camera_type()),
            model: input.camera_model().map(String::clone),
            input,
            reader: None,
            size: data.len() as u64,
        })
    }

    /// Parses a file of `size` bytes that `read(offset, length)` returns the parts of, as an `Uint8Array` (or an
    /// `ArrayBuffer`). `progress(fraction)`, when given, is called as the parse goes on
    pub fn from_reader(size: f64, filename: &str, read: Function, progress: Option<Function>) -> Result<Parser, JsValue> {
        if !(size >= 0.0) || size > 2f64.powi(53) {
            return Err(JsValue::from(js_sys::Error::new("Invalid file size")));
        }
        let size = size as u64;
        READS.with(|r| r.set((0, 0)));
        let mut reader = JsReader::new(read, size);
        let last = Cell::new(-1.0f64);
        let progress_cb = |p: f64| {
            if let Some(f) = &progress {
                if p >= 1.0 || p - last.get() >= 0.005 || p < last.get() {
                    last.set(p);
                    let _ = f.call1(&JsValue::NULL, &JsValue::from_f64(p));
                }
            }
        };
        let input = Input::from_stream(&mut reader, size, filename, progress_cb, Arc::new(AtomicBool::new(false))).map_err(io_err)?;
        // The parse's reads are done: later reads (`video_metadata`) start with an empty window
        reader.window = Vec::new();
        Ok(Self {
            camera: Some(input.camera_type()),
            model: input.camera_model().map(String::clone),
            input,
            reader: Some(reader),
            size,
        })
    }

    #[wasm_bindgen(getter)]
    pub fn camera(&self) -> Option<String> { self.camera.clone() }

    #[wasm_bindgen(getter)]
    pub fn model(&self) -> Option<String> { self.model.clone() }

    /// What was read of a streamed file so far: `{ calls, bytes }`
    pub fn read_stats(&self) -> JsValue {
        let o = Object::new();
        // All the files' reads: the other files of a clip are read through `set_files`
        let (calls, bytes) = if self.reader.is_some() { READS.with(|r| r.get()) } else { (0, self.size) };
        set(&o, "calls", &JsValue::from_f64(calls as f64));
        set(&o, "bytes", &JsValue::from_f64(bytes as f64));
        o.into()
    }

    /// The file: `{ camera, model, parser, size, sample_count, tracks, start_ms, end_ms, frame_readout_time,
    /// has_accurate_timestamps, segments, segment_table_error }`
    pub fn info(&self) -> JsValue {
        let o = Object::new();
        set(&o, "camera", &self.camera.clone().into());
        set(&o, "model", &self.model.clone().into());
        set(&o, "parser", &self.input.parser_name().into());
        set(&o, "size", &JsValue::from_f64(self.size as f64));
        let samples = self.input.samples.as_deref().unwrap_or_default();
        set(&o, "sample_count", &JsValue::from_f64(samples.len() as f64));
        let mut tracks: Vec<usize> = samples.iter().map(|s| s.track_index).collect();
        tracks.sort_unstable();
        tracks.dedup();
        set(&o, "tracks", &to_js(&tracks));
        let start = samples.iter().map(|s| s.timestamp_ms).fold(f64::INFINITY, f64::min);
        let end = samples.iter().map(|s| s.timestamp_ms + s.duration_ms.max(0.0)).fold(f64::NEG_INFINITY, f64::max);
        set(&o, "start_ms", &if start.is_finite() { JsValue::from_f64(start) } else { JsValue::NULL });
        set(&o, "end_ms", &if end.is_finite() { JsValue::from_f64(end) } else { JsValue::NULL });
        set(&o, "frame_readout_time", &self.input.frame_readout_time().map_or(JsValue::NULL, JsValue::from_f64));
        set(&o, "has_accurate_timestamps", &self.input.has_accurate_timestamps().into());
        set(&o, "segments", &self.input.trim_segments().map_or(JsValue::NULL, |s| JsValue::from_f64(s.len() as f64)));
        set(&o, "segment_table_error", &self.input.unusable_segment_table().map_or(JsValue::NULL, JsValue::from_str));
        o.into()
    }

    /// The video track's `{ width, height, fps, duration_s, rotation }`, `null` when the file has none (or isn't
    /// streamed)
    pub fn video_metadata(&mut self) -> JsValue {
        let size = self.size;
        let Some(reader) = self.reader.as_mut() else { return JsValue::NULL; };
        if reader.seek(SeekFrom::Start(0)).is_err() { return JsValue::NULL; }
        let md = util::get_video_metadata(reader, size);
        reader.window = Vec::new();
        match md {
            Ok(md) if md.width > 0 && md.height > 0 => {
                let o = Object::new();
                set(&o, "width", &JsValue::from_f64(md.width as f64));
                set(&o, "height", &JsValue::from_f64(md.height as f64));
                set(&o, "fps", &JsValue::from_f64(md.fps));
                set(&o, "duration_s", &JsValue::from_f64(md.duration_s));
                set(&o, "rotation", &JsValue::from_f64(md.rotation as f64));
                o.into()
            }
            _ => JsValue::NULL,
        }
    }

    /// Where the samples are: `{ t, d, track }`, their timestamps and durations in ms and their tracks, by index
    pub fn sample_times(&self) -> JsValue {
        let samples = self.input.samples.as_deref().unwrap_or_default();
        let t: Vec<f64> = samples.iter().map(|s| s.timestamp_ms).collect();
        let d: Vec<f64> = samples.iter().map(|s| s.duration_ms).collect();
        let track: Vec<u32> = samples.iter().map(|s| s.track_index as u32).collect();
        let o = Object::new();
        set(&o, "t", &Float64Array::from(&t[..]).into());
        set(&o, "d", &Float64Array::from(&d[..]).into());
        set(&o, "track", &Uint32Array::from(&track[..]).into());
        o.into()
    }

    /// Every tag of the file, once: `[{ group, tag, description, native_id, count, kind, len, channels }]`, with the
    /// number of samples it's in and the shape of its first value (`kind_of`, `len` items, the numbers in `channels`)
    pub fn tags(&self) -> JsValue {
        struct Entry<'a> { first: &'a TagDescription, count: usize }
        let mut order: Vec<(String, String)> = Vec::new();
        let mut entries: HashMap<(String, String), Entry> = HashMap::new();
        for sample in self.input.samples.as_deref().unwrap_or_default() {
            let Some(map) = &sample.tag_map else { continue; };
            for (group, tags) in map {
                for (tag, desc) in tags {
                    let key = (group.to_string(), tag.to_string());
                    entries.entry(key.clone()).and_modify(|e| e.count += 1).or_insert_with(|| { order.push(key); Entry { first: desc, count: 1 } });
                }
            }
        }
        let out = Array::new();
        for key in order {
            let e = &entries[&key];
            let json = serde_json::to_value(&e.first.value).unwrap_or(serde_json::Value::Null);
            let kind = kind_of(&e.first.value, &json);
            let mut channels = Vec::new();
            let mut len = JsValue::NULL;
            if kind != "bytes" {
                let (is_list, items) = readings(&e.first.value, &json);
                if is_list { len = JsValue::from_f64(items.len() as f64); }
                if let Some(first) = items.first() {
                    leaves(first, &mut String::new(), &mut channels, 0);
                }
            }
            let o = Object::new();
            set(&o, "group", &key.0.as_str().into());
            set(&o, "tag", &key.1.as_str().into());
            set(&o, "description", &e.first.description.as_str().into());
            set(&o, "native_id", &e.first.native_id.map_or(JsValue::NULL, |x| JsValue::from_f64(x as f64)));
            set(&o, "count", &JsValue::from_f64(e.count as f64));
            set(&o, "kind", &kind.into());
            set(&o, "len", &len);
            set(&o, "channels", &to_js(&channels.into_iter().map(|x| x.0).collect::<Vec<_>>()));
            out.push(&o);
        }
        out.into()
    }

    /// The sample `index`: `{ index, track, sample_index, timestamp_ms, duration_ms, groups: [{ name, tags: [{ tag,
    /// description, native_id, kind, len, text, truncated }] }], values }`. `text` is the value as the parser formats
    /// it, cut to `max_text` bytes; `values` has the values of the `[group, tag]` pairs of `wanted`, as JSON, `null`
    /// for the ones the sample doesn't have
    pub fn sample(&self, index: usize, wanted: JsValue, max_text: Option<usize>) -> Result<JsValue, JsValue> {
        let samples = self.input.samples.as_deref().unwrap_or_default();
        let sample = samples.get(index).ok_or_else(|| JsValue::from(js_sys::Error::new("No such sample")))?;
        let max_text = max_text.unwrap_or(1200);
        let o = Object::new();
        set(&o, "index", &JsValue::from_f64(index as f64));
        set(&o, "track", &JsValue::from_f64(sample.track_index as f64));
        set(&o, "sample_index", &JsValue::from_f64(sample.sample_index as f64));
        set(&o, "timestamp_ms", &JsValue::from_f64(sample.timestamp_ms));
        set(&o, "duration_ms", &JsValue::from_f64(sample.duration_ms));
        let groups = Array::new();
        if let Some(map) = &sample.tag_map {
            for (group, tags) in map {
                let g = Object::new();
                set(&g, "name", &group.to_string().into());
                let list = Array::new();
                for (tag, desc) in tags {
                    let json = serde_json::to_value(&desc.value).unwrap_or(serde_json::Value::Null);
                    let kind = kind_of(&desc.value, &json);
                    let len = match &json { serde_json::Value::Array(a) if kind != "tuple" => JsValue::from_f64(a.len() as f64), _ => JsValue::NULL };
                    drop(json);
                    let mut text = desc.value.to_string();
                    let truncated = truncate(&mut text, max_text);
                    let t = Object::new();
                    set(&t, "tag", &tag.to_string().into());
                    set(&t, "description", &desc.description.as_str().into());
                    set(&t, "native_id", &desc.native_id.map_or(JsValue::NULL, |x| JsValue::from_f64(x as f64)));
                    set(&t, "kind", &kind.into());
                    set(&t, "len", &len);
                    set(&t, "text", &text.into());
                    set(&t, "truncated", &truncated.into());
                    list.push(&t);
                }
                set(&g, "tags", &list);
                groups.push(&g);
            }
        }
        set(&o, "groups", &groups);
        let wanted: Vec<(String, String)> = if wanted.is_undefined() || wanted.is_null() { Vec::new() } else {
            serde_wasm_bindgen::from_value(wanted).map_err(|e| JsValue::from(js_sys::Error::new(&format!("wanted: {e}"))))?
        };
        let values = Array::new();
        for (group, tag) in &wanted {
            values.push(&find_tag(sample, group, tag).map_or(JsValue::NULL, |d| to_js(&d.value)));
        }
        set(&o, "values", &values);
        Ok(o.into())
    }

    /// The samples that have a tag, by index
    pub fn tag_samples(&self, group: &str, tag: &str) -> Uint32Array {
        let list: Vec<u32> = self.input.samples.as_deref().unwrap_or_default().iter().enumerate()
            .filter(|(_, s)| find_tag(s, group, tag).is_some())
            .map(|(i, _)| i as u32)
            .collect();
        Uint32Array::from(&list[..])
    }

    /// The value of a tag of the sample `index`, as a JavaScript value (`null` when the sample doesn't have it)
    pub fn tag_value(&self, index: usize, group: &str, tag: &str) -> JsValue {
        self.input.samples.as_deref().unwrap_or_default().get(index)
            .and_then(|s| find_tag(s, group, tag))
            .map_or(JsValue::NULL, |d| to_js(&d.value))
    }

    /// The value of a tag of the sample `index`, as pretty-printed JSON (`null` when the sample doesn't have it)
    pub fn tag_json(&self, index: usize, group: &str, tag: &str) -> Option<String> {
        let sample = self.input.samples.as_deref().unwrap_or_default().get(index)?;
        let desc = find_tag(sample, group, tag)?;
        if let TagValue::Unknown(t) = &desc.value {
            return Some(serde_json::to_string(&serde_json::json!({ "hex": util::to_hex(&t.raw_data) })).unwrap_or_default());
        }
        serde_json::to_string_pretty(&desc.value).ok()
    }

    /// The numbers of a tag over the whole file: `{ channels, t, v, readings, truncated }`. `t` has the time of each
    /// reading in ms, `v` its `channels.length` numbers (NaN where a reading has no such number). A sample's list of
    /// readings is spread over the sample's duration (`reading_time`)
    pub fn series(&self, group: &str, tag: &str) -> JsValue {
        let mut channels: Vec<String> = Vec::new();
        let mut index: HashMap<String, usize> = HashMap::new();
        let mut t: Vec<f64> = Vec::new();
        let mut rows: Vec<Vec<(usize, f64)>> = Vec::new();
        let mut any_list = false;
        let mut truncated = false;
        let mut buf = Vec::new();
        'samples: for sample in self.input.samples.as_deref().unwrap_or_default() {
            let Some(desc) = find_tag(sample, group, tag) else { continue; };
            if matches!(desc.value, TagValue::Unknown(_)) { continue; }
            let json = match serde_json::to_value(&desc.value) { Ok(v) => v, Err(_) => continue };
            let (is_list, items) = readings(&desc.value, &json);
            any_list |= is_list;
            let n = items.len();
            for (i, item) in items.into_iter().enumerate() {
                buf.clear();
                leaves(item, &mut String::new(), &mut buf, 0);
                if buf.is_empty() { continue; }
                let channels_before = channels.len();
                let mut row = Vec::with_capacity(buf.len());
                for (name, v) in buf.drain(..) {
                    let ch = match index.get(&name) {
                        Some(&c) => c,
                        None if channels.len() < MAX_CHANNELS => { channels.push(name.clone()); index.insert(name, channels.len() - 1); channels.len() - 1 },
                        None => continue,
                    };
                    row.push((ch, v));
                }
                // The numbers are a matrix of the readings by the channels, NaN where a reading lacks a channel: a
                // reading that would make it larger than the limit isn't added, nor are the channels only it has
                if (rows.len() + 1) * channels.len() > MAX_SERIES_VALUES {
                    channels.truncate(channels_before);
                    truncated = true;
                    break 'samples;
                }
                t.push(reading_time(sample, i, n));
                rows.push(row);
            }
        }
        let nc = channels.len();
        let mut v = vec![f64::NAN; rows.len() * nc];
        for (r, row) in rows.iter().enumerate() {
            for &(c, x) in row { v[r * nc + c] = x; }
        }
        drop(rows);
        let o = Object::new();
        set(&o, "channels", &to_js(&channels));
        set(&o, "t", &Float64Array::from(&t[..]).into());
        set(&o, "v", &Float64Array::from(&v[..]).into());
        set(&o, "readings", &any_list.into());
        set(&o, "truncated", &truncated.into());
        o.into()
    }

    /// Each sensor's readings on its own timeline, as Gyroflow reads them (`util::imu_readings`): `{ gyro, accl, magn }`,
    /// each `{ t, v }` (the time of each reading in ms, and its x, y, z: °/s, m/s², the magnetometer's unit) or `null`
    /// for a sensor the file has none of
    pub fn imu_readings(&self, orientation: Option<String>) -> Result<JsValue, JsValue> {
        if self.input.samples.is_none() { return Err(JsValue::from(js_sys::Error::new("No metadata"))); }
        let readings = util::imu_readings(&self.input, orientation).map_err(io_err)?;
        let o = Object::new();
        for (name, sensor) in [("gyro", &readings.gyro), ("accl", &readings.accl), ("magn", &readings.magn)] {
            if sensor.is_empty() { set(&o, name, &JsValue::NULL); continue; }
            let t: Vec<f64> = sensor.keys().map(|us| *us as f64 / 1000.0).collect();
            let mut v = Vec::with_capacity(sensor.len() * 3);
            for (r, _) in sensor.values() { v.extend([r.x as f32, r.y as f32, r.z as f32]); }
            let s = Object::new();
            set(&s, "t", &Float64Array::from(&t[..]).into());
            set(&s, "v", &Float32Array::from(&v[..]).into());
            set(&o, name, &s);
        }
        Ok(o.into())
    }

    /// The motion data at the gyroscope's times, the accelerometer and the magnetometer interpolated there
    /// (`util::normalized_imu_interpolated`): `{ t, gyro, accl, magn }`, the time of each reading in ms and its x, y, z
    /// (°/s, m/s² and the magnetometer's unit), `null` for a sensor the file has none of, NaN for readings without it
    pub fn imu(&self, orientation: Option<String>) -> Result<JsValue, JsValue> {
        if self.input.samples.is_none() { return Err(JsValue::from(js_sys::Error::new("No metadata"))); }
        let data = util::normalized_imu_interpolated(&self.input, orientation).map_err(io_err)?;
        let n = data.len();
        let mut t = Vec::with_capacity(n);
        let mut sensors = [Vec::with_capacity(n * 3), Vec::with_capacity(n * 3), Vec::with_capacity(n * 3)];
        let mut has = [false; 3];
        for d in &data {
            t.push(d.timestamp_ms);
            for (i, s) in [d.gyro, d.accl, d.magn].into_iter().enumerate() {
                match s {
                    Some(v) => { has[i] = true; sensors[i].extend(v.iter().map(|x| *x as f32)); }
                    None => sensors[i].extend([f32::NAN; 3]),
                }
            }
        }
        drop(data);
        let o = Object::new();
        set(&o, "t", &Float64Array::from(&t[..]).into());
        for (i, name) in ["gyro", "accl", "magn"].into_iter().enumerate() {
            set(&o, name, &if has[i] { Float32Array::from(&sensors[i][..]).into() } else { JsValue::NULL });
        }
        Ok(o.into())
    }

    /// All the samples' tags as JSON, `[{ group: { tag: value } }]` (the formatted values with `human_readable`)
    pub fn telemetry_json(&self, human_readable: Option<bool>) -> Result<String, JsValue> {
        let samples = self.input.samples.as_ref().ok_or_else(|| JsValue::from("No metadata"))?;
        let mut output = Vec::with_capacity(samples.len());
        for info in samples {
            let Some(groups_map) = &info.tag_map else { continue; };
            let mut groups = BTreeMap::new();
            for (group, map) in groups_map {
                let group_map = groups.entry(group.to_string()).or_insert_with(BTreeMap::new);
                for (tagid, info) in map {
                    let value = if human_readable.unwrap_or(false) { serde_json::Value::String(info.value.to_string()) } else { serde_json::to_value(&info.value).unwrap_or_default() };
                    group_map.insert(tagid.to_string(), value);
                }
            }
            output.push(serde_json::json!({ "timestamp_ms": info.timestamp_ms, "duration_ms": info.duration_ms, "tags": groups }));
        }
        serde_json::to_string(&output).map_err(|e| JsValue::from(e.to_string()))
    }

    // Kept for existing users of the module ------------------------------------------------------------------------

    pub fn telemetry(&self, human_readable: Option<bool>) -> Result<JsValue, JsValue> {
        if self.input.samples.is_none() { return Err(JsValue::from("No metadata")); }

        let samples = self.input.samples.as_ref().unwrap();
        let mut output = Vec::with_capacity(samples.len());

        for info in samples {
            if info.tag_map.is_none() { continue; }

            let mut groups = BTreeMap::new();
            let groups_map = info.tag_map.as_ref().unwrap();

            for (group, map) in groups_map {
                let group_map = groups.entry(group).or_insert_with(BTreeMap::new);
                for (tagid, info) in map {
                    let value = if human_readable.unwrap_or(false) {
                        serde_json::to_value(info.value.to_string())
                    } else {
                        serde_json::to_value(&info.value)
                    }.unwrap();
                    group_map.insert(tagid, value);
                }
            }

            output.push(groups);
        }

        Ok(serde_wasm_bindgen::to_value(&output).unwrap())
    }

    pub fn normalized_imu(&self, orientation: Option<String>) -> Result<JsValue, JsValue> {
        if self.input.samples.is_none() { return Err(JsValue::from("No metadata")); }

        let imu_data = util::normalized_imu(&self.input, orientation).map_err(io_err)?;

        Ok(serde_wasm_bindgen::to_value(&imu_data).unwrap())
    }
}
