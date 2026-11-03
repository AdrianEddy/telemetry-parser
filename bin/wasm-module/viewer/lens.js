// SPDX-License-Identifier: MIT OR Apache-2.0
// Lens models, as Gyroflow evaluates them (src/core/stabilization/distortion_models): each one projects a ray
// (x, y, z), z along the optical axis, to the normalized image plane; pixel = (fx·u + cx, fy·v + cy).

const DEG = Math.PI / 180;

/// The largest p in [0, pMax] up to which `f` keeps increasing, sampled
function monotonicEnd(f, pMax, steps = 400) {
    let prev = f(0);
    for (let i = 1; i <= steps; i++) {
        const p = pMax * i / steps;
        const v = f(p);
        if (!(v > prev)) return pMax * (i - 1) / steps;
        prev = v;
    }
    return pMax;
}

/// p in [0, pEnd] with f(p) = target, for an increasing f; null past its end
function invert(f, target, pEnd) {
    if (!(target >= f(0))) return 0;
    // The end itself, give or take rounding: the angle of a ray built from it (atan2 of its sin and cos) can come
    // back an ulp past it
    const fEnd = f(pEnd);
    if (target > fEnd) return target - fEnd < 1e-9 ? pEnd : null;
    let lo = 0, hi = pEnd;
    for (let i = 0; i < 60; i++) {
        const mid = (lo + hi) / 2;
        if (f(mid) < target) lo = mid; else hi = mid;
    }
    return (lo + hi) / 2;
}

/// Natural cubic spline through the points (xs[i], ys[i]), xs increasing, continued with its end slopes outside them
/// (Gyroflow's `splines::NaturalSpline`, the one Sony's SDK computes)
function naturalSpline(xs, ys) {
    const n = xs.length - 1;
    if (n < 1) return () => ys[0] || 0;
    const h = Array.from({ length: n }, (_, i) => xs[i + 1] - xs[i]);
    // Second derivatives M, M_0 = M_n = 0: h_{i-1}·M_{i-1} + 2(h_{i-1} + h_i)·M_i + h_i·M_{i+1} = 6·(Δ_i − Δ_{i-1})
    const M = new Float64Array(n + 1);
    if (n >= 2) {
        const m = n - 1, cp = new Float64Array(m), dp = new Float64Array(m);
        for (let j = 0; j < m; j++) {
            const i = j + 1;
            const rhs = 6 * ((ys[i + 1] - ys[i]) / h[i] - (ys[i] - ys[i - 1]) / h[i - 1]);
            const a = j > 0 ? h[i - 1] : 0, bb = 2 * (h[i - 1] + h[i]), c = h[i];
            const den = bb - a * (j > 0 ? cp[j - 1] : 0);
            cp[j] = c / den;
            dp[j] = (rhs - a * (j > 0 ? dp[j - 1] : 0)) / den;
        }
        for (let j = m - 1; j >= 0; j--) M[j + 1] = dp[j] - cp[j] * (j < m - 1 ? M[j + 2] : 0);
    }
    // Segment i (dx = x − xs[i]): ys_i + b_i·dx + M_i/2·dx² + (M_{i+1} − M_i)/(6h_i)·dx³
    const b = h.map((hi, i) => (ys[i + 1] - ys[i]) / hi - hi * (2 * M[i] + M[i + 1]) / 6);
    const endSlope = b[n - 1] + h[n - 1] * (M[n - 1] + M[n]) / 2;
    return x => {
        if (x <= xs[0]) return ys[0] + b[0] * (x - xs[0]);
        if (x >= xs[n]) return ys[n] + endSlope * (x - xs[n]);
        let lo = 0, hi = n - 1;
        while (lo < hi) { const mid = (lo + hi + 1) >> 1; if (xs[mid] <= x) lo = mid; else hi = mid - 1; }
        const i = lo, dx = x - xs[i];
        return ys[i] + b[i] * dx + M[i] / 2 * dx * dx + (M[i + 1] - M[i]) / (6 * h[i]) * dx * dx * dx;
    };
}

/// A radially symmetric lens given by its radius → angle map (normalized radius, radians)
function fromAngleOfRadius(angleOf, rMax) {
    const rEnd = monotonicEnd(angleOf, rMax);
    return {
        angleOf,
        project(x, y, z) {
            const r = Math.hypot(x, y);
            if (r < 1e-12) return [0, 0];
            const theta = Math.atan2(r, z);
            const rn = invert(angleOf, theta, rEnd);
            if (rn === null) return null;
            return [x / r * rn, y / r * rn];
        },
        maxAngle: angleOf(rEnd),
    };
}

const models = {
    opencv_fisheye(k) {
        const [k1 = 0, k2 = 0, k3 = 0, k4 = 0] = k;
        if (!k1 && !k2 && !k3 && !k4) return models.pinhole();
        return {
            project(x, y, z) {
                const r = Math.hypot(x, y);
                const t = Math.atan2(r, z), t2 = t * t;
                const td = t * (1 + k1 * t2 + k2 * t2 * t2 + k3 * t2 * t2 * t2 + k4 * t2 * t2 * t2 * t2);
                const s = r < 1e-12 ? 1 : td / r;
                return [x * s, y * s];
            },
            maxAngle: foldAngle(t => { const t2 = t * t; return t * (1 + k1 * t2 + k2 * t2 * t2 + k3 * t2 * t2 * t2 + k4 * t2 * t2 * t2 * t2); }, Math.PI * 0.999),
        };
    },
    opencv_standard(k) {
        const q = Array.from({ length: 12 }, (_, i) => k[i] || 0);
        return {
            project(x, y, z) {
                if (!(z > 1e-9)) return null;
                x /= z; y /= z;
                const r2 = x * x + y * y, r4 = r2 * r2, r6 = r4 * r2;
                const cdist = 1 + q[0] * r2 + q[1] * r4 + q[4] * r6;
                const icdist2 = 1 / (1 + q[5] * r2 + q[6] * r4 + q[7] * r6);
                return [
                    x * cdist * icdist2 + q[2] * 2 * x * y + q[3] * (r2 + 2 * x * x) + q[8] * r2 + q[9] * r4,
                    y * cdist * icdist2 + q[2] * (r2 + 2 * y * y) + q[3] * 2 * x * y + q[10] * r2 + q[11] * r4,
                ];
            },
            maxAngle: foldAngle(t => { const r = Math.tan(t), r2 = r * r; return r * (1 + q[0] * r2 + q[1] * r2 * r2 + q[4] * r2 * r2 * r2) / (1 + q[5] * r2 + q[6] * r2 * r2 + q[7] * r2 * r2 * r2); }, 89.9 * DEG),
        };
    },
    insta360(k) {
        const [k1 = 0, k2 = 0, k3 = 0, p1 = 0, p2 = 0, xi = 0] = k;
        const radial = t => {
            const den = Math.cos(t) + xi;
            if (!(den > 1e-9)) return NaN;
            const r = Math.sin(t) / den, r2 = r * r;
            return r * (1 + k1 * r2 + k2 * r2 * r2 + k3 * r2 * r2 * r2);
        };
        return {
            project(x, y, z) {
                const len = Math.hypot(x, y, z);
                const den = z / len + xi;
                if (!(den > 1e-9)) return null;
                x = x / len / den; y = y / len / den;
                const r2 = x * x + y * y, r4 = r2 * r2, r6 = r4 * r2;
                const c = 1 + k1 * r2 + k2 * r4 + k3 * r6;
                return [x * c + 2 * p1 * x * y + p2 * (r2 + 2 * x * x), y * c + 2 * p2 * x * y + p1 * (r2 + 2 * y * y)];
            },
            maxAngle: foldAngle(radial, Math.PI * 0.999),
        };
    },
    poly3(k) {
        const [k1 = 0] = k;
        return radialPlane(r => r * (1 + k1 * r * r));
    },
    poly5(k) {
        const [k1 = 0, k2 = 0] = k;
        return radialPlane(r => { const r2 = r * r; return r * (1 + k1 * r2 + k2 * r2 * r2); });
    },
    ptlens(k) {
        const [a = 0, b = 0, c = 0] = k;
        return radialPlane(r => r * (a * r * r * r + b * r * r + c * r + 1));
    },
    gopro(k) {
        if (!k[1]) return models.pinhole();
        const poly = p => { let acc = 0, pw = 1; for (const c of k) { acc += c * pw; pw *= p; } return acc; };
        // POLY maps p = r_norm / k1 to the ray angle; r_norm = k1·p
        const lens = fromAngleOfRadius(r => poly(r / k[1]), Math.abs(k[1]) * 4);
        return lens;
    },
    /// Image radius as a polynomial of the ray angle, Σ k[i]·θ^(i+1), up to 12 terms (Gyroflow protobuf's
    /// `GenericPolynomial`); all-zero coefficients are a pinhole
    generic_polynomial(k) {
        const q = Array.from({ length: 12 }, (_, i) => Number(k[i]) || 0);
        if (q.every(x => !x)) return models.pinhole();
        const radiusOf = t => { let acc = 0, p = t; for (let i = 0; i < 12; i++) { acc += q[i] * p; p *= t; } return acc; };
        return {
            project(x, y, z) {
                const r = Math.hypot(x, y);
                if (r < 1e-12) return [0, 0];
                const s = radiusOf(Math.atan2(r, z)) / r;
                return [x * s, y * s];
            },
            maxAngle: foldAngle(radiusOf, Math.PI * 0.999),
        };
    },
    /// Gyroflow's own Sony model (distortion_models/sony.rs): k = [N, h, y_0..y_10, r_lin, c_1..c_10], the ray angle as a
    /// natural cubic spline of the normalized radius through y_i at i·h, a straight line up to 3° below r_lin
    sony(k) {
        const N = Math.round(k[0] || 0), h = k[1];
        if (!(N >= 1) || !(h > 0)) return models.pinhole();
        const y = Array.from({ length: N + 1 }, (_, i) => Number(k[2 + i]) || 0);
        const rlin = Number(k[13]) || 0;
        const c = Array.from({ length: N + 1 }, (_, i) => (i === 0 || i === N ? 0 : Number(k[13 + i]) || 0));
        const b = i => (y[i + 1] - y[i]) / h - h * (c[i + 1] + 2 * c[i]) / 3;
        const d = i => (c[i + 1] - c[i]) / (3 * h);
        const endSlope = b(N - 1) + 2 * c[N - 1] * h + 3 * d(N - 1) * h * h;
        const angleOf = r => {
            if (rlin > 0 && r < rlin) return 3 * DEG * r / rlin;
            if (r >= N * h) return y[N] + endSlope * (r - N * h);
            const i = Math.min(N - 1, Math.floor(r / h)), dx = r - i * h;
            return y[i] + b(i) * dx + c[i] * dx * dx + d(i) * dx * dx * dx;
        };
        return fromAngleOfRadius(angleOf, h * N * 1.5);
    },
    pinhole() {
        return {
            project(x, y, z) { return z > 1e-9 ? [x / z, y / z] : null; },
            maxAngle: 89.5 * DEG,
        };
    },
};

/// A model's projection, with no image for rays past where it folds back (`maxAngle`): a polynomial fitted over the
/// frame turns over somewhere outside it, and what it gives there is no lens
function bounded(m) {
    const limit = m.maxAngle ?? Math.PI;
    return { ...m, project: (x, y, z) => (Math.atan2(Math.hypot(x, y), z) > limit + 1e-9 ? null : m.project(x, y, z)) };
}

/// A lens on the z = 1 plane: radius → distorted radius
function radialPlane(f) {
    return {
        project(x, y, z) {
            if (!(z > 1e-9)) return null;
            x /= z; y /= z;
            const r = Math.hypot(x, y);
            if (r < 1e-12) return [0, 0];
            const s = f(r) / r;
            return [x * s, y * s];
        },
        maxAngle: foldAngle(t => f(Math.tan(t)), 89.9 * DEG),
    };
}

/// The angle where an angle → radius map stops increasing (the lens folds), sampled
function foldAngle(radiusOf, tMax) {
    let prev = 0;
    const steps = 720;
    for (let i = 1; i <= steps; i++) {
        const t = tMax * i / steps;
        const r = radiusOf(t);
        if (!(r > prev)) return tMax * (i - 1) / steps;
        prev = r;
    }
    return tMax;
}

export const MODEL_NAMES = {
    opencv_fisheye: 'OpenCV fisheye', opencv_standard: 'OpenCV standard', insta360: 'Insta360', poly3: 'Poly3',
    poly5: 'Poly5', ptlens: 'PTLens', gopro: 'GoPro POLY', sony: 'Sony lens curve', pinhole: 'Pinhole',
    generic_polynomial: 'Generic polynomial',
};

/// A lens from a Gyroflow lens profile (the `Lens / Data` JSON most parsers produce)
export function lensFromProfile(p) {
    const fp = p?.fisheye_params;
    const cm = fp?.camera_matrix;
    if (!Array.isArray(cm) || cm.length < 2) return null;
    // A placeholder (Gyroflow protobuf's: f = 1, no coefficients, the lens is in each frame's tags) isn't a lens
    if (!(cm[0][0] > 1)) return null;
    const model = p.distortion_model || 'opencv_fisheye';
    const make = models[model];
    const k = (fp.distortion_coeffs || []).map(Number);
    const notes = [];
    if (!make) notes.push(`The ${model} model isn't drawn; shown as a pinhole`);
    if (p.digital_lens) notes.push(`Plus the ${p.digital_lens} digital lens, not drawn`);
    const w = p.calib_dimension?.w || Math.round(cm[0][2] * 2);
    const h = p.calib_dimension?.h || Math.round(cm[1][2] * 2);
    return {
        source: 'profile',
        model, name: MODEL_NAMES[model] || model,
        title: [p.camera_brand, p.camera_model, p.lens_model].filter(Boolean).join(' '),
        fx: cm[0][0], fy: cm[1][1], cx: cm[0][2], cy: cm[1][2], w, h,
        coeffs: k, notes,
        ...bounded((make || models.pinhole)(k)),
    };
}

/// A lens from tags: a distortion model, its coefficients, the focal length and principal point in pixels
export function lensFromTags({ model, coeffs, focal, center, size }) {
    if (!model || !Array.isArray(coeffs) || !focal) return null;
    const make = models[model];
    const [fx, fy] = Array.isArray(focal) ? [focal[0], focal[1] ?? focal[0]] : [focal, focal];
    const [cx, cy] = center || [size?.[0] / 2, size?.[1] / 2];
    const w = size?.[0] || Math.round(cx * 2), h = size?.[1] || Math.round(cy * 2);
    return {
        source: 'tags', model, name: MODEL_NAMES[model] || model, title: '',
        fx, fy, cx, cy, w, h, coeffs, notes: make ? [] : [`The ${model} model isn't drawn; shown as a pinhole`],
        ...bounded((make || models.pinhole)(coeffs.map(Number))),
    };
}

/// Sony's lens curve (RTMD 0xe421), as Gyroflow reads it (gyro_source/sony.rs, `Sony::coefficients_from_lens_curve`):
/// coefficient i is the ray angle at radius (i + 1)·h, h = the effective sensor height / n (n = up to 10 of them), the
/// angle at radius 0 being 0; a natural cubic spline through them, and a straight line from the centre to where the
/// curve is at 3° (the 2026 SDK's linear region). `sensor` is the sensor in pixels, `pitch` the pixel pitch in nm
export function lensFromSony(table, sensor, pitch) {
    const coeffs = table?.coeffs;
    if (!Array.isArray(coeffs) || coeffs.length < 1) return null;
    const scale = Math.max(table.coeff_scale || 1, 1);
    const f_nm = table.focal_length_nm, h_nm = table.effective_sensor_height_nm;
    if (!(f_nm > 0) || !(h_nm > 0)) return null;
    const n = Math.min(coeffs.length, 10);
    const angles = [0, ...coeffs.slice(0, n).map(x => x / scale * DEG)];
    const step = h_nm / n / f_nm; // normalized radius between knots
    const radii = angles.map((_, i) => i * step);
    const spline = naturalSpline(radii, angles);
    const rising = angles.every((a, i) => i === 0 || a > angles[i - 1]);
    const rLin = rising ? naturalSpline(angles, radii)(3 * DEG) : 0;
    const angleOf = r => (rLin > 0 && r < rLin ? 3 * DEG * r / rLin : spline(r));
    const lens = fromAngleOfRadius(angleOf, step * n * 1.5);
    const p = Array.isArray(pitch) ? pitch[0] : pitch;
    const notes = [];
    let w, h, fx;
    if (sensor && p > 0) {
        [w, h] = sensor;
        fx = f_nm / p;
    } else {
        // No pixel size: draw in sensor units, the effective sensor height as the frame height
        fx = 1000;
        h = Math.round(h_nm / f_nm * fx);
        w = Math.round(h * 1.5);
        notes.push('No pixel pitch: the frame is approximate');
    }
    return {
        source: 'sony', model: 'sony', name: MODEL_NAMES.sony,
        title: `${(f_nm / 1e6).toFixed(1)} mm`,
        fx, fy: fx, cx: w / 2, cy: h / 2, w, h, coeffs: angles.slice(1).map(a => a / DEG), notes,
        knots: angles.map((a, i) => [radii[i], a]),
        ...bounded(lens),
    };
}

/// The image radius in pixels of a ray at `theta` along +x (and +y), null past the lens's field
export function radiusAt(lens, theta, axis = 0) {
    const ray = axis === 0 ? [Math.sin(theta), 0, Math.cos(theta)] : [0, Math.sin(theta), Math.cos(theta)];
    const p = lens.project(...ray);
    if (!p || !isFinite(p[0]) || !isFinite(p[1])) return null;
    return axis === 0 ? lens.fx * Math.abs(p[0]) : lens.fy * Math.abs(p[1]);
}

/// The ray angle (radians) whose image is `radius` pixels from the centre along an axis, by bisection
/// The largest angle to search a lens for an image radius: just inside its field, so the probe at the end is in it
const searchLimit = lens => Math.min(lens.maxAngle ?? Math.PI / 2, Math.PI * 0.999) - 1e-7;

export function angleAtRadius(lens, radius, axis = 0) {
    const tMax = searchLimit(lens);
    const rMax = radiusAt(lens, tMax, axis);
    if (rMax === null || radius > rMax) return null;
    let lo = 0, hi = tMax;
    for (let i = 0; i < 60; i++) {
        const mid = (lo + hi) / 2;
        const r = radiusAt(lens, mid, axis);
        if (r === null || r > radius) hi = mid; else lo = mid;
    }
    return (lo + hi) / 2;
}

/// The horizontal, vertical and diagonal field of view in degrees (null where the frame reaches past the lens)
export function fieldOfView(lens) {
    const hx = Math.max(lens.cx, lens.w - lens.cx), hy = Math.max(lens.cy, lens.h - lens.cy);
    const a = angleAtRadius(lens, lens.w / 2, 0), b = angleAtRadius(lens, lens.h / 2, 1);
    // The diagonal: along the corner's direction
    const dir = Math.atan2(hy, hx), rd = Math.hypot(lens.w / 2, lens.h / 2);
    const tMax = searchLimit(lens);
    let lo = 0, hi = tMax, d = null;
    const radiusDiag = t => {
        const p = lens.project(Math.sin(t) * Math.cos(dir), Math.sin(t) * Math.sin(dir), Math.cos(t));
        return p ? Math.hypot(lens.fx * p[0], lens.fy * p[1]) : null;
    };
    const rEnd = radiusDiag(tMax);
    if (rEnd !== null && rEnd >= rd) {
        for (let i = 0; i < 60; i++) {
            const mid = (lo + hi) / 2, r = radiusDiag(mid);
            if (r === null || r > rd) hi = mid; else lo = mid;
        }
        d = (lo + hi) / 2;
    }
    const deg = x => x === null ? null : 2 * x / DEG;
    return { h: deg(a), v: deg(b), d: deg(d), hx, hy };
}
