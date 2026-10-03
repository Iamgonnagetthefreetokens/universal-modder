// =============================================================================
//  Minimal DOM + Canvas2D stub so the presentation layer can be smoke-tested
//  in plain Node (no browser in this sandbox). Not a fake browser - just enough
//  surface to catch real wiring bugs: wrong selectors, missing fields, typos.
//  Used by tools/render-smoke.mjs.
// =============================================================================

const NOOP = () => {};

export function makeCtx(canvas) {
  const calls = { count: 0 };
  const base = {
    canvas,
    measureText: (s) => ({ width: String(s).length * 6, actualBoundingBoxAscent: 8, actualBoundingBoxDescent: 2 }),
    createLinearGradient: () => ({ addColorStop: NOOP }),
    createRadialGradient: () => ({ addColorStop: NOOP }),
    createPattern: () => ({ setTransform: NOOP }),
    getImageData: () => ({ data: new Uint8ClampedArray(4), width: 1, height: 1 }),
    setTransform: NOOP,
    getTransform: () => ({ a: 1, d: 1 }),
    isPointInPath: () => false,
  };
  return new Proxy(base, {
    get(t, k) {
      if (k in t) return t[k];
      if (typeof k !== 'string') return undefined;
      if (k.startsWith('on')) return undefined;
      // properties (fillStyle, lineWidth, font...) read back as strings
      if (k[0] === k[0].toUpperCase()) return undefined;
      return (...args) => {
        calls.count++;
        return undefined;
      };
    },
    set(t, k, v) {
      t[k] = v;
      return true;
    },
  });
}

class ClassList {
  constructor(el) {
    this.el = el;
  }
  get list() {
    return (this.el.className || '').split(/\s+/).filter(Boolean);
  }
  add(...c) {
    const s = new Set(this.list.concat(c));
    this.el.className = [...s].join(' ');
  }
  remove(...c) {
    this.el.className = this.list.filter((x) => !c.includes(x)).join(' ');
  }
  toggle(c, force) {
    const has = this.list.includes(c);
    const want = force === undefined ? !has : !!force;
    if (want) this.add(c);
    else this.remove(c);
    return want;
  }
  contains(c) {
    return this.list.includes(c);
  }
}

function matchOne(el, sel) {
  sel = sel.trim();
  if (!sel) return false;
  if (sel.startsWith('.')) return el.classList.contains(sel.slice(1));
  if (sel.startsWith('#')) return el.id === sel.slice(1);
  const m = sel.match(/^\[([\w-]+)(?:=([^\]]+))?\]$/);
  if (m) {
    const key = m[1].replace(/^data-/, '').replace(/-([a-z])/g, (_, c) => c.toUpperCase());
    const v = el.dataset[key] ?? el.attrs[m[1]];
    if (v === undefined) return false;
    return m[2] === undefined || String(v) === m[2].replace(/^['"]|['"]$/g, '');
  }
  return el.tagName === sel.toUpperCase();
}

export class El {
  constructor(tag) {
    this.nodeType = 1;
    this.tagName = String(tag).toUpperCase();
    this.className = '';
    this.id = '';
    this.children = [];
    this.parentNode = null;
    this.dataset = {};
    this.attrs = {};
    this.listeners = {};
    this.style = new Proxy(
      { setProperty: NOOP, removeProperty: NOOP, getPropertyValue: () => '' },
      { get: (t, k) => (k in t ? t[k] : ''), set: (t, k, v) => ((t[k] = v), true) }
    );
    this._text = '';
    this.disabled = false;
    this.value = '';
    if (this.tagName === 'CANVAS') {
      this.width = 300;
      this.height = 150;
      this._ctx = makeCtx(this);
    }
  }
  getContext() {
    if (!this._ctx) this._ctx = makeCtx(this);
    return this._ctx;
  }
  get parentElement() {
    return this.parentNode;
  }
  get classList() {
    if (!this._cl) this._cl = new ClassList(this);
    return this._cl;
  }
  set classListName(v) {
    this.className = v;
  }
  append(...kids) {
    for (let k of kids.flat()) {
      if (k === null || k === undefined || k === false) continue;
      if (typeof k === 'string' || typeof k === 'number') k = new TextNode(String(k));
      k.parentNode = this;
      this.children.push(k);
    }
  }
  get lastChild() {
    return this.children[this.children.length - 1] || null;
  }
  get firstChild() {
    return this.children[0] || null;
  }
  prepend(...kids) {
    const before = this.children.slice();
    this.children = [];
    this.append(...kids);
    this.children.push(...before);
    for (const k of this.children) if (k instanceof El) k.parentNode = this;
  }
  appendChild(k) {
    this.append(k);
    return k;
  }
  removeChild(k) {
    const i = this.children.indexOf(k);
    if (i >= 0) this.children.splice(i, 1);
    return k;
  }
  remove() {
    if (this.parentNode) this.parentNode.removeChild(this);
  }
  setAttribute(k, v) {
    this.attrs[k] = String(v);
    if (k.startsWith('data-')) this.dataset[k.slice(5).replace(/-([a-z])/g, (_, c) => c.toUpperCase())] = String(v);
    if (k === 'id') this.id = String(v);
    if (k === 'class') this.className = String(v);
  }
  getAttribute(k) {
    return this.attrs[k] ?? null;
  }
  addEventListener(type, fn) {
    (this.listeners[type] = this.listeners[type] || []).push(fn);
  }
  removeEventListener(type, fn) {
    this.listeners[type] = (this.listeners[type] || []).filter((f) => f !== fn);
  }
  dispatch(type, ev = {}) {
    for (const fn of this.listeners[type] || []) fn({ type, target: this, currentTarget: this, preventDefault: NOOP, stopPropagation: NOOP, ...ev });
  }
  querySelector(sel) {
    return this.querySelectorAll(sel)[0] || null;
  }
  querySelectorAll(sel) {
    const parts = String(sel)
      .trim()
      .split(/\s+/)
      .filter(Boolean);
    let cur = [this];
    for (const p of parts) {
      const next = [];
      const seen = new Set();
      for (const el of cur)
        for (const d of el.descendants())
          if (!seen.has(d) && matchOne(d, p)) {
            seen.add(d);
            next.push(d);
          }
      cur = next;
    }
    return cur;
  }
  descendants(out = []) {
    for (const c of this.children) {
      if (c instanceof El) {
        out.push(c);
        c.descendants(out);
      }
    }
    return out;
  }
  get textContent() {
    let out = this._text;
    for (const c of this.children) out += c instanceof TextNode ? c.data : c.textContent;
    return out;
  }
  set textContent(v) {
    this.children = this.children.filter((c) => !(c instanceof TextNode));
    this._text = String(v);
  }
  get innerHTML() {
    return '';
  }
  set innerHTML(v) {
    if (v !== '') throw new Error('innerHTML must only be used to clear (saw: ' + String(v).slice(0, 40) + ')');
    this.children = [];
    this._text = '';
  }
  getBoundingClientRect() {
    return { left: 0, top: 0, right: 1280, bottom: 720, width: 1280, height: 720, x: 0, y: 0 };
  }
  get clientWidth() {
    return this._cw ?? 1280;
  }
  set clientWidth(v) {
    this._cw = v;
  }
  get clientHeight() {
    return this._ch ?? 720;
  }
  set clientHeight(v) {
    this._ch = v;
  }
  animate() {
    return { finished: Promise.resolve(), cancel: NOOP };
  }
  setPointerCapture() {}
  releasePointerCapture() {}
  focus() {}
  click() {
    this.dispatch('click', {});
  }
}

class TextNode {
  constructor(data) {
    this.data = data;
    this.nodeType = 3;
  }
}

export function installDom({ width = 1280, height = 720 } = {}) {
  const registry = new Map();
  const byId = new Map();
  const root = new El('body');
  const mk = (tag) => {
    const el = new El(tag);
    el.clientWidth = width;
    el.clientHeight = height;
    registry.set(el, tag);
    return el;
  };
  const document = {
    createElement: mk,
    createTextNode: (t) => new TextNode(String(t)),
    getElementById: (id) => byId.get(id) || null,
    querySelector: (sel) => root.querySelector(sel),
    querySelectorAll: (sel) => root.querySelectorAll(sel),
    addEventListener: NOOP,
    removeEventListener: NOOP,
    body: root,
    documentElement: root,
    visibilityState: 'visible',
    fonts: { ready: Promise.resolve() },
  };
  const stage = mk('div');
  stage.id = 'stage';
  byId.set('stage', stage);
  root.append(stage);
  for (const id of ['pitch', 'hudHost', 'ui']) {
    const el = mk(id === 'pitch' ? 'canvas' : 'div');
    el.id = id;
    byId.set(id, el);
    stage.append(el);
  }
  const store = new Map();
  const localStorage = {
    getItem: (k) => (store.has(k) ? store.get(k) : null),
    setItem: (k, v) => store.set(k, String(v)),
    removeItem: (k) => store.delete(k),
  };
  const rafQueue = [];
  const window = {
    document,
    localStorage,
    devicePixelRatio: 2,
    innerWidth: width,
    innerHeight: height,
    addEventListener: NOOP,
    removeEventListener: NOOP,
    matchMedia: () => ({ matches: false, addEventListener: NOOP, addListener: NOOP }),
    requestAnimationFrame: (fn) => (rafQueue.push(fn), rafQueue.length),
    cancelAnimationFrame: NOOP,
    performance: { now: () => Date.now() },
    AudioContext: undefined,
    navigator: { getGamepads: () => [], userAgent: 'node' },
  };
  Object.assign(globalThis, {
    window,
    document,
    localStorage,
    matchMedia: window.matchMedia,
    requestAnimationFrame: window.requestAnimationFrame,
    devicePixelRatio: 2,
    HTMLElement: El,
  });
  return { document, window, root, byId, rafQueue, El, registry, localStorage };
}

export function flushRaf(dom, times = 1) {
  let t = 0;
  for (let i = 0; i < times; i++) {
    const q = dom.rafQueue.splice(0);
    t += 1000 / 60;
    for (const fn of q) fn(t);
  }
  return t;
}
