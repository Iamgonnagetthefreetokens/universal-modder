// =============================================================================
//  Sound, synthesised in WebAudio - no audio files, no downloads, ~2 kB of code.
//  A living crowd bed plus one-shots for the ball, the whistle and the goal horn.
// =============================================================================
export class Sound {
  constructor() {
    this.ctx = null;
    this.enabled = true;
    this.ready = false;
    this.noiseBuf = null;
    this.crowdGain = null;
    this.hum = null;
  }

  async start() {
    if (this.ctx) {
      if (this.ctx.state === 'suspended') await this.ctx.resume();
      return;
    }
    const AC = window.AudioContext || window.webkitAudioContext;
    if (!AC) return;
    this.ctx = new AC();
    const master = this.ctx.createGain();
    master.gain.value = 0.9;
    master.connect(this.ctx.destination);
    this.master = master;
    // one shared noise buffer, reused by every one-shot
    const len = this.ctx.sampleRate * 2;
    const buf = this.ctx.createBuffer(1, len, this.ctx.sampleRate);
    const d = buf.getChannelData(0);
    for (let i = 0; i < len; i++) d[i] = Math.random() * 2 - 1;
    this.noiseBuf = buf;
    this.buildCrowd();
    this.ready = true;
  }

  buildCrowd() {
    const ctx = this.ctx;
    const src = ctx.createBufferSource();
    src.buffer = this.noiseBuf;
    src.loop = true;
    const bp = ctx.createBiquadFilter();
    bp.type = 'bandpass';
    bp.frequency.value = 420;
    bp.Q.value = 0.7;
    const lp = ctx.createBiquadFilter();
    lp.type = 'lowpass';
    lp.frequency.value = 1400;
    const g = ctx.createGain();
    g.gain.value = 0.0;
    src.connect(bp);
    bp.connect(lp);
    lp.connect(g);
    g.connect(this.master);
    src.start();
    this.crowdGain = g;
    this.crowdFilter = bp;
    // slow swell so the arena breathes
    const lfo = ctx.createOscillator();
    lfo.frequency.value = 0.13;
    const lfoGain = ctx.createGain();
    lfoGain.gain.value = 120;
    lfo.connect(lfoGain);
    lfoGain.connect(bp.frequency);
    lfo.start();
    this.lfo = lfo;
  }

  setMuted(m) {
    this.enabled = !m;
    if (this.master) this.master.gain.value = m ? 0 : 0.9;
  }

  /** intensity 0..1 - rises when a goal goes in or the ball is in the box */
  crowdLevel(level) {
    if (!this.crowdGain) return;
    const target = this.enabled ? 0.03 + level * 0.16 : 0;
    this.crowdGain.gain.setTargetAtTime(target, this.ctx.currentTime, 0.35);
  }

  roar(strength = 1) {
    if (!this.ready) return;
    const ctx = this.ctx;
    const src = ctx.createBufferSource();
    src.buffer = this.noiseBuf;
    src.loop = true;
    const bp = ctx.createBiquadFilter();
    bp.type = 'bandpass';
    bp.frequency.value = 900;
    bp.Q.value = 0.5;
    const g = ctx.createGain();
    const now = ctx.currentTime;
    g.gain.setValueAtTime(0.0001, now);
    g.gain.exponentialRampToValueAtTime(0.34 * strength, now + 0.12);
    g.gain.exponentialRampToValueAtTime(0.0001, now + 2.6 + strength);
    src.connect(bp);
    bp.connect(g);
    g.connect(this.master);
    src.start(now);
    src.stop(now + 3.4 + strength);
  }

  blip({ freq = 620, type = 'square', dur = 0.09, gain = 0.16, slide = 0, delay = 0 }) {
    if (!this.ready || !this.enabled) return;
    const ctx = this.ctx;
    const o = ctx.createOscillator();
    const g = ctx.createGain();
    o.type = type;
    const now = ctx.currentTime + delay;
    o.frequency.setValueAtTime(freq, now);
    if (slide) o.frequency.exponentialRampToValueAtTime(Math.max(40, freq + slide), now + dur);
    g.gain.setValueAtTime(0.0001, now);
    g.gain.exponentialRampToValueAtTime(gain, now + 0.008);
    g.gain.exponentialRampToValueAtTime(0.0001, now + dur);
    o.connect(g);
    g.connect(this.master);
    o.start(now);
    o.stop(now + dur + 0.02);
  }

  noise({ dur = 0.12, freq = 1200, q = 1, gain = 0.2, type = 'bandpass', sweep = 0 }) {
    if (!this.ready || !this.enabled) return;
    const ctx = this.ctx;
    const src = ctx.createBufferSource();
    src.buffer = this.noiseBuf;
    const f = ctx.createBiquadFilter();
    f.type = type;
    f.frequency.value = freq;
    f.Q.value = q;
    const g = ctx.createGain();
    const now = ctx.currentTime;
    g.gain.setValueAtTime(gain, now);
    g.gain.exponentialRampToValueAtTime(0.0001, now + dur);
    if (sweep) f.frequency.exponentialRampToValueAtTime(Math.max(60, freq + sweep), now + dur);
    src.connect(f);
    f.connect(g);
    g.connect(this.master);
    src.start(now);
    src.stop(now + dur + 0.02);
  }

  kick(power = 0.5) {
    this.noise({ dur: 0.08, freq: 1500, q: 0.8, gain: 0.1 + power * 0.12, sweep: -900 });
    this.blip({ freq: 150 + power * 90, type: 'sine', dur: 0.11, gain: 0.16 + power * 0.1, slide: -80 });
  }

  pass() {
    this.noise({ dur: 0.06, freq: 900, q: 1.2, gain: 0.09, sweep: -500 });
  }

  net() {
    this.noise({ dur: 0.3, freq: 380, q: 0.7, gain: 0.18, sweep: -220 });
  }

  post() {
    this.blip({ freq: 1150, type: 'triangle', dur: 0.22, gain: 0.2, slide: -500 });
  }

  whistle(long = false) {
    const base = 2100;
    for (let i = 0; i < (long ? 3 : 1); i++) {
      this.blip({ freq: base + i * 40, type: 'square', dur: long ? 0.26 : 0.14, gain: 0.1, delay: i * 0.3 });
      this.blip({ freq: base * 1.02, type: 'square', dur: long ? 0.2 : 0.1, gain: 0.06, delay: i * 0.3 + 0.03 });
    }
  }

  goal() {
    // horn + cymbal + roar
    const chords = [220, 277, 330, 440];
    chords.forEach((f, i) => this.blip({ freq: f, type: 'sawtooth', dur: 1.5, gain: 0.09, delay: i * 0.05 }));
    this.noise({ dur: 1.1, freq: 5200, q: 0.4, gain: 0.1, type: 'highpass' });
    this.roar(1.6);
  }

  save() {
    this.noise({ dur: 0.24, freq: 260, q: 0.6, gain: 0.2, sweep: -160 });
    this.blip({ freq: 300, type: 'sine', dur: 0.16, gain: 0.12, slide: -140 });
  }

  card() {
    this.blip({ freq: 180, type: 'square', dur: 0.22, gain: 0.12, slide: -60 });
  }

  ui(up = true) {
    this.blip({ freq: up ? 780 : 460, type: 'triangle', dur: 0.07, gain: 0.09 });
  }

  select() {
    this.blip({ freq: 1120, type: 'triangle', dur: 0.05, gain: 0.07 });
  }
}

export const sound = new Sound();
