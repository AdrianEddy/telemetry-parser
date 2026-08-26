// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2026 Adrian <adrian.eddy at gmail>
//
// JSON reader for the Gyroflow Protobuf telemetry schema (see ./gyroflow.proto).
//
// The wire format is JSONL: one `Main` message per line, each written in the
// protobuf proto3 canonical JSON mapping, as emitted by
// `google.protobuf.util.JsonFormat`, Go's `protojson`, `prost-reflect`,
// `protobuf.js` and friends.
//
// The field-by-field work is done by the `serde::Deserialize` impls derived on
// the generated types, not here. What this module does is reshape the document
// into what those impls expect, because the canonical mapping and serde's view
// of the Rust structs disagree in exactly five places:
//
//   1. Field names are lowerCamelCase (`startTimestampUs`), while the derive
//      wants the Rust/proto spelling. The mapping also requires the original
//      proto name to be accepted, so renaming beats `rename_all` — which would
//      accept one spelling and silently drop the other.
//   2. Fields at their default value are omitted. Handled by `#[serde(default)]`
//      on every message in gyroflow_proto.rs (see build.rs) rather than here.
//   3. `oneof` variants are flattened into the containing message
//      (`"opencvFisheye"` sits directly in the LensData object); serde wants an
//      externally tagged enum under the oneof's own name.
//   4. Enums are their declared string name (`"TopToBottom"`); the generated
//      field is an `i32`.
//   5. `lensProfile` / `additionalData` are proto `string` fields carrying JSON,
//      which a JSON producer will naturally inline as real objects.
//
// Each of those needs schema knowledge, so each has a small table below. Every
// other field — including any added to gyroflow.proto later — flows through the
// derive untouched.
//
// A few shapes a hand-rolled producer tends to emit are tolerated on the same
// basis: serde-style wrapped oneofs, quaternions as `[w, x, y, z]`, IMU axes as
// `[x, y, z]`, `cameraIntrinsicMatrix` written as nested rows, and a single
// object where a repeated field expects an array.
//
// Not accepted: numbers written as JSON strings. The mapping permits it, but no
// real producer quotes 32-bit numbers and this schema has no 64-bit integer
// fields (the case the mapping actually mandates quoting for). Coercing them
// would take a list of every string-typed field, and getting that list stale
// would silently turn an all-digit serial number into a parse failure.

use std::borrow::Cow;

use serde_json::{ Map, Value };

use super::gyroflow_proto as pb;

/// A UTF-8 byte order mark. Editors and .NET's default `StreamWriter` prepend
/// one; it is legal UTF-8 but not legal JSON, so it has to come off the front of
/// the stream before either the format probe or the first line is parsed.
pub const UTF8_BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

// ------------------------------------------------------------------- tables --

/// `oneof` wrapper field -> its variants as (proto name, Rust variant name).
const ONEOFS: &[(&str, &[(&str, &str)])] = &[
    ("distortion", &[
        ("no_distortion",      "NoDistortion"),
        ("opencv_fisheye",     "OpencvFisheye"),
        ("opencv_standard",    "OpencvStandard"),
        ("lensfun_poly3",      "LensfunPoly3"),
        ("lensfun_poly5",      "LensfunPoly5"),
        ("lensfun_ptlens",     "LensfunPtlens"),
        ("generic_polynomial", "GenericPolynomial"),
    ]),
    ("data", &[
        ("quaternion", "Quaternion"),
        ("mesh_warp",  "MeshWarp"),
        ("matrix_4x4", "Matrix4x4"),
    ]),
];

/// Enum-typed fields, with the declared value names the mapping writes.
const ENUMS: &[(&str, &[(&str, i64)])] = &[
    ("frame_readout_direction", &[("TopToBottom", 0), ("BottomToTop", 1), ("RightToLeft", 2), ("LeftToRight", 3)]),
    ("fix_type",                &[("Unknown", 0), ("NoFix", 1), ("Fix2D", 2), ("Fix3D", 3), ("RTK", 4)]),
];

/// Proto `string` fields whose contents are themselves JSON.
const JSON_STRING_FIELDS: &[&str] = &["lens_profile", "additional_data"];

/// Quaternion-typed fields, accepted as `[w, x, y, z]` as well as `{w,x,y,z}`.
const QUATERNION_FIELDS: &[&str] = &["quat", "quaternion", "imu_rotation", "quats_rotation"];

/// IMU 3-vectors, accepted as `[x, y, z]` as well as `<name>_x` / `_y` / `_z`.
const IMU_VECTORS: &[&str] = &["gyroscope", "accelerometer", "magnetometer"];

/// Repeated fields, where a lone object stands in for a one-element array.
const REPEATED_FIELDS: &[&str] = &["lens", "imu", "quaternions", "ois", "ibis", "eis", "gps"];

// ------------------------------------------------------------------ helpers --

/// Compares two names ignoring ASCII case and `_` / `-` separators, so a table
/// entry matches whichever spelling the producer used. Needed on top of
/// [`proto_name`] for `matrix_4x4`, whose canonical JSON name (`matrix4x4`)
/// carries no capital to rebuild the underscore from.
fn key_matches(a: &str, b: &str) -> bool {
    let mut a = a.bytes().filter(|c| *c != b'_' && *c != b'-');
    let mut b = b.bytes().filter(|c| *c != b'_' && *c != b'-');
    loop {
        match (a.next(), b.next()) {
            (None, None) => return true,
            (Some(x), Some(y)) if x.eq_ignore_ascii_case(&y) => { }
            _ => return false,
        }
    }
}

/// The key an object actually uses for `proto_name`, if it has it at all.
fn actual_key(obj: &Map<String, Value>, proto_name: &str) -> Option<String> {
    if obj.contains_key(proto_name) { return Some(proto_name.to_owned()); }
    obj.keys().find(|k| key_matches(k, proto_name)).cloned()
}

/// Rewrites a JSON key into the proto spelling the derive expects:
/// `startTimestampUs` and `StartTimestampUs` both become `start_timestamp_us`,
/// `START_TIMESTAMP_US` is lowercased, and a key that is already snake_case is
/// returned borrowed and untouched.
fn proto_name(key: &str) -> Cow<'_, str> {
    if !key.bytes().any(|c| c.is_ascii_uppercase()) { return Cow::Borrowed(key); }
    if !key.bytes().any(|c| c.is_ascii_lowercase()) { return Cow::Owned(key.to_ascii_lowercase()); }
    let mut out = String::with_capacity(key.len() + 4);
    for (i, c) in key.char_indices() {
        if c.is_ascii_uppercase() {
            if i > 0 && !out.ends_with('_') { out.push('_'); }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    Cow::Owned(out)
}

// ---------------------------------------------------------------- normalize --

/// Reshapes a parsed document in place into the exact form the derived
/// `Deserialize` impls accept. See the module header for what diverges and why.
fn normalize(v: &mut Value) {
    match v {
        Value::Array(a) => {
            // A repeated float written as nested rows — a 3x3 intrinsic matrix,
            // a 4x4 transform. Unambiguous: every repeated field in this schema
            // holds either messages or scalars, never arrays.
            if a.iter().any(Value::is_array) {
                *a = std::mem::take(a).into_iter().flat_map(|x| match x {
                    Value::Array(inner) => inner,
                    other => vec![other],
                }).collect();
            }
            for x in a.iter_mut() { normalize(x); }
        }
        Value::Object(_) => normalize_object(v),
        _ => { }
    }
}

fn normalize_object(v: &mut Value) {
    let Some(obj) = v.as_object_mut() else { return };

    // Stringify the JSON-in-a-string payloads first, so the recursion below
    // leaves their own keys — a gyroflow lens profile's, say — alone.
    for name in JSON_STRING_FIELDS {
        let Some(key) = actual_key(obj, name) else { continue };
        let Some(val) = obj.get_mut(&key) else { continue };
        if val.is_object() || val.is_array() {
            *val = Value::String(serde_json::to_string(val).unwrap_or_default());
        }
    }

    // Rename this message's own fields to the proto spelling. A oneof wrapper's
    // inner key is deliberately left alone here — it names a serde variant, not
    // a field, and is handled below.
    if obj.keys().any(|k| matches!(proto_name(k), Cow::Owned(_))) {
        let renamed = std::mem::take(obj).into_iter().map(|(k, v)| (proto_name(&k).into_owned(), v));
        obj.extend(renamed);
    }

    for (name, variants) in ENUMS {
        let Some(key) = actual_key(obj, name) else { continue };
        let Some(val) = obj.get_mut(&key) else { continue };
        if let Some(s) = val.as_str() {
            let original = s.to_owned();
            // Producers that prefix value names with the enum type
            // (READOUT_DIRECTION_TOP_TO_BOTTOM) are matched on the suffix.
            let s: String = s.bytes().filter(|c| *c != b'_').map(|c| c.to_ascii_lowercase() as char).collect();
            let matched = variants.iter().find(|(n, _)| {
                let n: String = n.bytes().filter(|c| *c != b'_').map(|c| c.to_ascii_lowercase() as char).collect();
                s == n || s.ends_with(&n)
            });
            // An unrecognised name — a value added to the enum after this table
            // was written — falls back to the proto3 default rather than being
            // left as a string, which the derive would reject on an i32 field
            // and take the whole message down with it. Losing one field beats
            // losing the frame's gyro, lens and stabilizer data alongside it.
            match matched {
                Some((_, i)) => *val = Value::from(*i),
                None => {
                    log::warn!("Unknown value \"{original}\" for enum field `{name}`; falling back to 0");
                    *val = Value::from(0);
                }
            }
        }
    }

    for name in QUATERNION_FIELDS {
        let Some(key) = actual_key(obj, name) else { continue };
        let Some(val) = obj.get_mut(&key) else { continue };
        if let Some(a) = val.as_array() {
            if a.len() >= 4 {
                *val = Value::Object(["w", "x", "y", "z"].iter().zip(a).map(|(k, v)| (k.to_string(), v.clone())).collect());
            }
        }
    }

    for name in IMU_VECTORS {
        let Some(key) = actual_key(obj, name) else { continue };
        let Some(a) = obj.get(&key).and_then(Value::as_array).filter(|a| a.len() >= 3).cloned() else { continue };
        obj.remove(&key);
        for (axis, val) in ["x", "y", "z"].iter().zip(a) {
            obj.entry(format!("{name}_{axis}")).or_insert(val);
        }
    }

    for name in REPEATED_FIELDS {
        let Some(key) = actual_key(obj, name) else { continue };
        let Some(val) = obj.get_mut(&key) else { continue };
        if val.is_object() { *val = Value::Array(vec![val.take()]); }
    }

    // `oneof`: the canonical mapping flattens the variant into this message,
    // while a serde-style writer wraps it in an object under the oneof's own
    // name. Unwrap the latter so both encodings meet, then lift the variant
    // into the shape the generated enum deserializes from. The payload is
    // normalized here and skipped by the recursion below, so a wrapper key is
    // never walked as if it were a message.
    let mut wrapped: Vec<&str> = Vec::new();
    for (wrapper, variants) in ONEOFS {
        if let Some(key) = actual_key(obj, wrapper) {
            let entry = obj.get_mut(&key)
                           .and_then(Value::as_object_mut)
                           .and_then(|inner| { let k = inner.keys().next().cloned()?; let v = inner.remove(&k)?; Some((k, v)) });
            obj.remove(&key);
            // A wrapper holding nothing recognisable is dropped rather than
            // handed to serde as an unknown variant.
            if let Some((variant, val)) = entry { obj.insert(variant, val); }
        }
        let Some((key, rust)) = variants.iter().find_map(|(n, rust)| actual_key(obj, n).map(|k| (k, *rust))) else { continue };
        let Some(mut val) = obj.remove(&key) else { continue };
        normalize(&mut val);
        obj.insert((*wrapper).to_owned(), Value::Object([(rust.to_owned(), val)].into_iter().collect()));
        wrapped.push(wrapper);
    }

    for (key, val) in obj.iter_mut() {
        if wrapped.iter().any(|w| key == w) { continue; }
        normalize(val);
    }
}

// ---------------------------------------------------------------- documents --

/// Frame fields distinctive enough to recognise a bare `FrameMetadata` written
/// without its `Main` wrapper.
const FRAME_MARKERS: &[&str] = &["start_timestamp_us", "end_timestamp_us", "frame_number", "imu", "quaternions"];

pub fn main_from_json(mut v: Value) -> Option<pb::Main> {
    if !v.is_object() { return None; }
    normalize(&mut v);
    main_from_normalized(v)
}

fn main_from_normalized(v: Value) -> Option<pb::Main> {
    let obj = v.as_object()?;

    if !obj.contains_key("frame") && FRAME_MARKERS.iter().any(|k| obj.contains_key(*k)) {
        let frame = deserialize::<pb::FrameMetadata>(v, "FrameMetadata")?;
        return Some(pb::Main { frame: Some(frame), ..Default::default() });
    }

    let main = deserialize::<pb::Main>(v, "Main")?;
    // `#[serde(default)]` means an unrelated JSON object deserializes cleanly
    // into an empty message; there is nothing to emit for one.
    if main.header.is_none() && main.frame.is_none() { return None; }
    Some(main)
}

fn deserialize<T: serde::de::DeserializeOwned>(v: Value, what: &str) -> Option<T> {
    match serde_json::from_value(v) {
        Ok(x) => Some(x),
        Err(e) => { log::warn!("Skipping malformed {what} in the Gyroflow protobuf JSON: {e}"); None }
    }
}

// ------------------------------------------------------------------ writing --

/// One JSONL record: a `Main` message in the proto3 canonical JSON mapping,
/// with no trailing newline.
pub fn to_jsonl_line(main: &pb::Main) -> String {
    serde_json::to_string(&to_canonical_json(main)).unwrap_or_default()
}

/// Renders a message in the proto3 canonical JSON mapping.
///
/// The derived `Serialize` does the field work, exactly as `Deserialize` does
/// when reading; [`denormalize`] then reshapes its output the same five ways
/// [`normalize`] undoes, off the same tables — so the reader and the writer
/// cannot drift apart.
pub fn to_canonical_json(main: &pb::Main) -> Value {
    let mut v = serde_json::to_value(main).unwrap_or(Value::Null);
    denormalize(&mut v);
    v
}

fn denormalize(v: &mut Value) {
    match v {
        Value::Array(a) => for x in a.iter_mut() { denormalize(x); },
        Value::Object(_) => denormalize_object(v),
        _ => { }
    }
}

fn denormalize_object(v: &mut Value) {
    let Some(obj) = v.as_object_mut() else { return };

    // Flatten the oneof: the variant moves up into this message under its proto
    // name, which the rename at the end turns into the canonical spelling.
    for (wrapper, variants) in ONEOFS {
        let flattened = obj.get_mut(*wrapper)
                           .and_then(Value::as_object_mut)
                           .and_then(|inner| {
                               let variant = inner.keys().next().cloned()?;
                               let (proto, _) = variants.iter().find(|(_, rust)| variant == **rust)?;
                               Some(((*proto).to_owned(), inner.remove(&variant)?))
                           });
        if let Some((proto, val)) = flattened {
            obj.remove(*wrapper);
            obj.insert(proto, val);
        }
    }

    for (name, variants) in ENUMS {
        let Some(val) = obj.get_mut(*name) else { continue };
        let Some(i) = val.as_i64() else { continue };
        if let Some((name, _)) = variants.iter().find(|(_, v)| *v == i) {
            *val = Value::String((*name).to_owned());
        }
    }

    // Drop what the mapping omits. Only values proto3 gives no presence to, so
    // nothing is lost: an unset field, an empty repeated field, an empty string.
    // Numeric zeros and `false` stay, because serde renders `Some(0)` and a
    // plain `0` identically and dropping the former would change its meaning.
    obj.retain(|_, v| match v {
        Value::Null => false,
        Value::Array(a) => !a.is_empty(),
        Value::String(s) => !s.is_empty(),
        _ => true,
    });

    for (_, val) in obj.iter_mut() { denormalize(val); }

    let renamed = std::mem::take(obj).into_iter().map(|(k, v)| (json_name(&k), v));
    obj.extend(renamed);
}

/// The canonical JSON name for a proto field: drop each `_` and capitalise what
/// follows, so `start_timestamp_us` becomes `startTimestampUs`.
fn json_name(proto_name: &str) -> String {
    let mut out = String::with_capacity(proto_name.len());
    let mut upper = false;
    for c in proto_name.chars() {
        if c == '_' { upper = true; }
        else if upper { out.extend(c.to_uppercase()); upper = false; }
        else { out.push(c); }
    }
    out
}

/// Iterator over the `Main` messages in a JSONL stream — one message per line,
/// blank lines skipped.
///
/// A line that doesn't parse is logged and skipped rather than ending the
/// stream: one corrupt record costs one frame, not the rest of the clip.
pub struct MessageReader<R: std::io::BufRead> {
    lines:   std::io::Lines<R>,
    line_no: usize,
}

impl<R: std::io::BufRead> MessageReader<R> {
    pub fn new(reader: R) -> Self {
        Self { lines: reader.lines(), line_no: 0 }
    }
}

impl<R: std::io::BufRead> Iterator for MessageReader<R> {
    type Item = pb::Main;
    fn next(&mut self) -> Option<pb::Main> {
        loop {
            let line = match self.lines.next()? {
                Ok(line) => line,
                Err(e) => { log::error!("Failed to read the Gyroflow protobuf JSONL stream: {e}"); return None; }
            };
            self.line_no += 1;
            // A BOM decodes as U+FEFF and would make line 1 invalid JSON.
            let line = if self.line_no == 1 { line.trim_start_matches('\u{feff}') } else { line.as_str() };
            if line.trim().is_empty() { continue; }

            match serde_json::from_str::<Value>(line) {
                Ok(v) => if let Some(msg) = main_from_json(v) { return Some(msg); },
                Err(e) => log::warn!("Skipping line {}: invalid JSON: {e}", self.line_no),
            }
        }
    }
}
