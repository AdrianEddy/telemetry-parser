// SPDX-License-Identifier: MIT OR Apache-2.0
// The parser runs here: a dedicated worker can read parts of a File synchronously (FileReaderSync), which is what the
// Rust parsers' Read + Seek needs. Only the byte ranges the parsers ask for are read, never the whole file.

import init, { Parser, set_files, take_log, take_panic } from './pkg/telemetry_parser.js';

const ready = init();
let parser = null;
let file = null;
let tracks = [];   // per track: sample indices sorted by time
let times = null;  // { t, d, track }
let tagIndex = new Map(); // "group\0tag" → the samples that have it, by index (they are in time order per track)
let sparse = [];          // the tags only a few samples have (clip metadata, a lens profile, a clip's motion data in one sample)
let positions = null;     // each sample's position in its track
const heldCache = new Map(); // sample index → its tags, formatted (the samples sparse tags are held from)

const reader = new FileReaderSync();
function read(offset, length) {
    return new Uint8Array(reader.readAsArrayBuffer(file.slice(offset, offset + length)));
}

function errorText(e) {
    const panic = take_panic();
    if (panic) return panic;
    return (e && e.message) || String(e);
}

function buildTrackIndex() {
    times = parser.sample_times();
    const byTrack = new Map();
    for (let i = 0; i < times.t.length; i++) {
        const tr = times.track[i];
        if (!byTrack.has(tr)) byTrack.set(tr, []);
        byTrack.get(tr).push(i);
    }
    tracks = [...byTrack.values()].map(list => {
        list.sort((a, b) => times.t[a] - times.t[b]);
        return Int32Array.from(list);
    });
    positions = new Int32Array(times.t.length);
    for (const list of tracks) list.forEach((index, i) => { positions[index] = i; });
}

/// Per track, the sample at `ms`: the last one that starts at or before it (the first one before the track starts)
function samplesAt(ms) {
    const out = [];
    for (const list of tracks) {
        let lo = 0, hi = list.length - 1, found = -1;
        while (lo <= hi) {
            const mid = (lo + hi) >> 1;
            if (times.t[list[mid]] <= ms) { found = mid; lo = mid + 1; } else hi = mid - 1;
        }
        if (found < 0) found = 0;
        const index = list[found];
        const inside = ms >= times.t[index] && ms < times.t[index] + Math.max(times.d[index], 0.001);
        out.push({ index, position: found, count: list.length, inside });
    }
    return out;
}

/// The sample a tag's value at `ms` is from: the latest one at or before it that has the tag, the first one when none does
function latestWith(group, tag, ms) {
    const key = group + '\u0000' + tag;
    let list = tagIndex.get(key);
    if (!list) { list = parser.tag_samples(group, tag).sort((a, b) => times.t[a] - times.t[b]); tagIndex.set(key, list); }
    if (!list.length) return -1;
    let lo = 0, hi = list.length - 1, found = 0;
    while (lo <= hi) {
        const mid = (lo + hi) >> 1;
        if (times.t[list[mid]] <= ms) { found = mid; lo = mid + 1; } else hi = mid - 1;
    }
    return list[found];
}

/// The tags only a few samples have that the samples at `ms` don't, from the samples they're held from: the clip's
/// metadata stays in view after its first frame, and a clip whose motion data is all in one sample shows it
function heldSamples(ms, active) {
    const shown = new Set();
    for (const s of active) for (const g of s.sample.groups) for (const t of g.tags) shown.add(g.name + '\u0000' + t.tag);
    const bySample = new Map();
    for (const [group, tag] of sparse) {
        const key = group + '\u0000' + tag;
        if (shown.has(key)) continue;
        const index = latestWith(group, tag, ms);
        if (index < 0 || active.some(s => s.index === index)) continue;
        if (!bySample.has(index)) bySample.set(index, new Set());
        bySample.get(index).add(key);
    }
    const out = [];
    for (const [index, keys] of bySample) {
        let sample = heldCache.get(index);
        if (!sample) {
            sample = parser.sample(index, null, 1200);
            if (heldCache.size > 32) heldCache.clear();
            heldCache.set(index, sample);
        }
        const groups = sample.groups.map(g => ({ ...g, tags: g.tags.filter(t => keys.has(g.name + '\u0000' + t.tag)) })).filter(g => g.tags.length);
        const list = tracks.find(l => l[positions[index]] === index);
        out.push({ index, held: true, position: positions[index], count: list ? list.length : 0, inside: false, sample: { ...sample, groups } });
    }
    return out.sort((a, b) => a.index - b.index);
}

const ops = {
    /// Parses `file` at `path`, its clip's other files (`files`, `[{ path, file }]`: a RED clip's parts, a CinemaDNG
    /// sequence, a video's log) opened by the parser by name next to it
    async open({ file: f, path, files = [] }) {
        file = f;
        parser?.free();
        parser = null;
        tagIndex = new Map();
        path = path || '/' + f.name;
        const all = files.length ? files : [{ path, file: f }];
        set_files(all.map(({ path, file }) => ({ path, size: file.size, read: (o, l) => new Uint8Array(reader.readAsArrayBuffer(file.slice(o, o + l))) })));
        const started = performance.now();
        parser = Parser.from_reader(f.size, path, read, p => postMessage({ type: 'progress', value: p }));
        const parseMs = performance.now() - started;
        buildTrackIndex();
        heldCache.clear();
        const info = parser.info(), tags = parser.tags();
        sparse = tags.filter(t => t.count * 4 <= info.sample_count).map(t => [t.group, t.tag]);
        return {
            info,
            tags,
            stats: { ...parser.read_stats(), parseMs },
            log: take_log(),
        };
    },
    /// Each sensor's readings on its own timeline, as Gyroflow reads them
    imu() {
        try {
            const r = parser.imu_readings(null);
            const transfer = [];
            for (const k of ['gyro', 'accl', 'magn']) if (r[k]) transfer.push(r[k].t.buffer, r[k].v.buffer);
            return { value: r, transfer };
        } catch (e) {
            return { value: null, transfer: [] };
        }
    },
    /// The readings at the gyroscope's times, the other sensors interpolated there (for the CSV export)
    imuAligned() {
        const imu = parser.imu(null);
        return { value: imu, transfer: [imu.t.buffer, imu.gyro?.buffer, imu.accl?.buffer, imu.magn?.buffer].filter(Boolean) };
    },
    videoMetadata() {
        return parser.video_metadata();
    },
    times() {
        return { t: times.t.slice(), d: times.d.slice(), track: times.track.slice() };
    },
    /// The samples at `ms` (one per track, with all their tags), and for each [group, tag] of `wanted` the value of
    /// the latest sample at or before `ms` that has it (the first one when none does): a lens profile or a mesh is
    /// often stored once, or only now and then
    at({ ms, wanted = [], samples = true }) {
        const out = { samples: samples ? samplesAt(ms).map(s => ({ ...s, sample: parser.sample(s.index, null, 1200) })) : [], values: [] };
        if (samples) out.samples.push(...heldSamples(ms, out.samples));
        for (const [group, tag] of wanted) {
            const key = group + '\u0000' + tag;
            let list = tagIndex.get(key);
            if (!list) { list = parser.tag_samples(group, tag).sort((a, b) => times.t[a] - times.t[b]); tagIndex.set(key, list); }
            if (!list.length) { out.values.push(null); continue; }
            let lo = 0, hi = list.length - 1, found = 0;
            while (lo <= hi) {
                const mid = (lo + hi) >> 1;
                if (times.t[list[mid]] <= ms) { found = mid; lo = mid + 1; } else hi = mid - 1;
            }
            const index = list[found];
            out.values.push({ index, t: times.t[index], value: parser.tag_value(index, group, tag) });
        }
        return out;
    },
    /// The times of up to `max` samples that have a tag, evenly spread over them (the first and last included)
    tagTimes({ group, tag, max = 300 }) {
        const key = group + '\u0000' + tag;
        let list = tagIndex.get(key);
        if (!list) { list = parser.tag_samples(group, tag).sort((a, b) => times.t[a] - times.t[b]); tagIndex.set(key, list); }
        const n = list.length, out = [];
        if (!n) return out;
        const k = Math.min(n, max);
        for (let i = 0; i < k; i++) out.push(times.t[list[k === 1 ? 0 : Math.round(i * (n - 1) / (k - 1))]]);
        return out;
    },
    /// `at`'s values for each of `times`, in one call
    atMany({ times: list, wanted }) {
        return list.map(ms => ops.at({ ms, wanted, samples: false }).values);
    },
    sample({ index, wanted }) {
        return parser.sample(index, wanted, 1200);
    },
    series({ group, tag }) {
        const s = parser.series(group, tag);
        return { value: s, transfer: [s.t.buffer, s.v.buffer] };
    },
    /// A tag's value at `ms`, of the latest sample at or before it that has it: `{ index, text }`, only the index when
    /// that's `last`, the sample the page shows already
    tagJsonAt({ ms, group, tag, last }) {
        const index = latestWith(group, tag, ms);
        if (index < 0 || index === last) return { index };
        return { index, text: parser.tag_json(index, group, tag) };
    },
    tagJson({ index, group, tag }) {
        return parser.tag_json(index, group, tag);
    },
    exportJson({ human }) {
        return parser.telemetry_json(!!human);
    },
    stats() {
        return { ...parser.read_stats(), log: take_log() };
    },
};

onmessage = async ({ data }) => {
    const { id, op, args } = data;
    try {
        await ready;
        let result = await ops[op](args || {});
        let transfer = [];
        if (result && typeof result === 'object' && 'transfer' in result && 'value' in result) {
            transfer = result.transfer;
            result = result.value;
        }
        postMessage({ id, ok: true, result }, transfer);
    } catch (e) {
        postMessage({ id, ok: false, error: errorText(e), log: (() => { try { return take_log(); } catch { return []; } })() });
    }
};
