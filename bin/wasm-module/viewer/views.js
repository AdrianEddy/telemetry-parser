// SPDX-License-Identifier: MIT OR Apache-2.0
// Views of the metadata that is better seen than plotted: the sensor and its mesh, the lens, the camera's motion,
// the GPS track and the stabilizer's shift. Each is a tile of the monitor wall; it asks for the latest values of the
// tags it draws (`wanted`) and draws them at the playhead.

import { fitCanvas, fmtNum, el, css } from './util.js';
import { lensFromProfile, lensFromTags, lensFromSony, radiusAt, fieldOfView } from './lens.js';

const DEG = Math.PI / 180;
/// The tile's label sits over the top of the canvas: drawings start below it
const TOP = 34;

/// A label with a dark plate behind it, readable over grid lines
function plateText(ctx, c, text, x, y) {
    const w = ctx.measureText(text).width;
    ctx.save();
    ctx.fillStyle = c.bg;
    ctx.globalAlpha = 0.8;
    ctx.fillRect(x - 3, y - 2, w + 6, 15);
    ctx.restore();
    ctx.fillText(text, x, y);
}

function screenColors() {
    return {
        bg: css('--screen'), line: css('--screen-line'), grid: css('--screen-grid'), text: css('--screen-text'),
        muted: css('--screen-muted'), accent: css('--accent-screen'), warm: css('--screen-warm'),
        x: css('--axis-x-screen'), y: css('--axis-y-screen'), z: css('--axis-z-screen'),
        mono: css('--font-mono'), sans: css('--font-sans'),
    };
}

function lowerBound(t, x) {
    let lo = 0, hi = t.length;
    while (lo < hi) { const m = (lo + hi) >> 1; if (t[m] < x) lo = m + 1; else hi = m; }
    return lo;
}
function nearest(t, x) {
    if (!t.length) return -1;
    let i = lowerBound(t, x);
    if (i >= t.length) i = t.length - 1;
    if (i > 0 && Math.abs(t[i - 1] - x) <= Math.abs(t[i] - x)) i--;
    return i;
}

const has = (tags, group, tag) => tags.some(t => t.group === group && t.tag === tag);
const find = (tags, group, tag) => tags.find(t => t.group === group && t.tag === tag);

/// Fits a w × h box into the rectangle (x, y, W, H), centred
function fitBox(w, h, x, y, W, H) {
    const s = Math.min(W / w, H / h);
    return { s, ox: x + (W - w * s) / 2, oy: y + (H - h * s) / 2 };
}

class View {
    constructor(app, { id, label }) {
        this.app = app;
        this.id = id;
        this.values = [];
        this.el = el('section', { class: 'tile', 'data-view': id },
            el('header', { class: 'tile-label' }, el('span', { class: 'tile-name', text: label }), el('span', { class: 'tile-meta' })),
            el('div', { class: 'tile-tools' }),
            el('canvas', { class: 'tile-canvas' }),
            el('footer', { class: 'tile-foot' }));
        this.canvas = this.el.querySelector('canvas');
        this.meta = this.el.querySelector('.tile-meta');
        this.tools = this.el.querySelector('.tile-tools');
        this.foot = this.el.querySelector('.tile-foot');
        new ResizeObserver(() => this.draw()).observe(this.canvas);
    }
    get wanted() { return []; }
    setValues(values) { this.values = values; this.changed(); }
    changed() { this.draw(); }
    setTime(ms) { this.ms = ms; }
    value(i) { return this.values[i]?.value ?? null; }
    setFoot(text) { if (this.foot.textContent !== text) { this.foot.textContent = text; this.foot.title = text; } }
    async init() { }
    draw() { }
}

// ---------------------------------------------------------------------------------------------------------------------

class SensorView extends View {
    static detect(tags) {
        return has(tags, 'MeshCorrection', 'Data') || (has(tags, 'Imager', 'SensorSizePixels') && has(tags, 'Imager', 'CaptureAreaSize'));
    }
    constructor(app) {
        super(app, { id: 'sensor', label: 'Sensor' });
        this.gain = 'auto';
        const sel = el('select', { class: 'mini-select', title: 'How much to exaggerate the mesh displacement', 'aria-label': 'Mesh exaggeration' },
            ...[['auto', 'Mesh: auto'], ['1', 'Mesh ×1'], ['5', 'Mesh ×5'], ['20', 'Mesh ×20'], ['100', 'Mesh ×100']].map(([v, t]) => el('option', { value: v, text: t })));
        sel.addEventListener('change', () => { this.gain = sel.value; this.draw(); });
        if (has(app.tags, 'MeshCorrection', 'Data')) this.tools.append(sel);
    }
    get wanted() {
        return [['Imager', 'SensorSizePixels'], ['Imager', 'CaptureAreaOrigin'], ['Imager', 'CaptureAreaSize'], ['MeshCorrection', 'Data'], ['MeshCorrection', 'Enabled']];
    }
    draw() {
        const c = screenColors();
        const { ctx, W, H } = fitCanvas(this.canvas);
        const sensor = this.value(0), origin = this.value(1), crop = this.value(2), mesh = this.value(3), meshOn = this.value(4);
        const meshSize = Array.isArray(mesh?.size) ? mesh.size : null;
        const size = Array.isArray(sensor) ? sensor : meshSize || (Array.isArray(crop) ? crop : null);
        if (!size || !(size[0] > 0)) { this.empty(ctx, W, H, c, 'No sensor data at this time'); return; }
        const m = 16;
        const { s, ox, oy } = fitBox(size[0], size[1], m, TOP, W - 2 * m, H - TOP - m);
        const X = x => ox + x * s, Y = y => oy + y * s;

        // The sensor
        ctx.fillStyle = c.grid;
        ctx.globalAlpha = 0.35;
        ctx.fillRect(X(0), Y(0), size[0] * s, size[1] * s);
        ctx.globalAlpha = 1;
        ctx.strokeStyle = c.line;
        ctx.lineWidth = 1;
        ctx.strokeRect(X(0) + 0.5, Y(0) + 0.5, size[0] * s - 1, size[1] * s - 1);
        // Optical axis: the sensor centre
        ctx.strokeStyle = c.muted;
        ctx.setLineDash([3, 4]);
        ctx.beginPath();
        ctx.moveTo(X(size[0] / 2), Y(0)); ctx.lineTo(X(size[0] / 2), Y(size[1]));
        ctx.moveTo(X(0), Y(size[1] / 2)); ctx.lineTo(X(size[0]), Y(size[1] / 2));
        ctx.stroke();
        ctx.setLineDash([]);

        let foot = [`${size[0]} × ${size[1]} px`];

        // The mesh: the grid the camera's mesh correction moves, its displacement exaggerated
        if (mesh && Array.isArray(mesh.mesh) && Array.isArray(mesh.divisions)) {
            const [dx, dy] = mesh.divisions;
            const ms = meshSize || size;
            const off = Array.isArray(mesh.offset) ? mesh.offset : [0, 0];
            const ideal = (i, j) => [ms[0] / (dx - 1) * i, ms[1] / (dy - 1) * j];
            let maxShift = 0;
            for (let j = 0; j < dy; j++) for (let i = 0; i < dx; i++) {
                const p = mesh.mesh[j * dx + i]; if (!p) continue;
                const [ix, iy] = ideal(i, j);
                maxShift = Math.max(maxShift, Math.hypot(p[0] - ix, p[1] - iy));
            }
            let gain = Number(this.gain);
            if (this.gain === 'auto') {
                const cell = Math.min(ms[0] / (dx - 1), ms[1] / (dy - 1));
                const want = maxShift > 0 ? cell * 0.3 / maxShift : 1;
                gain = [1, 2, 5, 10, 20, 50, 100, 200, 500].reduce((a, b) => (b <= want ? b : a), 1);
            }
            const P = (i, j) => {
                const p = mesh.mesh[j * dx + i];
                const [ix, iy] = ideal(i, j);
                return p ? [off[0] + ix + (p[0] - ix) * gain, off[1] + iy + (p[1] - iy) * gain] : [ix, iy];
            };
            // Ideal grid
            ctx.strokeStyle = c.grid;
            ctx.beginPath();
            for (let i = 0; i < dx; i++) { const [x0] = ideal(i, 0); ctx.moveTo(X(x0), Y(0)); ctx.lineTo(X(x0), Y(ms[1])); }
            for (let j = 0; j < dy; j++) { const [, y0] = ideal(0, j); ctx.moveTo(X(0), Y(y0)); ctx.lineTo(X(ms[0]), Y(y0)); }
            ctx.stroke();
            // Moved grid
            ctx.strokeStyle = c.warm;
            ctx.globalAlpha = meshOn === false ? 0.4 : 1;
            ctx.lineWidth = 1.25;
            ctx.beginPath();
            for (let j = 0; j < dy; j++) for (let i = 0; i < dx; i++) { const [x, y] = P(i, j); if (i) ctx.lineTo(X(x), Y(y)); else ctx.moveTo(X(x), Y(y)); }
            for (let i = 0; i < dx; i++) for (let j = 0; j < dy; j++) { const [x, y] = P(i, j); if (j) ctx.lineTo(X(x), Y(y)); else ctx.moveTo(X(x), Y(y)); }
            ctx.stroke();
            ctx.fillStyle = c.warm;
            for (let j = 0; j < dy; j++) for (let i = 0; i < dx; i++) { const [x, y] = P(i, j); ctx.fillRect(X(x) - 1.5, Y(y) - 1.5, 3, 3); }
            ctx.globalAlpha = 1;
            ctx.lineWidth = 1;
            foot.push(`mesh ${dx}×${dy}, up to ${fmtNum(maxShift)} px, drawn ×${gain}${meshOn === false ? ' (off)' : ''}`);
        }

        // The crop the video is read from
        if (Array.isArray(crop) && crop[0] > 0) {
            const o = Array.isArray(origin) ? origin : [(size[0] - crop[0]) / 2, (size[1] - crop[1]) / 2];
            ctx.fillStyle = c.accent;
            ctx.globalAlpha = 0.12;
            ctx.fillRect(X(o[0]), Y(o[1]), crop[0] * s, crop[1] * s);
            ctx.globalAlpha = 1;
            ctx.strokeStyle = c.accent;
            ctx.lineWidth = 1.5;
            ctx.strokeRect(X(o[0]), Y(o[1]), crop[0] * s, crop[1] * s);
            ctx.lineWidth = 1;
            ctx.fillStyle = c.accent;
            ctx.font = `10.5px ${c.mono}`;
            ctx.textBaseline = 'top';
            plateText(ctx, c, `crop ${fmtNum(crop[0])}×${fmtNum(crop[1])} at ${fmtNum(o[0])}, ${fmtNum(o[1])}`, X(o[0]) + 6, Y(o[1]) + 6);
        }
        this.meta.textContent = '';
        this.setFoot(foot.join(' · '));
    }
    empty(ctx, W, H, c, text) {
        ctx.fillStyle = c.muted;
        ctx.font = `12px ${c.sans}`;
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        ctx.fillText(text, W / 2, H / 2);
        ctx.textAlign = 'left';
        this.foot.textContent = '';
    }
}

// ---------------------------------------------------------------------------------------------------------------------

class LensView extends View {
    static detect(tags) {
        // A lens profile (Gyroflow's JSON), not any lens data: Cooke's /i data is in Lens / Data too
        const profile = find(tags, 'Lens', 'Data');
        const isProfile = profile && (profile.channels || []).some(c => c.startsWith('calib_dimension') || c.startsWith('fisheye_params'));
        return isProfile || has(tags, 'LensDistortion', 'Data') || LensView.tagGroups(tags).length > 0;
    }
    /// Groups that state a distortion model with its coefficients (Gyroflow's protobuf, MotionCam's calibration)
    static tagGroups(tags) {
        const groups = new Set(tags.filter(t => t.tag === 'DistortionModel').map(t => t.group));
        return [...groups].filter(g => has(tags, g, 'DistortionCoefficients'));
    }
    constructor(app) {
        super(app, { id: 'lens', label: 'Lens' });
        this.groups = LensView.tagGroups(app.tags);
        this.key = null;
    }
    get wanted() {
        const w = [['Lens', 'Data'], ['LensDistortion', 'Data'], ['Imager', 'SensorSizePixels'], ['Imager', 'PixelPitch'], ['Imager', 'CaptureAreaOrigin'], ['Imager', 'CaptureAreaSize']];
        for (const g of this.groups) w.push([g, 'DistortionModel'], [g, 'DistortionCoefficients'], [g, 'PixelFocalLength'], [g, 'PrincipalPoint']);
        return w;
    }
    changed() {
        const key = this.values.map(v => v?.index ?? -1).join(',');
        if (key !== this.key) {
            this.key = key;
            this.lens = this.buildLens();
        }
        this.draw();
    }
    buildLens() {
        const profile = this.value(0);
        // A frame's own lens tags first (Gyroflow protobuf, MotionCam): they change with the zoom, and the profile next
        // to them can be a placeholder. The frame they're calibrated on comes from the profile, or is the video's
        for (let gi = 0; gi < this.groups.length; gi++) {
            const b = 6 + gi * 4;
            const model = this.value(b), coeffs = this.value(b + 1), focal = this.value(b + 2);
            let center = this.value(b + 3);
            const md = this.app.videoMeta, dim = profile?.calib_dimension;
            const size = dim?.w > 0 ? [dim.w, dim.h] : md ? [md.width, md.height] : null;
            const cm = profile?.fisheye_params?.camera_matrix;
            if (!Array.isArray(center) && Array.isArray(cm) && cm[0]?.[2] > 0) center = [cm[0][2], cm[1][2]];
            const l = lensFromTags({ model, coeffs, focal, center, size });
            if (l) {
                l.title = [profile?.camera_brand, profile?.camera_model, profile?.lens_model].filter(Boolean).join(' ');
                return l;
            }
        }
        if (profile && profile.fisheye_params) {
            const l = lensFromProfile(profile);
            if (l) return l;
        }
        const sony = this.value(1);
        if (sony) return lensFromSony(sony, this.value(2), this.value(3));
        return null;
    }
    draw() {
        const c = screenColors();
        const { ctx, W, H } = fitCanvas(this.canvas);
        const lens = this.lens;
        if (!lens) {
            ctx.fillStyle = c.muted; ctx.font = `12px ${c.sans}`; ctx.textAlign = 'center'; ctx.textBaseline = 'middle';
            ctx.fillText('No lens data at this time', W / 2, H / 2); ctx.textAlign = 'left';
            this.foot.textContent = ''; this.meta.textContent = '';
            return;
        }
        const fov = fieldOfView(lens);
        const wide = W / H > 1.25;
        const pad = 10, head = 16; // each pane's title
        const top = TOP + head, h = H - top - pad;
        const a = wide ? { x: pad, y: top, w: W / 2 - pad * 1.5, h } : { x: pad, y: top, w: W - pad * 2, h: h / 2 - head / 2 - pad / 2 };
        const b = wide ? { x: W / 2 + pad / 2, y: top, w: W / 2 - pad * 1.5, h } : { x: pad, y: top + h / 2 + head / 2 + pad / 2, w: W - pad * 2, h: h / 2 - head / 2 - pad / 2 };
        this.drawCurve(ctx, c, lens, fov, a);
        this.drawShape(ctx, c, lens, fov, b);
        const f = lens.fx === lens.fy || !lens.fy ? `f ${fmtNum(lens.fx)} px` : `f ${fmtNum(lens.fx)} / ${fmtNum(lens.fy)} px`;
        const fovs = [['H', fov.h], ['V', fov.v], ['D', fov.d]].filter(([, v]) => v !== null).map(([k, v]) => `${k}FOV ${v.toFixed(1)}°`);
        this.meta.textContent = [lens.name, lens.title].filter(Boolean).join(' · ');
        this.setFoot([...fovs, f, `${lens.w}×${lens.h}`, ...lens.notes].join(' · '));
    }
    /// The angle and radius a lens's curve is plotted to: a little past the frame's corner
    static extent(lens, fov = fieldOfView(lens)) {
        const lensMax = Math.min(lens.maxAngle ?? Math.PI / 2, Math.PI * 0.999);
        const cornerAngle = fov.d !== null ? fov.d / 2 * DEG : lensMax;
        const tMax = Math.min(lensMax, Math.max(cornerAngle * 1.12, 10 * DEG));
        let rMax = Math.hypot(lens.w / 2, lens.h / 2) * 1.12;
        const rEnd = radiusAt(lens, tMax, 0);
        if (rEnd !== null) rMax = Math.max(rMax, rEnd * 1.04);
        return { tMax, rMax };
    }
    /// The plot's range over the whole file, so a zoom doesn't rescale it every frame: the lens of up to 300 samples
    /// spread over the ones that state it
    async init() {
        const primary = this.groups.length ? [this.groups[0], 'DistortionCoefficients']
            : has(this.app.tags, 'LensDistortion', 'Data') ? ['LensDistortion', 'Data'] : ['Lens', 'Data'];
        const times = await this.app.rpc.call('tagTimes', { group: primary[0], tag: primary[1], max: 300 });
        if (!times.length) return;
        const rows = await this.app.rpc.call('atMany', { times, wanted: this.wanted });
        let tMax = 0, rMax = 0;
        for (const row of rows) {
            this.values = row;
            const lens = this.buildLens();
            if (!lens) continue;
            const e = LensView.extent(lens);
            tMax = Math.max(tMax, e.tMax);
            rMax = Math.max(rMax, e.rMax);
        }
        this.values = [];
        if (tMax > 0) this.fixed = { tMax, rMax };
    }
    /// Image radius against ray angle: the lens, a pinhole (f·tan θ) and an equidistant fisheye (f·θ) of the same f
    drawCurve(ctx, c, lens, fov, r) {
        const halfDiag = Math.hypot(lens.w / 2, lens.h / 2);
        // The file's range, widened by this frame should it reach further (a frame between the sampled ones)
        const own = LensView.extent(lens, fov);
        const tMax = Math.max(own.tMax, this.fixed?.tMax || 0), rMax = Math.max(own.rMax, this.fixed?.rMax || 0);
        const left = r.x + 34, bottom = r.y + r.h - 16, top = r.y + 4, right = r.x + r.w - 4;
        const X = t => left + t / tMax * (right - left), Y = v => bottom - v / rMax * (bottom - top);

        ctx.font = `10px ${c.mono}`;
        ctx.textBaseline = 'top';
        ctx.fillStyle = c.muted;
        ctx.strokeStyle = c.grid;
        ctx.lineWidth = 1;
        // Angle ticks, at least 34 px apart
        const aStep = [1, 2, 5, 10, 15, 30, 45].find(s => s * DEG / tMax * (right - left) >= 34) || 90;
        ctx.beginPath();
        for (let d = 0; d * DEG <= tMax + 1e-9; d += aStep) {
            const x = Math.round(X(d * DEG)) + 0.5;
            ctx.moveTo(x, top); ctx.lineTo(x, bottom);
            ctx.fillText(`${d}°`, x + 2, bottom + 3);
        }
        // Radius ticks
        const rStep = [100, 200, 250, 500, 1000, 2000, 2500, 5000].find(s => rMax / s <= 6) || 10000;
        ctx.textBaseline = 'middle';
        ctx.textAlign = 'right';
        for (let v = 0; v <= rMax; v += rStep) {
            const y = Math.round(Y(v)) + 0.5;
            ctx.moveTo(left, y); ctx.lineTo(right, y);
            ctx.fillText(String(v), left - 4, y);
        }
        ctx.stroke();
        ctx.textAlign = 'left';

        // The frame's edges and corner
        const marks = [['½W', lens.w / 2, 0, fov.h], ['½H', lens.h / 2, 1, fov.v], ['corner', halfDiag, 0, fov.d]];
        const roomy = r.h >= 130; // A small pane gets the lines without their labels and legend
        ctx.setLineDash([2, 3]);
        for (const [name, rad, , full] of marks) {
            const y = Y(rad);
            ctx.strokeStyle = c.line;
            ctx.beginPath(); ctx.moveTo(left, y); ctx.lineTo(right, y);
            if (full !== null) { const x = X(full / 2 * DEG); ctx.moveTo(x, y); ctx.lineTo(x, bottom); }
            ctx.stroke();
            ctx.fillStyle = c.muted;
            ctx.textBaseline = 'bottom';
            if (roomy) ctx.fillText(full !== null ? `${name} ${(full / 2).toFixed(1)}°` : name, left + 4, y - 2);
        }
        ctx.setLineDash([]);

        const plot = (fn, color, dash, width) => {
            ctx.strokeStyle = color; ctx.lineWidth = width; ctx.setLineDash(dash);
            ctx.beginPath();
            let pen = false;
            for (let i = 0; i <= 200; i++) {
                const t = tMax * i / 200, v = fn(t);
                if (v === null || !isFinite(v) || v > rMax * 1.5) { pen = false; continue; }
                if (pen) ctx.lineTo(X(t), Y(v)); else { ctx.moveTo(X(t), Y(v)); pen = true; }
            }
            ctx.stroke(); ctx.setLineDash([]); ctx.lineWidth = 1;
        };
        ctx.save();
        ctx.beginPath(); ctx.rect(left, top, right - left, bottom - top); ctx.clip();
        plot(t => t < Math.PI / 2 ? lens.fx * Math.tan(t) : null, c.muted, [5, 4], 1);
        plot(t => lens.fx * t, c.muted, [1, 3], 1);
        plot(t => radiusAt(lens, t, 0), c.accent, [], 2);
        if (lens.knots) {
            ctx.fillStyle = c.warm;
            for (const [rn, ang] of lens.knots) if (ang <= tMax) ctx.fillRect(X(ang) - 2, Y(rn * lens.fx) - 2, 4, 4);
        }
        ctx.restore();
        ctx.fillStyle = c.text;
        ctx.font = `10.5px ${c.sans}`;
        ctx.textBaseline = 'top';
        ctx.fillText('Image radius by ray angle', r.x, r.y - 14);
        // Legend, inside the plot's empty corner
        ctx.font = `10px ${c.mono}`;
        ctx.textAlign = 'right';
        const lx = right - 4;
        if (roomy) [['lens', c.accent, []], ['pinhole', c.muted, [5, 4]], ['f·θ', c.muted, [1, 3]]].forEach(([name, color, dash], i) => {
            const y = bottom - 12 - (2 - i) * 13;
            ctx.fillStyle = c.muted;
            ctx.fillText(name, lx - 26, y - 5);
            ctx.strokeStyle = color; ctx.lineWidth = name === 'lens' ? 2 : 1; ctx.setLineDash(dash);
            ctx.beginPath(); ctx.moveTo(lx - 20, y); ctx.lineTo(lx, y); ctx.stroke();
        });
        ctx.setLineDash([]); ctx.lineWidth = 1;
        ctx.textAlign = 'left';
    }
    /// A grid of straight lines in front of the camera as this lens images them, with rings of equal ray angle
    drawShape(ctx, c, lens, fov, r) {
        const m = 4;
        const { s, ox, oy } = fitBox(lens.w, lens.h, r.x + m, r.y + m, r.w - 2 * m, r.h - 2 * m);
        const PX = (u, v) => [ox + (lens.fx * u + lens.cx) * s, oy + (lens.fy * v + lens.cy) * s];
        ctx.save();
        ctx.beginPath(); ctx.rect(r.x, r.y, r.w, r.h); ctx.clip();
        ctx.fillStyle = c.grid; ctx.globalAlpha = 0.3; ctx.fillRect(ox, oy, lens.w * s, lens.h * s); ctx.globalAlpha = 1;

        const lensMax = Math.min(lens.maxAngle ?? Math.PI / 2, Math.PI * 0.999);
        // Rings of equal ray angle
        const cornerDeg = fov.d !== null ? fov.d / 2 : lensMax / DEG;
        const ringStep = cornerDeg > 80 ? 15 : cornerDeg > 40 ? 10 : cornerDeg > 15 ? 5 : 2;
        ctx.strokeStyle = c.line;
        ctx.fillStyle = c.muted;
        ctx.font = `9.5px ${c.mono}`;
        ctx.textBaseline = 'middle';
        for (let d = ringStep; d * DEG < lensMax; d += ringStep) {
            const t = d * DEG;
            ctx.beginPath();
            let pen = false, labelAt = null;
            for (let i = 0; i <= 120; i++) {
                const phi = i / 120 * Math.PI * 2;
                const p = lens.project(Math.sin(t) * Math.cos(phi), Math.sin(t) * Math.sin(phi), Math.cos(t));
                if (!p) { pen = false; continue; }
                const [x, y] = PX(p[0], p[1]);
                if (i === 0) labelAt = [x, y];
                if (pen) ctx.lineTo(x, y); else { ctx.moveTo(x, y); pen = true; }
            }
            ctx.stroke();
            if (labelAt && labelAt[0] < r.x + r.w - 20) ctx.fillText(`${d}°`, labelAt[0] + 3, labelAt[1] - 6);
        }
        // Straight lines on a plane in front of the camera, spaced so a dozen or so cross the frame; brighter inside it
        const ext = Math.tan(Math.min(lensMax, 82 * DEG));
        const hx = fov.h !== null ? Math.tan(Math.min(fov.h / 2, 80) * DEG) : ext;
        const spacing = Math.max(hx / 6, ext / 40);
        const N = Math.ceil(ext / spacing);
        const grid = new Path2D();
        for (let k = -N; k <= N; k++) {
            const a = k * spacing;
            for (const vertical of [true, false]) {
                let pen = false;
                for (let i = 0; i <= 96; i++) {
                    const b = (i / 96 * 2 - 1) * ext;
                    const [x, y] = vertical ? [a, b] : [b, a];
                    const p = Math.hypot(x, y) <= ext * 1.0001 ? lens.project(x, y, 1) : null;
                    if (!p) { pen = false; continue; }
                    const [sx, sy] = PX(p[0], p[1]);
                    if (pen) grid.lineTo(sx, sy); else { grid.moveTo(sx, sy); pen = true; }
                }
            }
        }
        ctx.strokeStyle = c.accent;
        ctx.lineWidth = 1;
        ctx.globalAlpha = 0.25;
        ctx.stroke(grid);
        ctx.globalAlpha = 1;
        ctx.beginPath(); ctx.rect(ox, oy, lens.w * s, lens.h * s); ctx.clip();
        ctx.stroke(grid);
        ctx.restore();
        // The frame, and the crop Sony reads the video from
        ctx.strokeStyle = c.text;
        ctx.strokeRect(ox + 0.5, oy + 0.5, lens.w * s - 1, lens.h * s - 1);
        const crop = this.value(5), origin = this.value(4);
        if (lens.source === 'sony' && Array.isArray(crop)) {
            const o = Array.isArray(origin) ? origin : [(lens.w - crop[0]) / 2, (lens.h - crop[1]) / 2];
            ctx.strokeStyle = c.warm; ctx.setLineDash([4, 3]);
            ctx.strokeRect(ox + o[0] * s, oy + o[1] * s, crop[0] * s, crop[1] * s);
            ctx.setLineDash([]);
        }
        ctx.fillStyle = c.text;
        ctx.font = `10.5px ${c.sans}`;
        ctx.textBaseline = 'top';
        ctx.fillText('Straight lines through the lens', r.x, r.y - 14);
    }
}

// ---------------------------------------------------------------------------------------------------------------------

const qmul = (a, b) => [
    a[0] * b[0] - a[1] * b[1] - a[2] * b[2] - a[3] * b[3],
    a[0] * b[1] + a[1] * b[0] + a[2] * b[3] - a[3] * b[2],
    a[0] * b[2] - a[1] * b[3] + a[2] * b[0] + a[3] * b[1],
    a[0] * b[3] + a[1] * b[2] - a[2] * b[1] + a[3] * b[0],
];
function qrot(q, v) {
    const [w, x, y, z] = q;
    const tx = 2 * (y * v[2] - z * v[1]), ty = 2 * (z * v[0] - x * v[2]), tz = 2 * (x * v[1] - y * v[0]);
    return [v[0] + w * tx + (y * tz - z * ty), v[1] + w * ty + (z * tx - x * tz), v[2] + w * tz + (x * ty - y * tx)];
}

class MotionView extends View {
    static sources(app) {
        const out = [];
        if (app.imu?.gyro) out.push({ id: 'gyro', name: 'Gyro, integrated' });
        for (const t of app.tags) {
            const ch = t.channels || [];
            const idx = ['w', 'x', 'y', 'z'].map(k => ch.indexOf(k) >= 0 ? ch.indexOf(k) : ch.indexOf('v.' + k));
            if (idx.every(i => i >= 0)) out.push({ id: `${t.group}/${t.tag}`, group: t.group, tag: t.tag, idx, name: `${t.group} ${t.tag === 'Data' ? '' : t.tag}`.trim() });
        }
        return out;
    }
    static detect(tags, app) { return MotionView.sources(app).length > 0; }
    constructor(app) {
        super(app, { id: 'motion', label: 'Orientation' });
        this.sources = MotionView.sources(app);
        this.source = this.sources[0];
        if (this.sources.length > 1) {
            const sel = el('select', { class: 'mini-select', 'aria-label': 'Orientation source' }, ...this.sources.map(s => el('option', { value: s.id, text: s.name })));
            sel.addEventListener('change', async () => { this.source = this.sources.find(s => s.id === sel.value); await this.load(); this.draw(); });
            this.tools.append(sel);
        }
    }
    async init() { await this.load(); }
    async load() {
        const s = this.source;
        if (s.id === 'gyro') {
            if (!s.data) s.data = MotionView.integrate(this.app.imu);
        } else if (!s.data) {
            const sr = await this.app.series(s.group, s.tag);
            const n = sr.t.length, nc = sr.channels.length;
            const q = new Float64Array(n * 4);
            for (let i = 0; i < n; i++) {
                let w = sr.v[i * nc + s.idx[0]], x = sr.v[i * nc + s.idx[1]], y = sr.v[i * nc + s.idx[2]], z = sr.v[i * nc + s.idx[3]];
                const l = Math.hypot(w, x, y, z) || 1;
                q[i * 4] = w / l; q[i * 4 + 1] = x / l; q[i * 4 + 2] = y / l; q[i * 4 + 3] = z / l;
            }
            s.data = { t: this.app.motionTimes(s.group, sr.t), q };
        }
    }
    /// The camera's orientation from its gyro readings: Gyroflow's IMU axes (x up, y left, z back) turned to the camera's
    /// (x right, y up, z back), each reading's rotation applied in the camera's own frame. It drifts, as any does
    static integrate(imu) {
        const { t, v: gyro } = imu.gyro;
        const n = t.length;
        const q = new Float64Array(n * 4);
        let cur = [1, 0, 0, 0];
        for (let i = 0; i < n; i++) {
            if (i > 0) {
                const dt = (t[i] - t[i - 1]) / 1000;
                const gx = gyro[i * 3], gy = gyro[i * 3 + 1], gz = gyro[i * 3 + 2];
                if (dt > 0 && dt < 0.5 && isFinite(gx + gy + gz)) {
                    const w = [-gy * DEG, gx * DEG, gz * DEG];
                    const ang = Math.hypot(...w) * dt;
                    if (ang > 1e-12) {
                        const s = Math.sin(ang / 2) / (ang / dt);
                        cur = qmul(cur, [Math.cos(ang / 2), w[0] * s, w[1] * s, w[2] * s]);
                        const l = Math.hypot(...cur); cur = cur.map(x => x / l);
                    }
                }
            }
            q.set(cur, i * 4);
        }
        return { t, q };
    }
    quatAt(ms) {
        const d = this.source?.data;
        if (!d || !d.t.length) return null;
        const i = nearest(d.t, ms);
        return [d.q[i * 4], d.q[i * 4 + 1], d.q[i * 4 + 2], d.q[i * 4 + 3]];
    }
    setTime(ms) { this.ms = ms; this.draw(); }
    draw() {
        const c = screenColors();
        const { ctx, W, H } = fitCanvas(this.canvas);
        const q = this.quatAt(this.ms ?? 0);
        if (!q) return;
        // The viewer's eye, behind, above and to the right of the camera
        const eye = [2.3, 1.5, 3.6];
        const fwd = (() => { const l = Math.hypot(...eye); return eye.map(x => -x / l); })();
        const right = (() => { const v = [-fwd[2], 0, fwd[0]]; const l = Math.hypot(...v); return v.map(x => x / l); })(); // fwd × world up
        const up = [right[1] * fwd[2] - right[2] * fwd[1], right[2] * fwd[0] - right[0] * fwd[2], right[0] * fwd[1] - right[1] * fwd[0]];
        // Sized so the camera and its field of view fill the tile at any size
        const F = Math.min(W * 0.8, H - TOP) * 1.45;
        const project = p => {
            const d = [p[0] - eye[0], p[1] - eye[1], p[2] - eye[2]];
            const z = d[0] * fwd[0] + d[1] * fwd[1] + d[2] * fwd[2];
            if (z < 0.1) return null;
            return [W / 2 + F * (d[0] * right[0] + d[1] * right[1] + d[2] * right[2]) / z, (H + TOP) / 2 - F * (d[0] * up[0] + d[1] * up[1] + d[2] * up[2]) / z];
        };
        const line = (a, b) => { const p = project(a), r = project(b); if (p && r) { ctx.moveTo(p[0], p[1]); ctx.lineTo(r[0], r[1]); } };

        // Floor
        ctx.strokeStyle = c.grid;
        ctx.lineWidth = 1;
        ctx.beginPath();
        for (let k = -3; k <= 3; k += 0.5) { line([k, -1.1, -3], [k, -1.1, 3]); line([-3, -1.1, k], [3, -1.1, k]); }
        ctx.stroke();
        // World axes at the origin of the floor
        const axes = [[[0.7, 0, 0], c.x], [[0, 0.7, 0], c.y], [[0, 0, 0.7], c.z]];
        ctx.lineWidth = 1.5;
        for (const [v, col] of axes) { ctx.strokeStyle = col; ctx.globalAlpha = 0.5; ctx.beginPath(); line([0, -1.1, 0], [v[0], v[1] - 1.1, v[2]]); ctx.stroke(); }
        ctx.globalAlpha = 1;

        const R = p => qrot(q, p);
        // Trail of where the camera pointed over the last 1.5 s
        const d = this.source.data;
        const i1 = nearest(d.t, this.ms), i0 = Math.max(0, lowerBound(d.t, this.ms - 1500));
        const step = Math.max(1, Math.floor((i1 - i0) / 90));
        ctx.lineWidth = 2;
        let prev = null;
        for (let i = i0; i <= i1; i += step) {
            const qi = [d.q[i * 4], d.q[i * 4 + 1], d.q[i * 4 + 2], d.q[i * 4 + 3]];
            const p = project(qrot(qi, [0, 0, -1.7]));
            if (p && prev) {
                ctx.globalAlpha = 0.15 + 0.85 * (i - i0) / Math.max(1, i1 - i0);
                ctx.strokeStyle = c.warm;
                ctx.beginPath(); ctx.moveTo(prev[0], prev[1]); ctx.lineTo(p[0], p[1]); ctx.stroke();
            }
            prev = p;
        }
        ctx.globalAlpha = 1;

        // Field of view
        const far = [[-0.9, 0.5, -1.7], [0.9, 0.5, -1.7], [0.9, -0.5, -1.7], [-0.9, -0.5, -1.7]].map(R);
        const apex = R([0, 0, -0.32]);
        const pf = far.map(project);
        if (pf.every(Boolean)) {
            ctx.fillStyle = c.accent; ctx.globalAlpha = 0.1;
            ctx.beginPath(); pf.forEach((p, i) => i ? ctx.lineTo(p[0], p[1]) : ctx.moveTo(p[0], p[1])); ctx.closePath(); ctx.fill();
            ctx.globalAlpha = 1;
        }
        ctx.strokeStyle = c.accent; ctx.lineWidth = 1;
        ctx.beginPath();
        for (let i = 0; i < 4; i++) { line(apex, far[i]); line(far[i], far[(i + 1) % 4]); }
        ctx.stroke();

        // The camera: body, lens, and a mark on its top
        const box = [];
        for (const x of [-0.45, 0.45]) for (const y of [-0.3, 0.3]) for (const z of [0, 0.4]) box.push(R([x, y, z]));
        ctx.strokeStyle = c.text; ctx.lineWidth = 1.5;
        ctx.beginPath();
        const edges = [[0, 1], [2, 3], [4, 5], [6, 7], [0, 2], [1, 3], [4, 6], [5, 7], [0, 4], [1, 5], [2, 6], [3, 7]];
        for (const [a, b] of edges) line(box[a], box[b]);
        const ring = z => Array.from({ length: 25 }, (_, i) => R([0.2 * Math.cos(i / 24 * 2 * Math.PI), 0.2 * Math.sin(i / 24 * 2 * Math.PI), z]));
        for (const z of [0, -0.32]) { const rg = ring(z); for (let i = 0; i < 24; i++) line(rg[i], rg[i + 1]); }
        for (const a of [0, 6, 12, 18]) line(ring(0)[a], ring(-0.32)[a]);
        ctx.stroke();
        ctx.fillStyle = c.warm;
        const top = [R([-0.12, 0.3, 0.1]), R([0.12, 0.3, 0.1]), R([0, 0.42, 0.2])].map(project);
        if (top.every(Boolean)) { ctx.beginPath(); top.forEach((p, i) => i ? ctx.lineTo(p[0], p[1]) : ctx.moveTo(p[0], p[1])); ctx.closePath(); ctx.fill(); }

        // Angles: where the lens points, and the roll about it
        const f = R([0, 0, -1]), rgt = R([1, 0, 0]), u = R([0, 1, 0]);
        const yaw = Math.atan2(f[0], -f[2]) / DEG, pitch = Math.asin(Math.max(-1, Math.min(1, f[1]))) / DEG, roll = Math.atan2(-rgt[1], u[1]) / DEG;
        this.meta.textContent = this.sources.length > 1 ? '' : this.source.name; // Otherwise the selector names it
        this.setFoot(`yaw ${yaw.toFixed(1)}° · pitch ${pitch.toFixed(1)}° · roll ${roll.toFixed(1)}°` + (this.source.id === 'gyro' ? ' · from the start, drifts' : ' · as recorded'));
    }
}

// ---------------------------------------------------------------------------------------------------------------------

class GpsView extends View {
    static source(tags) {
        for (const t of tags) {
            if (t.group !== 'GPS') continue;
            const ch = t.channels || [];
            if (ch.includes('lat') && ch.includes('lon')) return { kind: 'gpsdata', tag: t };
        }
        const g5 = find(tags, 'GPS', 'Data');
        if (g5 && (g5.channels || []).length >= 4 && g5.channels[0] === '0') return { kind: 'gopro', tag: g5 };
        if (has(tags, 'GPS', 'GPSLatitude') && has(tags, 'GPS', 'GPSLongitude')) return { kind: 'exif' };
        return null;
    }
    static detect(tags) { return !!GpsView.source(tags); }
    constructor(app) {
        super(app, { id: 'gps', label: 'GPS' });
        this.src = GpsView.source(app.tags);
        this.link = el('a', { class: 'mini-link', target: '_blank', rel: 'noopener', text: 'Open map ↗', title: 'Open this position in OpenStreetMap' });
        this.tools.append(this.link);
    }
    async init() {
        const { kind, tag } = this.src;
        let t, lat, lon, alt, speed;
        if (kind === 'gpsdata') {
            const s = await this.app.series(tag.group, tag.tag);
            const nc = s.channels.length, ci = k => s.channels.indexOf(k);
            const [la, lo, al, sp, acq] = ['lat', 'lon', 'altitude', 'speed', 'is_acquired'].map(ci);
            const keep = [];
            for (let i = 0; i < s.t.length; i++) {
                const a = s.v[i * nc + la], b = s.v[i * nc + lo];
                if (!isFinite(a) || !isFinite(b) || (a === 0 && b === 0) || (acq >= 0 && s.v[i * nc + acq] === 0)) continue;
                keep.push(i);
            }
            t = keep.map(i => s.t[i]); lat = keep.map(i => s.v[i * nc + la]); lon = keep.map(i => s.v[i * nc + lo]);
            alt = keep.map(i => al >= 0 ? s.v[i * nc + al] : NaN); speed = keep.map(i => sp >= 0 ? s.v[i * nc + sp] : NaN);
        } else if (kind === 'gopro') {
            // GPS5: latitude, longitude, altitude, 2D speed, 3D speed, scaled by SCAL
            const s = await this.app.series('GPS', 'Data');
            const scaleV = (await this.app.valuesAt(this.app.domain[0], [['GPS', 'Scale']]))[0]?.value;
            const sc = Array.isArray(scaleV) ? scaleV.map(Number) : [1e7, 1e7, 1e3, 1e3, 1e2];
            const nc = s.channels.length;
            t = []; lat = []; lon = []; alt = []; speed = [];
            for (let i = 0; i < s.t.length; i++) {
                const a = s.v[i * nc] / (sc[0] || 1), b = s.v[i * nc + 1] / (sc[1] || 1);
                if (!isFinite(a) || !isFinite(b) || (a === 0 && b === 0) || Math.abs(a) > 90) continue;
                t.push(s.t[i]); lat.push(a); lon.push(b); alt.push(s.v[i * nc + 2] / (sc[2] || 1)); speed.push(s.v[i * nc + 3] / (sc[3] || 1) * 3.6);
            }
        } else {
            const [la, lo] = await Promise.all([this.app.series('GPS', 'GPSLatitude'), this.app.series('GPS', 'GPSLongitude')]);
            const refs = await this.app.valuesAt(this.app.domain[0], [['GPS', 'GPSLatitudeRef'], ['GPS', 'GPSLongitudeRef']]);
            const sLat = refs[0]?.value === 83 ? -1 : 1, sLon = refs[1]?.value === 87 ? -1 : 1;
            const dms = (v, i) => v[i * 3] + v[i * 3 + 1] / 60 + v[i * 3 + 2] / 3600;
            t = []; lat = []; lon = []; alt = []; speed = [];
            for (let i = 0; i < Math.min(la.t.length, lo.t.length); i++) {
                const a = dms(la.v, i) * sLat, b = dms(lo.v, i) * sLon;
                if (!isFinite(a) || !isFinite(b) || (a === 0 && b === 0)) continue;
                t.push(la.t[i]); lat.push(a); lon.push(b); alt.push(NaN); speed.push(NaN);
            }
        }
        this.track = { t, lat, lon, alt, speed };
        const lat0 = lat.length ? lat.reduce((a, b) => a + b, 0) / lat.length : 0;
        this.k = Math.cos(lat0 * DEG);
    }
    setTime(ms) { this.ms = ms; this.draw(); }
    draw() {
        const c = screenColors();
        const { ctx, W, H } = fitCanvas(this.canvas);
        const tr = this.track;
        if (!tr || !tr.t.length) {
            ctx.fillStyle = c.muted; ctx.font = `12px ${c.sans}`; ctx.textAlign = 'center'; ctx.textBaseline = 'middle';
            ctx.fillText(tr ? 'No GPS fix in this file' : 'Reading GPS…', W / 2, H / 2); ctx.textAlign = 'left';
            return;
        }
        const M = 111320; // m per degree of latitude
        const xs = tr.lon.map(v => v * this.k * M), ys = tr.lat.map(v => v * M);
        let x0 = Math.min(...xs), x1 = Math.max(...xs), y0 = Math.min(...ys), y1 = Math.max(...ys);
        const span = Math.max(x1 - x0, y1 - y0, 20);
        const cxm = (x0 + x1) / 2, cym = (y0 + y1) / 2;
        x0 = cxm - span / 2; x1 = cxm + span / 2; y0 = cym - span / 2; y1 = cym + span / 2;
        const pad = 22;
        const s = Math.min((W - 2 * pad) / (x1 - x0), (H - 2 * pad - 10) / (y1 - y0));
        const X = x => W / 2 + (x - cxm) * s, Y = y => H / 2 - 5 - (y - cym) * s;
        const i = nearest(tr.t, this.ms ?? 0);

        ctx.lineJoin = 'round';
        ctx.lineWidth = 2;
        ctx.strokeStyle = c.line;
        ctx.beginPath();
        xs.forEach((x, k) => k ? ctx.lineTo(X(x), Y(ys[k])) : ctx.moveTo(X(x), Y(ys[k])));
        ctx.stroke();
        ctx.strokeStyle = c.accent;
        ctx.beginPath();
        for (let k = 0; k <= i; k++) k ? ctx.lineTo(X(xs[k]), Y(ys[k])) : ctx.moveTo(X(xs[k]), Y(ys[k]));
        ctx.stroke();
        // Start and the current position
        ctx.fillStyle = c.muted;
        ctx.beginPath(); ctx.arc(X(xs[0]), Y(ys[0]), 3, 0, Math.PI * 2); ctx.fill();
        ctx.fillStyle = c.warm;
        ctx.beginPath(); ctx.arc(X(xs[i]), Y(ys[i]), 5, 0, Math.PI * 2); ctx.fill();
        ctx.strokeStyle = c.warm; ctx.lineWidth = 1;
        ctx.beginPath(); ctx.arc(X(xs[i]), Y(ys[i]), 10, 0, Math.PI * 2); ctx.stroke();

        // Scale bar and north
        const bar = [1, 2, 5, 10, 20, 50, 100, 200, 500, 1000, 2000, 5000, 10000, 20000, 50000].find(m => m * s > 60) || 100000;
        ctx.strokeStyle = c.text; ctx.lineWidth = 1.5;
        ctx.beginPath(); ctx.moveTo(pad, H - 12); ctx.lineTo(pad + bar * s, H - 12); ctx.moveTo(pad, H - 16); ctx.lineTo(pad, H - 8); ctx.moveTo(pad + bar * s, H - 16); ctx.lineTo(pad + bar * s, H - 8); ctx.stroke();
        ctx.fillStyle = c.text; ctx.font = `10px ${c.mono}`; ctx.textBaseline = 'bottom';
        ctx.fillText(bar >= 1000 ? `${bar / 1000} km` : `${bar} m`, pad + bar * s + 6, H - 7);
        ctx.textAlign = 'right'; ctx.fillText('N ↑', W - 10, 20); ctx.textAlign = 'left';

        const lat = tr.lat[i], lon = tr.lon[i];
        const parts = [`${Math.abs(lat).toFixed(6)}° ${lat >= 0 ? 'N' : 'S'}, ${Math.abs(lon).toFixed(6)}° ${lon >= 0 ? 'E' : 'W'}`];
        if (isFinite(tr.alt[i])) parts.push(`${tr.alt[i].toFixed(1)} m`);
        if (isFinite(tr.speed[i])) parts.push(`${tr.speed[i].toFixed(1)} km/h`);
        this.setFoot(parts.join(' · '));
        this.meta.textContent = `${tr.t.length} fixes`;
        this.link.href = `https://www.openstreetmap.org/?mlat=${lat.toFixed(6)}&mlon=${lon.toFixed(6)}#map=17/${lat.toFixed(6)}/${lon.toFixed(6)}`;
    }
}

// ---------------------------------------------------------------------------------------------------------------------

/// The stabilizer as one rigid assembly: the body (housing) is fixed, the sensor shifts and turns inside it (IBIS), and
/// the lens's stabilizing group moves the image, so its optical axis, over it (OSS). As Gyroflow applies them
/// (cpu_undistort.rs, `rotate_and_distort`), a point the lens images at p (px from the principal point, y down) lands
/// on the sensor at R(−a)·(p − S + O): the sensor's centre is at +S in the body, turned by +a, and the lens axis at +O.
/// Sony's shifts are in nm (sensor px = nm / pixel pitch), the roll is IBIS table 2's z / e402 (millidegrees)
class ShiftView extends View {
    static detect(tags) {
        const xy = g => !!find(tags, g, 'Data')?.channels?.includes('x');
        return xy('IBIS') || xy('LensOSS') || !!find(tags, 'IBIS', 'Data2')?.channels?.includes('z');
    }
    constructor(app) {
        super(app, { id: 'shift', label: 'Stabilizer' });
    }
    get wanted() { return [['Imager', 'CaptureAreaOrigin'], ['Imager', 'CaptureAreaSize']]; }
    changed() { this.draw(); }

    /// A shift table's x, y in sensor px (or as recorded without a pixel pitch), NaN for the (−1, −1, −1) of a lens that
    /// doesn't report its position
    async loadShift(group) {
        const tag = find(this.app.tags, group, 'Data');
        if (!tag?.channels?.includes('x')) return null;
        const sr = await this.app.series(group, 'Data');
        const nc = sr.channels.length, xi = sr.channels.indexOf('x'), yi = sr.channels.indexOf('y'), zi = sr.channels.indexOf('z');
        const n = sr.t.length, x = new Float64Array(n), y = new Float64Array(n);
        let any = false, peak = 0;
        for (let i = 0; i < n; i++) {
            const a = sr.v[i * nc + xi], b = sr.v[i * nc + yi], c = zi >= 0 ? sr.v[i * nc + zi] : 0;
            if (a === -1 && b === -1 && c === -1) { x[i] = y[i] = NaN; continue; }
            x[i] = a / this.pitch[0]; y[i] = b / this.pitch[1];
            if (isFinite(x[i] + y[i])) { any = true; peak = Math.max(peak, Math.abs(x[i]), Math.abs(y[i])); }
        }
        return any ? { t: sr.t, x, y, peak } : null;
    }
    async init() {
        const [sensorV, pitchV] = (await this.app.valuesAt(this.app.domain[0], [['Imager', 'SensorSizePixels'], ['Imager', 'PixelPitch']])).map(v => v?.value);
        this.sensor = Array.isArray(sensorV) && sensorV[0] > 0 ? sensorV : [3000, 2000];
        this.hasSensor = Array.isArray(sensorV);
        this.hasPitch = Array.isArray(pitchV) && pitchV[0] > 0 && pitchV[1] > 0;
        this.pitch = this.hasPitch ? pitchV : [1, 1];
        this.ibis = await this.loadShift('IBIS');
        this.ois = await this.loadShift('LensOSS');
        if (find(this.app.tags, 'IBIS', 'Data2')?.channels?.includes('z')) {
            const sr = await this.app.series('IBIS', 'Data2');
            const [scaleV, unitV] = (await this.app.valuesAt(this.app.domain[0], [['IBIS', '0xe402'], ['IBIS', '0xe401']])).map(v => v?.value);
            const scale = Number(scaleV) > 0 ? Number(scaleV) : 1000;
            const toDeg = unitV === 1 ? 180 / Math.PI : 1;
            const nc = sr.channels.length, zi = sr.channels.indexOf('z'), n = sr.t.length;
            const a = new Float64Array(n);
            let peak = 0;
            for (let i = 0; i < n; i++) { a[i] = sr.v[i * nc + zi] / scale * toDeg; if (isFinite(a[i])) peak = Math.max(peak, Math.abs(a[i])); }
            this.roll = { t: sr.t, a, peak };
        }
        this.available = !!(this.ibis || this.ois || this.roll);
        // Exaggerated, steady over the file: the largest shift drawn as 7% of the sensor's width, the largest roll as 8°
        const shiftPeak = Math.max(this.ibis?.peak || 0, this.ois?.peak || 0);
        const want = shiftPeak > 0 ? this.sensor[0] * 0.07 / shiftPeak : 1;
        this.shiftGain = [1, 2, 5, 10, 20, 50, 100, 200, 500, 1000, 2000].reduce((g, x) => (x <= want ? x : g), 1);
        const rollWant = this.roll?.peak > 0 ? 8 / this.roll.peak : 1;
        this.rollGain = [1, 2, 5, 10, 20, 50, 100, 200].reduce((g, x) => (x <= rollWant ? x : g), 1);
        this.travel = shiftPeak * this.shiftGain;
    }
    setTime(ms) { this.ms = ms; this.draw(); }
    at(src, ms) {
        if (!src) return null;
        const i = nearest(src.t, ms);
        return i < 0 || !isFinite(src.x[i] + src.y[i]) ? null : [src.x[i], src.y[i]];
    }
    draw() {
        const c = screenColors();
        const { ctx, W, H } = fitCanvas(this.canvas);
        if (!this.available) return;
        const ms = this.ms ?? 0;
        const [w, h] = this.sensor;
        // The housing: the sensor's rest place with room for its travel
        const margin = this.travel + Math.max(w, h) * 0.06;
        const hw = w + 2 * margin, hh = h + 2 * margin;
        const { s, ox, oy } = fitBox(hw, hh, 14, TOP + 4, W - 28, H - TOP - 12);
        const cx = ox + hw * s / 2, cy = oy + hh * s / 2;
        const B = (x, y) => [cx + x * s, cy + y * s]; // body frame, px from the optical axis at rest, y down

        // Housing
        ctx.fillStyle = c.grid; ctx.globalAlpha = 0.25;
        ctx.beginPath(); ctx.roundRect(ox, oy, hw * s, hh * s, 8); ctx.fill();
        ctx.globalAlpha = 1;
        ctx.strokeStyle = c.line; ctx.lineWidth = 1.5;
        ctx.beginPath(); ctx.roundRect(ox + 0.5, oy + 0.5, hw * s - 1, hh * s - 1, 8); ctx.stroke();
        ctx.fillStyle = c.line;
        for (const [fx, fy] of [[0, 0], [1, 0], [0, 1], [1, 1]]) {
            ctx.beginPath(); ctx.arc(ox + 9 + fx * (hw * s - 18), oy + 9 + fy * (hh * s - 18), 2.5, 0, Math.PI * 2); ctx.fill();
        }
        // The sensor's place at rest, and the body's optical axis
        ctx.setLineDash([4, 4]); ctx.strokeStyle = c.line; ctx.lineWidth = 1;
        ctx.strokeRect(...B(-w / 2, -h / 2), w * s, h * s);
        ctx.setLineDash([]);
        ctx.beginPath(); ctx.moveTo(cx - 8, cy); ctx.lineTo(cx + 8, cy); ctx.moveTo(cx, cy - 8); ctx.lineTo(cx, cy + 8); ctx.stroke();

        // Trails of the last 0.6 s: the sensor's centre and the lens axis
        const trail = (src, color) => {
            if (!src) return;
            const i1 = nearest(src.t, ms), i0 = Math.max(0, lowerBound(src.t, ms - 600));
            ctx.strokeStyle = color; ctx.lineWidth = 1.5; ctx.lineCap = 'round';
            for (let i = i0 + 1; i <= i1; i++) {
                if (!isFinite(src.x[i] + src.y[i] + src.x[i - 1] + src.y[i - 1])) continue;
                ctx.globalAlpha = 0.1 + 0.8 * (i - i0) / Math.max(1, i1 - i0);
                const [x0, y0] = B(src.x[i - 1] * this.shiftGain, src.y[i - 1] * this.shiftGain), [x1, y1] = B(src.x[i] * this.shiftGain, src.y[i] * this.shiftGain);
                ctx.beginPath(); ctx.moveTo(x0, y0); ctx.lineTo(x1, y1); ctx.stroke();
            }
            ctx.globalAlpha = 1; ctx.lineCap = 'butt';
        };
        trail(this.ibis, c.accent);
        trail(this.ois, c.warm);

        // The sensor: shifted and turned, with the crop the video is read from
        const S = this.at(this.ibis, ms) || [0, 0];
        const ri = this.roll ? nearest(this.roll.t, ms) : -1;
        const roll = ri >= 0 && isFinite(this.roll.a[ri]) ? this.roll.a[ri] : 0;
        const [sx, sy] = B(S[0] * this.shiftGain, S[1] * this.shiftGain);
        ctx.save();
        ctx.translate(sx, sy);
        ctx.rotate(roll * this.rollGain * DEG);
        ctx.fillStyle = c.accent; ctx.globalAlpha = 0.12;
        ctx.fillRect(-w / 2 * s, -h / 2 * s, w * s, h * s);
        ctx.globalAlpha = 1;
        ctx.strokeStyle = c.accent; ctx.lineWidth = 1.75;
        ctx.strokeRect(-w / 2 * s, -h / 2 * s, w * s, h * s);
        const crop = this.value(1), origin = this.value(0);
        if (Array.isArray(crop) && crop[0] > 0 && this.hasSensor) {
            const o = Array.isArray(origin) ? origin : [(w - crop[0]) / 2, (h - crop[1]) / 2];
            ctx.setLineDash([3, 3]); ctx.lineWidth = 1; ctx.globalAlpha = 0.7;
            ctx.strokeRect((o[0] - w / 2) * s, (o[1] - h / 2) * s, crop[0] * s, crop[1] * s);
            ctx.setLineDash([]); ctx.globalAlpha = 1;
        }
        // Its centre, and a mark on its top edge to show the turn
        ctx.lineWidth = 1.25;
        ctx.beginPath(); ctx.moveTo(-6, 0); ctx.lineTo(6, 0); ctx.moveTo(0, -6); ctx.lineTo(0, 6);
        ctx.moveTo(0, -h / 2 * s - 6); ctx.lineTo(0, -h / 2 * s + 6); ctx.stroke();
        ctx.restore();

        // The lens axis, where the lens's stabilizing group puts the image's centre
        const O = this.at(this.ois, ms);
        if (O) {
            const [lx, ly] = B(O[0] * this.shiftGain, O[1] * this.shiftGain);
            ctx.strokeStyle = c.warm; ctx.lineWidth = 1.5;
            ctx.beginPath(); ctx.arc(lx, ly, 7, 0, Math.PI * 2); ctx.stroke();
            ctx.beginPath(); ctx.moveTo(lx - 11, ly); ctx.lineTo(lx + 11, ly); ctx.moveTo(lx, ly - 11); ctx.lineTo(lx, ly + 11); ctx.stroke();
        }

        // Legend
        ctx.font = `10px ${c.mono}`; ctx.textBaseline = 'top';
        let lx = ox + 12;
        const ly = oy + hh * s - 18;
        for (const [name, color, on] of [['sensor (IBIS)', c.accent, this.ibis || this.roll], ['lens axis (OSS)', c.warm, this.ois]]) {
            if (!on) continue;
            ctx.fillStyle = color; ctx.fillRect(lx, ly + 3, 8, 3);
            ctx.fillStyle = c.muted; ctx.fillText(name, lx + 12, ly);
            lx += ctx.measureText(name).width + 28;
        }

        const unit = this.hasPitch ? 'px' : '(raw)';
        const fmt = p => `${p[0].toFixed(2)}, ${p[1].toFixed(2)} ${unit}`;
        const parts = [];
        if (this.ibis) parts.push(`IBIS ${fmt(S)}`);
        if (this.roll) parts.push(`roll ${roll.toFixed(3)}°`);
        if (this.ois) parts.push(O ? `OSS ${fmt(O)}` : 'OSS not reported');
        parts.push(`drawn: shift ×${this.shiftGain}${this.roll ? `, roll ×${this.rollGain}` : ''}`);
        this.meta.textContent = [this.ibis || this.roll ? 'IBIS' : null, this.ois ? 'Lens OSS' : null].filter(Boolean).join(' + ');
        this.setFoot(parts.join(' · '));
    }
}

// ---------------------------------------------------------------------------------------------------------------------

/// Sony's focal plane distortion table (RTMD 0xe423), as Gyroflow applies it (cpu_undistort.rs, `FocalPlaneDistortion`):
/// 8 bands of sensor rows, `unk2` rows each (an eighth of the sensor when 0), each with an (x, y) slope of raw / 32768
/// px per row. A row's shift is the slopes summed down to it: whole bands above, part of its own; rows past the last
/// band go on with its slope. Gyroflow subtracts that shift from the row, in full-sensor pixels
function focalPlaneShift(table, sensorH) {
    const bands = Array.isArray(table?.unk4) ? table.unk4 : [];
    if (bands.length !== 8) return null;
    const band = table.unk2 > 0 ? table.unk2 : sensorH / 8;
    const sx = bands.map(b => b[0] / 32768), sy = bands.map(b => b[1] / 32768);
    const at = y => {
        const idx = Math.min(7, Math.max(0, Math.floor(y / band)));
        const delta = y - band * idx;
        let dx = sx[idx] * delta, dy = sy[idx] * delta;
        for (let j = 0; j < idx; j++) { dx += sx[j] * band; dy += sy[j] * band; }
        return [dx, dy];
    };
    return { band, at, sx, sy };
}

class FocalPlaneView extends View {
    static detect(tags) { return has(tags, 'FocalPlaneDistortion', 'Data'); }
    constructor(app) {
        super(app, { id: 'fpd', label: 'Focal plane' });
        this.gain = 'auto';
        const sel = el('select', { class: 'mini-select', title: 'How much to exaggerate the row shift', 'aria-label': 'Row shift exaggeration' },
            ...[['auto', 'Shift: auto'], ['1', 'Shift ×1'], ['10', 'Shift ×10'], ['50', 'Shift ×50'], ['200', 'Shift ×200']].map(([v, t]) => el('option', { value: v, text: t })));
        sel.addEventListener('change', () => { this.gain = sel.value; this.draw(); });
        this.tools.append(sel);
    }
    get wanted() { return [['FocalPlaneDistortion', 'Data'], ['Imager', 'SensorSizePixels'], ['Imager', 'CaptureAreaOrigin'], ['Imager', 'CaptureAreaSize'], ['FocalPlaneDistortion', 'Enabled']]; }
    /// Whether any frame has a table, and the largest shift of the file, which keeps the scale steady while it plays
    async init() {
        const s = await this.app.series('FocalPlaneDistortion', 'Data');
        const ch = s.channels, nc = ch.length;
        const ix = [], iy = [];
        for (let j = 0; j < 8; j++) { ix.push(ch.indexOf(`unk4.${j}.0`)); iy.push(ch.indexOf(`unk4.${j}.1`)); }
        const iband = ch.indexOf('unk2');
        this.available = ix.every(i => i >= 0);
        if (!this.available) return;
        const [sensorV] = (await this.app.valuesAt(this.app.domain[0], [['Imager', 'SensorSizePixels']])).map(v => v?.value);
        const H = Array.isArray(sensorV) ? sensorV[1] : 0;
        let peak = 0;
        for (let r = 0; r < s.t.length; r++) {
            const band = iband >= 0 && s.v[r * nc + iband] > 0 ? s.v[r * nc + iband] : H / 8;
            let dx = 0, dy = 0;
            for (let j = 0; j < 8; j++) {
                const a = s.v[r * nc + ix[j]], b = s.v[r * nc + iy[j]];
                if (!isFinite(a) || !isFinite(b)) break;
                dx += a / 32768 * band; dy += b / 32768 * band;
                peak = Math.max(peak, Math.abs(dx), Math.abs(dy));
            }
        }
        this.peak = peak;
    }
    draw() {
        const c = screenColors();
        const { ctx, W, H } = fitCanvas(this.canvas);
        const table = this.value(0), sensor = this.value(1), origin = this.value(2), crop = this.value(3), on = this.value(4);
        const size = Array.isArray(sensor) ? sensor : null;
        const fp = size && focalPlaneShift(table, size[1]);
        if (!fp) {
            ctx.fillStyle = c.muted; ctx.font = `12px ${c.sans}`; ctx.textAlign = 'center'; ctx.textBaseline = 'middle';
            ctx.fillText('No focal plane table in this frame', W / 2, (H + TOP) / 2); ctx.textAlign = 'left';
            this.setFoot('');
            return;
        }
        const [w, h] = size;
        // Sensor on the left, the shift by row on the right, sharing the rows
        const plotW = Math.max(90, Math.min(180, W * 0.3));
        const m = 14, top = TOP + 18; // room for the plot's title under the tile's controls
        const { s, ox, oy } = fitBox(w, h, m, top, W - plotW - 3 * m, H - top - m - 12);
        const X = x => ox + x * s, Y = y => oy + y * s;
        const peak = Math.max(this.peak || 0, 1e-6);
        let gain = Number(this.gain);
        if (this.gain === 'auto') {
            const want = w * 0.04 / peak; // the largest shift of the file drawn as 4% of the sensor's width
            gain = [1, 2, 5, 10, 20, 50, 100, 200, 500, 1000].reduce((g, x) => (x <= want ? x : g), 1);
        }

        // Sensor and its bands
        ctx.fillStyle = c.grid; ctx.globalAlpha = 0.35; ctx.fillRect(X(0), Y(0), w * s, h * s); ctx.globalAlpha = 1;
        ctx.strokeStyle = c.line; ctx.lineWidth = 1;
        ctx.strokeRect(X(0) + 0.5, Y(0) + 0.5, w * s - 1, h * s - 1);
        ctx.setLineDash([3, 4]);
        ctx.beginPath();
        for (let j = 1; j < 8; j++) { const y = Math.round(Y(j * fp.band)) + 0.5; ctx.moveTo(X(0), y); ctx.lineTo(X(w), y); }
        ctx.stroke();
        ctx.setLineDash([]);
        // The crop the video is read from
        if (Array.isArray(crop) && crop[0] > 0) {
            const o = Array.isArray(origin) ? origin : [(w - crop[0]) / 2, (h - crop[1]) / 2];
            ctx.strokeStyle = c.accent; ctx.globalAlpha = 0.6;
            ctx.strokeRect(X(o[0]), Y(o[1]), crop[0] * s, crop[1] * s);
            ctx.globalAlpha = 1;
        }
        // Vertical lines of the sensor, each row moved by its shift (exaggerated)
        ctx.strokeStyle = c.warm; ctx.lineWidth = 1.5; ctx.globalAlpha = on === false ? 0.4 : 1;
        const rows = [];
        for (let j = 0; j <= 8; j++) rows.push(Math.min(h, j * fp.band));
        if (rows[rows.length - 1] < h) rows.push(h);
        ctx.beginPath();
        for (let k = 0; k <= 8; k++) {
            const x0 = w * k / 8;
            rows.forEach((y, i) => { const [dx, dy] = fp.at(y); const px = X(x0 + dx * gain), py = Y(y + dy * gain); i ? ctx.lineTo(px, py) : ctx.moveTo(px, py); });
        }
        ctx.stroke();
        ctx.globalAlpha = 1; ctx.lineWidth = 1;

        // Shift by row, in px: x and y
        const px0 = W - plotW - m, px1 = W - m, mid = (px0 + px1) / 2;
        // This frame's shifts, on a round scale that only changes in steps
        const frameMax = Math.max(...rows.map(y => Math.max(...fp.at(y).map(Math.abs))));
        const range = [0.5, 1, 2, 5, 10, 20, 50, 100, 200, 500].find(r => r >= frameMax * 1.1) || frameMax * 1.1;
        const PX = v => mid + v / range * (px1 - px0) / 2;
        ctx.strokeStyle = c.grid;
        ctx.beginPath();
        ctx.moveTo(Math.round(mid) + 0.5, Y(0)); ctx.lineTo(Math.round(mid) + 0.5, Y(h));
        for (let j = 0; j <= 8; j++) { const y = Math.round(Y(Math.min(h, j * fp.band))) + 0.5; ctx.moveTo(px0, y); ctx.lineTo(px1, y); }
        ctx.stroke();
        for (const [k, color] of [[0, c.x], [1, c.y]]) {
            ctx.strokeStyle = color; ctx.lineWidth = 1.75;
            ctx.beginPath();
            rows.forEach((y, i) => { const v = fp.at(y)[k]; i ? ctx.lineTo(PX(v), Y(y)) : ctx.moveTo(PX(v), Y(y)); });
            ctx.stroke();
        }
        ctx.lineWidth = 1;
        ctx.font = `10px ${c.mono}`; ctx.fillStyle = c.muted; ctx.textBaseline = 'top';
        const label = v => (v < 1 ? v.toFixed(1) : String(Math.round(v)));
        ctx.textAlign = 'left'; ctx.fillText(`−${label(range)}`, px0, Y(h) + 3);
        ctx.textAlign = 'center'; ctx.fillText('0', mid, Y(h) + 3);
        ctx.textAlign = 'right'; ctx.fillText(`+${label(range)} px`, px1, Y(h) + 3);
        ctx.textAlign = 'left';
        ctx.fillStyle = c.text; ctx.font = `10.5px ${c.sans}`;
        ctx.fillText('Row shift', px0, Y(0) - 15);
        const tw = ctx.measureText('Row shift ').width;
        ctx.font = `10px ${c.mono}`;
        ctx.fillStyle = c.x; ctx.fillText('x', px0 + tw + 2, Y(0) - 14);
        ctx.fillStyle = c.y; ctx.fillText('y', px0 + tw + 14, Y(0) - 14);
        // Rows: first read at the top
        ctx.fillStyle = c.muted;
        ctx.fillText('row 0', px0 + 2, Y(0) + 3);
        ctx.textBaseline = 'bottom';
        ctx.fillText(`row ${h}`, px0 + 2, Y(h) - 3);
        ctx.textBaseline = 'top';

        const [bx, by] = fp.at(h);
        this.meta.textContent = '';
        this.setFoot(`8 bands × ${Math.round(fp.band)} rows · last row shifted x ${bx.toFixed(2)} px, y ${by.toFixed(2)} px · drawn ×${gain}${on === false ? ' · off' : ''}`);
    }
}

// ---------------------------------------------------------------------------------------------------------------------

/// A lens's breathing table (Sony's LPIF, `LensBreathing / Data`): per zoom row, `entries` focus positions and the
/// magnification at each (×4096). `factor` is Gyroflow's `Profile::factor` (gyro_source/sony/breathing.rs): the zoom
/// code holds the row in its upper bits and a 7-bit fraction to the next one, bits 14 and 15 an unknown position
function breathingProfile(lens) {
    if (!lens) return null;
    const zp = Number(lens.zoom_positions) || 0, n = Number(lens.entries) || 0;
    const fc = lens.focus_curve || [], mg = lens.magnification || [];
    const rows = Math.max(zp, 1);
    if (!n || fc.length < rows * n || mg.length < rows * n) return null;
    const usable = Array.from({ length: rows }, (_, z) => { const k = mg.slice(z * n, (z + 1) * n).indexOf(0); return k < 0 ? n : k; });
    const at = (t, r, i) => t[r + i] ?? 0;
    const row = zoom => {
        if (zp === 1) return { zi: 0, zi1: 0, fz: 0, a: 0, b: 0 };
        const zi = zoom >> 7;
        if (zi > 128) return null;
        const zi1 = zi === 128 ? 128 : zi + 1;
        const [a, b] = zoom & 0xc000 ? [128, 128] : [zi, zi1];
        return { zi, zi1, fz: zoom & 0x7f, a, b };
    };
    /// The table at a zoom code: its focus positions and magnifications (raw), for the entries it has
    const at_zoom = zoom => {
        const r = row(zoom);
        if (!r) return null;
        let u = Math.min(usable[r.a] ?? 0, usable[r.b] ?? 0);
        if (u === 0 || u >= n) u = n;
        const r0 = (r.zi * n) & 0xffff, r1 = (r.zi1 * n) & 0xffff, wz = 128 - r.fz;
        const curve = i => (r.fz * at(fc, r1, i) + wz * at(fc, r0, i)) >> 7;
        const mag = i => (wz * at(mg, r0, i) + r.fz * at(mg, r1, i)) / 128;
        return { usable: u, curve, mag };
    };
    const factor = (zoom, focus) => {
        const t = at_zoom(zoom);
        if (!t || t.usable < 2) return 1;
        const { usable: u, curve, mag } = t;
        const v0 = curve(0);
        let lo, hi, base, step;
        if (focus < v0) { lo = 0; hi = 1; base = v0; step = curve(1) - v0; }
        else {
            let i = 1, prev = v0, cur = curve(1);
            while (focus >= cur && i < u - 1) { i++; prev = cur; cur = curve(i); }
            lo = i - 1; hi = i; base = prev; step = cur - prev;
        }
        let ff = 0;
        if (step !== 0) {
            const d = Math.max(focus - base, -10000);
            ff = Math.trunc((d < 0x2711 ? d * 4096 : 0x2710000) / step);
        }
        return (mag(hi) * ff + (4096 - ff) * mag(lo)) / 4096 / 4096;
    };
    return { at_zoom, factor };
}

class BreathingView extends View {
    static detect(tags) { return has(tags, 'LensBreathing', '0xe536'); }
    constructor(app) { super(app, { id: 'breathing', label: 'Breathing' }); }
    get wanted() {
        return ['0xe536', '0xe530', '0xe531', '0xe537', '0xe539', '0xe534', '0xe535', '0xe524', '0xe523', '0xe502', 'Data', '0xe504'].map(t => ['LensBreathing', t]);
    }
    /// The magnification over the file, for a steady scale
    async init() {
        const s = await this.app.series('LensBreathing', '0xe536');
        let lo = Infinity, hi = -Infinity;
        for (let i = 0; i < s.t.length; i++) { const v = s.v[i * s.channels.length]; if (v > 0) { lo = Math.min(lo, v); hi = Math.max(hi, v); } }
        this.range = lo <= hi ? [lo, hi] : null;
        this.available = !!this.range;
    }
    changed() {
        const lensIdx = this.values[10]?.index ?? -1;
        if (lensIdx !== this.profileIdx) {
            this.profileIdx = lensIdx;
            const lenses = this.value(10), id = this.value(9);
            const lens = Array.isArray(lenses) ? (lenses.find(l => l.lens_id === id) || (lenses.length === 1 ? lenses[0] : null)) : null;
            this.profile = breathingProfile(lens);
        }
        this.draw();
    }
    draw() {
        const c = screenColors();
        const { ctx, W, H } = fitCanvas(this.canvas);
        const m = this.value(0);
        if (!(m > 0)) { this.setFoot(''); return; }
        const enabled = this.value(1), applied = this.value(2), cropM = this.value(3), ref = this.value(4);
        const mMin = this.value(5), mMax = this.value(6);
        const wide = W / (H - TOP) > 1.25;
        const pad = 12, top = TOP + 16;
        const a = wide ? { x: pad, y: top, w: W * 0.45 - pad * 1.5, h: H - top - pad } : { x: pad, y: top, w: W - 2 * pad, h: (H - top) * 0.5 - pad };
        const b = wide ? { x: W * 0.45 + pad / 2, y: top, w: W * 0.55 - pad * 1.5, h: H - top - pad } : { x: pad, y: top + (H - top) * 0.5 + 8, w: W - 2 * pad, h: (H - top) * 0.5 - pad - 8 };
        this.drawField(ctx, c, a, m, applied ? cropM : null, mMin, mMax);
        const focusPos = this.drawCurve(ctx, c, b, m);
        const pct = x => `${x >= 1 ? '+' : '−'}${Math.abs((x - 1) * 100).toFixed(1)}%`;
        const parts = [`field ${pct(m)} of the reference focus's`];
        if (this.range) parts.push(`${this.range[0].toFixed(3)}–${this.range[1].toFixed(3)} over the file`);
        parts.push(applied ? `compensated in camera (crop ×${Number(cropM).toFixed(3)})` : enabled ? 'compensation on, not applied' : 'no in-camera compensation');
        if (focusPos !== null) parts.push(`focus position ${focusPos}`);
        this.meta.textContent = `m ${m.toFixed(4)}`;
        this.setFoot(parts.join(' · '));
    }
    /// The field of view against the reference focus's: it goes with m (the picture scales by 1/m)
    drawField(ctx, c, r, m, cropM, mMin, mMax) {
        const md = this.app.videoMeta;
        const aspect = md?.width > 0 ? md.width / md.height : 16 / 9;
        const hiM = Math.max(1, m, mMax || 0, this.range?.[1] || 0) * 1.04;
        const bw = Math.min(r.w, r.h * aspect) / hiM, bh = bw / aspect;
        const cx = r.x + r.w / 2, cy = r.y + r.h / 2;
        const rect = k => [cx - bw * k / 2, cy - bh * k / 2, bw * k, bh * k];
        ctx.lineWidth = 1;
        if (mMin > 0 && mMax > 0) {
            ctx.fillStyle = c.grid; ctx.globalAlpha = 0.5;
            ctx.fillRect(...rect(mMax)); ctx.globalAlpha = 1;
            ctx.fillStyle = c.bg; ctx.fillRect(...rect(mMin));
        }
        ctx.strokeStyle = c.muted; ctx.setLineDash([4, 4]);
        ctx.strokeRect(...rect(1));
        ctx.setLineDash([]);
        ctx.strokeStyle = c.accent; ctx.lineWidth = 2;
        ctx.strokeRect(...rect(m));
        if (cropM > 0) {
            ctx.strokeStyle = c.warm; ctx.lineWidth = 1.5;
            ctx.strokeRect(...rect(m / cropM));
        }
        ctx.lineWidth = 1;
        ctx.font = `10px ${c.mono}`; ctx.textBaseline = 'top';
        const items = [['reference', c.muted], ['field now', c.accent]];
        if (mMin > 0 && mMax > 0) items.push(['lens range', c.line]);
        if (cropM > 0) items.push(['camera output', c.warm]);
        let lx = r.x;
        // A legend only where it fits on one line
        const legendW = items.reduce((w, [name]) => w + ctx.measureText(name).width + 24, 0);
        if (legendW <= r.w) for (const [name, color] of items) {
            ctx.fillStyle = color; ctx.fillRect(lx, r.y + r.h - 8, 8, 3);
            ctx.fillStyle = c.muted; ctx.fillText(name, lx + 11, r.y + r.h - 12);
            lx += ctx.measureText(name).width + 24;
        }
        ctx.fillStyle = c.text; ctx.font = `10.5px ${c.sans}`;
        ctx.fillText('Field of view', r.x, r.y - 15);
    }
    /// The lens's magnification by focus position at this zoom (its LPIF table), and where the focus is
    drawCurve(ctx, c, r, m) {
        ctx.fillStyle = c.text; ctx.font = `10.5px ${c.sans}`; ctx.textBaseline = 'top';
        ctx.fillText('Magnification by focus position', r.x, r.y - 15);
        const p = this.profile;
        const zooms = this.value(8), foci = this.value(7);
        const mid = a => (Array.isArray(a) && a.length ? a[a.length >> 1] : null);
        const zoom = mid(zooms) ?? 0, focus = mid(foci);
        const t = p?.at_zoom(zoom);
        if (!t || t.usable < 2) {
            ctx.fillStyle = c.muted; ctx.font = `11px ${c.sans}`;
            ctx.fillText(p ? 'No table at this zoom' : 'The file has no breathing table for this lens', r.x, r.y + 6);
            return null;
        }
        const pts = Array.from({ length: t.usable }, (_, i) => [t.curve(i), t.mag(i) / 4096]);
        let x0 = Math.min(...pts.map(q => q[0])), x1 = Math.max(...pts.map(q => q[0]));
        let y0 = Math.min(...pts.map(q => q[1]), m), y1 = Math.max(...pts.map(q => q[1]), m);
        if (focus !== null) { x0 = Math.min(x0, focus); x1 = Math.max(x1, focus); }
        const py = (y1 - y0) * 0.12 || 0.01; y0 -= py; y1 += py;
        const left = r.x + 40, right = r.x + r.w - 6, top = r.y + 2, bottom = r.y + r.h - 16;
        const X = v => left + (v - x0) / ((x1 - x0) || 1) * (right - left), Y = v => bottom - (v - y0) / (y1 - y0) * (bottom - top);
        // Grid and value ticks
        ctx.strokeStyle = c.grid; ctx.fillStyle = c.muted; ctx.font = `10px ${c.mono}`; ctx.textBaseline = 'middle'; ctx.textAlign = 'right';
        const step = niceStepLocal((y1 - y0) / 4);
        ctx.beginPath();
        for (let v = Math.ceil(y0 / step) * step; v <= y1; v += step) { const y = Math.round(Y(v)) + 0.5; ctx.moveTo(left, y); ctx.lineTo(right, y); ctx.fillText(v.toFixed(step < 0.01 ? 3 : 2), left - 4, y); }
        ctx.stroke();
        // The table's own focus units: which end is near isn't known
        ctx.textAlign = 'left'; ctx.textBaseline = 'top';
        ctx.fillText(String(Math.round(x0)), left, bottom + 4);
        if (right - left > 220) { ctx.textAlign = 'center'; ctx.fillText('focus position', (left + right) / 2, bottom + 4); }
        ctx.textAlign = 'right'; ctx.fillText(String(Math.round(x1)), right, bottom + 4); ctx.textAlign = 'left';
        // Reference (m = 1) and the camera's m for this frame
        ctx.setLineDash([4, 4]); ctx.strokeStyle = c.muted;
        if (y0 < 1 && y1 > 1) { ctx.beginPath(); ctx.moveTo(left, Y(1)); ctx.lineTo(right, Y(1)); ctx.stroke(); }
        ctx.setLineDash([]);
        // The table
        ctx.strokeStyle = c.accent; ctx.lineWidth = 2;
        ctx.beginPath(); pts.forEach(([x, y], i) => (i ? ctx.lineTo(X(x), Y(y)) : ctx.moveTo(X(x), Y(y)))); ctx.stroke();
        ctx.fillStyle = c.accent; for (const [x, y] of pts) ctx.fillRect(X(x) - 1.5, Y(y) - 1.5, 3, 3);
        ctx.lineWidth = 1;
        // Where the focus is: the table's value there (filled), and the camera's m for the frame (ring). e504's bit 0 only
        // picks which samples Gyroflow weights (breathing.rs `delayed`), the positions are good either way
        if (focus !== null) {
            const tm = p.factor(zoom, focus);
            ctx.strokeStyle = c.warm; ctx.beginPath(); ctx.moveTo(X(focus), top); ctx.lineTo(X(focus), bottom); ctx.stroke();
            ctx.fillStyle = c.warm; ctx.beginPath(); ctx.arc(X(focus), Y(tm), 4.5, 0, Math.PI * 2); ctx.fill();
            ctx.strokeStyle = c.text; ctx.beginPath(); ctx.arc(X(focus), Y(m), 6.5, 0, Math.PI * 2); ctx.stroke();
            return focus;
        }
        return null;
    }
}

function niceStepLocal(raw) {
    const p = Math.pow(10, Math.floor(Math.log10(raw || 1e-3)));
    const f = raw / p;
    return (f <= 1 ? 1 : f <= 2 ? 2 : f <= 5 ? 5 : 10) * p;
}

export const VIEW_TYPES = [SensorView, FocalPlaneView, LensView, BreathingView, MotionView, GpsView, ShiftView];
