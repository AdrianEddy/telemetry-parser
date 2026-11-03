// SPDX-License-Identifier: MIT OR Apache-2.0
// Gyroflow Telemetry Viewer: telemetry-parser in a worker, the video in a <video> element straight from the file
// (a blob: URL, which the browser reads in ranges as it plays), and the metadata on a shared timeline.

import { Timeline } from './timeline.js';
import { VIEW_TYPES } from './views.js';
import { fmtTime, fmtNum, fmtBytes, fmtCount, fmtNativeId, el, download, css } from './util.js';
import { JsonTree, parse as parseJson, stringify as stringifyJson, pathText } from './jsonview.js';

const $ = s => document.querySelector(s);

// Containers a browser may be able to play; anything else is opened as telemetry only
const VIDEO_EXT = new Set(['mp4', 'mov', 'm4v', 'mkv', 'webm', 'insv', 'lrv', '360', 'osv', 'avi', 'mxf', 'mts', 'm2ts', 'crm']);
const PLAYABLE_GUESS = new Set(['mp4', 'mov', 'm4v', 'mkv', 'webm', 'insv', 'lrv', '360', 'osv']);
const ext = name => (name.match(/\.([^.]+)$/)?.[1] || '').toLowerCase();

// ---------------------------------------------------------------------------------------------------------------------
// The worker

class Rpc {
    constructor() {
        this.worker = new Worker(new URL('./worker.js', import.meta.url), { type: 'module' });
        this.seq = 0;
        this.pending = new Map();
        this.onProgress = null;
        this.worker.onmessage = ({ data }) => {
            if (data.type === 'progress') { this.onProgress?.(data.value); return; }
            const p = this.pending.get(data.id);
            if (!p) return;
            this.pending.delete(data.id);
            if (data.ok) p.resolve(data.result); else { const e = new Error(data.error); e.log = data.log; p.reject(e); }
        };
        this.worker.onerror = e => {
            for (const p of this.pending.values()) p.reject(new Error(e.message || 'The parser stopped'));
            this.pending.clear();
        };
    }
    call(op, args) {
        const id = ++this.seq;
        return new Promise((resolve, reject) => {
            this.pending.set(id, { resolve, reject });
            this.worker.postMessage({ id, op, args });
        });
    }
    terminate() { this.worker.terminate(); for (const p of this.pending.values()) p.reject(new Error('Cancelled')); this.pending.clear(); }
}

// ---------------------------------------------------------------------------------------------------------------------
// The clock: the video's when there's one to play, otherwise a timer over the telemetry's span

class Clock {
    constructor(loop = false) { this.video = null; this.base = 0; this.wall = 0; this.playing = false; this.rate = 1; this.start = 0; this.end = 0; this._loop = loop; }
    get loop() { return this._loop; }
    set loop(on) { this._loop = on; if (this.video) this.video.loop = on; }
    attach(video) { this.video = video; video.loop = this._loop; }
    get time() {
        if (this.video) return this.video.currentTime * 1000;
        if (!this.playing) return this.base;
        const t = this.base + (performance.now() - this.wall) * this.rate;
        if (t >= this.end) {
            if (this._loop && this.end > this.start) {
                // Start over, carrying what ran past the end
                this.base = this.start + (t - this.end) % (this.end - this.start);
                this.wall = performance.now();
                return this.base;
            }
            this.playing = false; this.base = this.end; app.onPlayState(); return this.end;
        }
        return t;
    }
    play() {
        if (this.video) { this.video.playbackRate = this.rate; this.video.play().catch(() => { }); return; }
        if (this.base >= this.end) this.base = this.start;
        this.wall = performance.now();
        this.playing = true;
        app.onPlayState();
    }
    pause() {
        if (this.video) { this.video.pause(); return; }
        this.base = this.time;
        this.playing = false;
        app.onPlayState();
    }
    get isPlaying() { return this.video ? !this.video.paused && !this.video.ended : this.playing; }
    seek(ms, fast = false) {
        if (this.video) {
            const d = (this.video.duration || 0) * 1000;
            const t = Math.max(0, Math.min(d || ms, ms)) / 1000;
            if (fast && this.video.fastSeek) this.video.fastSeek(t); else this.video.currentTime = t;
            return;
        }
        this.base = Math.max(this.start, Math.min(this.end, ms));
        this.wall = performance.now();
    }
    setRate(r) { this.base = this.time; this.wall = performance.now(); this.rate = r; if (this.video) this.video.playbackRate = r; }
}

// ---------------------------------------------------------------------------------------------------------------------

const PALETTE = ['--series-1', '--series-2', '--series-3', '--series-4', '--series-5', '--series-6'];

const app = window.app = {
    rpc: null,
    clock: new Clock(),
    tags: [],
    info: null,
    imu: null,
    videoMeta: null,
    domain: [0, 1000],
    views: [],
    tiles: [],
    primary: null,
    hidden: new Set(),
    collapsed: new Set(),
    seriesCache: new Map(),
    lastMs: -1,

    async series(group, tag) {
        const key = group + '\u0000' + tag;
        if (!this.seriesCache.has(key)) this.seriesCache.set(key, this.rpc.call('series', { group, tag }));
        return this.seriesCache.get(key);
    },
    async valuesAt(ms, wanted) {
        return (await this.rpc.call('at', { ms, wanted, samples: false })).values;
    },
    onPlayState() { updateTransport(); },
    /// A series' times on the video's timeline: the motion data of an off-speed clip is timed by its capture
    motionTimes(group, t) {
        return this.motionScale !== 1 && ['Gyroscope', 'Accelerometer', 'Magnetometer', 'Quaternion', 'CameraOrientation', 'ImageOrientation', 'GravityVector'].includes(group) ? t.map(x => x * this.motionScale) : t;
    },
};

// Scrubbing pauses the clip while the pointer is held and plays it on after, from where it was let go
let scrub = null;
const timeline = new Timeline($('#timeline'), {
    onSeek: (ms, scrubbing) => { if (scrub) scrub.ms = ms; seek(ms, scrubbing); },
    onScrub: on => {
        if (on) {
            scrub = { wasPlaying: app.clock.isPlaying, ms: app.clock.time };
            if (scrub.wasPlaying) app.clock.pause();
        } else if (scrub) {
            const s = scrub;
            scrub = null;
            seek(s.ms); // exact, after the keyframe seeks of the scrub
            if (s.wasPlaying) app.clock.play();
            updateTransport();
        }
    },
});
$('#timeline').addEventListener('laneremoved', e => { syncPlotButtons(); });

// ---------------------------------------------------------------------------------------------------------------------
// Opening files

// The formats a clip is several files of (a RED clip's parts and RMD, a CinemaDNG sequence) or that are a clip of
// their own, in the order a folder's file to parse is picked; then the motion logs that go with a video
const CLIP_EXT = ['r3d', 'braw', 'mcraw', 'dng', 'ari', 'arx', 'zraw', 'crm', 'mxf'];
const LOG_EXT = ['gcsv', 'jsonl', 'bbl', 'bfl', 'bin', 'log', 'csv', 'txt', 'rmd'];

/// Opens what was picked or dropped: a file, several (a video and its log, a RED clip's parts), or a folder. All of them
/// are given to the parser, which opens a clip's other files by name next to the one it parses; a playable video among
/// them plays along (a RED clip's proxy, the video of a log)
async function openFiles(input) {
    const entries = [...input].map(x => (x instanceof File ? { file: x, path: '/' + (x.webkitRelativePath || x.name) } : x)).filter(e => e?.file);
    if (!entries.length) return;
    entries.sort((a, b) => a.path.localeCompare(b.path, undefined, { numeric: true }));
    const first = list => { for (const e of list) { const hit = entries.find(x => ext(x.file.name) === e); if (hit) return hit; } return null; };
    const video = entries.find(e => PLAYABLE_GUESS.has(ext(e.file.name))) || null;
    const main = first(CLIP_EXT) || first(LOG_EXT) || video || entries.find(e => VIDEO_EXT.has(ext(e.file.name))) || entries[0];
    await openFile(main.file, video?.file || null, main.path, entries);
}

async function openFile(file, videoFile, path = '/' + file.name, entries = [{ file, path: '/' + file.name }]) {
    reset();
    document.body.dataset.state = 'loading';
    $('#load-name').textContent = file.name;
    $('#load-size').textContent = fmtBytes(file.size);
    $('#load-bar').style.setProperty('--p', 0);
    $('#load-pct').textContent = '';
    app.rpc = new Rpc();
    app.rpc.onProgress = p => { $('#load-bar').style.setProperty('--p', p); $('#load-pct').textContent = `${Math.round(p * 100)}%`; };
    app.file = file;
    app.files = entries;

    // The video starts loading while the telemetry is parsed
    const videoReady = videoFile ? setupVideo(videoFile) : Promise.resolve(false);

    let opened;
    try {
        opened = await app.rpc.call('open', { file, path, files: entries.map(e => ({ path: e.path, file: e.file })) });
    } catch (e) {
        showError(file, e);
        return;
    }
    app.info = opened.info;
    app.tags = opened.tags;
    app.stats = opened.stats;
    app.log = opened.log || [];

    const [imu, hasVideo] = await Promise.all([app.rpc.call('imu').catch(() => null), videoReady]);
    app.imu = imu && ['gyro', 'accl', 'magn'].some(k => imu[k]?.t.length) ? imu : null;

    // An off-speed clip's motion data is timed by its capture, at the rate the file states (Default / FrameRate), and
    // the video plays at another one: the motion data is put on the video's timeline, scaled by capture / video rate.
    // When the rates differ is Gyroflow's rule (`capture_frame_rate`): any difference in a Blackmagic RAW clip, which
    // states both as exact ratios, more than a frame per second otherwise
    app.motionScale = 1;
    app.captureRate = null;
    if (app.tags.some(t => t.group === 'Default' && t.tag === 'FrameRate' && t.kind === 'scalar')) {
        const rate = (await app.valuesAt(app.info.start_ms ?? 0, [['Default', 'FrameRate']]))[0]?.value;
        const md = await app.rpc.call('videoMetadata').catch(() => null);
        app.videoMeta = md;
        if (typeof rate === 'number' && rate > 0 && md?.fps > 0) {
            const off = app.info.parser === 'BlackmagicBraw' ? Math.abs(rate - md.fps) > 1e-6 * md.fps : Math.abs(rate - md.fps) > 1;
            if (off) { app.motionScale = rate / md.fps; app.captureRate = rate; }
        }
    }
    if (app.imu && app.motionScale !== 1) for (const k of ['gyro', 'accl', 'magn']) { const s = app.imu[k]; if (s) s.t = s.t.map(x => x * app.motionScale); }

    // The span: the video's and the telemetry's
    const spans = [];
    if (app.info.start_ms !== null) spans.push(app.info.start_ms, app.info.end_ms);
    for (const k of ['gyro', 'accl', 'magn']) { const s = app.imu?.[k]; if (s?.t.length) spans.push(s.t[0], s.t[s.t.length - 1]); }
    if (hasVideo) spans.push(0, app.clock.video.duration * 1000);
    const start = spans.length ? Math.min(...spans) : 0, end = spans.length ? Math.max(...spans) : 1000;
    app.domain = [start, end > start ? end : start + 1000];
    app.clock.start = app.domain[0];
    app.clock.end = app.domain[1];
    app.clock.base = hasVideo ? 0 : app.domain[0];
    timeline.setDomain(...app.domain);

    document.body.dataset.state = 'ready';
    document.title = `${file.name} · Gyroflow Telemetry Viewer`;
    renderHeader(file, hasVideo);
    addImuLanes();
    // Without readings Gyroflow can normalize, the camera's own orientation is the motion data worth plotting
    if (!app.imu) {
        const quat = app.tags.find(t => ['Quaternion', 'CameraOrientation'].includes(t.group) && t.tag === 'Data' && t.channels?.length);
        if (quat) await addTagLane(quat.group, quat.tag);
    }
    await buildViews(hasVideo);
    renderFilePanel();
    $('#sidebar').dataset.tab = 'tags';
    syncTabs();
    app.lastMs = -1;
    onTime(app.clock.time, true);

    // The video track's size and frame rate: for a large file this rereads the ends of it, so it comes after the
    // first view is up
    const rpc = app.rpc;
    const md = app.videoMeta || await rpc.call('videoMetadata').catch(() => null);
    if (rpc !== app.rpc) return; // Another file was opened meanwhile
    app.videoMeta = md;
    if (md?.duration_s && md.duration_s * 1000 > app.domain[1] && !app.clock.video) {
        app.domain[1] = md.duration_s * 1000;
        app.clock.end = app.domain[1];
        timeline.setDomain(...app.domain);
    }
    const meta = document.querySelector('.tile-video .tile-meta');
    if (meta && md) meta.textContent = `${md.width}×${md.height}${md.fps ? ' · ' + fmtNum(md.fps) + ' fps' : ''}`;
    const stats = await rpc.call('stats').catch(() => null);
    if (stats) { app.stats = { ...app.stats, calls: stats.calls, bytes: stats.bytes }; app.log.push(...(stats.log || [])); updateReadStats(); }
    for (const v of app.views) v.key = null;
    renderFilePanel();
    onTime(app.clock.time, true);
}

function reset() {
    app.rpc?.terminate();
    app.rpc = null;
    if (app.clock.video) { app.clock.video.pause(); URL.revokeObjectURL(app.clock.video.src); }
    app.clock = new Clock(app.clock.loop);
    app.seriesCache = new Map();
    app.views = [];
    app.tiles = [];
    app.primary = null;
    app.hidden = new Set();
    app.imu = null;
    app.videoMeta = null;
    app.videoNote = null;
    if ($('#json-dialog').open) $('#json-dialog').close();
    timeline.clear();
    $('#wall').replaceChildren();
    $('#tile-toggles').replaceChildren();
    $('#inspector-list').replaceChildren();
    rows.clear();
    groupsEl.clear();
    tracksEl.clear();
}

function showError(file, e) {
    document.body.dataset.state = 'error';
    $('#error-title').textContent = `${file.name} couldn't be read`;
    const msg = e.message || String(e);
    $('#error-text').textContent = /Unsupported file format/i.test(msg)
        ? 'telemetry-parser found no metadata format it knows in this file. If it comes from a camera on the list, the file may be a re-encoded copy: editors and phones often drop the metadata track.'
        : msg;
    const log = (e.log || []).map(l => `${l.level}: ${l.message}`).join('\n');
    $('#error-log').textContent = log;
    $('#error-log').hidden = !log;
}

/// A <video> for the file, from a blob: URL. Resolves to whether the browser can show its picture
function setupVideo(file) {
    return new Promise(resolve => {
        const video = el('video', { playsinline: true, preload: 'auto', muted: true });
        video.muted = true;
        const url = URL.createObjectURL(file);
        let done = false;
        const finish = ok => {
            if (done) return;
            done = true;
            if (ok) {
                app.clock.attach(video);
                video.addEventListener('play', () => app.onPlayState());
                video.addEventListener('pause', () => app.onPlayState());
                video.addEventListener('ended', () => app.onPlayState());
                video.addEventListener('seeked', () => onTime(app.clock.time, true));
            } else {
                URL.revokeObjectURL(url);
                app.videoNote = `${ext(file.name).toUpperCase()} video: this browser can't decode it, so the timeline plays without picture`;
            }
            resolve(ok);
        };
        video.addEventListener('loadeddata', () => finish(video.videoWidth > 0));
        video.addEventListener('loadedmetadata', () => { if (!video.videoWidth) setTimeout(() => finish(video.videoWidth > 0), 1500); });
        video.addEventListener('error', () => finish(false));
        setTimeout(() => finish(video.readyState >= 2 && video.videoWidth > 0), 15000);
        video.src = url;
        app.pendingVideo = video;
    });
}

// ---------------------------------------------------------------------------------------------------------------------
// Header and file panel

function renderHeader(file, hasVideo) {
    const i = app.info;
    const others = (app.files?.length || 1) - 1;
    $('#file-name').textContent = others ? `${file.name} + ${others} more` : file.name;
    $('#file-name').title = (app.files || []).map(e => e.path).join('\n') || file.name;
    const cam = [i.camera, i.model].filter(Boolean).join(' ');
    $('#file-camera').textContent = cam || 'Unknown camera';
    $('#file-parser').textContent = i.parser;
    $('#file-parser').hidden = !i.parser || i.parser === i.camera;
    updateReadStats();
}

function updateReadStats() {
    const s = app.stats;
    if (!s) return;
    // Of all the files given: a clip can be several (`openFiles`)
    const total = (app.files || [{ file: app.file }]).reduce((sum, e) => sum + e.file.size, 0);
    const pct = total ? s.bytes / total * 100 : 0;
    // A small file is read whole, some parts more than once: the share of the file only says something for large ones
    $('#read-stats').textContent = s.bytes < total ? `read ${fmtBytes(s.bytes)} of ${fmtBytes(total)}` : `${fmtBytes(total)}, read in full`;
    $('#read-stats').title = `${fmtCount(s.calls)} reads, ${pct < 0.1 ? '<0.1' : pct.toFixed(pct < 10 ? 1 : 0)}% of the file, parsed in ${(s.parseMs / 1000).toFixed(2)} s`;
}

function renderFilePanel() {
    const i = app.info, md = app.videoMeta, s = app.stats;
    const rows = [];
    const add = (k, v) => { if (v !== null && v !== undefined && v !== '') rows.push(el('div', { class: 'kv' }, el('dt', { text: k }), el('dd', { text: String(v) }))); };
    add('Camera', [i.camera, i.model].filter(Boolean).join(' ') || 'Unknown');
    add('Parser', i.parser);
    add('File', `${app.file.name}`);
    if (app.files?.length > 1) add('Files given', `${fmtCount(app.files.length)}, the clip's other files opened from them by name`);
    add('Size', `${fmtBytes(i.size)} (${fmtCount(i.size)} bytes)`);
    add('Read', s ? `${fmtBytes(s.bytes)} in ${fmtCount(s.calls)} reads, ${(s.parseMs / 1000).toFixed(2)} s` : null);
    add('Samples', `${fmtCount(i.sample_count)} in ${i.tracks.length} track${i.tracks.length === 1 ? '' : 's'}`);
    if (i.start_ms !== null) add('Telemetry span', `${fmtTime(i.start_ms)} – ${fmtTime(i.end_ms)}`);
    if (md) add('Video', `${md.width} × ${md.height}, ${md.fps ? fmtNum(md.fps) + ' fps' : 'fps unknown'}${md.rotation ? `, rotated ${md.rotation}°` : ''}, ${fmtTime(md.duration_s * 1000)}`);
    if (app.captureRate) add('Off-speed', `captured at ${fmtNum(app.captureRate)} fps: the motion data is drawn ×${fmtNum(app.motionScale)} slower, on the video's timeline`);
    if (app.videoNote) add('Playback', app.videoNote);
    if (i.frame_readout_time) add('Frame readout', `${fmtNum(i.frame_readout_time)} ms`);
    add('Camera timestamps', i.has_accurate_timestamps ? 'Accurate' : 'Not accurate');
    if (i.segments !== null) add('Segment table', `${i.segments} segments`);
    if (i.segment_table_error) add('Segment table', `Unusable: ${i.segment_table_error}`);
    if (app.imu) {
        // Each sensor on its own timeline, at its own rate
        const names = { gyro: 'Gyroscope', accl: 'Accelerometer', magn: 'Magnetometer' };
        for (const k of ['gyro', 'accl', 'magn']) {
            const s = app.imu[k];
            if (!s?.t.length) continue;
            const t = s.t, n = t.length;
            const rate = n > 1 ? (n - 1) / ((t[n - 1] - t[0]) / 1000) : 0;
            add(names[k], `${fmtCount(n)} readings, about ${fmtNum(rate)} Hz`);
        }
    } else add('Motion data', 'None');
    $('#file-info').replaceChildren(...rows);

    const log = app.log || [];
    $('#file-log').replaceChildren(...log.map(l => el('li', { class: `log-${l.level}` }, el('span', { class: 'log-level', text: l.level }), el('span', { text: l.message }))));
    $('#file-log-section').hidden = !log.length;
    $('#tab-file .badge').textContent = log.length ? String(log.length) : '';
    $('#tab-file .badge').classList.toggle('badge-error', log.some(l => l.level === 'error'));
    $('#export-imu').disabled = !app.imu;
}

// ---------------------------------------------------------------------------------------------------------------------
// Chart lanes

function axisColors() { return [css('--axis-x'), css('--axis-y'), css('--axis-z'), css('--axis-w')]; }

function addImuLanes() {
    if (!app.imu) return;
    const [cx, cy, cz] = axisColors();
    const xyz = [{ name: 'x', color: cx }, { name: 'y', color: cy }, { name: 'z', color: cz }];
    const lanes = [['gyro', 'Gyroscope', '°/s'], ['accl', 'Accelerometer', 'm/s²'], ['magn', 'Magnetometer', '']];
    for (const [k, title, unit] of lanes) {
        const s = app.imu[k];
        if (!s?.t.length) continue;
        timeline.addLane({ id: 'imu:' + k, title, unit, t: s.t, v: s.v, channels: xyz.map(c => ({ ...c })), height: k === 'gyro' ? 150 : 120 });
    }
}

async function addTagLane(group, tag) {
    const id = `tag:${group}\u0000${tag}`;
    if (timeline.lanes.some(l => l.id === id)) { timeline.removeLane(id); syncPlotButtons(); return; }
    const info = app.tags.find(t => t.group === group && t.tag === tag);
    const btns = document.querySelectorAll(`[data-plot="${CSS.escape(group + '\u0000' + tag)}"]`);
    btns.forEach(b => b.classList.add('busy'));
    try {
        const s = await app.series(group, tag);
        if (!s.t.length) { toast(`${info?.description || tag} has no numbers to plot`); return; }
        const t = app.motionTimes(group, s.t);
        const xyz = { x: 0, y: 1, z: 2, w: 3 };
        const axis = axisColors();
        const pal = PALETTE.map(css);
        const channels = s.channels.map((name, i) => {
            const short = name.replace(/^v\./, '');
            return { name: short, color: short in xyz && s.channels.length <= 4 ? axis[xyz[short]] : pal[i % pal.length], visible: i < 12 };
        });
        timeline.addLane({ id, title: `${group} · ${info?.description || tag}`, unit: s.readings ? `${fmtCount(s.t.length)} readings` : '', t, v: s.v, channels, removable: true, height: 110 });
        if (s.truncated) toast('That series is very long: only its first part is plotted');
        // Scroll the new lane into view
        timeline.lanes[timeline.lanes.length - 1]?.el.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
    } catch (e) {
        toast(`Couldn't plot ${tag}: ${e.message}`);
    } finally {
        btns.forEach(b => b.classList.remove('busy'));
        syncPlotButtons();
    }
}

function syncPlotButtons() {
    const on = new Set(timeline.lanes.map(l => l.id));
    document.querySelectorAll('[data-plot]').forEach(b => b.classList.toggle('on', on.has('tag:' + b.dataset.plot)));
}

// ---------------------------------------------------------------------------------------------------------------------
// The monitor wall

async function buildViews(hasVideo) {
    const wall = $('#wall');
    if (hasVideo) {
        const v = app.clock.video;
        const md = app.videoMeta;
        const tile = el('section', { class: 'tile tile-video', 'data-view': 'video' },
            el('header', { class: 'tile-label' }, el('span', { class: 'tile-name', text: 'Video' }),
                el('span', { class: 'tile-meta', text: md ? `${md.width}×${md.height}${md.fps ? ' · ' + fmtNum(md.fps) + ' fps' : ''}` : `${v.videoWidth}×${v.videoHeight}` })),

            el('div', { class: 'tile-tools' }),
            v);
        app.tiles.push({ id: 'video', name: 'Video', el: tile });
    }
    // The views load what they draw from the whole file first: one that finds nothing to draw (say, a focal plane table
    // that is empty in every frame) gets no tile
    const candidates = [];
    for (const T of VIEW_TYPES) {
        let ok = false;
        try { ok = T.detect(app.tags, app); } catch { ok = false; }
        if (ok) candidates.push(new T(app));
    }
    await Promise.all(candidates.map(v => v.init().catch(e => console.warn(v.id, e))));
    for (const view of candidates) {
        if (view.available === false) continue;
        app.views.push(view);
        app.tiles.push({ id: view.id, name: view.el.querySelector('.tile-name').textContent, el: view.el, view });
    }
    $('#viewer').dataset.tiles = app.tiles.length;
    if (!app.tiles.length) {
        const kind = ext(app.file.name).toUpperCase();
        const where = timeline.lanes.length ? 'Its tags are in the panel on the right and its motion data in the charts below.' : 'Its tags are in the panel on the right; press a tag\'s chart button to plot it.';
        wall.append(el('div', { class: 'wall-empty' },
            el('strong', { text: 'No picture and no views for this file' }),
            el('span', { text: `${app.videoNote ? app.videoNote + '.' : `${kind} files have no picture a browser can play.`} ${where}` })));
    }
    for (const t of app.tiles) {
        wall.append(t.el);
        const name = t.el.querySelector('.tile-name');
        const max = el('button', { class: 'icon-btn tile-max', title: 'Make this the large tile', 'aria-label': `Make ${t.name} the large tile`, text: '⤢' });
        max.addEventListener('click', () => { app.primary = t.id; layoutWall(); });
        t.el.querySelector('.tile-label').append(max);
        t.el.addEventListener('dblclick', e => { if (e.target.closest('select, a, button')) return; app.primary = t.id; layoutWall(); });
        const chip = el('button', { class: 'toggle on', 'aria-pressed': 'true', text: t.name });
        chip.addEventListener('click', () => {
            if (app.hidden.has(t.id)) app.hidden.delete(t.id); else app.hidden.add(t.id);
            chip.classList.toggle('on', !app.hidden.has(t.id));
            chip.setAttribute('aria-pressed', String(!app.hidden.has(t.id)));
            layoutWall();
        });
        $('#tile-toggles').append(chip);
        void name;
    }
    if (app.videoNote) $('#tile-toggles').append(el('span', { class: 'toggle-note', text: app.videoNote }));
    app.primary = app.tiles[0]?.id ?? null;
    layoutWall();
    app.wanted = [];
    for (const v of app.views) { v.wantedAt = app.wanted.length; app.wanted.push(...v.wanted); }
}

function layoutWall() {
    const wall = $('#wall');
    const tiles = app.tiles.filter(t => !app.hidden.has(t.id));
    for (const t of app.tiles) t.el.hidden = app.hidden.has(t.id);
    const n = tiles.length;
    const primary = tiles.find(t => t.id === app.primary) || tiles[0];
    for (const t of tiles) { t.el.classList.toggle('primary', t === primary && n > 2); t.el.style.gridRow = ''; t.el.style.gridColumn = ''; t.el.style.order = t === primary ? -1 : 0; }
    const narrow = wall.clientWidth < 700;
    let cols = '1fr', rws = '1fr';
    if (narrow) { cols = '1fr'; rws = `repeat(${Math.max(n, 1)}, minmax(240px, 1fr))`; }
    else if (n === 2) { const side = wall.clientWidth / wall.clientHeight > 1.3; cols = side ? '1fr 1fr' : '1fr'; rws = side ? '1fr' : '1fr 1fr'; }
    else if (n === 3) { cols = '2fr 1fr'; rws = '1fr 1fr'; primary.el.style.gridRow = 'span 2'; }
    else if (n === 4) { cols = '2fr 1fr'; rws = '1fr 1fr 1fr'; primary.el.style.gridRow = 'span 3'; }
    else if (n >= 5) { cols = '2fr 1fr 1fr'; rws = n <= 5 ? '1fr 1fr' : '1fr 1fr 1fr'; primary.el.style.gridRow = n <= 5 ? 'span 2' : 'span 3'; }
    wall.style.gridTemplateColumns = cols;
    wall.style.gridTemplateRows = rws;
    wall.dataset.n = n;
}
new ResizeObserver(() => { if (app.tiles.length) layoutWall(); }).observe($('#wall'));

// ---------------------------------------------------------------------------------------------------------------------
// The tags at the playhead

const rows = new Map();     // key → { el, val, text }
const groupsEl = new Map(); // track/group → { el, body, count }
const tracksEl = new Map(); // track → { el, head }
let filter = '';

function tagInfo(group, tag) { return app.tags.find(t => t.group === group && t.tag === tag); }

function renderInspector(samples) {
    const list = $('#inspector-list');
    const multi = samples.filter(s => !s.held).length > 1;
    const seen = new Set();
    const order = [];
    for (const s of samples) {
        const sm = s.sample;
        const tk = s.held ? `h${s.index}` : `t${sm.track}`;
        let tr = tracksEl.get(tk);
        if (!tr) {
            tr = { el: el('div', { class: 'track' }), head: el('div', { class: 'track-head' }) };
            tr.el.append(tr.head);
            tracksEl.set(tk, tr);
        }
        tr.head.replaceChildren(
            el('span', { class: 'track-name', text: s.held ? 'Earlier sample' : multi ? `Track ${sm.track}` : 'Sample', title: s.held ? 'Tags only a few samples have, from the latest of them at or before the playhead' : null }),
            el('span', { class: 'track-pos', text: `${fmtCount(s.position + 1)} / ${fmtCount(s.count)}` }),
            el('span', { class: 'track-time', text: `${fmtTime(sm.timestamp_ms)} + ${fmtNum(sm.duration_ms)} ms` }));
        tr.el.classList.toggle('stale', !s.inside && !s.held);
        tr.el.classList.toggle('held', !!s.held);
        order.push(tr.el);
        const groupOrder = [];
        for (const g of sm.groups) {
            const gk = `${tk}/${g.name}`;
            let ge = groupsEl.get(gk);
            if (!ge) {
                const summary = el('summary', {}, el('span', { class: 'group-name', text: g.name }), el('span', { class: 'group-count' }));
                const det = el('details', { class: 'group' }, summary, el('div', { class: 'group-body' }));
                det.open = !app.collapsed.has(g.name);
                det.addEventListener('toggle', () => { if (det.open) app.collapsed.delete(g.name); else app.collapsed.add(g.name); });
                ge = { el: det, body: det.querySelector('.group-body'), count: summary.querySelector('.group-count') };
                groupsEl.set(gk, ge);
            }
            const rowOrder = [];
            let visible = 0;
            for (const t of g.tags) {
                const key = `${gk}/${t.tag}`;
                seen.add(key);
                let r = rows.get(key);
                if (!r) { r = makeRow(g.name, t, sm); rows.set(key, r); }
                r.index = sm.index;
                const text = t.text + (t.truncated ? ' …' : '');
                if (r.text !== text) { r.text = text; r.val.textContent = text; }
                const n = t.kind === 'readings' || t.kind === 'list' ? t.len : null;
                const countText = n !== null && n !== undefined ? `${fmtCount(n)}×` : '';
                if (r.countText !== countText) { r.countText = countText; r.count.textContent = countText; r.count.hidden = !countText; }
                r.more.hidden = !t.truncated && t.kind !== 'struct' && t.kind !== 'readings' && t.kind !== 'bytes';
                r.val.classList.toggle('openable', !r.more.hidden);
                r.val.title = r.more.hidden ? '' : 'Show the full value';
                const match = !filter || r.search.includes(filter);
                r.el.hidden = !match;
                if (match) visible++;
                rowOrder.push(r.el);
            }
            syncChildren(ge.body, rowOrder);
            ge.count.textContent = filter ? `${visible} of ${g.tags.length}` : String(g.tags.length);
            ge.el.hidden = visible === 0;
            if (filter && visible) ge.el.open = true;
            groupOrder.push(ge.el);
        }
        syncChildren(tr.el, [tr.head, ...groupOrder]);
    }
    syncChildren(list, order);
    for (const [k, r] of rows) if (!seen.has(k)) { r.el.remove(); rows.delete(k); }
    $('#inspector-empty').hidden = samples.length > 0;
}

/// Puts `wanted` in `parent` in this order, moving only what's out of place
function syncChildren(parent, wanted) {
    let cur = parent.firstChild;
    for (const w of wanted) {
        if (cur === w) { cur = cur.nextSibling; continue; }
        parent.insertBefore(w, cur);
    }
    while (cur) { const next = cur.nextSibling; cur.remove(); cur = next; }
}

function makeRow(group, t) {
    const info = tagInfo(group, t.tag);
    const plottable = info && info.channels?.length && !['bytes', 'text', 'list', 'empty'].includes(info.kind);
    const nid = fmtNativeId(t.native_id);
    const idText = nid && nid !== t.tag ? `${t.tag} · ${nid}` : t.tag;
    const val = el('div', { class: 'tag-val' });
    const count = el('span', { class: 'tag-count', hidden: true });
    const more = el('button', { class: 'icon-btn', title: 'Show the full value', 'aria-label': `Show the full value of ${t.description}`, text: '{ }' });
    const plot = plottable ? el('button', { class: 'icon-btn plot-btn', title: 'Plot over time', 'aria-label': `Plot ${t.description} over time`, 'data-plot': group + '\u0000' + t.tag }) : null;
    if (plot) plot.innerHTML = '<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M1 12 L5 7 L8 9.5 L11 4 L15 8" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round" stroke-linecap="round"/></svg>';
    const row = el('div', { class: 'tag' },
        el('div', { class: 'tag-name' }, el('span', { class: 'tag-desc', text: t.description || t.tag }), el('code', { class: 'tag-id', text: idText })),
        el('div', { class: 'tag-body' }, count, val),
        el('div', { class: 'tag-actions' }, more, plot));
    const r = { el: row, val, count, more, text: null, countText: null, search: `${group} ${t.tag} ${t.description} ${nid}`.toLowerCase(), index: 0 };
    more.addEventListener('click', () => showJson(group, t.tag, t.description, r.index));
    // A value with more to it than fits opens in the viewer too
    val.addEventListener('click', () => { if (val.classList.contains('openable')) showJson(group, t.tag, t.description, r.index); });
    plot?.addEventListener('click', () => addTagLane(group, t.tag));
    if (plot && timeline.lanes.some(l => l.id === `tag:${group}\u0000${t.tag}`)) plot.classList.add('on');
    return r;
}

// The full value of a tag, as a tree (or as text)
const jsonTree = new JsonTree($('#json-tree'), {
    onSelect: (n, path) => {
        $('#json-path').textContent = path.length ? pathText(path) : '(the whole value)';
        $('#json-copy-path').hidden = !path.length;
        $('#json-copy-node').hidden = false;
    },
});
let jsonText = '';
/// The tag the viewer shows, `{ group, tag, index, follow }`: with `follow`, the value at the playhead
let jsonState = null;

function jsonSubtitle() {
    $('#json-sub').textContent = `${jsonState.group} · ${jsonState.tag} · sample ${fmtCount(jsonState.index + 1)}${jsonState.follow ? ' · at the playhead' : ''}`;
}

/// Shows `text`: a new tag's value as a new tree, the next sample's of the same tag changed in place
function applyJson(text, sameTag) {
    jsonText = text ?? 'null';
    $('#json-body').textContent = jsonText;
    let node;
    try { node = parseJson(jsonText); } catch { jsonAsTree(false); return; } // Not JSON: as it is
    if (sameTag && jsonTree.node) jsonTree.update(node); else jsonTree.set(node);
}

// Following the playhead: one request at a time, the latest time when it's answered
let jsonBusy = false, jsonPending = null;
function followJson(ms) {
    if (!jsonState?.follow || !$('#json-dialog').open) return;
    jsonPending = ms;
    if (!jsonBusy) pumpJson();
}
async function pumpJson() {
    jsonBusy = true;
    while (jsonPending !== null && jsonState && app.rpc) {
        const ms = jsonPending;
        jsonPending = null;
        const st = jsonState;
        try {
            const r = await app.rpc.call('tagJsonAt', { ms, group: st.group, tag: st.tag, last: st.index });
            if (st !== jsonState || r.index < 0 || r.text === undefined) continue;
            st.index = r.index;
            jsonSubtitle();
            applyJson(r.text, true);
        } catch (e) {
            console.warn(e);
        }
    }
    jsonBusy = false;
}

function jsonAsTree(tree) {
    $('#json-tree').hidden = !tree;
    $('#json-body').hidden = tree;
    $('.dlg-tools').hidden = !tree;
    $('.dlg-foot').hidden = !tree;
    $('#json-as-tree').classList.toggle('on', tree); $('#json-as-tree').setAttribute('aria-pressed', String(tree));
    $('#json-as-text').classList.toggle('on', !tree); $('#json-as-text').setAttribute('aria-pressed', String(!tree));
}

async function showJson(group, tag, description, index) {
    const dlg = $('#json-dialog');
    jsonState = { group, tag, index, follow: $('#json-follow').classList.contains('on') };
    $('#json-title').textContent = description || tag;
    jsonSubtitle();
    $('#json-search').value = '';
    $('#json-count').textContent = '';
    $('#json-path').textContent = 'Select a value to see its path';
    $('#json-copy-path').hidden = $('#json-copy-node').hidden = true;
    $('#json-tree').replaceChildren(el('p', { class: 'muted pad', text: 'Reading…' }));
    $('#json-body').textContent = '';
    jsonAsTree(true);
    // Not modal: the transport and the timeline stay usable, so the value can be watched while the clip plays
    if (!dlg.open) dlg.show();
    const st = jsonState;
    let text;
    try { text = await app.rpc.call('tagJson', { index, group, tag }); } catch (e) { text = e.message; }
    if (st !== jsonState) return;
    applyJson(text, false);
    followJson(app.clock.time);
}

async function copyText(text) {
    try { await navigator.clipboard.writeText(text); toast('Copied'); return; } catch { }
    const ta = el('textarea', { style: 'position:fixed;opacity:0' });
    ta.value = text;
    $('#json-dialog').append(ta);
    ta.select();
    const ok = document.execCommand?.('copy');
    ta.remove();
    toast(ok ? 'Copied' : "Couldn't copy: select the text and press Ctrl+C");
}

$('#json-copy').addEventListener('click', () => copyText(jsonText));
$('#json-copy-path').addEventListener('click', () => copyText(pathText(jsonTree.selectedPath || [])));
$('#json-copy-node').addEventListener('click', () => jsonTree.selectedNode && copyText(stringifyJson(jsonTree.selectedNode)));
$('#json-close').addEventListener('click', () => $('#json-dialog').close());
$('#json-dialog').addEventListener('close', () => { jsonState = null; });
$('#json-follow').addEventListener('click', e => {
    const on = !e.currentTarget.classList.contains('on');
    e.currentTarget.classList.toggle('on', on);
    e.currentTarget.setAttribute('aria-pressed', String(on));
    if (jsonState) { jsonState.follow = on; jsonSubtitle(); if (on) followJson(app.clock.time); }
});
$('#json-as-tree').addEventListener('click', () => jsonAsTree(true));
$('#json-as-text').addEventListener('click', () => jsonAsTree(false));
$('#json-expand').addEventListener('click', () => jsonTree.expandAll());
$('#json-collapse').addEventListener('click', () => jsonTree.collapseAll());
{
    let timer = 0;
    $('#json-search').addEventListener('input', e => {
        clearTimeout(timer);
        timer = setTimeout(() => {
            const q = e.target.value.trim();
            const n = jsonTree.search(q);
            $('#json-count').textContent = q ? (n ? `${fmtCount(n)} match${n === 1 ? '' : 'es'}` : 'No matches') : '';
        }, 150);
    });
}

$('#tag-filter').addEventListener('input', e => {
    filter = e.target.value.trim().toLowerCase();
    for (const r of rows.values()) r.el.hidden = filter && !r.search.includes(filter);
    if (app.lastAt) renderInspector(app.lastAt.samples);
});

// ---------------------------------------------------------------------------------------------------------------------
// Time

let atBusy = false, atPending = null;
function requestAt(ms) {
    atPending = ms;
    if (!atBusy) pumpAt();
}
async function pumpAt() {
    atBusy = true;
    while (atPending !== null && app.rpc) {
        const ms = atPending;
        atPending = null;
        try {
            const r = await app.rpc.call('at', { ms, wanted: app.wanted || [] });
            app.lastAt = r;
            renderInspector(r.samples);
            for (const v of app.views) v.setValues(r.values.slice(v.wantedAt, v.wantedAt + v.wanted.length));
        } catch (e) {
            console.warn(e);
        }
    }
    atBusy = false;
}

function onTime(ms, force) {
    if (!force && ms === app.lastMs) return;
    app.lastMs = ms;
    const playing = app.clock.isPlaying;
    timeline.setTime(ms, playing);
    const span = app.domain[1] - app.domain[0];
    $('#tc-now').textContent = fmtTime(ms, 0);
    $('#tc-end').textContent = fmtTime(app.domain[1], span > 3600000 ? 1 : 0);
    const fps = app.videoMeta?.fps;
    $('#tc-frame').textContent = fps ? `frame ${fmtCount(Math.floor(ms / 1000 * fps + 1e-6))}` : '';
    for (const v of app.views) v.setTime(ms);
    if (app.rpc) requestAt(ms);
    followJson(ms);
}

function tick() {
    // While scrubbing the playhead is where the pointer is, whatever frame the video found nearby
    if (document.body.dataset.state === 'ready') onTime(scrub ? scrub.ms : app.clock.time);
    requestAnimationFrame(tick);
}
requestAnimationFrame(tick);

/// `fast` while scrubbing: the nearest keyframe where the browser can seek to one (fastSeek), the exact frame after
function seek(ms, fast = false) {
    app.clock.seek(ms, fast);
    onTime(app.clock.video && fast ? ms : app.clock.time, true);
}

function frameMs() { return app.videoMeta?.fps ? 1000 / app.videoMeta.fps : 1000 / 30; }

function step(frames) {
    if (app.clock.isPlaying) app.clock.pause();
    // Land on frame boundaries: a hair past the start of the frame
    const f = frameMs();
    const cur = Math.floor(app.clock.time / f + 1e-6);
    seek((cur + frames) * f + Math.min(1, f / 10));
}

function updateTransport() {
    const playing = app.clock.isPlaying;
    $('#play').classList.toggle('playing', playing);
    $('#play').setAttribute('aria-label', playing ? 'Pause' : 'Play');
    $('#play').title = playing ? 'Pause (Space)' : 'Play (Space)';
}

$('#play').addEventListener('click', () => { app.clock.isPlaying ? app.clock.pause() : app.clock.play(); updateTransport(); });
$('#step-back').addEventListener('click', () => step(-1));
$('#step-fwd').addEventListener('click', () => step(1));
$('#rate').addEventListener('change', e => app.clock.setRate(Number(e.target.value)));
$('#zoom-in').addEventListener('click', () => timeline.zoom(0.5, app.clock.time));
$('#zoom-out').addEventListener('click', () => timeline.zoom(2, app.clock.time));
$('#zoom-fit').addEventListener('click', () => timeline.fit());
function setLoop(on) {
    app.clock.loop = on;
    $('#loop').classList.toggle('on', on);
    $('#loop').setAttribute('aria-pressed', String(on));
    try { localStorage.setItem('tv-loop', on ? '1' : '0'); } catch { }
}
$('#loop').addEventListener('click', () => setLoop(!app.clock.loop));
try { setLoop(localStorage.getItem('tv-loop') === '1'); } catch { setLoop(false); }
$('#follow').addEventListener('click', e => { timeline.follow = !timeline.follow; e.currentTarget.classList.toggle('on', timeline.follow); e.currentTarget.setAttribute('aria-pressed', String(timeline.follow)); });

document.addEventListener('keydown', e => {
    if (document.body.dataset.state !== 'ready') return;
    if (e.key === 'Escape' && $('#json-dialog').open) { $('#json-dialog').close(); return; }
    // Keys work with the value viewer open too, but not where they type or press its own buttons
    if (e.defaultPrevented || e.target.closest('input, select, textarea')) return;
    if (e.key === ' ' && e.target.closest('button')) return;
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    switch (e.key) {
        case ' ': e.preventDefault(); $('#play').click(); break;
        case 'ArrowLeft': e.preventDefault(); e.shiftKey ? seek(app.clock.time - 1000) : step(-1); break;
        case 'ArrowRight': e.preventDefault(); e.shiftKey ? seek(app.clock.time + 1000) : step(1); break;
        case 'Home': e.preventDefault(); seek(app.domain[0]); break;
        case 'End': e.preventDefault(); seek(app.domain[1]); break;
        case '+': case '=': timeline.zoom(0.5, app.clock.time); break;
        case '-': timeline.zoom(2, app.clock.time); break;
        case 'f': timeline.fit(); break;
        case 'l': setLoop(!app.clock.loop); break;
    }
});

// ---------------------------------------------------------------------------------------------------------------------
// Exports

$('#export-imu').addEventListener('click', async () => {
    let aligned;
    try { aligned = await app.rpc.call('imuAligned'); } catch (e) { toast(`Couldn't export the motion data: ${e.message}`); return; }
    const { t, gyro, accl, magn } = aligned;
    const head = ['time_ms', ...(gyro ? ['gyro_x', 'gyro_y', 'gyro_z'] : []), ...(accl ? ['accl_x', 'accl_y', 'accl_z'] : []), ...(magn ? ['magn_x', 'magn_y', 'magn_z'] : [])];
    const lines = [head.join(',')];
    for (let i = 0; i < t.length; i++) {
        const row = [t[i].toFixed(3)];
        for (const s of [gyro, accl, magn]) if (s) row.push(s[i * 3], s[i * 3 + 1], s[i * 3 + 2]);
        lines.push(row.join(','));
    }
    download(app.file.name.replace(/\.[^.]+$/, '') + '.imu.csv', lines.join('\n'), 'text/csv');
});
$('#export-json').addEventListener('click', async e => {
    const btn = e.currentTarget;
    btn.disabled = true;
    btn.textContent = 'Preparing…';
    try {
        const json = await app.rpc.call('exportJson', { human: false });
        download(app.file.name.replace(/\.[^.]+$/, '') + '.telemetry.json', json, 'application/json');
    } catch (err) { toast(err.message); }
    btn.disabled = false;
    btn.textContent = 'Telemetry as JSON';
});

// ---------------------------------------------------------------------------------------------------------------------
// Chrome: tabs, drag and drop, theme, toast

function syncTabs() {
    const tab = $('#sidebar').dataset.tab;
    for (const b of document.querySelectorAll('.tab')) { const on = b.dataset.tab === tab; b.setAttribute('aria-selected', String(on)); b.tabIndex = on ? 0 : -1; }
    $('#panel-tags').hidden = tab !== 'tags';
    $('#panel-file').hidden = tab !== 'file';
}
for (const b of document.querySelectorAll('.tab')) b.addEventListener('click', () => { $('#sidebar').dataset.tab = b.dataset.tab; syncTabs(); });

for (const input of document.querySelectorAll('.file-input')) input.addEventListener('change', e => { openFiles(e.target.files); e.target.value = ''; });
let dragDepth = 0;
window.addEventListener('dragenter', e => { if (e.dataTransfer?.types.includes('Files')) { e.preventDefault(); dragDepth++; document.body.classList.add('dragging'); } });
window.addEventListener('dragleave', () => { if (--dragDepth <= 0) { dragDepth = 0; document.body.classList.remove('dragging'); } });
window.addEventListener('dragover', e => { if (e.dataTransfer?.types.includes('Files')) e.preventDefault(); });
window.addEventListener('drop', async e => {
    e.preventDefault();
    dragDepth = 0;
    document.body.classList.remove('dragging');
    // The entries have to be taken while the event is handled: a dropped folder is read from them
    const items = [...(e.dataTransfer?.items || [])].map(i => i.webkitGetAsEntry?.()).filter(Boolean);
    if (items.some(x => x.isDirectory)) openFiles(await readEntries(items));
    else if (e.dataTransfer?.files?.length) openFiles(e.dataTransfer.files);
});

/// The files of dropped files and folders, folders' contents included, with their paths
async function readEntries(entries) {
    const out = [];
    const walk = async entry => {
        if (entry.isFile) {
            out.push({ file: await new Promise((res, rej) => entry.file(res, rej)), path: entry.fullPath });
        } else if (entry.isDirectory) {
            const reader = entry.createReader();
            for (;;) {
                const batch = await new Promise((res, rej) => reader.readEntries(res, rej));
                if (!batch.length) break;
                for (const b of batch) await walk(b);
            }
        }
    };
    for (const entry of entries) await walk(entry);
    return out;
}

matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => {
    timeline.readColors();
    const [x, y, z, w] = axisColors();
    for (const l of timeline.lanes) if (l.id.startsWith('imu:')) { l.channels.forEach((c, i) => c.color = [x, y, z, w][i]); l.legend.querySelectorAll('.chip-ch').forEach((ch, i) => ch.style.setProperty('--ch', l.channels[i].color)); }
    for (const v of app.views) v.draw();
});

let toastTimer = 0;
function toast(text) {
    const t = $('#toast');
    t.textContent = text;
    t.hidden = false;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => t.hidden = true, 3200);
}

// Timeline height: drag the splitter
{
    const split = $('#splitter');
    let start = null;
    split.addEventListener('pointerdown', e => { split.setPointerCapture(e.pointerId); start = { y: e.clientY, h: $('#timeline-wrap').getBoundingClientRect().height }; });
    split.addEventListener('pointermove', e => {
        if (!start) return;
        const h = Math.max(140, Math.min(window.innerHeight - 260, start.h - (e.clientY - start.y)));
        document.documentElement.style.setProperty('--timeline-h', h + 'px');
    });
    split.addEventListener('pointerup', () => { start = null; try { localStorage.setItem('tv-timeline-h', getComputedStyle(document.documentElement).getPropertyValue('--timeline-h')); } catch { } });
    try { const h = localStorage.getItem('tv-timeline-h'); if (h) document.documentElement.style.setProperty('--timeline-h', h); } catch { }
}

$('#error-back').addEventListener('click', () => { document.body.dataset.state = 'empty'; });
document.body.dataset.state = 'empty';

// For scripting the page from the console (and for automated checks)
Object.assign(app, { seek, timeline, addTagLane, openFiles });
