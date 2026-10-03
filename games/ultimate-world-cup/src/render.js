// =============================================================================
//  Renderer: one canvas, top-down "sensible" view of a whole pitch.
//  Everything is drawn procedurally - turf, crowd, nets, kits, confetti - so the
//  game ships as plain files with no assets to download.
// =============================================================================
import { P } from './match.js';
import { drawFlag } from './flags.js';

const SKIN = ['#f0c39b', '#d9a06b', '#a56a3c', '#6b4226', '#f7d7b6', '#8d5524'];

export class Renderer {
  constructor(canvas) {
    this.canvas = canvas;
    this.ctx = canvas.getContext('2d', { alpha: false });
    this.dpr = 1;
    this.scale = 8;
    this.ox = 0;
    this.oy = 0;
    this.camX = 0;
    this.camY = 0;
    this.crowd = null;
    this.crowdKey = '';
    this.resize();
  }

  /**
   * Where the camera sits. Normally the whole pitch; a shootout steps in on the
   * penalty area so a kick reads at 1:1 instead of 1:12.
   */
  cameraFor(match) {
    const zoom = () => ({ scale: Math.min(this.cssW / 32, this.cssH / 21) * 0.96 });
    const p = match.pens;
    if (match.state === 'shootout' && p && p.spot) {
      return { x: p.spot.x + Math.sign(p.spot.x || 1) * 7.5, y: 0, ...zoom() };
    }
    // a penalty given in open play gets the same treatment
    const r = match.restart;
    if (match.state === 'restart' && r && r.kind === 'penalty') {
      return { x: r.x + Math.sign(r.x || 1) * 7.5, y: 0, ...zoom() };
    }
    return { x: 0, y: 0, scale: this.fitScale };
  }

  applyCamera(match) {
    const cam = this.cameraFor(match);
    if (cam.scale !== this.scale || cam.x !== this.camX || cam.y !== this.camY) {
      this.scale = cam.scale;
      this.camX = cam.x;
      this.camY = cam.y;
      this.ox = this.cssW / 2 - cam.x * cam.scale;
      this.oy = this.originY - cam.y * cam.scale;
      this.crowdKey = '';
    }
  }

  resize() {
    const parent = this.canvas.parentElement || document.body;
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    const w = Math.max(320, parent.clientWidth);
    const h = Math.max(220, parent.clientHeight);
    this.canvas.width = Math.round(w * dpr);
    this.canvas.height = Math.round(h * dpr);
    this.canvas.style.width = w + 'px';
    this.canvas.style.height = h + 'px';
    this.dpr = dpr;
    this.cssW = w;
    this.cssH = h;
    // in portrait the pitch should be as wide as the screen allows, with the stands as a band
    const portrait = h > w * 1.12;
    const marginX = portrait ? 1.5 : 11;
    const marginY = portrait ? 6 : 9;
    this.fitScale = Math.min(w / (P.L + marginX * 2), h / (P.W + marginY * 2)) * 0.99;
    this.scale = this.fitScale;
    this.camX = 0;
    this.camY = 0;
    this.ox = w / 2;
    // in portrait the pitch rides high so the thumb pad gets its own third of the screen
    this.originY = portrait ? h * 0.42 : h / 2;
    this.oy = this.originY;
    this.crowd = null;
    this.crowdKey = '';
  }

  X(x) {
    return this.ox + x * this.scale;
  }
  Y(y) {
    return this.oy + y * this.scale;
  }

  /** the turf including its run-off, which is what the stands and boards wrap */
  grassRect() {
    const r = this.pitchRect();
    const ax = 5 * this.scale;
    const ay = 4 * this.scale;
    return { left: r.left - ax, top: r.top - ay, right: r.right + ax, bottom: r.bottom + ay, w: r.w + ax * 2, h: r.h + ay * 2 };
  }

  /** how big N screen pixels are in pitch metres - keeps sprites legible at any zoom */
  px(n) {
    return n / (this.scale || 8);
  }

  /** px-space rect of the pitch, used by the DOM overlays for positioning. */
  pitchRect() {
    return {
      left: this.X(-P.HL),
      top: this.Y(-P.HW),
      right: this.X(P.HL),
      bottom: this.Y(P.HW),
      w: P.L * this.scale,
      h: P.W * this.scale,
    };
  }

  draw(match, view = {}) {
    const ctx = this.ctx;
    this.applyCamera(match);
    const s = this.scale * this.dpr;
    const t = match.t;
    ctx.save();
    ctx.setTransform(this.dpr, 0, 0, this.dpr, 0, 0);
    // screen shake
    const sh = match.shake;
    if (sh > 0.01) {
      ctx.translate((Math.random() - 0.5) * sh * 9, (Math.random() - 0.5) * sh * 9);
    }
    this.drawStadium(match, view);
    ctx.restore();

    ctx.save();
    ctx.setTransform(s, 0, 0, s, this.X(0) * this.dpr, this.Y(0) * this.dpr);
    // now 1 unit = 1 metre, origin at centre spot
    this.drawPitch(match);
    this.drawGoals(match);
    this.drawShadows(match);
    this.drawPlayers(match, view);
    this.drawBall(match);
    this.drawEffects(match);
    ctx.restore();
    ctx.save();
    ctx.setTransform(this.dpr, 0, 0, this.dpr, 0, 0);
    this.drawOverlayText(match, view);
    if (match.flash > 0.01) {
      ctx.fillStyle = `rgba(255,255,255,${Math.min(0.5, match.flash * 0.35)})`;
      ctx.fillRect(0, 0, this.cssW, this.cssH);
    }
    ctx.restore();
  }

  // --- arena -------------------------------------------------------------
  drawStadium(match, view) {
    const ctx = this.ctx;
    const w = this.cssW;
    const h = this.cssH;
    const rect = this.grassRect();
    const st = match.venue || { turf: '#2f8f3e', crowd: ['#243055', '#c9d3ff', '#4a5b8c'], name: 'Stadium' };
    const g = ctx.createLinearGradient(0, 0, 0, h);
    g.addColorStop(0, '#05070c');
    g.addColorStop(0.55, '#0b1018');
    g.addColorStop(1, '#05070c');
    ctx.fillStyle = g;
    ctx.fillRect(0, 0, w, h);

    // floodlight pools
    ctx.save();
    ctx.globalCompositeOperation = 'lighter';
    for (const fx of [0.18, 0.5, 0.82]) {
      const rg = ctx.createRadialGradient(w * fx, h * 0.12, 10, w * fx, h * 0.12, h * 0.9);
      rg.addColorStop(0, 'rgba(190,220,255,0.10)');
      rg.addColorStop(1, 'rgba(0,0,0,0)');
      ctx.fillStyle = rg;
      ctx.fillRect(0, 0, w, h);
    }
    ctx.restore();

    // crowd: cached dot pattern, animated when a goal goes in
    const key = `${Math.round(w)}x${Math.round(h)}|${st.crowd.join(',')}|${Math.round(this.scale)}`;
    if (this.crowdKey !== key) this.buildCrowd(st, rect);
    ctx.drawImage(this.crowdCanvas, 0, 0, w, h);

    // LED boards hugging the touchlines
    const boardH = Math.max(9, Math.min(20, h * 0.026));
    for (const y of [rect.top - boardH * 1.5, rect.bottom + boardH * 0.5]) {
      ctx.fillStyle = '#0a0f18';
      ctx.fillRect(0, y, w, boardH);
      ctx.save();
      ctx.beginPath();
      ctx.rect(0, y, w, boardH);
      ctx.clip();
      const txt = `  ULTIMATE WORLD CUP 26  ·  ${st.name.toUpperCase()}  ·  ${match.teams.home.team.name.toUpperCase()} v ${match.teams.away.team.name.toUpperCase()}  ·  `;
      ctx.font = `700 ${boardH * 0.72}px "Segoe UI", system-ui, sans-serif`;
      ctx.fillStyle = '#8ef23a';
      ctx.textBaseline = 'middle';
      let x = -((this.timeShift || 0) % (txt.length * boardH * 0.55));
      while (x < w) {
        ctx.fillText(txt, x, y + boardH * 0.55);
        x += ctx.measureText(txt).width;
      }
      ctx.restore();
    }
  }

  buildCrowd(st, rect) {
    const w = this.cssW;
    const h = this.cssH;
    const c = document.createElement('canvas');
    const dpr = this.dpr || 1;
    c.width = Math.max(1, Math.round(w * dpr));
    c.height = Math.max(1, Math.round(h * dpr));
    const g = c.getContext('2d');
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    const colors = st.crowd;
    const gap = Math.max(4.2, Math.min(8, Math.min(w, h) / 170));
    const pad = gap * 2.4;
    // depth of the stands: enough to feel like a bowl, never a wall of dots
    const band = Math.max(26, Math.min(Math.min(w, h) * 0.16, Math.min(w, h) * 0.5 - Math.min(rect.h, rect.w) * 0.1));
    const x0 = rect.left - pad - band;
    const x1 = rect.right + pad + band;
    const y0 = rect.top - pad - band;
    const y1 = rect.bottom + pad + band;
    const near = (x, y) => x > x0 && x < x1 && y > y0 && y < y1;
    const inPitch = (x, y) => x > rect.left - pad && x < rect.right + pad && y > rect.top - pad && y < rect.bottom + pad;
    for (let y = Math.max(gap * 0.5, y0); y < Math.min(h, y1); y += gap) {
      for (let x = Math.max(gap * 0.5, x0); x < Math.min(w, x1); x += gap) {
        if (inPitch(x, y) || !near(x, y)) continue;
        const i = Math.round(x / gap) * 31 + Math.round(y / gap) * 17;
        const jitterX = ((i * 7919) % 13) / 13 - 0.5;
        const jitterY = ((i * 104729) % 11) / 11 - 0.5;
        // the far edge of each stand fades into the tunnel dark
        const d = Math.min(Math.abs(x - (x < rect.left ? rect.left : rect.right)) , Math.abs(y - (y < rect.top ? rect.top : rect.bottom)));
        g.fillStyle = colors[i % colors.length];
        g.globalAlpha = (0.34 + ((i * 37) % 42) / 100) * Math.max(0.12, 1 - d / (band + 1));
        g.beginPath();
        g.arc(x + jitterX * gap * 0.5, y + jitterY * gap * 0.5, gap * 0.31, 0, Math.PI * 2);
        g.fill();
      }
    }
    g.globalAlpha = 1;
    // moat between the stands and the turf
    g.strokeStyle = 'rgba(3,6,10,0.9)';
    g.lineWidth = gap * 1.2;
    g.strokeRect(rect.left - pad, rect.top - pad, rect.w + pad * 2, rect.h + pad * 2);
    g.strokeStyle = 'rgba(140,190,255,0.06)';
    g.lineWidth = 1;
    g.strokeRect(rect.left - pad * 2.2, rect.top - pad * 2.2, rect.w + pad * 4.4, rect.h + pad * 4.4);
    this.crowdCanvas = c;
    this.crowdKey = `${Math.round(w)}x${Math.round(h)}|${colors.join(',')}|${Math.round(this.scale)}`;
  }

  // --- pitch -------------------------------------------------------------
  drawPitch(match) {
    const ctx = this.ctx;
    const s = 1;
    const turf = (match.venue && match.venue.turf) || '#2f8f3e';
    ctx.save();
    ctx.fillStyle = turf;
    ctx.fillRect(-P.HL - 5, -P.HW - 4, P.L + 10, P.W + 8);
    // mow stripes
    const stripes = 14;
    for (let i = 0; i < stripes; i++) {
      ctx.fillStyle = i % 2 ? 'rgba(255,255,255,0.045)' : 'rgba(0,0,0,0.05)';
      ctx.fillRect(-P.HL - 5 + (i * (P.L + 10)) / stripes, -P.HW - 4, (P.L + 10) / stripes, P.W + 8);
    }
    // wet sheen
    const grad = ctx.createRadialGradient(0, 0, P.HL * 0.2, 0, 0, P.HL * 1.15);
    grad.addColorStop(0, 'rgba(255,255,255,0.05)');
    grad.addColorStop(1, 'rgba(0,0,0,0.22)');
    ctx.fillStyle = grad;
    ctx.fillRect(-P.HL - 5, -P.HW - 4, P.L + 10, P.W + 8);

    // markings
    ctx.strokeStyle = 'rgba(255,255,255,0.85)';
    ctx.lineWidth = 0.16;
    ctx.lineCap = 'butt';
    ctx.strokeRect(-P.HL, -P.HW, P.L, P.W);
    ctx.beginPath();
    ctx.moveTo(0, -P.HW);
    ctx.lineTo(0, P.HW);
    ctx.stroke();
    ctx.beginPath();
    ctx.arc(0, 0, P.CC_R, 0, Math.PI * 2);
    ctx.stroke();
    for (const sx of [-1, 1]) {
      // penalty area + six-yard box + spot + D
      ctx.strokeRect(sx > 0 ? P.HL - P.BOX_D : -P.HL, -P.BOX_HW, P.BOX_D, P.BOX_HW * 2);
      ctx.strokeRect(sx > 0 ? P.HL - P.SIX_D : -P.HL, -P.SIX_HW, P.SIX_D, P.SIX_HW * 2);
      ctx.beginPath();
      ctx.arc(sx * (P.HL - P.SPOT), 0, 0.28, 0, Math.PI * 2);
      ctx.fillStyle = 'rgba(255,255,255,0.85)';
      ctx.fill();
      ctx.beginPath();
      const cx = sx * (P.HL - P.SPOT);
      const a0 = sx > 0 ? Math.PI * 0.62 : -Math.PI * 0.38;
      const a1 = sx > 0 ? Math.PI * 1.38 : Math.PI * 0.38;
      ctx.arc(cx, 0, P.CC_R, a0, a1);
      ctx.stroke();
      // corner arcs
      for (const sy of [-1, 1]) {
        ctx.beginPath();
        ctx.arc(sx * P.HL, sy * P.HW, 1.1, 0, Math.PI * 2);
        ctx.stroke();
      }
    }
    ctx.beginPath();
    ctx.arc(0, 0, 0.35, 0, Math.PI * 2);
    ctx.fillStyle = 'rgba(255,255,255,0.85)';
    ctx.fill();
    ctx.restore();
  }

  drawGoals(match) {
    const ctx = this.ctx;
    const ripple = match.netRipple || 0;
    for (const sx of [-1, 1]) {
      const gx = sx * P.HL;
      ctx.save();
      ctx.translate(gx, 0);
      ctx.scale(sx, 1);
      // net
      ctx.save();
      ctx.beginPath();
      ctx.rect(0, -P.GOAL_HALF, P.GOAL_DEPTH, P.GOAL_HALF * 2);
      ctx.clip();
      ctx.fillStyle = 'rgba(10,14,20,0.38)';
      ctx.fillRect(0, -P.GOAL_HALF, P.GOAL_DEPTH, P.GOAL_HALF * 2);
      ctx.strokeStyle = 'rgba(255,255,255,0.34)';
      ctx.lineWidth = Math.max(0.05, this.px(0.7));
      const step = Math.max(0.42, this.px(3.2)) + ripple * 0.12;
      for (let i = 0; i <= P.GOAL_HALF * 2 / step + 2; i++) {
        const yy = -P.GOAL_HALF + i * step + Math.sin(this.tGlobal * 6 + i) * ripple * 0.18;
        ctx.beginPath();
        ctx.moveTo(0, yy);
        ctx.lineTo(P.GOAL_DEPTH, yy);
        ctx.stroke();
      }
      for (let i = 0; i <= P.GOAL_DEPTH / step + 1; i++) {
        const xx = i * step;
        ctx.beginPath();
        ctx.moveTo(xx, -P.GOAL_HALF);
        ctx.lineTo(xx, P.GOAL_HALF);
        ctx.stroke();
      }
      ctx.restore();
      // frame: posts + bar seen from above
      ctx.strokeStyle = '#f6f8ff';
      ctx.lineWidth = Math.max(0.3, this.px(2.2));
      ctx.beginPath();
      ctx.moveTo(0, -P.GOAL_HALF);
      ctx.lineTo(P.GOAL_DEPTH, -P.GOAL_HALF);
      ctx.moveTo(0, P.GOAL_HALF);
      ctx.lineTo(P.GOAL_DEPTH, P.GOAL_HALF);
      ctx.stroke();
      ctx.fillStyle = '#ffffff';
      for (const py of [-P.GOAL_HALF, P.GOAL_HALF]) {
        ctx.beginPath();
        ctx.arc(0, py, 0.22, 0, Math.PI * 2);
        ctx.fill();
      }
      ctx.restore();
    }
  }

  drawShadows(match) {
    const ctx = this.ctx;
    ctx.save();
    ctx.fillStyle = 'rgba(0,0,0,0.28)';
    for (const p of match.players) {
      if (p.off) continue;
      ctx.beginPath();
      ctx.ellipse(p.x + 0.28, p.y + 0.5, 0.62, 0.3, 0, 0, Math.PI * 2);
      ctx.fill();
    }
    const b = match.ball;
    const lift = 1 + Math.min(2.4, b.z) * 0.55;
    ctx.globalAlpha = clampN(0.42 - b.z * 0.06, 0.1, 0.42);
    ctx.beginPath();
    ctx.ellipse(b.x + b.z * 0.16, b.y + 0.4 * lift, 0.3 * lift, 0.16 * lift, 0, 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();
  }

  drawPlayers(match, view) {
    const ctx = this.ctx;
    const list = match.players.filter((p) => !p.off).sort((a, b) => a.y - b.y);
    const active = match.active;
    for (const p of list) {
      const team = match.teams[p.side];
      const col = team.kit || team.team.colors;
      const r = Math.max(p.gk ? 0.74 : 0.72, this.px(6.6));
      ctx.save();
      ctx.translate(p.x, p.y);
      if (p === active) {
        const pulse = 1 + Math.sin(match.t * 7) * 0.1;
        ctx.save();
        ctx.strokeStyle = 'rgba(190,255,74,0.95)';
        ctx.lineWidth = Math.max(0.14, this.px(1.7));
        ctx.beginPath();
        ctx.arc(0, 0, r * 1.75 * pulse, 0, Math.PI * 2);
        ctx.stroke();
        ctx.restore();
      }
      // slide: squash the body along the direction of travel
      if (p.sliding > 0) {
        ctx.rotate(p.facing);
        ctx.scale(1.55, 0.72);
        ctx.rotate(-p.facing);
      }
      // legs: two little dashes that alternate as the player runs
      const stride = Math.sin(p.anim * 6) * 0.4;
      ctx.strokeStyle = col.shorts === '#ffffff' ? 'rgba(0,0,0,0.5)' : 'rgba(255,255,255,0.5)';
      ctx.lineWidth = 0.18;
      ctx.beginPath();
      ctx.moveTo(-0.1, -0.1 + stride * 0.4);
      ctx.lineTo(0.12, -0.5 - stride * 0.3);
      ctx.moveTo(0.1, 0.1 - stride * 0.4);
      ctx.lineTo(-0.12, 0.5 + stride * 0.3);
      ctx.stroke();
      // torso
      ctx.rotate(p.facing);
      drawKit(ctx, p, team.team, r, col);
      ctx.rotate(-p.facing);
      // head
      const skin = SKIN[(p.num + p.idx) % SKIN.length];
      ctx.fillStyle = skin;
      ctx.beginPath();
      ctx.arc(Math.cos(p.facing) * 0.16, Math.sin(p.facing) * 0.16, 0.3, 0, Math.PI * 2);
      ctx.fill();
      ctx.strokeStyle = 'rgba(0,0,0,0.35)';
      ctx.lineWidth = 0.05;
      ctx.stroke();
      // shirt number when there is room for it
      if (r * this.scale > 8.2) {
        ctx.fillStyle = 'rgba(255,255,255,0.85)';
        ctx.font = `700 ${0.62}px system-ui, sans-serif`;
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        ctx.fillText(String(p.num), -Math.cos(p.facing) * 0.3, -Math.sin(p.facing) * 0.3);
      }
      // card pip
      if (p.card) {
        ctx.fillStyle = p.card === 'red' ? '#ff3b30' : '#ffd60a';
        ctx.fillRect(-0.16, -r - 0.55, 0.32, 0.22);
      }
      // low stamina
      if (p.stamina < 0.5) {
        ctx.strokeStyle = 'rgba(255,120,60,0.85)';
        ctx.lineWidth = 0.1;
        ctx.beginPath();
        ctx.arc(0, 0, r + 0.22, -Math.PI / 2, -Math.PI / 2 + Math.PI * 2 * clampN((p.stamina - 0.28) / 0.22, 0, 1));
        ctx.stroke();
      }
      ctx.restore();
    }
  }

  drawBall(match) {
    const ctx = this.ctx;
    const b = match.ball;
    if (b.owner) {
      // ball at the carrier's boots, drawn smaller so the player reads clearly
    }
    // motion trail
    if (b.trail.length > 1) {
      for (let i = 0; i < b.trail.length; i++) {
        const tr = b.trail[i];
        const a = (i / b.trail.length) * 0.28;
        ctx.fillStyle = `rgba(255,255,255,${a})`;
        ctx.beginPath();
        ctx.arc(tr.x, tr.y, Math.max(0.16 + i * 0.012, this.px(1.5)), 0, Math.PI * 2);
        ctx.fill();
      }
    }
    const z = b.z || 0;
    const rr = Math.max(0.26 + z * 0.03, this.px(4.3) + z * 0.05);
    ctx.save();
    ctx.translate(b.x, b.y - z * 0.42);
    ctx.rotate(b.rot || 0);
    ctx.fillStyle = '#ffffff';
    ctx.beginPath();
    ctx.arc(0, 0, rr, 0, Math.PI * 2);
    ctx.fill();
    ctx.strokeStyle = 'rgba(0,0,0,0.5)';
    ctx.lineWidth = this.px(0.9);
    ctx.stroke();
    ctx.fillStyle = 'rgba(20,24,32,0.85)';
    for (let i = 0; i < 5; i++) {
      const a = (i / 5) * Math.PI * 2;
      ctx.beginPath();
      ctx.arc(Math.cos(a) * rr * 0.52, Math.sin(a) * rr * 0.52, rr * 0.24, 0, Math.PI * 2);
      ctx.fill();
    }
    ctx.beginPath();
    ctx.arc(0, 0, rr * 0.26, 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();
  }

  drawEffects(match) {
    const ctx = this.ctx;
    for (const f of match.fx) {
      if (f.t < 0) continue;
      const k = clampN(f.t / f.life, 0, 1);
      if (f.kind === 'dust') {
        ctx.fillStyle = `rgba(230,225,205,${(1 - k) * 0.5})`;
        ctx.beginPath();
        ctx.arc(f.x, f.y, 0.35 + k * 1.5, 0, Math.PI * 2);
        ctx.fill();
      } else if (f.kind === 'impact') {
        ctx.strokeStyle = f.big ? `rgba(255,230,120,${1 - k})` : `rgba(255,255,255,${(1 - k) * 0.7})`;
        ctx.lineWidth = 0.16;
        ctx.beginPath();
        ctx.arc(f.x, f.y, (f.big ? 0.5 : 0.3) + k * (f.big ? 2.2 : 1.1), 0, Math.PI * 2);
        ctx.stroke();
      } else if (f.kind === 'ring') {
        ctx.strokeStyle = `rgba(255,255,255,${1 - k})`;
        ctx.lineWidth = 0.2;
        ctx.beginPath();
        ctx.arc(f.x, f.y, 0.3 + k * 1.6, 0, Math.PI * 2);
        ctx.stroke();
      } else if (f.kind === 'confetti') {
        ctx.fillStyle = f.c;
        ctx.globalAlpha = 1 - k * 0.8;
        const yy = f.y + f.vy * f.t * 2.2 + f.t * f.t * 1.6;
        const xx = f.x + f.vx * f.t + Math.sin(f.t * 5 + f.x) * 0.7;
        ctx.fillRect(xx, yy, 0.42, 0.24);
        ctx.globalAlpha = 1;
      } else if (f.kind === 'net') {
        ctx.strokeStyle = `rgba(255,255,255,${0.6 * (1 - k)})`;
        ctx.lineWidth = 0.12;
        ctx.beginPath();
        ctx.arc(f.x, f.y, 0.6 + k * 3.4, 0, Math.PI * 2);
        ctx.stroke();
      }
    }
    // aiming aids
    const m = match;
    if (m.pens && m.pens.phase === 'aim' && m.pens.humanTaker) {
      const gx = m.goalX(m.pens.kickSide);
      const ay = m.pens.aim || 0;
      ctx.save();
      ctx.strokeStyle = 'rgba(190,255,74,0.9)';
      ctx.lineWidth = 0.18;
      ctx.beginPath();
      ctx.arc(gx, ay, 0.85 + Math.sin(m.t * 9) * 0.12, 0, Math.PI * 2);
      ctx.stroke();
      ctx.setLineDash([0.5, 0.55]);
      ctx.strokeStyle = 'rgba(255,255,255,0.5)';
      ctx.beginPath();
      ctx.moveTo(m.ball.x, m.ball.y);
      ctx.lineTo(gx, ay);
      ctx.stroke();
      ctx.restore();
    }
    if (m.active && m.humanSide && m.ball.owner === m.active && m.input && m.input.shoot) {
      const held = clampN((m.t - m.holdStart) / 0.9, 0, 1);
      ctx.save();
      ctx.strokeStyle = 'rgba(0,0,0,0.35)';
      ctx.lineWidth = 0.24;
      ctx.beginPath();
      ctx.arc(m.active.x, m.active.y, 1.25, 0, Math.PI * 2);
      ctx.stroke();
      ctx.strokeStyle = held > 0.82 ? '#ff5a3c' : '#befe4a';
      ctx.beginPath();
      ctx.arc(m.active.x, m.active.y, 1.25, -Math.PI / 2, -Math.PI / 2 + Math.PI * 2 * held);
      ctx.stroke();
      ctx.restore();
    }
  }

  drawOverlayText(match, view) {
    const ctx = this.ctx;
    this.tGlobal = match.t;
    if (view.speedLabel) {
      ctx.save();
      ctx.font = '700 11px ui-monospace, monospace';
      ctx.fillStyle = 'rgba(255,255,255,0.35)';
      ctx.textAlign = 'right';
      ctx.fillText(view.speedLabel, this.cssW - 12, this.cssH - 10);
      ctx.restore();
    }
    const b = match.banner;
    if (!b) return;
    const remain = b.until - match.t;
    if (remain <= 0) return;
    const k = clampN(remain / 0.35, 0, 1) * clampN((b.until - b.from || 1) / 1, 0, 1);
    const w = this.cssW;
    const h = this.cssH;
    const cy = h * 0.42;
    ctx.save();
    ctx.globalAlpha = Math.min(1, remain * 3);
    const grad = ctx.createLinearGradient(0, cy - 70, 0, cy + 70);
    grad.addColorStop(0, 'rgba(4,8,14,0)');
    grad.addColorStop(0.5, 'rgba(4,8,14,0.86)');
    grad.addColorStop(1, 'rgba(4,8,14,0)');
    ctx.fillStyle = grad;
    ctx.fillRect(0, cy - 70, w, 140);
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    const big = Math.min(74, w * 0.085);
    ctx.font = `900 ${big}px "Segoe UI", system-ui, sans-serif`;
    const isGoal = /GOAL|SCORED|WIN|PENALTY|SHOOTOUT|THROUGH/i.test(b.text);
    const team = b.side ? match.teams[b.side].team : null;
    ctx.fillStyle = isGoal ? team ? team.trim : '#befe4a' : '#ffffff';
    ctx.strokeStyle = 'rgba(0,0,0,0.55)';
    ctx.lineWidth = big * 0.09;
    ctx.strokeText(b.text, w / 2, cy - big * 0.1);
    ctx.fillText(b.text, w / 2, cy - big * 0.1);
    const textW = ctx.measureText(b.text).width;
    if (b.sub) {
      ctx.font = `700 ${Math.min(20, w * 0.021)}px "Segoe UI", system-ui, sans-serif`;
      ctx.fillStyle = 'rgba(255,255,255,0.86)';
      ctx.fillText(b.sub, w / 2, cy + big * 0.52);
    }
    if (team) {
      // broadcast-graphic style: the scoring nation on the left, the other one on the right
      const other = match.teams[b.side === 'home' ? 'away' : 'home'].team;
      const fw = Math.max(30, Math.min(big * 0.62, w * 0.09));
      const fh = fw / 1.5;
      const y = cy - big * 0.1 - fh / 2;
      const pad = Math.max(10, w * 0.014);
      drawFlag(ctx, team.flag, w / 2 - textW / 2 - pad - fw, y, fw, fh, { radius: 3, outline: true });
      drawFlag(ctx, other.flag, w / 2 + textW / 2 + pad, y, fw, fh, { radius: 3, outline: true });
    }
    ctx.restore();
  }
}

function drawKit(ctx, p, team, r, col) {
  ctx.fillStyle = p.gk ? '#1fbf6f' : col.jersey;
  ctx.beginPath();
  ctx.arc(0, 0, r, 0, Math.PI * 2);
  ctx.fill();
  const pattern = p.gk ? 'plain' : team.pattern;
  ctx.save();
  ctx.beginPath();
  ctx.arc(0, 0, r, 0, Math.PI * 2);
  ctx.clip();
  if (pattern === 'stripes') {
    ctx.fillStyle = team.trim;
    for (let i = -2; i <= 2; i++) ctx.fillRect(-r, i * 0.42 - 0.1, r * 2, 0.2);
  } else if (pattern === 'hoops') {
    ctx.strokeStyle = team.trim;
    ctx.lineWidth = 0.16;
    for (let i = 1; i < 4; i++) {
      ctx.beginPath();
      ctx.arc(0, 0, (r * i) / 4, 0, Math.PI * 2);
      ctx.stroke();
    }
  } else if (pattern === 'halves') {
    ctx.fillStyle = team.trim;
    ctx.fillRect(-r, -r, r, r * 2);
  } else if (pattern === 'sash') {
    ctx.strokeStyle = team.trim;
    ctx.lineWidth = 0.24;
    ctx.beginPath();
    ctx.moveTo(-r, -r);
    ctx.lineTo(r, r);
    ctx.stroke();
  } else if (pattern === 'checks') {
    ctx.fillStyle = 'rgba(255,255,255,0.92)';
    for (let i = 0; i < 3; i++) for (let j = 0; j < 3; j++) if ((i + j) % 2) ctx.fillRect(-r + (i * r * 2) / 3, -r + (j * r * 2) / 3, (r * 2) / 3, (r * 2) / 3);
  }
  // shoulder highlight so bodies read as round
  const g = ctx.createLinearGradient(-r, -r, r, r);
  g.addColorStop(0, 'rgba(255,255,255,0.22)');
  g.addColorStop(0.6, 'rgba(255,255,255,0)');
  g.addColorStop(1, 'rgba(0,0,0,0.18)');
  ctx.fillStyle = g;
  ctx.fillRect(-r, -r, r * 2, r * 2);
  ctx.restore();
  ctx.strokeStyle = 'rgba(0,0,0,0.45)';
  ctx.lineWidth = 0.07;
  ctx.beginPath();
  ctx.arc(0, 0, r, 0, Math.PI * 2);
  ctx.stroke();
}

const clampN = (v, a, b) => (v < a ? a : v > b ? b : v);
