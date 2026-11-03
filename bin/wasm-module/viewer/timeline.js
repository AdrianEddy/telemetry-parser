// SPDX-License-Identifier: MIT OR Apache-2.0
// Chart lanes on a shared time axis. Each lane caches its plot in a canvas and redraws it only when the view, the size
// or the data changes; the playhead and the hover line are DOM elements over all lanes. Long series are drawn from a
// min/max pyramid, so a million readings cost about as much to draw as the lane is wide.

import { fmtTime, niceStep, fmtNum, css } from './util.js';

const LEVELS = [16, 256, 4096, 65536];

function lowerBound(t, x) {
    let lo = 0, hi = t.length;
    while (lo < hi) { const m = (lo + hi) >> 1; if (t[m] < x) lo = m + 1; else hi = m; }
    return lo;
}

/// Per channel, the min and max of each block of LEVELS[i] readings
function buildPyramid(v, stride, n) {
    const levels = [];
    let prev = null, prevBs = 1;
    for (const bs of LEVELS) {
        const nb = Math.ceil(n / bs);
        if (nb < 64) break;
        const min = new Float32Array(nb * stride).fill(NaN), max = new Float32Array(nb * stride).fill(NaN);
        const f = bs / prevBs;
        for (let b = 0; b < nb; b++) {
            for (let c = 0; c < stride; c++) {
                let lo = Infinity, hi = -Infinity;
                if (!prev) {
                    const end = Math.min(n, (b + 1) * bs);
                    for (let i = b * bs; i < end; i++) { const x = v[i * stride + c]; if (x < lo) lo = x; if (x > hi) hi = x; }
                } else {
                    const end = Math.min(prev.nb, (b + 1) * f);
                    for (let i = b * f; i < end; i++) { const a = prev.min[i * stride + c], z = prev.max[i * stride + c]; if (a < lo) lo = a; if (z > hi) hi = z; }
                }
                if (lo <= hi) { min[b * stride + c] = lo; max[b * stride + c] = hi; }
            }
        }
        prev = { bs, nb, min, max };
        prevBs = bs;
        levels.push(prev);
    }
    return levels;
}

export class Lane {
    constructor(timeline, opts) {
        this.tl = timeline;
        this.id = opts.id;
        this.title = opts.title;
        this.unit = opts.unit || '';
        this.t = opts.t;
        this.v = opts.v;
        this.stride = opts.channels.length;
        this.n = Math.min(this.t.length, Math.floor(this.v.length / this.stride));
        this.channels = opts.channels.map((c, i) => ({ ...c, visible: c.visible !== false, index: i }));
        this.removable = !!opts.removable;
        this.height = opts.height || 120;
        this.pyramid = buildPyramid(this.v, this.stride, this.n);

        const el = this.el = document.createElement('div');
        el.className = 'lane';
        el.style.minHeight = this.height + 'px';
        el.innerHTML = `
            <canvas class="lane-plot"></canvas>
            <div class="lane-head">
                <span class="lane-title"></span>
                <span class="lane-unit"></span>
                <span class="lane-legend"></span>
                ${this.removable ? '<button class="lane-remove icon-btn" title="Remove this chart" aria-label="Remove this chart">×</button>' : ''}
            </div>`;
        el.querySelector('.lane-title').textContent = this.title;
        el.querySelector('.lane-unit').textContent = this.unit;
        this.canvas = el.querySelector('canvas');
        this.legend = el.querySelector('.lane-legend');
        this.chipValues = [];
        for (const ch of this.channels) {
            const chip = document.createElement('button');
            chip.className = 'chip-ch';
            chip.style.setProperty('--ch', ch.color);
            chip.innerHTML = `<i></i><span class="ch-name"></span><span class="ch-val">–</span>`;
            chip.querySelector('.ch-name').textContent = ch.name;
            chip.title = `Show or hide ${ch.name}`;
            chip.addEventListener('click', e => { e.stopPropagation(); ch.visible = !ch.visible; chip.classList.toggle('off', !ch.visible); this.dirty = true; this.tl.requestDraw(); });
            chip.addEventListener('pointerdown', e => e.stopPropagation());
            this.legend.appendChild(chip);
            this.chipValues.push(chip.querySelector('.ch-val'));
        }
        el.querySelector('.lane-remove')?.addEventListener('click', e => { e.stopPropagation(); this.tl.removeLane(this.id); });
        el.querySelector('.lane-remove')?.addEventListener('pointerdown', e => e.stopPropagation());
        this.dirty = true;
    }

    /// The readings' index range covering [t0, t1], one reading beyond each end
    range(t0, t1) {
        const i0 = Math.max(0, lowerBound(this.t, t0) - 1);
        const i1 = Math.min(this.n, lowerBound(this.t, t1) + 1);
        return [i0, i1];
    }

    /// The value range of the visible channels over [t0, t1]
    extent(t0, t1) {
        const [i0, i1] = this.range(t0, t1);
        let lo = Infinity, hi = -Infinity;
        const count = i1 - i0;
        const level = this.pickLevel(count, 2000);
        for (const ch of this.channels) {
            if (!ch.visible) continue;
            const c = ch.index;
            if (!level) {
                for (let i = i0; i < i1; i++) { const x = this.v[i * this.stride + c]; if (x < lo) lo = x; if (x > hi) hi = x; }
            } else {
                const b0 = Math.floor(i0 / level.bs), b1 = Math.min(level.nb, Math.ceil(i1 / level.bs));
                for (let b = b0; b < b1; b++) { const a = level.min[b * this.stride + c], z = level.max[b * this.stride + c]; if (a < lo) lo = a; if (z > hi) hi = z; }
            }
        }
        if (!(lo <= hi)) return [-1, 1];
        if (lo === hi) { const d = Math.abs(lo) * 0.05 || 1; return [lo - d, hi + d]; }
        const pad = (hi - lo) * 0.08;
        return [lo - pad, hi + pad];
    }

    /// The coarsest pyramid level that still has `perPx` blocks per… the requested resolution
    pickLevel(count, cols) {
        let pick = null;
        for (const level of this.pyramid) if (count / level.bs >= cols * 2) pick = level;
        return pick;
    }

    valueAt(ms) {
        if (!this.n) return null;
        let i = lowerBound(this.t, ms);
        if (i >= this.n) i = this.n - 1;
        if (i > 0 && Math.abs(this.t[i - 1] - ms) < Math.abs(this.t[i] - ms)) i--;
        // Too far from any reading: nothing to show
        const span = (this.t[this.n - 1] - this.t[0]) || 1;
        if (Math.abs(this.t[i] - ms) > Math.max(span / this.n * 4, 50)) return null;
        return i;
    }

    showValues(ms) {
        const i = this.valueAt(ms);
        for (const ch of this.channels) {
            const x = i === null ? NaN : this.v[i * this.stride + ch.index];
            this.chipValues[ch.index].textContent = isNaN(x) ? '–' : fmtNum(x);
        }
    }

    draw() {
        const tl = this.tl;
        const dpr = window.devicePixelRatio || 1;
        const W = this.canvas.clientWidth, H = this.canvas.clientHeight;
        if (!W || !H) return;
        if (this.canvas.width !== Math.round(W * dpr) || this.canvas.height !== Math.round(H * dpr)) {
            this.canvas.width = Math.round(W * dpr);
            this.canvas.height = Math.round(H * dpr);
        }
        const ctx = this.canvas.getContext('2d');
        ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
        ctx.clearRect(0, 0, W, H);
        const [t0, t1] = tl.view;
        const top = 26, bottom = H - 6;
        const [lo, hi] = this.extent(t0, t1);
        const sy = (bottom - top) / (hi - lo);
        const Y = v => bottom - (v - lo) * sy;
        const sx = W / (t1 - t0);

        // Grid: time ticks from the shared axis, value ticks of this lane
        ctx.lineWidth = 1;
        ctx.strokeStyle = tl.colors.grid;
        ctx.beginPath();
        for (const x of tl.ticks) { const px = Math.round((x - t0) * sx) + 0.5; ctx.moveTo(px, top - 4); ctx.lineTo(px, bottom); }
        ctx.stroke();
        const vstep = niceStep((hi - lo) / Math.max(2, Math.floor((bottom - top) / 34)));
        ctx.font = `10px ${tl.colors.mono}`;
        ctx.fillStyle = tl.colors.muted;
        ctx.textBaseline = 'middle';
        ctx.beginPath();
        for (let v = Math.ceil(lo / vstep) * vstep; v <= hi; v += vstep) {
            const py = Math.round(Y(v)) + 0.5;
            if (py < top + 6) continue;
            ctx.moveTo(0, py); ctx.lineTo(W, py);
            ctx.fillText(fmtNum(Math.abs(v) < vstep / 1e6 ? 0 : v), 4, py - 7);
        }
        ctx.strokeStyle = tl.colors.grid;
        ctx.stroke();
        if (lo < 0 && hi > 0) {
            ctx.strokeStyle = tl.colors.zero;
            ctx.beginPath(); ctx.moveTo(0, Math.round(Y(0)) + 0.5); ctx.lineTo(W, Math.round(Y(0)) + 0.5); ctx.stroke();
        }

        // The channels
        const [i0, i1] = this.range(t0, t1);
        const count = i1 - i0;
        const cols = Math.ceil(W);
        ctx.lineWidth = 1.25;
        ctx.lineJoin = 'round';
        for (const ch of this.channels) {
            if (!ch.visible) continue;
            const c = ch.index;
            ctx.strokeStyle = ch.color;
            ctx.beginPath();
            if (count <= cols * 2) {
                let pen = false;
                for (let i = i0; i < i1; i++) {
                    const x = this.v[i * this.stride + c];
                    if (isNaN(x)) { pen = false; continue; }
                    const px = (this.t[i] - t0) * sx, py = Y(x);
                    if (pen) ctx.lineTo(px, py); else { ctx.moveTo(px, py); pen = true; }
                }
                ctx.stroke();
                if (count < cols / 6) { // Few readings: mark each one
                    ctx.fillStyle = ch.color;
                    for (let i = i0; i < i1; i++) {
                        const x = this.v[i * this.stride + c];
                        if (!isNaN(x)) ctx.fillRect((this.t[i] - t0) * sx - 1.5, Y(x) - 1.5, 3, 3);
                    }
                }
            } else {
                // Min/max per pixel column
                const level = this.pickLevel(count, cols);
                const bs = level ? level.bs : 1;
                const b0 = Math.floor(i0 / bs), b1 = level ? Math.min(level.nb, Math.ceil(i1 / bs)) : i1;
                const colMin = new Float32Array(cols + 1).fill(Infinity), colMax = new Float32Array(cols + 1).fill(-Infinity);
                for (let b = b0; b < b1; b++) {
                    const i = b * bs;
                    if (i >= this.n) break;
                    const px = Math.floor((this.t[i] - t0) * sx);
                    if (px < 0 || px > cols) continue;
                    const a = level ? level.min[b * this.stride + c] : this.v[i * this.stride + c];
                    const z = level ? level.max[b * this.stride + c] : a;
                    if (a < colMin[px]) colMin[px] = a;
                    if (z > colMax[px]) colMax[px] = z;
                }
                let pen = false;
                for (let px = 0; px <= cols; px++) {
                    if (!(colMin[px] <= colMax[px])) { continue; }
                    const ya = Y(colMax[px]), yb = Y(colMin[px]);
                    if (pen) ctx.lineTo(px + 0.5, ya); else { ctx.moveTo(px + 0.5, ya); pen = true; }
                    ctx.lineTo(px + 0.5, yb + (yb - ya < 1 ? 1 : 0));
                }
                ctx.stroke();
            }
        }
        this.dirty = false;
    }
}

export class Timeline {
    constructor(root, { onSeek, onScrub }) {
        this.root = root;
        this.onSeek = onSeek;   // (ms, scrubbing)
        this.onScrub = onScrub; // (true when a scrub starts, false when it ends)
        this.lanes = [];
        this.domain = [0, 1000];
        this.view = [0, 1000];
        this.time = 0;
        this.hover = null;
        this.ticks = [];
        this.follow = true;

        root.innerHTML = `
            <div class="tl-axis"><canvas></canvas></div>
            <div class="tl-lanes" tabindex="-1">
                <div class="tl-content">
                    <div class="tl-stack"></div>
                    <div class="tl-hover" hidden></div>
                    <div class="tl-playhead"></div>
                </div>
                <div class="tl-empty" hidden>No motion data or numeric tags yet. Choose a tag in the Tags panel and press its chart button to plot it.</div>
            </div>
            <div class="tl-overview"><canvas></canvas><div class="tl-brush"></div><div class="tl-ov-head"></div></div>`;
        this.axisCanvas = root.querySelector('.tl-axis canvas');
        this.lanesEl = root.querySelector('.tl-lanes');
        this.stack = root.querySelector('.tl-stack');
        this.playhead = root.querySelector('.tl-playhead');
        this.hoverEl = root.querySelector('.tl-hover');
        this.emptyEl = root.querySelector('.tl-empty');
        this.ovCanvas = root.querySelector('.tl-overview canvas');
        this.brush = root.querySelector('.tl-brush');
        this.ovHead = root.querySelector('.tl-ov-head');
        this.readColors();

        new ResizeObserver(() => { this.lanes.forEach(l => l.dirty = true); this.requestDraw(true); }).observe(root);
        this.bindPointer();
    }

    readColors() {
        this.colors = {
            grid: css('--chart-grid'), zero: css('--chart-zero'), muted: css('--muted'), text: css('--text'),
            accent: css('--accent'), mono: css('--font-mono'), ovFill: css('--chart-overview'),
        };
        this.lanes.forEach(l => l.dirty = true);
        this.requestDraw(true);
    }

    setDomain(start, end) {
        if (!(end > start)) end = start + 1000;
        this.domain = [start, end];
        this.view = [start, end];
        this.lanes.forEach(l => l.dirty = true);
        this.requestDraw(true);
    }

    addLane(opts) {
        if (this.lanes.some(l => l.id === opts.id)) return null;
        const lane = new Lane(this, opts);
        this.lanes.push(lane);
        this.stack.appendChild(lane.el);
        this.emptyEl.hidden = true;
        lane.showValues(this.time);
        this.requestDraw(true);
        return lane;
    }

    removeLane(id) {
        const i = this.lanes.findIndex(l => l.id === id);
        if (i < 0) return;
        this.lanes[i].el.remove();
        this.lanes.splice(i, 1);
        this.emptyEl.hidden = this.lanes.length > 0;
        this.root.dispatchEvent(new CustomEvent('laneremoved', { detail: id }));
        this.requestDraw(true);
    }

    clear() {
        for (const l of this.lanes) l.el.remove();
        this.lanes = [];
        this.emptyEl.hidden = false;
    }

    setView(t0, t1, fromUser = true) {
        const [d0, d1] = this.domain;
        const minSpan = 2;
        let span = Math.max(minSpan, Math.min(t1 - t0, d1 - d0));
        if (t0 < d0) t0 = d0;
        if (t0 + span > d1) t0 = d1 - span;
        this.view = [t0, t0 + span];
        if (fromUser) this.userViewAt = performance.now();
        this.lanes.forEach(l => l.dirty = true);
        this.requestDraw(true);
    }

    zoom(factor, atMs) {
        const [t0, t1] = this.view;
        const at = atMs ?? (t0 + t1) / 2;
        this.setView(at - (at - t0) * factor, at + (t1 - at) * factor);
    }

    fit() { this.setView(...this.domain); }

    setTime(ms, playing) {
        this.time = ms;
        const [t0, t1] = this.view;
        const span = t1 - t0;
        // Keep the playhead in view while playing, unless the view was just moved by hand
        if (playing && this.follow && (ms < t0 || ms > t1 - span * 0.02) && !(performance.now() - (this.userViewAt || 0) < 1500)) {
            this.setView(ms - span * 0.1, ms + span * 0.9, false);
        }
        if (this.hover === null) for (const l of this.lanes) l.showValues(ms);
        this.positionOverlays();
    }

    /// The playhead to where the pointer is: at the view's edge when it's past one, the view moving along meanwhile
    scrubTo(clientX) {
        const r = this.lanesEl.getBoundingClientRect();
        this.scrubX = clientX;
        const over = clientX < r.left ? clientX - r.left : clientX > r.right ? clientX - r.right : 0;
        if (over) this.startEdgePan(); else this.stopEdgePan();
        const x = Math.max(r.left, Math.min(r.right, clientX));
        this.onSeek(Math.max(this.domain[0], Math.min(this.domain[1], this.msAt(x))), true);
    }

    startEdgePan() {
        if (this.edgeRaf) return;
        const step = () => {
            const r = this.lanesEl.getBoundingClientRect();
            const over = this.scrubX < r.left ? this.scrubX - r.left : this.scrubX > r.right ? this.scrubX - r.right : 0;
            if (!over) { this.edgeRaf = 0; return; }
            const [t0, t1] = this.view;
            // Faster the further past the edge: a tenth of the view per frame 100 px out
            const dt = (t1 - t0) * Math.max(-0.1, Math.min(0.1, over / 1000));
            this.setView(t0 + dt, t1 + dt);
            const edge = over < 0 ? this.view[0] : this.view[1];
            this.onSeek(Math.max(this.domain[0], Math.min(this.domain[1], edge)), true);
            this.edgeRaf = requestAnimationFrame(step);
        };
        this.edgeRaf = requestAnimationFrame(step);
    }

    stopEdgePan() {
        if (this.edgeRaf) cancelAnimationFrame(this.edgeRaf);
        this.edgeRaf = 0;
    }

    msAt(clientX) {
        const r = this.lanesEl.getBoundingClientRect();
        const [t0, t1] = this.view;
        return t0 + (clientX - r.left) / r.width * (t1 - t0);
    }

    bindPointer() {
        const el = this.lanesEl;
        const axis = this.axisCanvas.parentElement;
        el.title = axis.title = 'Drag to scrub · Shift-drag or middle-drag to pan · Wheel to zoom · Double-click to fit';
        // Press and drag scrubs, on the lanes and on the time ruler: the playhead follows the pointer while it's held,
        // and the view moves along past its edges. Shift-drag or the middle button pans the view
        let drag = null;
        const start = (target, e) => {
            if (e.button !== 0 && e.button !== 1) return;
            e.preventDefault();
            target.setPointerCapture(e.pointerId);
            const pan = e.button === 1 || e.shiftKey;
            drag = { pan, x: e.clientX, view: [...this.view] };
            el.classList.add(pan ? 'panning' : 'scrubbing');
            axis.classList.toggle('scrubbing', !pan);
            if (!pan) {
                this.onScrub?.(true);
                this.scrubTo(e.clientX);
            }
        };
        const move = e => {
            if (!drag) return;
            if (drag.pan) {
                const w = el.getBoundingClientRect().width;
                const [t0, t1] = drag.view;
                const dt = -(e.clientX - drag.x) / w * (t1 - t0);
                this.setView(t0 + dt, t1 + dt);
            } else {
                this.scrubTo(e.clientX);
            }
        };
        const end = () => {
            if (!drag) return;
            this.stopEdgePan();
            if (!drag.pan) this.onScrub?.(false);
            el.classList.remove('scrubbing', 'panning');
            axis.classList.remove('scrubbing');
            drag = null;
        };
        for (const target of [el, axis]) {
            target.addEventListener('pointerdown', e => start(target, e));
            target.addEventListener('pointermove', move);
            target.addEventListener('pointerup', end);
            target.addEventListener('pointercancel', end);
            target.addEventListener('lostpointercapture', end);
        }
        el.addEventListener('pointermove', e => {
            this.hover = drag && !drag.pan ? null : this.msAt(e.clientX);
            for (const l of this.lanes) l.showValues(this.hover ?? this.time);
            this.positionOverlays();
        });
        el.addEventListener('pointerleave', () => {
            if (drag) return;
            this.hover = null;
            for (const l of this.lanes) l.showValues(this.time);
            this.positionOverlays();
        });
        el.addEventListener('dblclick', () => this.fit());
        el.addEventListener('wheel', e => {
            e.preventDefault();
            if (Math.abs(e.deltaX) > Math.abs(e.deltaY)) {
                const [t0, t1] = this.view;
                const dt = e.deltaX / el.clientWidth * (t1 - t0);
                this.setView(t0 + dt, t1 + dt);
            } else {
                this.zoom(Math.pow(1.0015, e.deltaY * (e.deltaMode === 1 ? 30 : 1)), this.msAt(e.clientX));
            }
        }, { passive: false });

        // Overview: drag the brush to move the view, click elsewhere to centre it there
        const ov = this.ovCanvas.parentElement;
        let ovDrag = null;
        const ovMs = x => { const r = ov.getBoundingClientRect(); const [d0, d1] = this.domain; return d0 + (x - r.left) / r.width * (d1 - d0); };
        ov.addEventListener('pointerdown', e => {
            ov.setPointerCapture(e.pointerId);
            const ms = ovMs(e.clientX);
            const [t0, t1] = this.view;
            if (ms < t0 || ms > t1) this.setView(ms - (t1 - t0) / 2, ms + (t1 - t0) / 2);
            ovDrag = { ms: ovMs(e.clientX), view: [...this.view] };
        });
        ov.addEventListener('pointermove', e => {
            if (!ovDrag) return;
            const d = ovMs(e.clientX) - ovDrag.ms;
            this.setView(ovDrag.view[0] + d, ovDrag.view[1] + d);
        });
        ov.addEventListener('pointerup', () => ovDrag = null);
        ov.addEventListener('dblclick', () => this.fit());
    }

    requestDraw(full) {
        if (full) this.full = true;
        if (this.raf) return;
        this.raf = requestAnimationFrame(() => {
            this.raf = 0;
            this.drawAxis();
            for (const l of this.lanes) if (l.dirty || this.full) l.draw();
            if (this.full) this.drawOverview();
            this.full = false;
            this.positionOverlays();
        });
    }

    positionOverlays() {
        const [t0, t1] = this.view;
        const W = this.lanesEl.clientWidth;
        const x = (this.time - t0) / (t1 - t0) * W;
        this.playhead.style.transform = `translateX(${Math.round(x)}px)`;
        this.playhead.hidden = x < -1 || x > W + 1;
        if (this.hover !== null) {
            this.hoverEl.hidden = false;
            this.hoverEl.style.transform = `translateX(${Math.round((this.hover - t0) / (t1 - t0) * W)}px)`;
            this.hoverEl.dataset.t = fmtTime(this.hover, t1 - t0);
        } else this.hoverEl.hidden = true;
        // Brush and playhead on the overview
        const [d0, d1] = this.domain;
        const OW = this.ovCanvas.clientWidth;
        const bx0 = (t0 - d0) / (d1 - d0) * OW, bx1 = (t1 - d0) / (d1 - d0) * OW;
        this.brush.style.left = bx0 + 'px';
        this.brush.style.width = Math.max(3, bx1 - bx0) + 'px';
        this.ovHead.style.transform = `translateX(${Math.round((this.time - d0) / (d1 - d0) * OW)}px)`;
    }

    drawAxis() {
        const c = this.axisCanvas, dpr = window.devicePixelRatio || 1;
        const W = c.clientWidth, H = c.clientHeight;
        if (!W) return;
        c.width = Math.round(W * dpr); c.height = Math.round(H * dpr);
        const ctx = c.getContext('2d');
        ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
        const [t0, t1] = this.view;
        const span = t1 - t0;
        const step = niceStep(span / Math.max(2, W / 110), true);
        this.ticks = [];
        ctx.font = `10.5px ${this.colors.mono}`;
        ctx.fillStyle = this.colors.muted;
        ctx.strokeStyle = this.colors.grid;
        ctx.textBaseline = 'middle';
        ctx.beginPath();
        for (let x = Math.ceil(t0 / step) * step; x <= t1; x += step) {
            this.ticks.push(x);
            const px = Math.round((x - t0) / span * W) + 0.5;
            ctx.moveTo(px, H - 6); ctx.lineTo(px, H);
            ctx.fillText(fmtTime(x, span), px + 4, H / 2);
        }
        ctx.stroke();
    }

    drawOverview() {
        const c = this.ovCanvas, dpr = window.devicePixelRatio || 1;
        const W = c.clientWidth, H = c.clientHeight;
        if (!W) return;
        c.width = Math.round(W * dpr); c.height = Math.round(H * dpr);
        const ctx = c.getContext('2d');
        ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
        ctx.clearRect(0, 0, W, H);
        const lane = this.lanes[0];
        if (!lane || !lane.n) return;
        // The magnitude of the first lane's channels, as an envelope
        const [d0, d1] = this.domain;
        const cols = Math.ceil(W);
        const mag = new Float32Array(cols + 1);
        const level = lane.pickLevel(lane.n, cols);
        const bs = level ? level.bs : 1, nb = level ? level.nb : lane.n;
        let peak = 0;
        for (let b = 0; b < nb; b++) {
            const i = b * bs;
            const px = Math.floor((lane.t[Math.min(i, lane.n - 1)] - d0) / (d1 - d0) * W);
            if (px < 0 || px > cols) continue;
            let s = 0;
            for (let ch = 0; ch < lane.stride; ch++) {
                const a = level ? Math.max(Math.abs(level.min[b * lane.stride + ch]), Math.abs(level.max[b * lane.stride + ch])) : Math.abs(lane.v[i * lane.stride + ch]);
                if (a > s) s = a;
            }
            if (s > mag[px]) mag[px] = s;
            if (s > peak) peak = s;
        }
        if (!peak) return;
        ctx.fillStyle = this.colors.ovFill;
        ctx.beginPath();
        ctx.moveTo(0, H);
        for (let px = 0; px <= cols; px++) ctx.lineTo(px, H - 2 - mag[px] / peak * (H - 6));
        ctx.lineTo(W, H);
        ctx.fill();
    }
}
