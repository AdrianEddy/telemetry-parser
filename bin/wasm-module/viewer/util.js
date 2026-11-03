// SPDX-License-Identifier: MIT OR Apache-2.0

export const css = name => getComputedStyle(document.documentElement).getPropertyValue(name).trim();

/// A round step near `raw`: 1, 2 or 5 × 10^n; for times (ms) also whole seconds and minutes
export function niceStep(raw, time = false) {
    if (!(raw > 0)) return 1;
    if (time && raw >= 1000) {
        for (const s of [1000, 2000, 5000, 10000, 15000, 30000, 60000, 120000, 300000, 600000, 900000, 1800000, 3600000]) if (s >= raw) return s;
        return Math.ceil(raw / 3600000) * 3600000;
    }
    const p = Math.pow(10, Math.floor(Math.log10(raw)));
    const f = raw / p;
    return (f <= 1 ? 1 : f <= 2 ? 2 : f <= 5 ? 5 : 10) * p;
}

/// m:ss.mmm, with fewer decimals when `span` (the visible duration) is long
export function fmtTime(ms, span = 0) {
    if (!isFinite(ms)) return '–';
    const neg = ms < 0;
    ms = Math.abs(ms);
    const decimals = span > 120000 ? 0 : span > 12000 ? 1 : span > 1500 ? 2 : 3;
    let s = ms / 1000;
    const h = Math.floor(s / 3600);
    s -= h * 3600;
    const m = Math.floor(s / 60);
    s -= m * 60;
    let ss = s.toFixed(decimals);
    if (parseFloat(ss) >= 60) ss = (59.999).toFixed(decimals);
    if (s < 10) ss = '0' + ss;
    return (neg ? '−' : '') + (h ? `${h}:${String(m).padStart(2, '0')}:` : `${m}:`) + ss;
}

export function fmtNum(x) {
    if (!isFinite(x)) return String(x);
    const a = Math.abs(x);
    if (a !== 0 && (a >= 1e7 || a < 1e-4)) return x.toExponential(2);
    if (a >= 1000) return x.toFixed(0);
    if (a >= 100) return x.toFixed(1);
    if (a >= 10) return x.toFixed(2);
    return x.toFixed(3);
}

export function fmtBytes(n) {
    if (!(n >= 0)) return '–';
    const u = ['B', 'KB', 'MB', 'GB', 'TB'];
    let i = 0;
    while (n >= 1000 && i < u.length - 1) { n /= 1000; i++; }
    return `${n >= 100 || i === 0 ? n.toFixed(0) : n >= 10 ? n.toFixed(1) : n.toFixed(2)} ${u[i]}`;
}

export function fmtCount(n) {
    return Math.round(n).toLocaleString('en-US').replace(/,/g, ' ');
}

/// A tag's native id: a FourCC when its bytes are printable, hex otherwise
export function fmtNativeId(id) {
    if (id === null || id === undefined) return '';
    const b = [(id >>> 24) & 255, (id >>> 16) & 255, (id >>> 8) & 255, id & 255];
    if (b.every(x => x >= 0x20 && x < 0x7f) && b.some(x => /[A-Za-z]/.test(String.fromCharCode(x)))) return b.map(x => String.fromCharCode(x)).join('');
    return '0x' + id.toString(16);
}

export function el(tag, attrs = {}, ...children) {
    const e = document.createElement(tag);
    for (const [k, v] of Object.entries(attrs)) {
        if (v === undefined || v === null || v === false) continue;
        if (k === 'class') e.className = v;
        else if (k === 'text') e.textContent = v;
        else if (k.startsWith('on')) e.addEventListener(k.slice(2), v);
        else e.setAttribute(k, v === true ? '' : v);
    }
    for (const c of children) if (c !== null && c !== undefined && c !== false) e.append(c);
    return e;
}

export function download(name, data, type) {
    const blob = data instanceof Blob ? data : new Blob([data], { type });
    const url = URL.createObjectURL(blob);
    const a = el('a', { href: url, download: name });
    document.body.appendChild(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(url), 10000);
}

/// Sets a canvas's backing size to its CSS size × the device pixel ratio; returns the context in CSS pixels
export function fitCanvas(canvas) {
    const dpr = window.devicePixelRatio || 1;
    const W = canvas.clientWidth, H = canvas.clientHeight;
    const w = Math.max(1, Math.round(W * dpr)), h = Math.max(1, Math.round(H * dpr));
    if (canvas.width !== w || canvas.height !== h) { canvas.width = w; canvas.height = h; }
    const ctx = canvas.getContext('2d');
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, W, H);
    return { ctx, W, H };
}
