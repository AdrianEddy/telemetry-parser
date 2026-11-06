# telemetry-parser-wasm

# Building
1. Install `wasm-pack`: `cargo install wasm-pack`
2. Build wasm module: `wasm-pack build --release --target web`
3. Resulting files will be in the `pkg` directory

# Usage
A file in memory:
```js
import init, { Parser } from './pkg/telemetry_parser.js';
await init();
const parser = new Parser(new Uint8Array(await file.arrayBuffer()), file.name);
```

A file of any size, streamed. `read(offset, length)` must return those bytes synchronously, which a Web Worker can do
for a `File` with `FileReaderSync`. The parser only reads the parts of the file it needs:
```js
const reader = new FileReaderSync();
const parser = Parser.from_reader(file.size, file.name,
    (offset, length) => new Uint8Array(reader.readAsArrayBuffer(file.slice(offset, offset + length))),
    progress => console.log(progress));
```

A file in memory, with only some of its tags, as one flat `Float64Array` (a single value for JavaScript, not one per
number). `filter` is a group, `"Group"`, or one tag, `"Group/Tag"`:
```js
import init, { telemetry_flat } from './pkg/telemetry_parser.js';
const flat = telemetry_flat(new Uint8Array(await file.arrayBuffer()), file.name, 'Group/Tag');
// [ frame_count, then per frame: timestamp_ms, duration_ms, value_count, ...values ]
let i = 1;
for (let f = 0; f < flat[0]; f++) {
    const [t, duration, n] = [flat[i], flat[i + 1], flat[i + 2]];
    const values = flat.subarray(i + 3, i + 3 + n);
    i += 3 + n;
}
```
Only the frames that have a matching tag are in it, and only the numbers of its tags (not text or bytes). The tags are
filtered by the parser while it reads the file, and the other tags' values are never decoded.

Then:
- `parser.info()`: camera, model, parser, sample count, time span, frame readout time
- `parser.imu()`: the motion data as Gyroflow reads it, as typed arrays (`t` in ms; `gyro` °/s, `accl` m/s², `magn`, 3 per reading)
- `parser.tags()`: every tag in the file once, with its shape and the numbers it holds
- `parser.sample(index)`: the tags of a sample, formatted
- `parser.series(group, tag)`: a tag's numbers over the whole file, as typed arrays
- `parser.tag_value(index, group, tag)`, `parser.tag_json(...)`, `parser.tag_samples(group, tag)`
- `parser.video_metadata()`, `parser.read_stats()`, `take_log()` (the parsers' warnings)
- `parser.telemetry()` and `parser.normalized_imu()`, as before

# Telemetry Viewer
**[Try online demo](https://gyroflow.xyz/telemetry-viewer/)**

`viewer/` is a page that shows a file's metadata on a timeline with its video: the motion data and any tag as charts,
the tags at the playhead, and views of the sensor and its mesh, the lens, the camera's orientation, the GPS track and
the stabilizer's shift. The video plays from the file itself and RAW files open without a picture.

1. Build the module into the viewer: `wasm-pack build --release --target web --out-dir viewer/pkg`
2. Serve the folder over HTTP (module workers and wasm don't load from `file://`), for example
   `python -m http.server -d viewer 8000`, and open http://localhost:8000
