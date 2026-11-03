// SPDX-License-Identifier: MIT OR Apache-2.0
// A JSON tree: collapsible, searchable, numbers kept exactly as written (JSON.parse rounds 64-bit integers such as
// microsecond timestamps), nodes built only when opened and long arrays shown a few hundred items at a time.

import { el } from './util.js';

// ---------------------------------------------------------------------------------------------------------------------
// Parsing: { t: 'o', k: [keys], v: [nodes] } | { t: 'a', v: [nodes] } | { t: 's' | 'n' | 'b', v } | { t: 'z' }

export function parse(text) {
    let i = 0;
    const ws = () => { while (i < text.length && ' \t\n\r'.includes(text[i])) i++; };
    const fail = what => { throw new Error(`${what} at character ${i}`); };
    const str = () => {
        let out = '';
        i++; // "
        for (;;) {
            const j = text.indexOf('"', i);
            if (j < 0) fail('Unterminated string');
            let k = text.indexOf('\\', i);
            if (k < 0 || k > j) { out += text.slice(i, j); i = j + 1; return out; }
            out += text.slice(i, k);
            const c = text[k + 1];
            if (c === 'u') { out += String.fromCharCode(parseInt(text.slice(k + 2, k + 6), 16)); i = k + 6; }
            else { out += { n: '\n', t: '\t', r: '\r', b: '\b', f: '\f' }[c] ?? c; i = k + 2; }
        }
    };
    const value = () => {
        ws();
        const c = text[i];
        if (c === '{') {
            i++;
            const node = { t: 'o', k: [], v: [] };
            ws();
            if (text[i] === '}') { i++; return node; }
            for (;;) {
                ws();
                if (text[i] !== '"') fail('Expected a key');
                node.k.push(str());
                ws();
                if (text[i++] !== ':') fail('Expected ":"');
                node.v.push(value());
                ws();
                if (text[i] === ',') { i++; continue; }
                if (text[i] === '}') { i++; return node; }
                fail('Expected "," or "}"');
            }
        }
        if (c === '[') {
            i++;
            const node = { t: 'a', v: [] };
            ws();
            if (text[i] === ']') { i++; return node; }
            for (;;) {
                node.v.push(value());
                ws();
                if (text[i] === ',') { i++; continue; }
                if (text[i] === ']') { i++; return node; }
                fail('Expected "," or "]"');
            }
        }
        if (c === '"') return { t: 's', v: str() };
        if (text.startsWith('true', i)) { i += 4; return { t: 'b', v: true }; }
        if (text.startsWith('false', i)) { i += 5; return { t: 'b', v: false }; }
        if (text.startsWith('null', i)) { i += 4; return { t: 'z' }; }
        const m = /^-?\d+(\.\d+)?([eE][+-]?\d+)?/.exec(text.slice(i, i + 64));
        if (!m) fail('Unexpected character');
        i += m[0].length;
        return { t: 'n', v: m[0] };
    };
    const root = value();
    ws();
    if (i < text.length) fail('Trailing characters');
    return root;
}

const quote = s => JSON.stringify(s);

/// The node as JSON text, numbers as they were
export function stringify(node, indent = 2, depth = 0) {
    const pad = n => ' '.repeat(n * indent);
    switch (node.t) {
        case 's': return quote(node.v);
        case 'n': return node.v;
        case 'b': return String(node.v);
        case 'z': return 'null';
        case 'a': {
            if (!node.v.length) return '[]';
            if (node.v.every(isPrimitive) && node.v.length <= 16) return `[${node.v.map(x => stringify(x)).join(', ')}]`;
            return `[\n${node.v.map(x => pad(depth + 1) + stringify(x, indent, depth + 1)).join(',\n')}\n${pad(depth)}]`;
        }
        case 'o': {
            if (!node.k.length) return '{}';
            return `{\n${node.k.map((k, j) => `${pad(depth + 1)}${quote(k)}: ${stringify(node.v[j], indent, depth + 1)}`).join(',\n')}\n${pad(depth)}}`;
        }
    }
    return '';
}

const isPrimitive = n => n.t !== 'o' && n.t !== 'a';
const size = n => (n.t === 'o' ? n.k.length : n.t === 'a' ? n.v.length : 0);

// ---------------------------------------------------------------------------------------------------------------------
// The tree

const PAGE = 200; // array items shown at once
const INLINE = 12; // an array of up to this many numbers or short strings is shown on one line

function primitiveEl(n) {
    switch (n.t) {
        case 's': return el('span', { class: 'jv-s', text: quote(n.v) });
        case 'n': return el('span', { class: 'jv-n', text: n.v });
        case 'b': return el('span', { class: 'jv-b', text: String(n.v) });
        default: return el('span', { class: 'jv-z', text: 'null' });
    }
}

function inlineArray(n) {
    return n.t === 'a' && n.v.length > 0 && n.v.length <= INLINE && n.v.every(x => x.t === 'n' || x.t === 'b' || x.t === 'z' || (x.t === 's' && x.v.length <= 12));
}

/// A short preview of a closed object or array
function preview(n) {
    if (n.t === 'a') {
        if (!n.v.length) return '[]';
        const first = n.v.slice(0, 4).map(x => (isPrimitive(x) ? (x.t === 's' ? quote(x.v.length > 16 ? x.v.slice(0, 16) + '…' : x.v) : String(x.v ?? 'null')) : x.t === 'a' ? '[…]' : '{…}'));
        return `[${first.join(', ')}${n.v.length > 4 ? ', …' : ''}]`;
    }
    if (!n.k.length) return '{}';
    let out = '{ ', len = 2;
    for (let j = 0; j < n.k.length; j++) {
        const x = n.v[j];
        const v = isPrimitive(x) ? (x.t === 's' ? quote(x.v.length > 20 ? x.v.slice(0, 20) + '…' : x.v) : String(x.v ?? 'null')) : x.t === 'a' ? `[${x.v.length}]` : '{…}';
        const part = `${n.k[j]}: ${v}`;
        if (len + part.length > 70 && j > 0) { out += '…'; break; }
        out += (j ? ', ' : '') + part;
        len += part.length + 2;
    }
    return out + ' }';
}

const kindOf = n => (isPrimitive(n) ? 'p' : inlineArray(n) ? 'i' : n.t === 'o' ? 'o' : 'a');
const primText = n => (n.t === 's' ? quote(n.v) : n.t === 'z' ? 'null' : String(n.v));
const primClass = n => ({ s: 'jv-s', n: 'jv-n', b: 'jv-b', z: 'jv-z' })[n.t];
const childAt = (n, j) => (n.t === 'o' ? [n.k[j], n.v[j]] : [j, n.v[j]]);

function inlineEl(n) {
    const span = el('span', { class: 'jv-inline' }, '[');
    span.items = n.v.map((x, j) => { if (j) span.append(', '); const e = primitiveEl(x); span.append(e); return e; });
    span.append(']');
    return span;
}
const inlineText = n => n.v.map(primText).join(', ');

function flash(e) {
    e.classList.remove('jv-changed');
    void e.offsetWidth; // restart the animation
    e.classList.add('jv-changed');
}

/// The tree of a JSON value. `update` takes a new value of the same tag (the next sample's) and changes the tree in
/// place: rows stay where they are, open and closed sections stay so (by their path), a changed value briefly lights
/// up, and the selected row stays selected
export class JsonTree {
    constructor(container, { onSelect } = {}) {
        this.root = container;
        this.onSelect = onSelect;
        this.openKeys = new Set();  // the paths of the open sections (JSON of the path)
        this.autoDepth = 0;         // sections shallower than this open when first built
        this.filter = '';
        this.hitKey = new Set();    // paths whose key matches the search
        this.hitValue = new Set();  // paths whose value matches it
        this.onPath = new Set();    // paths with a match below them
        this.selectedKey = null;
        this.rec = null;
    }

    /// A new value to show: the sections two levels deep open
    set(node) {
        this.node = node;
        this.openKeys = new Set();
        this.autoDepth = 2;
        this.selectedKey = null;
        this.selectedRec = null;
        this.computeHits();
        this.render();
        this.autoDepth = 0;
    }

    /// The same tag's value at another sample, changed in place
    update(node) {
        this.node = node;
        this.computeHits();
        if (!this.rec) { this.render(); return; }
        const scroll = this.root.scrollTop;
        this.rec = this.reconcile(this.rec, null, node, [], 0);
        this.root.scrollTop = scroll;
        if (this.selectedRec) this.onSelect?.(this.selectedRec.node, this.selectedRec.path, true);
    }

    render() {
        const scroll = this.root.scrollTop;
        this.root.replaceChildren();
        this.selectedRec = null;
        this.rec = null;
        if (!this.node) return;
        this.rec = this.build(null, this.node, [], 0);
        this.root.append(this.rec.wrap);
        this.root.scrollTop = scroll;
    }

    /// Opens everything, up to `limit` rows so a huge value stays usable
    expandAll(limit = 4000) {
        let rows = 0;
        const walk = (n, path) => {
            if (rows > limit || isPrimitive(n) || inlineArray(n)) return;
            this.openKeys.add(JSON.stringify(path));
            for (let j = 0; j < Math.min(size(n), PAGE); j++) { rows++; const [k, x] = childAt(n, j); walk(x, [...path, k]); }
        };
        walk(this.node, []);
        this.render();
    }

    collapseAll() {
        this.openKeys = new Set([JSON.stringify([])]);
        this.render();
    }

    /// Marks what matches `text` and opens the sections with matches below them
    search(text) {
        this.filter = text.trim().toLowerCase();
        this.computeHits();
        if (this.filter) this.openKeys = new Set([...this.onPath, JSON.stringify([])]);
        this.render();
        return this.hitKey.size + this.hitValue.size;
    }

    computeHits() {
        this.hitKey = new Set();
        this.hitValue = new Set();
        this.onPath = new Set();
        if (!this.filter || !this.node) return;
        const f = this.filter;
        const walk = (key, n, path) => {
            const pk = JSON.stringify(path);
            let hit = false;
            if (key !== null && String(key).toLowerCase().includes(f)) { this.hitKey.add(pk); hit = true; }
            if (isPrimitive(n) && String(n.v ?? 'null').toLowerCase().includes(f)) { this.hitValue.add(pk); hit = true; }
            let below = false;
            if (!isPrimitive(n)) for (let j = 0; j < size(n); j++) { const [k, x] = childAt(n, j); below = walk(k, x, [...path, k]) || below; }
            if (below) this.onPath.add(pk);
            return hit || below;
        };
        walk(null, this.node, []);
    }

    // A record of a row: its path, value and elements, and the records of its children built so far
    build(key, n, path, depth) {
        const r = { key, path, pkey: JSON.stringify(path), depth, node: n, kind: kindOf(n), children: [], built: 0 };
        const container = r.kind === 'o' || r.kind === 'a';
        r.row = el('div', { class: 'jv-row', tabindex: '-1' });
        r.row.style.paddingLeft = `${depth * 14 + (container ? 0 : 14)}px`;
        r.wrap = el('div', { class: 'jv-node' }, r.row);
        if (container) r.row.append(el('span', { class: 'jv-caret', 'aria-hidden': 'true' }));
        if (key !== null) {
            r.keyEl = el('span', { class: typeof key === 'number' ? 'jv-i' : 'jv-k', text: String(key) });
            r.row.append(r.keyEl, el('span', { class: 'jv-p', text: ': ' }));
        }
        if (r.kind === 'p') { r.valEl = primitiveEl(n); r.row.append(r.valEl); }
        else if (r.kind === 'i') { r.valEl = inlineEl(n); r.inline = inlineText(n); r.row.append(r.valEl); }
        else {
            r.countEl = el('span', { class: 'jv-count' });
            r.prevEl = el('span', { class: 'jv-preview' });
            r.row.append(r.countEl, r.prevEl);
            r.childrenEl = el('div', { class: 'jv-children', hidden: true });
            r.wrap.append(r.childrenEl);
            this.summary(r);
        }
        this.marks(r);
        r.row.addEventListener('click', e => {
            if (e.target.closest('button')) return;
            if (container) this.open(r, r.childrenEl.hidden);
            this.select(r);
        });
        if (container) {
            r.row.addEventListener('keydown', e => {
                if (e.key === 'ArrowRight') { this.open(r, true); e.preventDefault(); }
                if (e.key === 'ArrowLeft') { this.open(r, false); e.preventDefault(); }
            });
            if (depth < this.autoDepth) this.openKeys.add(r.pkey);
            if (this.openKeys.has(r.pkey)) this.open(r, true);
        }
        if (r.pkey === this.selectedKey) { r.row.classList.add('jv-sel'); this.selectedRec = r; }
        return r;
    }

    summary(r) {
        r.countEl.textContent = r.kind === 'a' ? `[${r.node.v.length}]` : `{${r.node.k.length}}`;
        r.prevEl.textContent = preview(r.node);
    }

    marks(r) {
        r.keyEl?.classList.toggle('jv-hit', this.hitKey.has(r.pkey));
        if (r.kind === 'p') r.valEl.classList.toggle('jv-hit', this.hitValue.has(r.pkey));
    }

    open(r, on) {
        if (on && !r.built) this.buildChildren(r, Math.min(size(r.node), PAGE));
        r.childrenEl.hidden = !on;
        r.wrap.classList.toggle('open', on);
        r.prevEl.hidden = on;
        if (on) this.openKeys.add(r.pkey); else this.openKeys.delete(r.pkey);
    }

    /// Builds children of `r` up to `upto`, then the button for the rest
    buildChildren(r, upto) {
        for (let j = r.built; j < upto; j++) {
            const [k, x] = childAt(r.node, j);
            const c = this.build(k, x, [...r.path, k], r.depth + 1);
            r.children.push(c);
            r.childrenEl.insertBefore(c.wrap, r.moreBtn || null);
        }
        r.built = Math.max(r.built, upto);
        this.moreButton(r);
    }

    moreButton(r) {
        const left = size(r.node) - r.built;
        if (left <= 0) { r.moreBtn?.remove(); r.moreBtn = null; return; }
        if (!r.moreBtn) {
            r.moreBtn = el('button', { class: 'jv-more' });
            r.moreBtn.style.marginLeft = `${(r.depth + 1) * 14 + 14}px`;
            r.moreBtn.addEventListener('click', () => this.buildChildren(r, Math.min(size(r.node), r.built + PAGE)));
            r.childrenEl.append(r.moreBtn);
        }
        r.moreBtn.textContent = `Show ${Math.min(PAGE, left)} more of ${left}`;
    }

    /// `r` showing `n` instead, changed in place where it's the same kind of value; the record that shows it
    reconcile(r, key, n, path, depth) {
        const kind = kindOf(n);
        if (kind !== r.kind) {
            const nr = this.build(key, n, path, depth);
            r.wrap.replaceWith(nr.wrap);
            return nr;
        }
        r.node = n;
        if (kind === 'p') {
            const text = primText(n);
            if (r.valEl.textContent !== text) {
                r.valEl.textContent = text;
                r.valEl.className = primClass(n);
                flash(r.valEl);
            }
        } else if (kind === 'i') {
            const text = inlineText(n);
            if (text !== r.inline) {
                if (r.valEl.items?.length === n.v.length) {
                    // The same items: only the numbers that changed change, and light up
                    n.v.forEach((x, j) => {
                        const e = r.valEl.items[j], t = primText(x);
                        if (e.textContent !== t) { e.textContent = t; e.className = primClass(x); flash(e); }
                    });
                } else {
                    const v = inlineEl(n);
                    r.valEl.replaceWith(v);
                    r.valEl = v;
                    flash(v);
                }
                r.inline = text;
            }
        } else {
            this.summary(r);
            if (r.built) {
                // The children built stay built, those of a value grown within the first page too
                const total = size(n);
                const upto = Math.min(total, Math.max(r.built, Math.min(PAGE, total)));
                const old = kind === 'o' ? new Map(r.children.map(c => [c.key, c])) : null;
                const next = [];
                for (let j = 0; j < upto; j++) {
                    const [k, x] = childAt(n, j);
                    const prev = kind === 'o' ? old.get(k) : r.children[j];
                    next.push(prev ? this.reconcile(prev, k, x, [...path, k], depth + 1) : this.build(k, x, [...path, k], depth + 1));
                    if (prev && kind === 'o') old.delete(k);
                }
                if (kind === 'o') for (const c of old.values()) c.wrap.remove();
                else for (const c of r.children.slice(upto)) c.wrap.remove();
                // Put them in order, moving only what's out of place
                let at = r.childrenEl.firstChild;
                for (const c of next) {
                    if (at === c.wrap) { at = at.nextSibling; continue; }
                    r.childrenEl.insertBefore(c.wrap, at);
                }
                r.children = next;
                r.built = upto;
                this.moreButton(r);
            }
        }
        this.marks(r);
        return r;
    }

    select(r) {
        this.selectedRec?.row.classList.remove('jv-sel');
        r.row.classList.add('jv-sel');
        this.selectedRec = r;
        this.selectedKey = r.pkey;
        this.onSelect?.(r.node, r.path, false);
    }

    get selectedNode() { return this.selectedRec?.node; }
    get selectedPath() { return this.selectedRec?.path; }
}

/// A path as JavaScript would index it: clip_meta.lens[0]["odd key"]
export function pathText(path) {
    return path.map((p, i) => (typeof p === 'number' ? `[${p}]` : /^[A-Za-z_$][\w$]*$/.test(p) ? (i ? '.' : '') + p : `[${quote(p)}]`)).join('');
}
