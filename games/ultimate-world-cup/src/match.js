// =============================================================================
//  ULTIMATE WORLD CUP - match engine
//  Pure logic: no DOM, no canvas, no browser APIs. It runs identically in node
//  (see ../tools/smoke.mjs) and in the browser, so the whole 90 minutes is testable.
//
//  Coordinates: metres, origin on the centre spot. Home defends -x and attacks +x.
// =============================================================================
import { squadFor, teamStrength, STADIUMS } from './teams.js';

export const P = {
  L: 105,
  W: 68,
  HL: 52.5,
  HW: 34,
  GOAL_HALF: 3.66,
  GOAL_H: 2.44,
  POST: 0.16,
  GOAL_DEPTH: 2.4,
  BOX_D: 16.5,
  BOX_HW: 20.16,
  SIX_D: 5.5,
  SIX_HW: 9.16,
  SPOT: 11,
  ARCREACH: 9.15,
  CC_R: 9.15, // centre circle
  ARC_R: 9.15, // alias kept for the renderer
  PEN_R: 0.44, // penalty spot radius
  FLAG_R: 0.3, // corner flag
  R: 0.62,
};

export const DIFFICULTY = {
  amateur: { label: 'Amateur', react: 0.36, skill: 0.7, aggr: 0.45, speed: 0.93, press: 0.7, save: 0.82 },
  pro: { label: 'Pro', react: 0.22, skill: 0.86, aggr: 0.65, speed: 0.98, press: 0.9, save: 0.95 },
  world: { label: 'World Class', react: 0.12, skill: 1.0, aggr: 0.85, speed: 1.02, press: 1.05, save: 1.08 },
  legend: { label: 'Legend', react: 0.07, skill: 1.1, aggr: 1.0, speed: 1.06, press: 1.2, save: 1.2 },
};

// 4-3-3 as [forwardFraction of pitch, lateralFraction of half-width] in team frame
export const FORMATION = {
  GK: [0.035, 0],
  RB: [0.27, 0.72],
  CB1: [0.185, 0.29],
  CB2: [0.185, -0.29],
  LB: [0.27, -0.72],
  DM: [0.375, 0.04],
  CM1: [0.465, 0.46],
  CM2: [0.465, -0.46],
  RW: [0.71, 0.82],
  ST: [0.775, 0.02],
  LW: [0.71, -0.82],
};
export const SLOT_LINE = ['GK', 'RB', 'CB1', 'CB2', 'LB', 'DM', 'CM1', 'CM2', 'RW', 'ST', 'LW'];

export const MIN_PER_SEC_BASE = 45; // match minutes per half
const GRAVITY = 13.5;

const clamp = (v, a, b) => (v < a ? a : v > b ? b : v);
const lerp = (a, b, t) => a + (b - a) * t;
const d2 = (a, b) => (a.x - b.x) ** 2 + (a.y - b.y) ** 2;
const dist = (a, b) => Math.hypot(a.x - b.x, a.y - b.y);

export function mulberry32(seed) {
  let a = (seed || 1) >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function nearest(list, pt, penalty) {
  let best = null;
  let bv = Infinity;
  for (const p of list) {
    if (!p || p.off) continue;
    const v = Math.hypot(p.x - pt.x, p.y - pt.y) + (penalty ? penalty(p) : 0);
    if (v < bv) {
      bv = v;
      best = p;
    }
  }
  return best;
}

/** How open a lane is between two points: 0 (blocked) .. 1 (clean). */
function laneClarity(match, from, to, band = 1.6, teamFilter) {
  const dx = to.x - from.x;
  const dy = to.y - from.y;
  const len = Math.hypot(dx, dy) || 1;
  const nx = -dy / len;
  const ny = dx / len;
  let worst = 1;
  for (const p of match.players) {
    if (p.off || p === from || p === to) continue;
    if (teamFilter && p.side === teamFilter) continue;
    const rx = p.x - from.x;
    const ry = p.y - from.y;
    const along = (rx * dx + ry * dy) / (len * len);
    if (along < 0.02 || along > 0.98) continue;
    const across = Math.abs(rx * nx + ry * ny);
    const t = 1 - clamp(across / band, 0, 1);
    worst = Math.min(worst, 1 - t * 0.92);
  }
  return worst;
}

/** Best short option for a carrier. */
function bestMate(match, from, maxD = 30, preferForward = true) {
  const t = match.teams[from.side];
  let best = null;
  let bs = -Infinity;
  for (const m of t.players) {
    if (m === from || m.off || (m.gk && !preferForward)) continue;
    const d = dist(m, from);
    if (d < 1.6 || d > maxD) continue;
    const forward = ((m.x - from.x) * from.dir) / (d || 1);
    const pressure = nearest(match.other(from.side).players, m, () => 0);
    const pd = pressure ? dist(pressure, m) : 99;
    const s = forward * 6 + Math.min(pd, 8) * 1.6 - Math.max(0, d - 14) * 0.35 + laneClarity(match, from, m, 1.7, from.side) * 9;
    if (s > bs) {
      bs = s;
      best = m;
    }
  }
  return best;
}

/** A teammate worth targeting inside the box from a cross or set piece. */
function bestBoxTarget(match, team) {
  const gx = match.goalX(team.side);
  let best = null;
  let bs = -Infinity;
  for (const p of team.players) {
    if (p.off || p.gk) continue;
    const inBox = Math.abs(gx - p.x) < P.BOX_D && Math.abs(p.y) < P.BOX_HW;
    if (!inBox) continue;
    const s = Math.abs(p.fy) * -1 + 4 - Math.abs(gx - p.x) * 0.2 + (p.line === 'ST' ? 3 : 0);
    if (s > bs) {
      bs = s;
      best = p;
    }
  }
  return best;
}

function stepToward(p, dt, mul = 1) {
  const dx = p.targetX - p.x;
  const dy = p.targetY - p.y;
  const d = Math.hypot(dx, dy);
  if (d < 0.35) {
    p.vx = lerp(p.vx, 0, 1 - Math.exp(-9 * dt));
    p.vy = lerp(p.vy, 0, 1 - Math.exp(-9 * dt));
    return;
  }
  const sp = match_speed(p) * mul * clamp(d / 5, 0.25, 1);
  p.vx = lerp(p.vx, (dx / d) * sp, 1 - Math.exp(-8 * dt));
  p.vy = lerp(p.vy, (dy / d) * sp, 1 - Math.exp(-8 * dt));
}

function match_speed(p) {
  const base = 5.4 + (p.pac / 100) * 4.6;
  return base * (0.74 + p.stamina * 0.3) * (p.gk ? 0.78 : 1);
}

export class Match {
  constructor(cfg) {
    this.cfg = cfg;
    this.seed = cfg.seed ?? 2026;
    this.rng = mulberry32(this.seed);
    this.rand = mulberry32((this.seed ^ 0x9e3779b9) >>> 0);
    this.diff = DIFFICULTY[cfg.difficulty] || DIFFICULTY.pro;
    this.halfSeconds = cfg.halfSeconds ?? 90;
    this.knockout = !!cfg.knockout;
    this.humanSide = 'humanTeam' in cfg ? cfg.humanTeam : 'home';
    this.onMatchEnd = null;
    this.userKeeperPens = !!cfg.shootoutUserKeeper;
    this.venue =
      (cfg.venue && typeof cfg.venue === 'object' && cfg.venue.name && cfg.venue.turf) ? cfg.venue
      : STADIUMS.find((v) => v.id === cfg.venue) || STADIUMS[Math.abs(this.seed || 0) % STADIUMS.length];

    this.teams = { home: this.buildTeam('home', cfg.home, 1), away: this.buildTeam('away', cfg.away, -1) };
    this.resolveKitClash();
    this.ball = { x: 0, y: 0, z: 0, vx: 0, vy: 0, vz: 0, owner: null, lastTouch: null, lastTouchSide: null, recapture: 0, curve: 0, rot: 0, trail: [] };
    this.stats = { home: emptyStats(), away: emptyStats(), total: 0 };
    this.events = [];
    this.commentary = [];
    this.fx = [];
    this.score = { home: 0, away: 0 };
    this.scorers = [];
    this.half = 1;
    this.minute = 0;
    this.stoppage = 0;
    this.state = 'intro';
    this.stateTimer = 1.4;
    this.restart = null;
    this.active = null;
    this.t = 0;
    this.shake = 0;
    this.flash = 0;
    this.banner = null;
    this.pens = null;
    this.result = null;
    this.lastGoal = null;
    this.lastScorer = null;
    this.lastOwnGoal = false;
    this.kickoffTo = 'home';
    this.aiClock = 0;
    this.assignCooldown = 0;
    this.holdStart = -1;
    this.paused = false;
    this.netRipple = 0;
    this.celebrationSide = null;
    this.pendingHalfEnd = false;
    this.wall = [];
    this.assignTimer = 0;
    this.looseTimer = 0;
    this.input = { mx: 0, my: 0, sprint: false, shoot: false, pass: false, through: false, tackle: false, sw: false, press: false };
    for (const side of ['home', 'away']) this.teams[side].pressTimer = 0;
    this.resetKickoff('home', true);
    this.state = 'intro';
    this.stateTimer = 1.4;
  }

  /**
   * Two nations can draw near-identical shirts (Belgium and Iran both read dark at a distance), so the
   * away side is quietly re-kitted into whatever contrasts with the home jersey. Officials' job, automated.
   */
  resolveKitClash() {
    const rgb = (c) => {
      const m = /^#?([\da-f]{2})([\da-f]{2})([\da-f]{2})$/i.exec(String(c || '').trim());
      return m ? [1, 2, 3].map((i) => parseInt(m[i], 16) / 255) : [0.5, 0.5, 0.5];
    };
    // weighted euclidean distance: 0 is identical, ~0.6 is unmistakable at 12 px
    const apart = (x, y) => {
      const [r1, g1, b1] = rgb(x);
      const [r2, g2, b2] = rgb(y);
      return Math.sqrt(0.3 * (r1 - r2) ** 2 + 0.59 * (g1 - g2) ** 2 + 0.11 * (b1 - b2) ** 2);
    };
    const lum = (c) => {
      const [r, g, b] = rgb(c);
      return 0.2126 * r + 0.7152 * g + 0.0722 * b;
    };
    const a = this.teams.home.team.colors;
    const b = this.teams.away.team.colors;
    if (apart(a.jersey, b.jersey) >= 0.22) return;
    const candidates = ['#f4f7ff', '#12161f', this.teams.away.team.trim, b.socks, '#f2c14b'].filter(Boolean);
    let best = candidates[0];
    let bestD = -1;
    for (const c of candidates) {
      const d = Math.max(apart(a.jersey, c), apart(a.jersey, c) + Math.abs(lum(c) - lum(a.jersey)) * 0.5);
      if (d > bestD) {
        bestD = d;
        best = c;
      }
    }
    const dark = lum(best) > 0.5 ? '#12161f' : '#f4f7ff';
    this.kitClash = true;
    this.teams.away.kit = { jersey: best, shorts: dark, socks: dark, trim: dark };
  }

  buildTeam(side, team, dir) {
    const squad = squadFor(team, String(this.seed));
    const players = squad.slice(0, 11).map((p, i) => {
      const line = SLOT_LINE[i];
      const [fx, fy] = FORMATION[line];
      return {
        id: `${side[0]}${i}`,
        side,
        idx: i,
        slot: p.slot,
        line,
        num: p.num,
        first: p.first,
        name: p.name,
        label: `${p.num} ${p.name}`,
        ovr: p.ovr,
        pac: p.pac,
        shoot: p.shoot,
        pass: p.pass,
        def: p.def,
        gk: p.slot === 'GK',
        fx,
        fy,
        x: 0,
        y: 0,
        vx: 0,
        vy: 0,
        facing: dir > 0 ? 0 : Math.PI,
        stamina: 1,
        card: null,
        off: false,
        role: 'shape',
        targetX: 0,
        targetY: 0,
        sliding: 0,
        cooldown: 0,
        kickLock: 0,
        react: 0,
        contactCool: 0,
        tackleCool: 0,
        celebrate: 0,
        anim: this.rand() * 6,
        dive: 0,
        diveDir: 0,
        holdTimer: 0,
        shooting: false,
        tell: 0,
      };
    });
    return { side, dir, team, players, bench: squad.slice(11), strength: teamStrength(team), attackBias: 0, deepBlock: 0, pressTimer: 0, chaser: null, presser: null };
  }

  get players() {
    return this.teams.home.players.concat(this.teams.away.players);
  }

  other(side) {
    return this.teams[side === 'home' ? 'away' : 'home'];
  }

  goalX(side) {
    return side === 'home' ? P.HL : -P.HL; // goal this side attacks
  }
  ownGoalX(side) {
    return side === 'home' ? -P.HL : P.HL; // goal this side defends
  }
  defendsGoal(side, x) {
    return Math.sign(x || 1) === -this.teams[side].dir;
  }
  inBoxAt(side, x, y) {
    return Math.abs(this.ownGoalX(side) - x) <= P.BOX_D && Math.abs(y) <= P.BOX_HW;
  }

  // ===========================================================================
  //  tick
  // ===========================================================================
  update(dt, input) {
    if (this.paused) return;
    if (input) this.readInput(input);
    this.t += dt;
    this.shake = Math.max(0, this.shake - dt * 2.4);
    this.flash = Math.max(0, this.flash - dt * 1.6);
    this.netRipple = Math.max(0, this.netRipple - dt * 1.4);
    if (this.banner && this.t > this.banner.until) this.banner = null;
    for (const f of this.fx) f.t += dt;
    if (this.fx.length > 260) this.fx.splice(0, this.fx.length - 260);
    this.fx = this.fx.filter((f) => f.t < f.life && f.t > -3);

    switch (this.state) {
      case 'intro':
        this.walkOut(dt);
        return;
      case 'kickoff':
        this.shapeUp(dt);
        this.stateTimer -= dt;
        if (this.stateTimer <= 0) this.beginKickoff();
        return;
      case 'celebrate':
        this.celebrate(dt);
        return;
      case 'restart':
        this.updateRestart(dt);
        return;
      case 'shootout':
        this.updateShootout(dt);
        return;
      case 'halftime':
      case 'results':
        this.soft(dt);
        return;
      default:
        break;
    }

    this.tickClock(dt);
    if (this.state !== 'play') return;
    this.assignRoles();
    if (this.humanSide) this.handleHuman(dt);
    this.aiUpdate(dt);
    this.movePlayers(dt);
    this.updateBall(dt);
    if (this.state === 'play' && this.ball.owner) this.stats[this.ball.owner.side].possession += dt;
  }

  readInput(inp) {
    const i = this.input;
    // continuous
    i.mx = inp.mx;
    i.my = inp.my;
    i.sprint = !!inp.sprint;
    i.shoot = !!inp.shoot;
    // edge-triggered: pass through one-shot flags
    for (const k of ['pass', 'through', 'tackle', 'sw', 'press']) if (inp[k]) i[k] = true;
  }

  consumeEdges() {
    const i = this.input;
    i.pass = false;
    i.through = false;
    i.tackle = false;
    i.sw = false;
    i.press = false;
  }

  // ===========================================================================
  //  flow
  // ===========================================================================
  push(type, data = {}) {
    this.events.push({ type, minute: Math.round(this.minute), ...data });
    if (data.text) {
      this.commentary.unshift({ text: data.text, minute: Math.round(this.minute), key: `${type}${this.commentary.length}` });
      if (this.commentary.length > 30) this.commentary.length = 30;
    }
  }

  walkOut(dt) {
    this.stateTimer -= dt;
    this.soft(dt * 0.5);
    if (this.stateTimer <= 0) {
      this.push('whistle', { why: 'kickoff', text: `Kick off - ${this.teams.home.team.name} v ${this.teams.away.team.name}` });
      this.state = 'kickoff';
      this.stateTimer = 0.55;
    }
  }

  /** everybody walks to their shape while the match is not live */
  shapeUp(dt) {
    for (const p of this.players) {
      if (p.off) continue;
      this.placeShape(p, false);
      if (p !== this.active) stepToward(p, dt, 0.85);
      else {
        p.vx *= 0.7;
        p.vy *= 0.7;
      }
      p.x = clamp(p.x + p.vx * dt, -P.HL - 2, P.HL + 2);
      p.y = clamp(p.y + p.vy * dt, -P.HW - 1.6, P.HW + 1.6);
      p.anim += dt * 2;
    }
  }

  beginKickoff() {
    const taker = this.teams[this.kickoffTo].players[9];
    const b = this.ball;
    b.x = taker.x + this.teams[this.kickoffTo].dir * 0.8;
    b.y = taker.y;
    b.z = 0;
    b.vx = b.vy = b.vz = 0;
    b.owner = taker;
    b.lastTouch = taker;
    b.lastTouchSide = this.kickoffTo;
    b.trail.length = 0;
    this.state = 'play';
    if (this.humanSide === this.kickoffTo) this.active = taker;
  }

  resetKickoff(toSide, initial = false) {
    this.kickoffTo = toSide;
    this.restart = null;
    this.state = 'kickoff';
    this.stateTimer = initial ? 0.8 : 1.0;
    const b = this.ball;
    b.x = 0;
    b.y = 0;
    b.z = 0;
    b.vx = b.vy = b.vz = 0;
    b.owner = null;
    b.recapture = 0.6;
    b.trail.length = 0;
    for (const t of [this.teams.home, this.teams.away]) {
      for (const p of t.players) {
        p.vx = p.vy = 0;
        p.sliding = 0;
        p.cooldown = 0;
        p.celebrate = 0;
        p.dive = 0;
        p.holdTimer = 0;
        p.shooting = false;
        this.placeShape(p, true);
        if (!p.gk) {
          const lim = t.dir > 0 ? -0.8 : 0.8;
          p.x = t.dir > 0 ? Math.min(p.x, lim) : Math.max(p.x, lim);
        }
      }
    }
    const striker = this.teams[toSide].players[9];
    striker.x = toSide === 'home' ? -1.35 : 1.35;
    striker.y = 0.5;
    striker.facing = this.teams[toSide].dir > 0 ? 0 : Math.PI;
    this.active = this.humanSide === toSide ? striker : null;
    this.assignRoles(true);
  }

  /** Formation target for a player given where the ball is. */
  placeShape(p, hard = false) {
    const t = this.teams[p.side];
    const b = this.ball;
    const dir = t.dir;
    const ballAdv = clamp(b.x * dir, -P.HL, P.HL); // 105 range, forward-ness of the ball
    const adv = (ballAdv / P.HL) * 15 + t.attackBias * 4;
    const lineWeight = p.gk ? 0 : p.line.startsWith('CB') || p.line === 'GK' ? 0.42 : p.line === 'DM' ? 0.55 : 0.8;
    const baseX = -P.HL + p.fx * P.L;
    let x = baseX + adv * lineWeight;
    let y = p.fy * (P.HW - 3.6) + clamp(b.y, -P.HW, P.HW) * 0.18 * (p.gk ? 0.2 : 1);
    if (p.gk) {
      x = this.ownGoalX(p.side) + dir * 1.7;
      y = clamp(b.y * 0.14, -2.6, 2.6);
      if (b.owner && b.owner.side !== p.side && Math.abs(b.x - this.ownGoalX(p.side)) < P.BOX_D) {
        // narrow the angle
        const bx = b.x;
        const gx = this.ownGoalX(p.side);
        const t2 = clamp(1 - Math.abs(bx - gx) / 34, 0, 0.72);
        x = gx + dir * (1.7 + t2 * 3.4);
        y = clamp(b.y * 0.5, -P.GOAL_HALF - 0.6, P.GOAL_HALF + 0.6);
      }
    }
    // deep block when losing late
    if (t.deepBlock > 0 && !p.gk && ballAdv < 0) x -= dir * 7 * t.deepBlock;
    p.targetX = clamp(x, -P.HL - 1, P.HL + 1);
    p.targetY = clamp(y, -P.HW + 1, P.HW - 1);
    if (hard) {
      p.x = clamp(p.targetX, -P.HL - 1, P.HL - 1);
      p.y = p.targetY;
    }
  }

  celebrate(dt) {
    this.stateTimer -= dt;
    this.animCelebrate(dt);
    if (this.stateTimer <= 0) {
      if (this.pendingHalfEnd) return this.endHalf(true);
      this.resetKickoff(this.kickoffTo);
    }
  }

  /** celebration animation only - used by the shootout and the goal sequence */
  animCelebrate(dt) {
    for (const p of this.players) {
      if (p.celebrate > 0) {
        p.celebrate -= dt;
        const wob = Math.sin(this.t * 6 + p.idx * 1.7);
        p.vx = wob * 1.6;
        p.vy = Math.cos(this.t * 5 + p.idx) * 1.6;
        p.x = clamp(p.x + p.vx * dt, -P.HL + 1, P.HL - 1);
        p.y = clamp(p.y + p.vy * dt, -P.HW + 1, P.HW - 1);
      }
    }
    const b = this.ball;
    b.vx *= 1 - dt * 1.6;
    b.vy *= 1 - dt * 1.6;
    b.x += b.vx * dt;
    b.y += b.vy * dt;
  }

  /** teams mill about between halves */
  soft(dt) {
    for (const t of [this.teams.home, this.teams.away]) {
      for (const p of t.players) {
        if (p.gk) continue;
        p.targetX = clamp(p.x + Math.sin(this.t * 0.7 + p.idx) * 3, -P.HL + 6, P.HL - 6);
        p.targetY = clamp(p.y + Math.cos(this.t * 0.6 + p.idx) * 2, -P.HW + 4, P.HW - 4);
        stepToward(p, dt, 0.32);
        p.x += p.vx * dt;
        p.y += p.vy * dt;
      }
    }
  }

  tickClock(dt) {
    const span = this.half >= 3 ? 15 : MIN_PER_SEC_BASE;
    const before = this.minute;
    this.minute += (dt / this.halfSeconds) * span;
    if (before <= span && this.minute > span && !this.stoppageAnnounced) {
      this.stoppage = 1 + Math.floor(this.rand() * 3) + Math.round(this.stoppageExtra || 0);
      this.stoppageAnnounced = true;
      this.push('stoppage', { text: `Stoppage time: ${this.stoppage} min${this.stoppage > 1 ? 's' : ''} added` });
    }
    if (this.minute >= span + Math.max(1, this.stoppage)) {
      this.endHalf();
      return;
    }
    // possession tally + fatigue
    this.stats.total += dt;
    for (const t of [this.teams.home, this.teams.away]) {
      for (const p of t.players) {
        const sp = Math.hypot(p.vx, p.vy);
        const sprinting = p === this.active && this.input.sprint;
        const drain = sprinting ? 0.055 : sp > 7 ? 0.028 : sp > 3 ? 0.011 : -0.02;
        p.stamina = clamp(p.stamina - drain * dt * (this.half >= 3 ? 1.25 : 1) - (p.sliding > 0 ? 0.05 * dt : 0), 0.28, 1);
      }
      t.attackBias *= 1 - dt * 0.9;
      t.pressTimer = Math.max(0, t.pressTimer - dt);
    }
    const lead = this.score.home - this.score.away;
    const losing = lead > 0 ? 'away' : lead < 0 ? 'home' : null;
    const late = this.half >= 2 && this.minute > span - 12;
    for (const side of ['home', 'away']) {
      const t = this.teams[side];
      t.deepBlock = this.half >= 2 && this.knockout && lead !== 0 && losing === side ? 0 : this.knockout && late && losing !== side && lead !== 0 ? 0.5 : 0;
      if (this.knockout && late && losing === side) t.attackBias = Math.min(2.2, t.attackBias + dt * 0.6);
    }
  }

  /** the side the local player controls ('home' | 'away' | null for a CPU match) */
  get humanTeam() {
    return this.humanSide;
  }

  clockLabel() {
    const off = this.half === 3 ? 90 : this.half === 4 ? 105 : 0;
    const m = clamp(Math.floor(this.minute), 0, this.half >= 3 ? 15 : MIN_PER_SEC_BASE);
    if (this.state === 'shootout' || this.pens) return 'PENS';
    return `${off + m}'`;
  }

  endHalf(afterCelebration = false) {
    this.push('whistle', { why: `half ${this.half} ended` });
    const finish = () => {
      if (this.knockout && this.score.home === this.score.away) {
        this.startShootout();
        return true;
      }
      this.finishMatch();
      return true;
    };
    if (this.half === 1) {
      this.state = 'halftime';
      this.stateTimer = Infinity;
      this.push('halftime', { score: { ...this.score }, stats: this.stats });
      return;
    }
    if (this.half === 2) {
      if (this.knockout && this.score.home === this.score.away) {
        this.half = 3;
        this.minute = 0;
        this.stoppageAnnounced = false;
        this.stoppage = 0;
        this.banner = { text: 'EXTRA TIME', sub: '30 minutes to settle it', until: this.t + 2.4 };
        this.resetKickoff('away');
        this.push('extratime', { text: 'Level after 90 — extra time!' });
        return;
      }
      return finish();
    }
    if (this.half === 3) {
      this.half = 4;
      this.minute = 0;
      this.stoppageAnnounced = false;
      this.stoppage = 1;
      this.push('whistle', { why: 'halftime of extra time' });
      if (this.knockout && this.score.home !== this.score.away) return finish();
      this.resetKickoff('home');
      return;
    }
    return finish();
  }

  resume() {
    if (this.state === 'halftime') {
      this.half = 2;
      this.minute = 0;
      this.stoppage = 0;
      this.stoppageAnnounced = false;
      for (const t of [this.teams.home, this.teams.away]) for (const p of t.players) p.stamina = clamp(p.stamina + 0.34, 0, 1);
      this.banner = { text: 'SECOND HALF', sub: 'Teams swap ends', until: this.t + 1.6 };
      this.resetKickoff(this.kickoffTo === 'home' ? 'away' : 'home');
      return;
    }
    if (this.state === 'kickoff') this.stateTimer = 0.4;
  }

  finishMatch() {
    this.state = 'results';
    this.stateTimer = Infinity;
    const hs = this.score.home;
    const as = this.score.away;
    const winner = hs > as ? 'home' : as > hs ? 'away' : 'draw';
    this.push('whistle', { why: 'full time', text: `Full time: ${this.teams.home.team.name} ${hs} - ${as} ${this.teams.away.team.name}` });
    this.result = {
      home: hs,
      away: as,
      winner,
      scorers: this.scorers.slice(),
      stats: this.stats,
      pens: this.pens ? { home: this.pens.home.score, away: this.pens.away.score } : null,
      pom: this.pickPom(),
      attendance: this.cfg.attendance || null,
    };
    this.push('results', { result: this.result });
  }

  pickPom() {
    const rank = new Map();
    for (const s of this.scorers) rank.set(s.scorer, (rank.get(s.scorer) || 0) + 2);
    let best = null;
    let bv = -Infinity;
    for (const p of this.players) {
      const v = (rank.get(`${p.first} ${p.name}`) || 0) + p.ovr * 0.05 + (p.gk ? this.stats[p.side].saves * 0.5 : 0) + this.rand() * 1.2;
      if (v > bv) {
        bv = v;
        best = p;
      }
    }
    if (!best) return null;
    return { name: `${best.first} ${best.name}`, num: best.num, side: best.side, team: this.teams[best.side].team.name };
  }

  // ===========================================================================
  //  role assignment
  // ===========================================================================
  assignRoles(force = false) {
    if (!force) {
      this.assignTimer -= 1;
      if (this.assignTimer > 0) return;
      this.assignTimer = 8; // ~every 8 frames is plenty for role picking
    }
    const b = this.ball;
    const ownerSide = b.owner ? b.owner.side : null;
    for (const t of [this.teams.home, this.teams.away]) {
      const field = t.players.filter((p) => !p.off && !p.gk);
      if (!field.length) continue;
      const sorted = [...field].sort((a, c) => d2(a, b) - d2(c, b));
      t.chaser = sorted[0];
      t.presser = sorted[1] || null;
      // never let the human's man press too
      if (this.humanSide === t.side && this.active === t.chaser && sorted[1]) {
        t.presser = t.chaser;
        t.chaser = sorted[1];
      }
      const humanIsCarrier = ownerSide === t.side && b.owner === this.active && this.humanSide === t.side;
      if (humanIsCarrier && this.humanSide === t.side) {
        t.chaser = t.presser; // nearest AI teammate supports
      }
      for (const p of field) p.role = 'shape';
      if (t.chaser) t.chaser.role = ownerSide === t.side ? 'cover' : 'chase';
      if (t.presser && t.presser !== t.chaser) t.presser.role = ownerSide === t.side ? 'cover' : 'press';
      if (ownerSide === t.side) for (const p of field) p.role = p.role === 'shape' ? 'support' : 'shape';
      for (const p of t.players) if (p.gk) p.role = 'keeper';
    }
  }

  // ===========================================================================
  //  human control
  // ===========================================================================
  handleHuman(dt) {
    const side = this.humanSide;
    const t = this.teams[side];
    const inp = this.input;
    let p = this.active;
    if (!p || p.off || p.side !== side) {
      p = this.autoPick(side);
      this.active = p;
    }
    if (!p) return;
    if (this.state === 'play' && inp.sw) {
      const next = this.nextMan(side, p);
      if (next) this.active = next;
    }
    if (this.state === 'play' && inp.press) {
      t.pressTimer = 2.6;
      t.attackBias = Math.min(2, t.attackBias + 0.5);
    }
    p = this.active;
    if (p.off) {
      this.active = this.autoPick(side);
      return;
    }

    const mag = Math.hypot(inp.mx, inp.my);
    const nx = mag > 0.18 ? inp.mx / mag : 0;
    const ny = mag > 0.18 ? inp.my / mag : 0;
    const hasBall = this.ball.owner === p;
    const restartTaker = this.restart && this.restart.taker === p;

    if (p.sliding <= 0) {
      if (mag > 0.18) {
        p.facing = Math.atan2(ny, nx);
        const maxS = match_speed(p) * (inp.sprint ? 1.22 : 1) * (hasBall ? 0.94 : 1);
        p.vx = lerp(p.vx, nx * maxS, 1 - Math.exp(-12 * dt));
        p.vy = lerp(p.vy, ny * maxS, 1 - Math.exp(-12 * dt));
      } else {
        p.vx = lerp(p.vx, 0, 1 - Math.exp(-9 * dt));
        p.vy = lerp(p.vy, 0, 1 - Math.exp(-9 * dt));
      }
    }

    // ---- attacking inputs
    if (this.state === 'restart' && restartTaker) {
      // A set piece gets its own vocabulary: Space takes it (a strike, from a dangerous
      // free kick), J plays it short, K whips it into the box.
      const r = this.restart;
      if (inp.shoot) this.takeHumanRestart(p, r.dangerous ? 'shoot' : 'short');
      else if (inp.pass) this.takeHumanRestart(p, 'pass');
      else if (inp.through) this.takeHumanRestart(p, 'through');
      if (!inp.shoot) p.shooting = false;
      return;
    }
    if (hasBall || restartTaker) {
      if (inp.shoot && !p.shooting) {
        p.shooting = true;
        this.holdStart = this.t;
      }
      if (inp.shoot && p.shooting) {
        const held = clamp(this.t - this.holdStart, 0, 0.9);
        if (held > 0.9) {
          p.shooting = false;
          this.humanShoot(p, 0.9, nx, ny);
        }
      }
      if (!inp.shoot && p.shooting) {
        p.shooting = false;
        const held = clamp(this.t - this.holdStart, 0.06, 0.9);
        this.humanShoot(p, held, nx, ny);
      }
      if (inp.pass) {
        if (this.state === 'restart') this.takeHumanRestart(p, 'pass');
        else this.humanPass(p, nx, ny, false);
      }
      if (inp.through) {
        if (this.state === 'restart') this.takeHumanRestart(p, 'through');
        else this.humanPass(p, nx, ny, true);
      }
      if (inp.tackle && !hasBall) this.slideTackle(p, nx, ny, true);
      return;
    }

    // ---- off-the-ball inputs
    if (inp.pass) {
      // first touch / call for it
      if (dist(p, this.ball) < 2.4 && this.ball.z < 1.5 && !this.ball.owner) this.grab(p);
      else {
        t.attackBias = Math.min(2.4, t.attackBias + 0.7);
        p.support = 1.2;
      }
    }
    if (inp.tackle) this.slideTackle(p, nx, ny, true);
    if (inp.shoot && !p.shooting) {
      // no ball: a lunge to intercept
      p.shooting = true;
      this.blockAttempt(p);
    }
    if (!inp.shoot) p.shooting = false;
    // auto switch to the nearest man when the opponent carries it deep
    if (this.ball.owner && this.ball.owner.side !== side) {
      const n = nearest(t.players.filter((q) => !q.gk && !q.off), this.ball);
      if (n && n !== p && dist(p, this.ball) > 13 && dist(n, this.ball) < dist(p, this.ball) - 3) this.active = n;
    }
  }

  autoPick(side) {
    const t = this.teams[side];
    const field = t.players.filter((p) => !p.off && !p.gk);
    if (!field.length) return null;
    if (this.ball.owner && this.ball.owner.side === side) return this.ball.owner;
    if (t.chaser && field.includes(t.chaser)) return t.chaser;
    return nearest(field, this.ball);
  }

  nextMan(side, cur) {
    const list = this.teams[side].players
      .filter((p) => !p.off && !p.gk && p !== cur)
      .sort((a, b) => d2(a, this.ball) - d2(b, this.ball));
    return list.find((p) => p.sliding <= 0) || list[0] || null;
  }

  humanShoot(p, held, nx, ny) {
    if (p.kickLock > 0) return;
    const side = p.side;
    const gx = this.goalX(side);
    const toGoal = Math.hypot(gx - p.x, p.y);
    const power = lerp(18, 31, clamp(held / 0.85, 0, 1));
    const goalAng = Math.atan2(0 - p.y, gx - p.x);
    const aimAng = Math.atan2(ny, nx);
    let diff = Math.atan2(Math.sin(aimAng - goalAng), Math.cos(aimAng - goalAng));
    diff = clamp(diff, -0.85, 0.85);
    const aimY = clamp(Math.tan(diff) * Math.abs(gx - p.x) * 0.55, -5.2, 5.2);
    const skill = p.shoot / 100;
    const pressure = nearest(this.other(side).players, p) ;
    const underPressure = pressure && dist(pressure, p) < 3 ? 1 : 0;
    const err = (1 - skill) * (4.2 + toGoal * 0.1) + (held > 0.75 ? 0.9 : 0) + underPressure * 1.4;
    const y = clamp(aimY + (this.rand() - 0.5) * err, -7, 7);
    const loft = held > 0.62 ? 0.46 : 0.12;
    const curve = clamp(ny * 0.02, -0.04, 0.04);
    this.kick(p, { x: gx + (this.rand() - 0.5) * 1.5, y, loft, power, type: 'shot', curve });
    this.stopClockFor(0);
    this.push('shot', { side, text: `${p.first} ${p.name} shoots!`, toGoal });
  }

  stopClockFor() {}

  humanPass(p, nx, ny, through) {
    if (p.kickLock > 0) return;
    const mate = this.pickPassTarget(p, nx, ny, through);
    if (!mate) {
      this.kick(p, { x: p.x + this.teams[p.side].dir * 20, y: clamp(p.y + ny * 14, -P.HW + 2, P.HW - 2), loft: through ? 0.5 : 0.16, power: 20, type: 'clear' });
      this.push('clear', { side: p.side, text: `${p.name} plays it on` });
      return;
    }
    this.passBall(p, mate, through);
  }

  pickPassTarget(p, nx, ny, through) {
    const t = this.teams[p.side];
    let best = null;
    let bs = -Infinity;
    for (const m of t.players) {
      if (m === p || m.off || m.gk) continue;
      const dx = m.x - p.x;
      const dy = m.y - p.y;
      const d = Math.hypot(dx, dy);
      if (d < 1.8 || d > (through ? 48 : 36)) continue;
      const ux = dx / d;
      const uy = dy / d;
      const aligned = nx || ny ? ux * nx + uy * ny : 1;
      if (aligned < (through ? 0.15 : -0.4)) continue;
      const forward = ux * this.teams[p.side].dir;
      const lane = laneClarity(this, p, m, through ? 2.1 : 1.5, p.side);
      const pressure = nearest(this.other(p.side).players, m);
      const open = pressure ? clamp(dist(pressure, m) / 6, 0, 1) : 1;
      let s = aligned * 3 + forward * 2 + lane * 4 + open * 4 - Math.max(0, d - 20) * 0.12;
      if (through) s += forward * 3 + (m.pac / 100) * 2 - Math.max(0, d - 30) * 0.1;
      if (s > bs) {
        bs = s;
        best = m;
      }
    }
    return best;
  }

  blockAttempt(p) {
    if (p.cooldown > 0) return;
    const b = this.ball;
    const d = dist(p, b);
    if (b.owner && b.owner.side !== p.side && dist(p, b.owner) < 2.1) {
      const chance = 0.3 + (p.def / 100) * 0.4;
      if (this.rand() < chance) {
        this.push('block', { side: p.side, text: `Blocked! ${p.name} gets a foot in` });
        this.grab(p, true);
        this.stats[p.side].blocks += 1;
      } else {
        p.cooldown = 0.5;
      }
      return;
    }
    if (!b.owner && d < 2.2 && b.z < 1.4) {
      p.cooldown = 0.3;
      this.grab(p);
    }
  }

  // ===========================================================================
  //  AI (both teams for whatever the human is not playing)
  // ===========================================================================
  aiUpdate(dt) {
    for (const t of [this.teams.home, this.teams.away]) {
      const humanTeam = this.humanSide === t.side;
      for (const p of t.players) {
        if (p.off || p === this.active) {
          if (p === this.active) continue;
        }
        if (p.react > 0) p.react -= dt;
        if (p.gk) {
          this.aiKeeper(p, dt);
          continue;
        }
        if (this.ball.owner === p && !(humanTeam && p === this.active)) {
          this.aiCarrier(p, dt);
          continue;
        }
        this.aiOffBall(p, t, dt, humanTeam);
      }
    }
  }

  aiOffBall(p, t, dt, humanTeam) {
    const b = this.ball;
    const opp = this.other(p.side);
    const ballOwner = b.owner;
    // the intended receiver goes to get it
    if (!ballOwner && b.passTo === p && p.react <= 0) {
      p.targetX = b.x + b.vx * 0.09;
      p.targetY = b.y + b.vy * 0.09;
      stepToward(p, dt, 1.12);
      return;
    }
    this.placeShape(p, false);
    if (p.role === 'chase' || p.role === 'press' || (t.pressTimer > 0 && p === (t.presser || t.chaser))) {
      // hunt the ball with a lead on loose ones
      const lead = ballOwner ? 0 : clamp(Math.hypot(b.vx, b.vy) * 0.24, 0, 3.2);
      const ang = Math.atan2(b.vy, b.vx);
      const tx = b.x + Math.cos(ang) * lead;
      const ty = b.y + Math.sin(ang) * lead;
      p.targetX = clamp(tx, -P.HL - 1, P.HL + 1);
      p.targetY = clamp(ty, -P.HW - 1, P.HW + 1);
      stepToward(p, dt, p.role === 'chase' ? 1.04 : 0.99);
      // tackle when in range
      const carrier = ballOwner;
      const near = carrier ? dist(p, carrier) : dist(p, b);
      if (p.react <= 0 && p.sliding <= 0 && p.cooldown <= 0 && (p.tackleCool || 0) <= 0) {
        if (carrier && near < 2.35 && carrier.side !== p.side) {
          const skill = this.diff.aggr * (0.2 + p.def / 260);
          if (near < 1.9 && this.rand() < skill * 0.5) {
            p.tackleCool = 1.6 + this.rand() * 1.4;
            this.slideTackle(p, carrier.x - p.x, carrier.y - p.y, false);
            p.react = this.diff.react * (1.4 + this.rand());
          } else if (near < 1.5) {
            this.stealAttempt(p, carrier);
            p.react = 0.45;
          }
        } else if (!carrier && near < 1.9 && b.z < 1.2) {
          this.grab(p);
        }
      }
      return;
    }
    if (p.role === 'cover') {
      // sit goal-side of the ball, in the passing lane
      const gx = this.ownGoalX(p.side);
      const dir = this.teams[p.side].dir;
      p.targetX = lerp(b.x, gx, 0.42) + dir * 2;
      p.targetY = lerp(b.y, 0, 0.32) + (p.fy * 5);
      stepToward(p, dt, 0.92);
      return;
    }
    if (p.role === 'support' && ballOwner && ballOwner.side === p.side) {
      // offer a line: push into space ahead of the carrier
      const dir = t.dir;
      p.targetX = clamp(p.fx * P.L - P.HL + dir * 12 + t.attackBias * 4, -P.HL + 2, P.HL - 2);
      p.targetY = clamp(p.fy * (P.HW - 8) + b.y * 0.1, -P.HW + 2, P.HW - 2);
      stepToward(p, dt, 0.95);
      return;
    }
    // mark the closest opponent in this player's channel
    const mark = opp.players.filter((q) => !q.gk && !q.off).sort((a, c) => d2(a, p) - d2(c, p))[0];
    if (mark && !ballOwner && Math.abs(mark.x - p.x) < 16) {
      p.targetX = lerp(p.targetX, mark.x, 0.35);
      p.targetY = lerp(p.targetY, mark.y - Math.sign(mark.y || 1) * 0.4, 0.35);
    }
    if (p.sliding <= 0) stepToward(p, dt, 0.86);
  }

  aiCarrier(p, dt) {
    const t = this.teams[p.side];
    const dir = t.dir;
    const gx = this.goalX(p.side);
    const toGoal = Math.hypot(gx - p.x, p.y);
    const opp = this.other(p.side);
    const guard = nearest(opp.players, p);
    const gd = guard ? dist(guard, p) : 99;
    p.targetX = clamp(p.x + dir * 8, -P.HL, P.HL);
    p.targetY = clamp(p.y - Math.sign(p.y || 1) * -1.5, -P.HW + 2, P.HW - 2);
    if (p.react > 0) {
      this.dribbleSteer(p, dt, gx, gd, guard);
      return;
    }
    // thinking time: set before any decision so every action leaves a beat
    p.react = (0.3 + this.diff.react) * (0.85 + this.rand() * 0.7);

    // 1) shoot when in range with a lane
    const shooting = toGoal < 23.5 - (p.shoot - 72) * 0.1 && Math.abs(p.y) < 22;
    if (shooting) {
      const lane = laneClarity(this, p, { x: gx, y: 0 }, 2.2, p.side);
      const inside = this.inBoxAt(opp.side, p.x, p.y);
      const want = 0.16 + lane * 0.45 + (inside ? 0.24 : 0) - (gd < 1.6 ? 0.22 : 0);
      if (this.rand() < clamp(want, 0.05, 0.95) * this.diff.skill) {
        this.aiShoot(p, toGoal, lane);
        return;
      }
    }
    // 2) pass when pressured, or occasionally to keep the move alive
    if (gd < 3.6 || this.rand() < 0.06) {
      const mate = bestMate(this, p, 28, true);
      if (mate) {
        const lane = laneClarity(this, p, mate, 1.9, p.side);
        const quality = lane * 2 + (gd < 2.2 ? 1.4 : 0) + (((mate.x - p.x) * dir) / 30) * 1.2;
        if (quality > 1.5) {
          const through = this.rand() < 0.3 && mate.x * dir > p.x * dir + 9 && lane > 0.4;
          this.passBall(p, mate, through);
          return;
        }
      }
      if (gd < 1.5 && toGoal < 22 && this.rand() < 0.5) {
        this.aiShoot(p, toGoal, laneClarity(this, p, { x: gx, y: 0 }, 2.2, p.side));
        return;
      }
      if (gd < 1.15) {
        // really in trouble: put it out of danger
        this.kick(p, { x: clamp(p.x + dir * 34, -P.HL, P.HL), y: clamp(p.y + (this.rand() - 0.5) * 26, -P.HW + 3, P.HW - 3), loft: 0.7, power: 22, type: 'clear' });
        return;
      }
    }
    // 3) cross from the byline
    if (Math.abs(p.y) > 20 && Math.abs(gx - p.x) < 22 && this.rand() < 0.55) {
      const target = bestBoxTarget(this, t) || { x: gx - dir * 7, y: -Math.sign(p.y) * 4 };
      this.kick(p, { x: target.x, y: target.y, loft: 0.72, power: 19, type: 'cross' });
      return;
    }
    // 4) through ball to a runner
    if (this.rand() < 0.3 * this.diff.skill) {
      const runner = this.findRunner(p);
      if (runner) {
        this.passBall(p, runner, true);
        return;
      }
    }
    this.dribbleSteer(p, dt, gx, gd, guard);
  }

  findRunner(p) {
    const t = this.teams[p.side];
    const dir = t.dir;
    let best = null;
    let bs = 0;
    for (const m of t.players) {
      if (m === p || m.off || m.gk) continue;
      const ahead = (m.x - p.x) * dir;
      if (ahead < 9 || ahead > 42) continue;
      const space = laneClarity(this, m, { x: this.goalX(p.side), y: m.y * 0.4 }, 2.6, p.side);
      const s = space * (m.pac / 100) * ahead;
      if (s > bs) {
        bs = s;
        best = m;
      }
    }
    return bs > 12 ? best : null;
  }

  dribbleSteer(p, dt, gx, gd, guard) {
    const dir = this.teams[p.side].dir;
    let tx = clamp(gx - dir * 12, -P.HL, P.HL);
    let ty = clamp(p.y * 0.82, -P.HW + 6, P.HW - 6);
    if (guard && gd < 6) {
      // steer away from the challenge, toward space
      const away = Math.atan2(p.y - guard.y, (p.x - guard.x) * 0.5);
      tx = p.x + Math.cos(away) * 4 + dir * 10;
      ty = p.y + Math.sin(away) * 5;
    }
    p.targetX = clamp(tx, -P.HL + 2, P.HL - 2);
    p.targetY = clamp(ty, -P.HW + 2, P.HW - 2);
    stepToward(p, dt, gd < 2.4 ? 1.05 : 0.92);
    // a skilled carrier knocks it past a lunging defender, but only with space to run onto
    if (gd < 2.2 && Math.abs(p.x - gx) > 18 && this.rand() < 0.007) {
      this.kick(p, { x: p.x + dir * 11, y: clamp(p.y + (this.rand() - 0.5) * 5, -P.HW + 2, P.HW - 2), loft: 0.08, power: 13, type: 'dribble', to: p });
    }
  }

  aiShoot(p, toGoal, lane) {
    const gx = this.goalX(p.side);
    const near = this.other(p.side).players;
    const sk = (p.shoot / 100) * this.diff.skill;
    const aimCorner = this.rand() < 0.6;
    let y = aimCorner ? (this.rand() < 0.5 ? -1 : 1) * (1.7 + this.rand() * 2.6) : (this.rand() - 0.5) * 3.4;
    y += (1 - sk * 0.55) * (this.rand() - 0.5) * (3.4 + toGoal * 0.26);
    if (this.rand() > 0.55 + sk * 0.32) y *= 1.25 + this.rand() * 0.75; // dragged wide / over the bar
    const power = lerp(19, 28, clamp(1 - toGoal / 40, 0.15, 1)) * lerp(0.92, 1.02, sk);
    const loft = toGoal < 9 ? 0.55 : this.rand() < 0.2 ? 0.5 : 0.1;
    this.kick(p, { x: gx, y, loft, power, type: 'shot', curve: (this.rand() - 0.5) * 0.03 });
    if (lane > 0.6) this.push('shot', { side: p.side, text: `${p.first} ${p.name} has a crack at goal!` });
    else this.push('shot', { side: p.side, text: `${p.name} forces a try` });
  }

  stealAttempt(p, carrier) {
    if (p.cooldown > 0 || carrier.holdTimer > 0) return;
    p.cooldown = 0.6;
    const chance = clamp(0.2 + (p.def - carrier.pass) / 220 + (this.diff.aggr - 0.6) * 0.12, 0.04, 0.52);
    if (this.rand() < chance) {
      this.grab(p, true);
      this.push('turnover', { side: p.side, text: `${p.name} nicks it!` });
    } else if (this.rand() < 0.13) {
      this.foul(p, carrier, 'trip');
    }
  }

  // ===========================================================================
  //  goalkeeping
  // ===========================================================================
  aiKeeper(k, dt) {
    const t = this.teams[k.side];
    const b = this.ball;
    const gx = this.ownGoalX(k.side);
    const dir = t.dir;
    const incoming = b.owner === k;
    if (incoming) {
      // distribute after holding it
      k.holdTimer -= dt;
      k.vx = k.vy = 0;
      k.targetX = gx + dir * 2;
      k.targetY = lerp(k.y, b.y * 0.2, dt * 2);
      stepToward(k, dt, 0.6);
      if (k.holdTimer <= 0) this.keeperDistribute(k);
      return;
    }
    const towardGoal = b.owner ? false : b.vx * (dir > 0 ? -1 : 1) > 3.5;
    let py = clamp(b.y * 0.55, -4.2, 4.2);
    let px = gx + dir * 1.7;
    if (!b.owner) {
      const vxTo = dir > 0 ? -b.vx : b.vx;
      if (Math.abs(b.x - gx) < 44 && vxTo > 4) {
        const time = (gx - b.x) / b.vx;
        if (time > 0 && time < 4) {
          py = clamp(b.y + b.vy * time, -P.GOAL_HALF - 2.6, P.GOAL_HALF + 2.6);
          const danger = clamp(1 - Math.abs(b.x - gx) / 40, 0, 1);
          px = gx + dir * (1.5 + danger * 1.9 + (1 - clamp(Math.abs(py), 0, 4) / 4) * 0.6);
        }
      }
      // sweep through balls
      if (b.owner === null && Math.abs(b.x - gx) < P.BOX_D && Math.abs(b.y) < P.BOX_HW && Math.hypot(b.vx, b.vy) < 5) {
        const oppNear = nearest(this.other(k.side).players, b);
        if (oppNear && dist(oppNear, b) > 2.6) {
          px = b.x;
          py = b.y;
        }
      }
    }
    k.targetX = clamp(px, gx - 2.6, gx + dir * 12);
    k.targetY = clamp(py, -P.GOAL_HALF - 3.6, P.GOAL_HALF + 3.6);
    if (k.dive > 0) {
      k.dive -= dt;
      k.vx = lerp(k.vx, k.diveDir * 9, 1 - Math.exp(-9 * dt));
      k.vy = lerp(k.vy, k.diveLat * 11, 1 - Math.exp(-9 * dt));
    } else {
      stepToward(k, dt, towardGoal ? 1.15 : 0.8);
    }
    // reaction: only commits to a dive when the ball is genuinely on its way
    if (!b.owner && k.react <= 0) {
      k.react = Math.max(0.05, this.diff.react * 0.9);
      const distToLine = Math.abs(b.x - gx);
      const speed = Math.hypot(b.vx, b.vy);
      const toward = (dir > 0 ? -b.vx : b.vx) > 1;
      const time = toward ? distToLine / Math.max(1.5, Math.abs(b.vx)) : 99;
      const crossY = clamp(b.y + b.vy * time, -12, 12);
      const crossZ = Math.max(0, b.z + b.vz * time - 0.5 * GRAVITY * time * time);
      const onFrame = Math.abs(crossY) < P.GOAL_HALF + 1.15 && crossZ < P.GOAL_H + 0.5;
      if (toward && speed > 8 && distToLine < 40 && onFrame && k.dive <= 0) {
        const gap = Math.abs(crossY - k.y);
        const reach = 1.5 + (k.dive > 0 ? 1.15 : 0) + Math.min(0.5, speed * 0.022);
        const skill = clamp(0.46 * this.diff.save + (k.def - 70) / 240, 0.08, 0.8);
        if (gap < reach && this.rand() < skill) {
          k.dive = 0.5;
          k.diveDir = 0;
          k.diveLat = Math.sign(crossY - k.y || 1);
          k.diving = true;
          const catchable = speed < 15.5 && crossZ < 1.7 && this.rand() < 0.5 + k.def / 400;
          if (catchable) this.keeperCatch(k);
          else this.keeperParry(k, crossY);
          return;
        }
      }
      if (toward && speed > 9 && distToLine < 12 && crossZ > P.GOAL_H - 0.45 && crossZ < P.GOAL_H + 1.5 && Math.abs(crossY) < 5.5 && this.rand() < 0.34) {
        this.kick(k, { x: gx + dir * 46, y: (this.rand() - 0.5) * 44, loft: 0.95, power: 21, type: 'punch' });
        this.push('punch', { side: k.side, text: 'Punched away by the keeper!' });
      }
    }
  }

  keeperCatch(k) {
    const b = this.ball;
    b.owner = k;
    b.vx = b.vy = b.vz = 0;
    b.vz = 0;
    b.z = 0;
    b.x = k.x + this.teams[k.side].dir * 0.55;
    b.y = k.y;
    b.lastTouch = k;
    b.lastTouchSide = k.side;
    b.recapture = 0.35;
    b.passFrom = null;
    b.passTo = null;
    b.trail.length = 0;
    k.holdTimer = 0.95;
    k.dive = 0;
    k.diving = false;
    this.stats[k.side].saves += 1;
    this.push('save', { side: k.side, keeper: true, text: `SAVED! ${k.first} ${k.name} holds it` });
    this.shake = 0.45;
    this.fx.push({ kind: 'impact', x: k.x, y: k.y, t: 0, life: 0.3, big: true });
  }

  keeperParry(k, shotY) {
    const b = this.ball;
    const dir = this.teams[k.side].dir;
    const behind = this.rand() < 0.26; // punched/deflected behind for a corner
    const safe = !behind && this.rand() < 0.6;
    const out = shotY > 0 ? 1 : -1;
    b.vx = behind ? -dir * (5 + this.rand() * 4) : dir * (safe ? 6.5 : -3.5);
    b.vy = out * (3 + this.rand() * 7);
    b.vz = safe ? 3.5 : 1.2;
    b.z = Math.max(b.z, 0.3);
    b.recapture = 0.3;
    b.lastTouch = k;
    b.lastTouchSide = k.side;
    this.stats[k.side].saves += 1;
    this.push('save', { side: k.side, text: `Clawed out! ${k.name}` });
    this.shake = 0.55;
    this.fx.push({ kind: 'impact', x: k.x, y: k.y, t: 0, life: 0.3, big: true });
  }

  keeperDistribute(k) {
    const t = this.teams[k.side];
    const dir = t.dir;
    const b = this.ball;
    const oppNear = nearest(this.other(k.side).players.filter((p) => !p.gk), k);
    if (oppNear && dist(oppNear, k) < 6.5 && this.rand() < 0.75) {
      this.kick(k, { x: k.x + dir * (34 + this.rand() * 16), y: (this.rand() - 0.5) * 44, loft: 0.95, power: 24, type: 'clear' });
    } else {
      const mate = t.players.filter((m) => !m.off && !m.gk && m.x * dir < -P.HL + 30).sort((a, c) => dist(a, k) - dist(c, k))[0];
      if (mate) this.passBall(k, mate, false);
      else this.kick(k, { x: k.x + dir * 30, y: (this.rand() - 0.5) * 30, loft: 0.9, power: 22, type: 'clear' });
    }
    this.state = 'play';
    this.restart = null;
    b.owner = null;
    k.holdTimer = 0;
  }

  // ===========================================================================
  //  player movement
  // ===========================================================================
  movePlayers(dt) {
    const ps = this.players;
    for (const p of ps) {
      if (p.off) continue;
      if (p.sliding > 0) {
        p.sliding -= dt;
        p.vx *= 1 - dt * 1.1;
        p.vy *= 1 - dt * 1.1;
        this.tackleContact(p);
        if (p.sliding <= 0) {
          p.cooldown = 0.5;
          p.vx *= 0.25;
          p.vy *= 0.25;
          this.fx.push({ kind: 'dust', x: p.x, y: p.y, t: 0, life: 0.5 });
        }
      }
      p.x = clamp(p.x + p.vx * dt, -P.HL - 3.2, P.HL + 3.2);
      p.y = clamp(p.y + p.vy * dt, -P.HW - 2.2, P.HW + 2.2);
      const sp = Math.hypot(p.vx, p.vy);
      if (sp > 0.7 && p.sliding <= 0) p.facing = Math.atan2(p.vy, p.vx);
      p.anim += dt * (0.9 + sp * 0.85);
      p.cooldown = Math.max(0, p.cooldown - dt);
      p.kickLock = Math.max(0, p.kickLock - dt);
      p.holdTimer = Math.max(0, p.holdTimer - dt);
      p.contactCool = Math.max(0, p.contactCool - dt);
      if (p.gk) {
        const own = this.ownGoalX(p.side);
        const dir = this.teams[p.side].dir;
        p.y = clamp(p.y, -P.GOAL_HALF - 3.8, P.GOAL_HALF + 3.8);
        p.x = clamp(p.x, Math.min(own - dir * 2.6, own + dir * 13), Math.max(own - dir * 2.6, own + dir * 13));
      }
    }
    // separation + shoulder challenges
    for (let i = 0; i < ps.length; i++) {
      const a = ps[i];
      if (a.off) continue;
      for (let j = i + 1; j < ps.length; j++) {
        const b = ps[j];
        if (b.off) continue;
        const dx = b.x - a.x;
        const dy = b.y - a.y;
        const dd = dx * dx + dy * dy;
        const min = P.R * 2.1;
        if (dd > min * min || dd < 1e-6) continue;
        const d = Math.sqrt(dd);
        const push = (min - d) * 0.5;
        const nx = dx / d;
        const ny = dy / d;
        const wa = a.sliding > 0 ? 1.7 : 1;
        const wb = b.sliding > 0 ? 1.7 : 1;
        a.x -= nx * push * wa;
        a.y -= ny * push * wa;
        b.x += nx * push * wb;
        b.y += ny * push * wb;
        if (a.sliding <= 0 && b.sliding <= 0) this.shoulderContact(a, b);
      }
    }
  }

  tackleContact(slider) {
    if (slider.contactCool > 0) return;
    const carrier = this.ball.owner;
    const b = this.ball;
    for (const o of this.players) {
      if (o === slider || o.off || o.side === slider.side) continue;
      if (Math.hypot(o.x - slider.x, o.y - slider.y) > 1.5) continue;
      slider.contactCool = 1.4;
      slider.sliding = Math.min(slider.sliding, 0.1);
      const fromBehind = Math.cos(slider.facing - Math.atan2(o.vy, o.vx)) > 0.35;
      const gotBall = dist(slider, b) < 1.9;
      if (o.gk && o.holdTimer > 0) {
        this.foul(slider, o, 'reckless', 0.75);
        return;
      }
      if (carrier === o) {
        if (gotBall) {
          const win = clamp(0.62 + (slider.def - o.pass) / 260, 0.2, 0.9);
          if (this.rand() < win) {
            this.grab(slider, true);
            this.push('tackle', { side: slider.side, text: `HUGE TACKLE by ${slider.first} ${slider.name}!` });
            this.fx.push({ kind: 'impact', x: o.x, y: o.y, t: 0, life: 0.3, big: true });
            this.shake = Math.max(this.shake, 0.3);
            if (fromBehind && this.rand() < 0.3) this.foul(slider, o, 'late');
            return;
          }
        }
        this.foul(slider, o, fromBehind ? 'reckless' : 'trip', gotBall ? 0.25 : 0.72);
        return;
      }
      if (!carrier && gotBall && Math.hypot(o.x - b.x, o.y - b.y) > 1.5) {
        this.grab(slider, true);
        return;
      }
      if (!carrier) {
        // scything through a challenge for a loose ball
        this.foul(slider, o, fromBehind ? 'reckless' : 'trip', 0.4);
        return;
      }
      this.foul(slider, o, 'reckless', 0.85);
      return;
    }
  }

  shoulderContact(a, b) {
    if (a.side === b.side) return;
    const carrier = this.ball.owner;
    if (!carrier || carrier.gk) return;
    const ch = carrier === a ? b : carrier === b ? a : null;
    if (!ch || ch.cooldown > 0 || ch.gk) return;
    if (ch.contactCool > 0) return;
    ch.contactCool = 0.9;
    const chance = clamp(0.1 + (ch.def - carrier.pass) / 300, 0.02, 0.32) * (this.humanSide === ch.side ? 1.25 : this.diff.aggr);
    if (this.rand() < chance) {
      this.grab(ch, true);
      this.push('turnover', { side: ch.side, text: `${ch.first} ${ch.name} shoulders him off the ball` });
    }
  }

  // ===========================================================================
  //  ball
  // ===========================================================================
  updateBall(dt) {
    const b = this.ball;
    if (b.owner && !b.owner.gk) this.stats[b.owner.side].possession += dt;
    b.recapture = Math.max(0, b.recapture - dt);
    if (b.owner) {
      const o = b.owner;
      const tx = o.x + Math.cos(o.facing) * 0.78;
      const ty = o.y + Math.sin(o.facing) * 0.78;
      b.x = lerp(b.x, tx, 1 - Math.exp(-24 * dt));
      b.y = lerp(b.y, ty, 1 - Math.exp(-24 * dt));
      b.z = 0;
      b.vx = o.vx;
      b.vy = o.vy;
      b.rot += Math.hypot(o.vx, o.vy) * dt * 1.2;
      return;
    }
    b.x += b.vx * dt;
    b.y += b.vy * dt;
    const flying = b.z > 0.015 || b.vz > 0.05;
    if (flying) {
      b.vz -= GRAVITY * dt;
      b.z += b.vz * dt;
      b.vx *= 1 - 0.14 * dt;
      b.vy *= 1 - 0.14 * dt;
      if (b.z <= 0) {
        b.z = 0;
        if (b.vz < -1.6) {
          b.vz = -b.vz * 0.4;
          b.vx *= 0.78;
          b.vy *= 0.78;
          this.fx.push({ kind: 'dust', x: b.x, y: b.y, t: 0, life: 0.35 });
          if (b.vz < 1.15) b.vz = 0; // it has stopped bouncing: now it rolls
        } else b.vz = 0;
      }
    } else {
      b.z = 0;
      b.vz = 0;
      const f = 1 - 1.15 * dt;
      b.vx *= f;
      b.vy *= f;
      if (Math.hypot(b.vx, b.vy) < 0.25) {
        b.vx = 0;
        b.vy = 0;
      }
    }
    if (b.curve) {
      const ang = Math.atan2(b.vy, b.vx);
      const sp = Math.hypot(b.vx, b.vy);
      const na = ang + b.curve * dt * 8;
      b.vx = Math.cos(na) * sp;
      b.vy = Math.sin(na) * sp;
      b.curve *= 1 - 1.4 * dt;
      if (Math.abs(b.curve) < 0.0008) b.curve = 0;
    }
    b.rot += Math.hypot(b.vx, b.vy) * dt * 1.1 + b.z * dt * 0.5;
    const speed = Math.hypot(b.vx, b.vy);
    if (speed > 11) {
      b.trail.push({ x: b.x, y: b.y, z: b.z });
      if (b.trail.length > 10) b.trail.shift();
    } else if (b.trail.length) b.trail.shift();

    if ((b.passFrom === 'shot' || b.passFrom === 'freekick') && !b.owner && b.z < 2.3 && Math.hypot(b.vx, b.vy) > 12) {
      const shooter = b.lastTouch;
      if (shooter) {
        const px = b.x - b.vx * dt;
        const py = b.y - b.vy * dt;
        const segX = b.x - px;
        const segY = b.y - py;
        const segL2 = segX * segX + segY * segY || 1;
        for (const p of this.players) {
          if (p.off || p.side === shooter.side || p.gk) continue;
          // distance from the defender to the segment the ball just travelled
          const rx = p.x - px;
          const ry = p.y - py;
          const along = clamp((rx * segX + ry * segY) / segL2, 0, 1);
          const cxp = px + segX * along;
          const cyp = py + segY * along;
          if (Math.hypot(p.x - cxp, p.y - cyp) > 1.05) continue;
          const toGoal = Math.atan2(0 - b.y, this.goalX(shooter.side) - b.x);
          const between = Math.cos(Math.atan2(b.vy, b.vx) - toGoal) > -0.15;
          if (!between || this.rand() > 0.58) continue;
          b.vx *= -0.28;
          b.vy = (this.rand() - 0.5) * 11;
          b.vz = 1.2 + this.rand() * 2.4;
          b.z = Math.max(b.z, 0.1);
          b.recapture = 0.3;
          b.lastTouch = p;
          b.lastTouchSide = p.side;
          b.passFrom = 'clear';
          b.passTo = null;
          this.stats[p.side].blocked += 1;
          this.push('blocked', { side: p.side, text: `BLOCKED! ${p.first} ${p.name} gets in the way` });
          this.fx.push({ kind: 'impact', x: p.x, y: p.y, t: 0, life: 0.3, big: true });
          this.shake = Math.max(this.shake, 0.35);
          break;
        }
      }
    }
    if (this.wall.length && !b.owner && Math.hypot(b.vx, b.vy) > 10) {
      for (const w of this.wall) {
        if (Math.abs(w.x - b.x) < 0.8 && Math.abs(w.y - b.y) < 0.8 && b.z < 2.2) {
          b.vx *= -0.35;
          b.vy += (this.rand() - 0.5) * 8;
          b.recapture = 0.25;
          this.push('blocked', { side: w.side, text: `Blocked by the wall!` });
          this.fx.push({ kind: 'impact', x: w.x, y: w.y, t: 0, life: 0.3, big: true });
          this.stats[this.restart ? this.restart.side : w.side].blocked += 1;
          this.wall = [];
          break;
        }
      }
    }
    this.checkLines();
    if (this.state !== 'play') return;
    this.checkPossession(dt);
  }

  checkLines() {
    if (this.state !== 'play') return;
    const b = this.ball;
    if (b.owner) return;
    const crossed = b.x > P.HL || b.x < -P.HL;
    if (crossed) {
      const right = b.x > 0;
      const gx = right ? P.HL : -P.HL;
      const inMouth = Math.abs(b.y) <= P.GOAL_HALF && b.z <= P.GOAL_H;
      const hitPost = !inMouth && Math.abs(Math.abs(b.y) - P.GOAL_HALF) < 0.28 && b.z < P.GOAL_H + 0.35;
      if (hitPost) {
        b.x = gx - Math.sign(b.x) * 0.4;
        b.vx = -b.vx * 0.55;
        b.vy += (this.rand() - 0.5) * 4;
        this.push('post', { text: 'OFF THE POST!' });
        this.fx.push({ kind: 'ring', x: gx, y: Math.sign(b.y) * P.GOAL_HALF, t: 0, life: 0.5 });
        this.shake = 0.6;
        return;
      }
      if (inMouth) {
        const scorerSide = right ? 'home' : 'away';
        const lt = b.lastTouch;
        this.lastOwnGoal = !!(lt && lt.side !== scorerSide);
        this.lastScorer = lt && lt.side === scorerSide ? lt : this.teams[scorerSide].players.find((p) => !p.gk && !p.off) || null;
        b.x = gx + (right ? 1.1 : -1.1);
        b.vx *= 0.15;
        b.vy *= 0.4;
        b.z = 0;
        b.trail.length = 0;
        this.netRipple = 1;
        this.goalFor(scorerSide);
        return;
      }
      // over the bar
      if (b.z > P.GOAL_H && Math.abs(b.y) < P.BOX_HW && b.vz < 0 && this.rand() < 0.6) {
        const lastT = b.lastTouch;
        if (lastT && this.stats[lastT.side]) {
          this.push('over', { text: 'Over the bar!' });
          this.stats[lastT.side].offTarget += 1;
        }
      }
      b.x = clamp(b.x, -P.HL - 1.8, P.HL + 1.8);
      b.vx = -b.vx * 0.35;
      b.vy *= 0.5;
      this.ballOutGoalLine(b.x, b.y);
      return;
    }
    if (Math.abs(b.y) > P.HW) {
      b.y = Math.sign(b.y) * (P.HW + 0.35);
      b.vy = -b.vy * 0.32;
      b.vx *= 0.8;
      this.ballOutSide(b.x, b.y);
    }
  }

  checkPossession(dt) {
    const b = this.ball;
    if (b.owner) {
      const o = b.owner;
      // only lose it if the ball genuinely escaped the player (a deflection, a sending-off)
      if (o.off || d2(o, b) > 20) {
        b.owner = null;
        b.recapture = 0.08;
      }
      return;
    }
    if (b.recapture > 0) return;
    const speed = Math.hypot(b.vx, b.vy);
    const wanted = b.passTo;
    let best = null;
    let bestScore = Infinity;
    for (const p of this.players) {
      if (p.off || p.kickLock > 0) continue;
      if (p === b.lastTouch && b.passFrom !== 'dribble' && Math.hypot(p.x - b.x, p.y - b.y) < 1.15) continue;
      if (p.gk) {
        // keepers gather loose balls inside their six only
        if (!this.inBoxAt(p.side, b.x, b.y) || b.z > 1.7 || dist(p, b) > 1.6 || speed > 14) continue;
        this.keeperCatch(p);
        return;
      }
      const sliding = p.sliding > 0;
      const intended = p === wanted;
      const reach = (sliding ? 1.9 : 1.35) + Math.min(0.55, speed * 0.022) + (intended ? 0.9 : 0);
      if (Math.abs(p.x - b.x) > reach || Math.abs(p.y - b.y) > reach) continue;
      if (b.z > (sliding ? 1.8 : intended ? 2.1 : 1.5)) continue;
      const score = Math.hypot(p.x - b.x, p.y - b.y) - (intended ? 1.2 : 0) - (sliding ? 0.4 : 0);
      if (score < bestScore) {
        bestScore = score;
        best = p;
      }
    }
    if (!best) return;
    const hardShot = (b.passFrom === 'shot' || b.passFrom === 'freekick' || b.passFrom === 'penalty') && speed > 15;
    const mate = b.lastTouch && b.lastTouch.side === best.side;
    // a teammate's pass simply comes to rest on the boot; a hard shot has to be smothered
    if (hardShot && !mate) {
      if (this.rand() < 0.55 + best.def / 300) this.grab(best, true);
      else {
        b.vx *= 0.5;
        b.vy += (this.rand() - 0.5) * 6;
        b.recapture = 0.22;
        b.lastTouch = best;
        b.lastTouchSide = best.side;
        this.push('spill', { text: `${best.first} ${best.name} can't hold it!` });
      }
      return;
    }
    if (!mate && speed > 19 && this.rand() < 0.3) {
      b.vx *= 0.6;
      b.vy += (this.rand() - 0.5) * 5;
      b.recapture = 0.2;
      b.lastTouch = best;
      b.lastTouchSide = best.side;
      return;
    }
    const prev = b.lastTouch;
    this.grab(best);
    if (b.passFrom === 'pass' || b.passFrom === 'through' || b.passFrom === 'cross') {
      const side = best.side;
      if (prev && prev.side === side && prev !== best) this.stats[side].passOk += 1;
      if (b.passFrom === 'through' && prev && prev.side === side) this.stats[side].throughOk += 1;
    }
    b.passTo = null;
    b.passFrom = null;
  }

  grab(p, loose = false) {
    if (p.gk && this.state === 'play' && !this.inBoxAt(p.side, this.ball.x, this.ball.y)) return;
    const b = this.ball;
    b.owner = p;
    b.lastTouch = p;
    b.lastTouchSide = p.side;
    b.vx = b.vy = b.vz = 0;
    b.z = 0;
    b.curve = 0;
    b.trail.length = 0;
    b.recapture = loose ? 0.12 : 0.05;
    p.holdTimer = 0.2;
    // the ball has to be controlled before the next action: this is what makes the AI
    // carry it, look up and play a pass instead of hitting it on sight
    if (p !== this.active) p.react = Math.max(p.react, 0.32 + this.rand() * 0.3);
    if (p.side === this.humanSide && this.state === 'play') this.active = p;
    if (this.restart) {
      this.restart = null;
      this.state = 'play';
    }
  }

  kick(p, { x, y, loft = 0.2, power = 18, type = 'pass', curve = 0, to = null }) {
    const b = this.ball;
    const dx = x - p.x;
    const dy = y - p.y;
    const len = Math.hypot(dx, dy) || 1;
    const sp = clamp(power, 5, 34);
    b.owner = null;
    b.vx = (dx / len) * sp;
    b.vy = (dy / len) * sp;
    b.vz = loft * sp * 0.3;
    b.z = Math.max(b.z, 0.03);
    b.curve = curve;
    b.recapture = (type === 'pass' ? 0.1 : 0.16) + loft * 0.32;
    b.lastTouch = p;
    b.lastTouchSide = p.side;
    b.passFrom = type;
    if (p.side !== (b.passTo && b.passTo.side)) b.passTo = null;
    b.passTo = to || (type === 'shot' || type === 'freekick' || type === 'penalty' ? null : nearest(this.players.filter((q) => q.side === p.side && q !== p && !q.off), { x, y }));
    b.trail.length = 0;
    p.kickLock = (type === 'shot' || type === 'freekick' || type === 'penalty' ? 0.42 : 0.34) + loft * 0.16;
    p.facing = Math.atan2(dy, dx);
    p.shooting = false;
    this.fx.push({ kind: 'impact', x: p.x + (dx / len) * 0.6, y: p.y + (dy / len) * 0.6, t: 0, life: 0.22, big: type === 'shot' || type === 'penalty' });
    const st = this.stats[p.side];
    if (type === 'shot' || type === 'penalty' || type === 'freekick') {
      st.shots += 1;
      const gx = this.goalX(p.side);
      const t = Math.abs(gx - p.x) / Math.max(4, Math.abs(b.vx));
      const crossY = b.y + b.vy * t;
      const crossZ = Math.max(0, b.z + b.vz * t - 0.5 * GRAVITY * t * t);
      const onFrame = Math.abs(crossY) < P.GOAL_HALF + 0.2 && crossZ < P.GOAL_H + 0.1;
      if (onFrame) st.onTarget += 1;
      else st.offTarget += 1;
      this.fx.push({ kind: 'shot', x: p.x, y: p.y, side: p.side, t: 0, life: 0.001, on: onFrame });
    }
    if (type === 'pass' || type === 'through' || type === 'cross') {
      st.passTry += 1;
      if (type === 'through') st.throughTry += 1;
    }
    this.push('kick', { side: p.side, kind: type });
    if (this.state === 'restart') {
      this.state = 'play';
      this.restart = null;
    }
  }

  passBall(from, to, through = false) {
    const dir = this.teams[from.side].dir;
    const lead = through ? 1.05 : 0.55;
    const tx = to.x + to.vx * lead + (through ? dir * 9 : 0);
    const ty = to.y + to.vy * lead;
    const dd = Math.hypot(tx - from.x, ty - from.y);
    const sk = from.pass / 100;
    const err = (1 - sk) * (through ? 3.4 : 1.9);
    const power = clamp(11.5 + dd * (through ? 0.42 : 0.34), 13, 24);
    this.kick(from, { x: tx + (this.rand() - 0.5) * err, y: ty + (this.rand() - 0.5) * err, loft: through ? 0.3 : 0.08, power, type: through ? 'through' : 'pass', to });
    const name = `${to.first} ${to.name}`;
    if (through) this.push('through', { side: from.side, text: `Through ball to ${name}!` });
  }

  slideTackle(p, ax, ay, human) {
    if (p.cooldown > 0 || p.sliding > 0 || p.off) return;
    if (!ax && !ay) {
      ax = Math.cos(p.facing);
      ay = Math.sin(p.facing);
    }
    const m = Math.hypot(ax, ay) || 1;
    p.sliding = human ? 0.4 : 0.34;
    p.cooldown = 0.72;
    p.contactCool = 0;
    p.facing = Math.atan2(ay / m, ax / m);
    const boost = (human ? 9.4 : 8.2) * (0.8 + p.pac / 260);
    p.vx = (ax / m) * boost;
    p.vy = (ay / m) * boost;
    p.stamina = clamp(p.stamina - 0.055, 0.28, 1);
    this.fx.push({ kind: 'dust', x: p.x, y: p.y, t: 0, life: 0.45 });
  }

  // ===========================================================================
  //  fouls, cards
  // ===========================================================================
  foul(offender, victim, kind = 'trip', cardChance = 0.18) {
    if (this.state !== 'play') return;
    if (this.restart) return;
    const defSide = victim.side;
    const inDefBox = this.inBoxAt(offender.side, offender.x, offender.y);
    const lastMan = this.isLastDefender(offender);
    const throughBall = this.ball.owner === victim && victim.x * this.teams[defSide].dir > offender.x * this.teams[defSide].dir;
    const dogsо = lastMan && throughBall && Math.abs(this.goalX(defSide) - victim.x) < 40;
    this.stats[offender.side].fouls += 1;
    this.stoppageExtra = Math.min(4, (this.stoppageExtra || 0) + 0.25);
    this.fx.push({ kind: 'impact', x: (offender.x + victim.x) / 2, y: (offender.y + victim.y) / 2, t: 0, life: 0.3, big: true });
    this.shake = Math.max(this.shake, 0.25);
    // the offender is grounded for a beat so he cannot keep chasing
    offender.sliding = 0;
    offender.vx = offender.vy = 0;
    victim.vx = victim.vy = 0;
    const shootingAt = Math.abs(this.goalX(defSide) - victim.x) < 32;
    let card = 'none';
    if (dogsо && shootingAt && !offender.gk && this.teams[offender.side].players.filter((q) => q.gk && !q.off).length) card = 'red';
    else if (this.rand() < cardChance * (kind === 'reckless' ? 0.55 : 0.28)) card = 'yellow';
    if (inDefBox && this.ball.owner === victim) {
      this.push('foul', { side: offender.side, text: `Penalty! ${offender.first} ${offender.name} brings down ${victim.first} ${victim.name}` });
      if (card === 'red') this.card(offender, 'red');
      else if (card === 'yellow') this.card(offender, 'yellow');
      this.banner = { text: 'PENALTY', sub: `${this.teams[defSide].team.name}`, until: this.t + 2.2 };
      this.push('penalty', { side: defSide, text: 'PENALTY!' });
      this.setRestart({ kind: 'penalty', side: defSide, x: 0, y: 0 });
      return;
    }
    this.push('foul', {
      side: offender.side,
      text: `Foul on ${victim.first} ${victim.name} by ${offender.first} ${offender.name}`,
    });
    if (card === 'red') this.card(offender, 'red');
    else if (card === 'yellow') this.card(offender, 'yellow');
    const dangerous = Math.abs(this.goalX(defSide) - offender.x) < 27 && Math.abs(offender.y) < 25;
    this.setRestart({ kind: 'freekick', side: defSide, x: clamp(offender.x, -P.HL - 1, P.HL + 1), y: clamp(offender.y, -P.HW, P.HW), dangerous });
  }

  isLastDefender(p) {
    const carrier = this.ball.owner;
    if (!carrier || carrier.side === p.side || p.gk) return false;
    const gx = this.ownGoalX(p.side);
    const dir = this.teams[p.side].dir;
    // is there any teammate (or the keeper) goal-side of him?
    const ahead = this.teams[p.side].players.filter(
      (q) => q !== p && !q.off && (q.gk || (q.x - gx) * dir < (p.x - gx) * dir)
    );
    return ahead.length <= 1 && (carrier.x - gx) * dir > (p.x - gx) * dir;
  }

  card(p, colour) {
    if (p.card === 'red') return;
    if (colour === 'yellow') {
      p.card = p.card === 'yellow' ? 'red' : 'yellow';
      this.stats[p.side].yellow += 1;
      this.push('yellow', { side: p.side, player: `${p.first} ${p.name}`, text: `Yellow card for ${p.first} ${p.name}` });
      this.banner = { text: 'YELLOW CARD', sub: `${p.first} ${p.name} ${Math.round(this.minute)}'`, until: this.t + 1.6 };
    }
    if (p.card === 'red') {
      p.card = 'red';
      p.off = true;
      p.vx = p.vy = 0;
      this.stats[p.side].red += 1;
      this.push('red', { side: p.side, player: `${p.first} ${p.name}`, text: `RED CARD! ${p.first} ${p.name} is off!` });
      this.banner = { text: 'RED CARD', sub: `${p.first} ${p.name} ${Math.round(this.minute)}'`, until: this.t + 2.4 };
      if (this.active === p) this.active = this.autoPick(p.side);
      for (const q of this.teams[p.side].players) if (!q.gk) q.attackBias = Math.max(-0.4, q.attackBias - 0.2);
    }
  }

  // ===========================================================================
  //  set pieces
  // ===========================================================================
  setRestart(r) {
    const t = this.teams[r.side];
    const dir = t.dir;
    const b = this.ball;
    this.wall = [];
    if (r.kind === 'penalty') {
      const gx = this.goalX(r.side);
      r.x = gx - dir * P.SPOT;
      r.y = 0;
    }
    this.restart = { ...r, t: 0, taken: false, taker: null };
    this.state = 'restart';
    b.owner = null;
    b.x = r.x;
    b.y = r.y;
    b.z = 0;
    b.vx = b.vy = b.vz = 0;
    b.curve = 0;
    b.recapture = 0.25;
    b.trail.length = 0;

    let taker;
    if (r.kind === 'goalkick') taker = t.players.find((p) => p.gk && !p.off) || t.players[0];
    else taker = nearest(t.players.filter((p) => !p.gk && !p.off), b, (p) => (p.line === 'ST' ? -2 : p.line === 'CM1' || p.line === 'CM2' ? -1 : 0));
    this.restart.taker = taker;

    // where everybody goes
    const gx = this.goalX(r.side);
    const opp = this.other(r.side);
    for (const p of this.players) {
      if (p.off) continue;
      if (p.gk) {
        const own = this.ownGoalX(p.side);
        p.targetX = own + this.teams[p.side].dir * 1.5;
        p.targetY = r.kind === 'penalty' ? 0 : clamp(r.y * 0.1, -3, 3);
        if (r.kind === 'goalkick' && p === taker) {
          p.targetX = r.x;
          p.targetY = r.y;
        }
        continue;
      }
      const attack = p.side === r.side;
      if (p === taker && r.kind !== 'goalkick') {
        p.targetX = r.x - dir * 0.55;
        p.targetY = r.y + 0.4;
        continue;
      }
      if (r.kind === 'corner' || (r.kind === 'freekick' && r.dangerous)) {
        if (attack) {
          p.targetX = gx - dir * (3.2 + Math.abs(p.fy) * 8);
          p.targetY = clamp(p.fy * 11.5 + r.y * 0.06, -P.BOX_HW + 1.5, P.BOX_HW - 1.5);
        } else {
          p.targetX = gx - this.teams[p.side].dir * (4.5 + Math.abs(p.fy) * 6);
          p.targetY = clamp(p.fy * 9, -P.BOX_HW + 2, P.BOX_HW - 2);
        }
      } else if (r.kind === 'penalty') {
        p.targetX = clamp(r.x + dir * (16 + Math.abs(p.fy) * 12), -P.HL + 3, P.HL - 3);
        p.targetY = p.fy * 14;
      } else {
        this.placeShape(p, false);
      }
    }
    // a wall for dangerous free kicks
    if (r.kind === 'freekick' && r.dangerous) {
      const toGoal = Math.atan2(0 - r.y, gx - r.x);
      const men = opp.players
        .filter((p) => !p.gk && !p.off)
        .sort((a, c) => dist(a, r) - dist(c, r))
        .slice(0, 3);
      men.forEach((p, i) => {
        const off = (i - 1) * 0.95;
        p.targetX = r.x + Math.cos(toGoal) * 9.4 - Math.sin(toGoal) * off;
        p.targetY = r.y + Math.sin(toGoal) * 9.4 + Math.cos(toGoal) * off;
        this.wall.push(p);
      });
    }
    if (this.humanSide === r.side) this.active = taker;
    else if (r.kind === 'penalty' && this.userKeeperPens) this.active = opp.players.find((p) => p.gk) || null;
    this.assignRoles();
  }

  /** ball over the touchline: throw-in to the team that did not touch it last */
  ballOutSide(x, y) {
    const lastSide = this.ball.lastTouchSide;
    const to = lastSide ? this.other(lastSide).side : 'home';
    this.setRestart({ kind: 'throwin', side: to, x: clamp(x, -P.HL + 1.2, P.HL - 1.2), y: Math.sign(y) * P.HW });
    this.push('throwin', { side: to, text: `Throw-in ${this.teams[to].team.name}` });
  }

  /** ball over the byline: goal kick or corner */
  ballOutGoalLine(x, y) {
    const lastSide = this.ball.lastTouchSide;
    const attackedThatGoal = lastSide ? (this.teams[lastSide].dir > 0) === x > 0 : false;
    const defSide = x > 0 ? 'away' : 'home';
    const atkSide = x > 0 ? 'home' : 'away';
    if (attackedThatGoal) {
      const t = this.teams[defSide];
      this.setRestart({ kind: 'goalkick', side: defSide, x: this.ownGoalX(defSide) + t.dir * 2.4, y: clamp(y, -P.SIX_HW + 1, P.SIX_HW - 1) });
      this.push('goalkick', { side: defSide, text: `Goal kick ${t.team.name}` });
    } else {
      const t = this.teams[atkSide];
      this.setRestart({ kind: 'corner', side: atkSide, x: this.goalX(atkSide) - t.dir * 0.45, y: Math.sign(y || 1) * (P.HW - 0.45) });
      this.stats[atkSide].corners += 1;
      this.push('corner', { side: atkSide, text: `Corner ${t.team.name}` });
    }
  }

  updateRestart(dt) {
    const r = this.restart;
    if (!r) {
      this.state = 'play';
      return;
    }
    r.t += dt;
    const b = this.ball;
    const humanTurn = this.humanSide === r.side && r.taker && !r.taker.off;
    if (!r.taken) {
      b.x = lerp(b.x, r.x, 1 - Math.exp(-18 * dt));
      b.y = lerp(b.y, r.y, 1 - Math.exp(-18 * dt));
      b.z = 0;
      b.vx = b.vy = b.vz = 0;
    }
    for (const p of this.players) {
      if (p.off || p === this.active) continue;
      if (this.wall.includes(p)) {
        p.vx = p.vy = 0;
        p.facing = Math.atan2(0 - p.y, this.goalX(r.side) - p.x);
        continue;
      }
      if (p === r.taker && !r.taken) {
        if (!humanTurn) p.facing = Math.atan2(0 - p.y, this.goalX(r.side) - p.x);
        p.vx *= 0.4;
        p.vy *= 0.4;
        continue;
      }
      stepToward(p, dt, r.kind === 'penalty' ? 0.4 : 1.05);
    }
    if (r.kind === 'penalty' && !humanTurn && r.t < 1.35) return; // let the run-up breathe
    if (humanTurn) {
      this.handleHuman(dt);
      if (!r.taken && r.t > 6.5) this.aiTakeRestart(r, r.taker);
      return;
    }
    const wait = r.kind === 'corner' ? 1.15 : r.kind === 'goalkick' ? 1.0 : r.kind === 'throwin' ? 0.75 : r.kind === 'penalty' ? 1.4 : 1.25;
    if (r.t >= wait) this.aiTakeRestart(r, r.taker);
  }

  aiTakeRestart(r, taker) {
    if (!taker) {
      this.state = 'play';
      this.restart = null;
      return;
    }
    const t = this.teams[r.side];
    const dir = t.dir;
    const gx = this.goalX(r.side);
    r.taken = true;
    if (r.kind === 'penalty') {
      const gx = this.goalX(r.side);
      const taker2 = r.taker;
      const keeper = this.other(r.side).players.find((p) => p.gk && !p.off);
      this.kick(taker2, { x: gx, y: this.pickPenaltyAim(taker2), loft: 0.18, power: 27 + taker2.shoot / 40, type: 'penalty' });
      if (keeper) {
        keeper.diving = true;
        keeper.dive = 0.6;
        keeper.diveDir = this.rand() < 0.45 ? (this.rand() < 0.5 ? -1 : 1) : 0;
        keeper.diveLat = keeper.diveDir;
      }
      this.push('penaltyKick', { side: r.side, text: `${taker2.first} ${taker2.name} from twelve yards...` });
      return;
    }
    if (r.kind === 'corner') {
      const target = bestBoxTarget(this, t) || { x: gx - dir * 6.5, y: (this.rand() - 0.5) * 9 };
      if (this.rand() < 0.24) {
        this.kick(taker, { x: gx - dir * 1.9, y: r.y * 0.45, loft: 0.3, power: 15.5, type: 'pass' });
        this.push('cornerIn', { side: r.side, text: 'Short corner...' });
      } else {
        this.kick(taker, { x: target.x + dir * 1.1, y: target.y, loft: 0.68, power: 18.5, type: 'cross' });
        this.push('cornerIn', { side: r.side, text: 'Corner swung into the box...' });
      }
      return;
    }
    if (r.kind === 'goalkick') {
      const mate = t.players.filter((m) => !m.off && !m.gk).sort((a, b2) => dist(a, taker) - dist(b2, taker))[0];
      if (mate && this.rand() < 0.55) this.passBall(taker, mate, false);
      else this.kick(taker, { x: taker.x + dir * 36, y: (this.rand() - 0.5) * 36, loft: 0.92, power: 24, type: 'clear' });
      return;
    }
    if (r.kind === 'freekick' && r.dangerous) {
      const sk = (taker.shoot / 100) * this.diff.skill;
      if (this.rand() < 0.55) {
        const y = clamp((this.rand() - 0.5) * 8 * (1.35 - sk), -4.2, 4.2);
        this.kick(taker, { x: gx, y, loft: 0.44, power: 25 + sk * 4, type: 'freekick', curve: (this.rand() - 0.5) * 0.06 });
        this.push('freekick', { side: r.side, text: `${taker.first} ${taker.name} bends it round the wall!` });
      } else if (this.rand() < 0.6) {
        const target = bestBoxTarget(this, t) || { x: gx - dir * 8, y: (this.rand() - 0.5) * 12 };
        this.kick(taker, { x: target.x, y: target.y, loft: 0.72, power: 20, type: 'cross' });
        this.push('freekick', { side: r.side, text: 'Delivery into the box' });
      } else {
        const mate = bestMate(this, taker, 24, true);
        if (mate) this.passBall(taker, mate, false);
        else this.kick(taker, { x: taker.x + dir * 20, y: taker.y, loft: 0.2, power: 17, type: 'clear' });
      }
      return;
    }
    if (r.kind === 'throwin') {
      const mate = bestMate(this, taker, 18, true);
      if (mate) {
        this.stats[r.side].passTry += 1;
        this.kick(taker, { x: mate.x + mate.vx * 0.5, y: mate.y + mate.vy * 0.5, loft: 0.42, power: 15.5, type: 'pass' });
      } else this.kick(taker, { x: taker.x + dir * 14, y: taker.y + (this.rand() - 0.5) * 10, loft: 0.3, power: 14, type: 'clear' });
      return;
    }
    const mate = bestMate(this, taker, 26, true);
    if (mate) this.passBall(taker, mate, this.rand() < 0.22);
    else this.kick(taker, { x: taker.x + dir * 18, y: taker.y + (this.rand() - 0.5) * 12, loft: 0.25, power: 17, type: 'clear' });
  }

  /** Human set pieces: pass, cross, shoot or drive one on. */
  takeHumanRestart(p, kind) {
    const r = this.restart;
    if (!r || r.taken) return;
    const t = this.teams[r.side];
    const dir = t.dir;
    const gx = this.goalX(r.side);
    const aimX = Math.cos(p.facing);
    const aimY = Math.sin(p.facing);
    if (r.kind === 'penalty') {
      r.taken = true;
      return this.kick(p, { x: gx, y: clamp(aimY * 5, -4.4, 4.4), loft: 0.4, power: 16, type: 'penalty' });
    }
    if (kind === 'shoot') {
      const held = clamp(this.t - this.holdStart, 0.06, 0.9);
      r.taken = true;
      return this.humanShoot(p, held, aimX, aimY);
    }
    if (r.kind === 'corner' || (r.kind === 'freekick' && r.dangerous && Math.abs(p.y) > 12) || kind === 'through') {
      const target = kind === 'through' && Math.abs(aimY) < 0.3 ? null : bestBoxTarget(this, t) || { x: gx - dir * 6.5, y: 0 };
      if (target) {
        r.taken = true;
        return this.kick(p, { x: target.x + dir, y: target.y, loft: 0.66, power: 19, type: 'cross' });
      }
    }
    const mate = this.pickPassTarget(p, aimX, aimY, kind === 'through') || bestMate(this, p, r.kind === 'throwin' ? 18 : 26, r.kind !== 'throwin');
    r.taken = true;
    if (mate) {
      if (r.kind === 'throwin') {
        this.stats[r.side].passTry += 1;
        return this.kick(p, { x: mate.x + mate.vx * 0.5, y: mate.y + mate.vy * 0.5, loft: 0.42, power: 15.5, type: 'pass' });
      }
      return this.passBall(p, mate, kind === 'through');
    }
    this.kick(p, { x: p.x + dir * 18, y: p.y, loft: 0.25, power: 17, type: 'clear' });
  }

  // ===========================================================================
  //  penalty shootout  (phase machine: intro -> place -> aim -> flight -> result)
  // ===========================================================================
  startShootout() {
    this.state = 'shootout';
    this.pens = {
      home: { score: 0, taken: 0, kicks: [] },
      away: { score: 0, taken: 0, kicks: [] },
      turn: this.rand() < 0.5 ? 'home' : 'away',
      round: 1,
      phase: 'intro',
      timer: 2.2,
      sudden: false,
      message: 'Penalty shootout',
      aim: 0,
      wait: 0,
      readTime: 0,
      winner: null,
      order: [],
    };
    this.ball.owner = null;
    this.ball.vx = this.ball.vy = 0;
    this.banner = { text: 'PENALTY SHOOTOUT', sub: 'Five each, then sudden death', until: this.t + 2.4 };
    this.push('shootout', { text: 'Level after extra time — penalties!' });
  }

  updateShootout(dt) {
    const s = this.pens;
    if (!s) return this.finishMatch();
    switch (s.phase) {
      case 'intro':
        s.timer -= dt;
        this.soft(dt * 0.7);
        if (s.timer <= 0) this.pensPlace();
        return;
      case 'aim':
        this.pensAim(dt);
        return;
      case 'flight':
        this.pensFlight(dt);
        return;
      case 'result':
        s.timer -= dt;
        this.animCelebrate(dt * 0.7);
        if (s.timer <= 0) {
          if (this.pensCheck()) this.pensFinish();
          else this.pensPlace();
        }
        return;
      default:
        this.pensPlace();
    }
  }

  /**
   * Who steps up: best finishers first, keeper last, then the list wraps. Red cards and
   * already-sent-off players are skipped, so the order is only rebuilt when a shootout starts.
   */
  pickPenaltyTaker(t) {
    const s = this.pens;
    if (!s.takerOrder) {
      s.takerOrder = {};
      for (const side of ['home', 'away']) {
        const squad = this.teams[side].players.filter((p) => !p.gk).sort((a, b) => b.shoot - a.shoot);
        const gk = this.teams[side].players.find((p) => p.gk);
        s.takerOrder[side] = (squad.length ? squad : this.teams[side].players).slice(0, 7).concat(gk ? [gk] : []);
      }
    }
    const list = s.takerOrder[t.side] || t.players;
    if (!list.length) return t.players[0];
    const start = (s[t.side].taken || 0) % list.length;
    let p = list[start];
    for (let g = 0; g < list.length && (p.off || p.card === 'red'); g++) {
      p = list[(start + 1 + g) % list.length];
    }
    return p;
  }

  pensPlace() {
    const s = this.pens;
    const side = s.turn;
    // alternate, and keep the round number honest for the HUD
    s.turn = side === 'home' ? 'away' : 'home';
    s.round = Math.floor((s.home.taken + s.away.taken) / 2) + 1;
    const t = this.teams[side];
    const opp = this.other(side);
    const dir = t.dir;
    const gx = this.goalX(side);
    s.kickSide = side;
    s.taker = this.pickPenaltyTaker(t);
    s.keeper = opp.players.find((p) => p.gk && !p.off) || opp.players[0];
    s.spot = { x: gx - dir * P.SPOT, y: 0 };
    s.humanTaker = this.humanSide === side;
    s.userKeeper = !s.humanTaker && this.userKeeperPens && this.humanSide === opp.side;
    s.aim = 0;
    s.wait = 0;
    s.readTime = 0;
    s.outcome = null;
    s.savedBy = null;
    const b = this.ball;
    b.owner = null;
    b.x = s.spot.x;
    b.y = 0;
    b.z = 0;
    b.vx = b.vy = b.vz = 0;
    b.curve = 0;
    b.trail.length = 0;
    const tk = s.taker;
    const k = s.keeper;
    for (const p of this.players) {
      p.vx = p.vy = 0;
      p.sliding = 0;
      p.dive = 0;
      p.diving = false;
      p.diveIntent = 0;
      p.shooting = false;
      p.celebrate = 0;
    }
    tk.x = s.spot.x - dir * 2.1;
    tk.y = 0.45;
    tk.facing = Math.atan2(0 - tk.y, gx - tk.x);
    k.x = gx - dir * 0.9;
    k.y = 0;
    k.targetX = k.x;
    k.targetY = 0;
    for (const p of this.players) {
      if (p === tk || p === k || p.gk) continue;
      p.targetX = clamp(s.spot.x - dir * (14 + Math.abs(p.fy) * 16), -P.HL + 3, P.HL - 3);
      p.targetY = clamp(p.fy * 15, -18, 18);
      p.x = lerp(p.x, p.targetX, 0.35);
      p.y = lerp(p.y, p.targetY, 0.35);
    }
    s.phase = 'aim';
    s.timer = s.humanTaker ? Infinity : s.userKeeper ? 1.25 + this.rand() * 0.55 : 1.05 + this.rand() * 0.5;
    s.message = s.humanTaker
      ? `${t.team.name} — your kick. Move to aim, hold to power, release to shoot.`
      : s.userKeeper
        ? `${t.team.name} step up — pick a side and dive!`
        : `${t.team.name} take the kick`;
    if (s.humanTaker) this.active = tk;
    else if (s.userKeeper) this.active = k;
    else this.active = null;
    this.push('penaltyPlace', { side });
  }

  pensAim(dt) {
    const s = this.pens;
    const tk = s.taker;
    const k = s.keeper;
    const dir = this.teams[s.kickSide].dir;
    const inp = this.input;
    const mag = Math.hypot(inp.mx, inp.my);
    if (s.humanTaker) {
      s.aim = mag > 0.22 ? clamp(inp.my * 3.9, -4.4, 4.4) : s.aim * (1 - dt * 3);
      tk.facing = Math.atan2(s.aim * 0.35, dir * 20);
      s.wait += dt;
      if (inp.shoot && !tk.shooting) {
        tk.shooting = true;
        this.holdStart = this.t;
      }
      if (inp.shoot && tk.shooting) s.readTime += dt; // hold too long and the keeper reads you
      if (!inp.shoot && tk.shooting) {
        const held = clamp(this.t - this.holdStart, 0.05, 0.9);
        tk.shooting = false;
        this.pensKick(s.aim, lerp(24, 31.5, held / 0.9), clamp(s.readTime / 1.1, 0, 1));
        return;
      }
      if (s.wait > 11) this.pensKick(s.aim, 26, 0.6);
      // keeper sways on his line
      k.targetY = Math.sin(this.t * 2.4) * 0.6;
      k.targetX = this.goalX(s.kickSide) - dir * 0.9;
      stepToward(k, dt, 0.5);
      return;
    }
    if (s.userKeeper) {
      const gx = this.goalX(s.kickSide);
      if (mag > 0.2) {
        k.diveIntent = inp.mx < -0.35 ? -1 : inp.mx > 0.35 ? 1 : 0;
        k.targetY = clamp((k.targetY || 0) + inp.my * dt * 22, -3.2, 3.2);
        k.targetX = gx - dir * 0.9;
        stepToward(k, dt, 1.4);
      }
      if ((inp.shoot || inp.tackle) && !k.diving) {
        k.diving = true;
        k.dive = 0.62;
        k.diveDir = k.diveIntent || (inp.my > 0.2 ? 1 : inp.my < -0.2 ? -1 : 0);
        k.diveLat = k.diveDir;
      }
      // telegraph: the taker leans a touch before striking
      tk.x = lerp(tk.x, s.spot.x - dir * 0.7, dt * 1.1);
      s.timer -= dt;
      if (s.timer <= 0) this.pensKick(this.pickPenaltyAim(tk), 26 + tk.shoot / 45, 0);
      return;
    }
    s.timer -= dt;
    tk.x = lerp(tk.x, s.spot.x - dir * 0.65, dt * 1.3);
    if (s.timer <= 0) this.pensKick(this.pickPenaltyAim(tk), 26 + tk.shoot / 45, 0);
  }

  pickPenaltyAim(taker) {
    const sk = taker.shoot / 100;
    const r = this.rand();
    let y;
    if (r < 0.33) y = -2.4 - this.rand() * 1.0;
    else if (r < 0.66) y = 2.4 + this.rand() * 1.0;
    else y = (this.rand() - 0.5) * 1.4;
    if (this.rand() > 0.8 + sk * 0.13) y *= 1.85;
    return clamp(y, -5.8, 5.8);
  }

  pensKick(aimY, power, read = 0) {
    const s = this.pens;
    if (!s || s.phase !== 'aim') return;
    const side = s.kickSide;
    const tk = s.taker;
    const k = s.keeper;
    const gx = this.goalX(side);
    const dir = this.teams[side].dir;
    const sk = tk.shoot / 100;
    const y = clamp(aimY + (1 - sk) * (this.rand() - 0.5) * 2.2, -6.8, 6.8);
    this.kick(tk, { x: gx, y, loft: 0.14 + Math.abs(y) * 0.03, power: clamp(power, 19, 33), type: 'penalty' });
    if (!k.diving) {
      const reads = this.rand() < 0.16 + read * 0.55;
      const guess = reads ? Math.sign(y || 1) : this.rand() < 0.16 ? 0 : this.rand() < 0.5 ? -1 : 1;
      k.diving = true;
      k.dive = 0.62;
      k.diveDir = guess;
      k.diveLat = guess;
    }
    s.shot = { y, power };
    s.phase = 'flight';
    s.timer = 2.2;
    this.stats[side].shots += 1;
    this.push('penaltyTaken', { side, text: `${tk.first} ${tk.name} steps up...` });
  }

  pensFlight(dt) {
    const s = this.pens;
    const b = this.ball;
    const k = s.keeper;
    const dir = this.teams[s.kickSide].dir;
    const gx = this.goalX(s.kickSide);
    b.x += b.vx * dt;
    b.y += b.vy * dt;
    if (b.z > 0 || b.vz !== 0) {
      b.vz -= GRAVITY * dt;
      b.z += b.vz * dt;
      if (b.z < 0) {
        b.z = 0;
        b.vz = -b.vz * 0.42;
      }
    }
    if (k) {
      if (k.dive > 0) {
        k.dive -= dt;
        k.vy = lerp(k.vy, k.diveDir * 10.5, 1 - Math.exp(-14 * dt));
      } else if (!s.humanTaker) k.vy = lerp(k.vy, 0, dt * 6);
      else k.vy = lerp(k.vy, 0, dt * 4);
      k.y = clamp(k.y + k.vy * dt, -4.6, 4.6);
    }
    const crossed = dir > 0 ? b.x >= gx - 0.15 : b.x <= gx + 0.15;
    if (!crossed) return;
    const reach = 0.9 + (k && k.diving ? 1.15 : 0.28) + (k ? k.def / 320 : 0);
    const onTarget = Math.abs(b.y) <= P.GOAL_HALF && b.z <= P.GOAL_H;
    const saved = k && onTarget && Math.abs(b.y - k.y) < reach && b.z < 2.35;
    s.phase = 'result';
    s.timer = 1.7;
    const side = s.kickSide;
    s.order.push(side);
    if (saved) {
      b.vx = -dir * (4 + this.rand() * 4);
      b.vy = (this.rand() - 0.5) * 9;
      b.vz = 3.2;
      b.recapture = 0.6;
      s.outcome = 'save';
      s.savedBy = k;
      this.stats[side].onTarget += 1;
      this.stats[k.side].saves += 1;
      this.pens[side].taken += 1;
      this.pens[side].kicks.push(0);
      s.message = `SAVED! ${k.first} ${k.name}`;
      this.banner = { text: 'SAVED!', sub: `${k.first} ${k.name}`, until: this.t + 1.7 };
      this.push('penaltySave', { side: k.side, keeper: true, text: `PENALTY SAVED by ${k.first} ${k.name}!` });
      this.shake = 0.5;
      for (const p of this.teams[k.side].players) p.celebrate = 1.1 + this.rand() * 0.5;
      return;
    }
    if (!onTarget) {
      s.outcome = 'miss';
      this.stats[side].offTarget += 1;
      this.pens[side].taken += 1;
      this.pens[side].kicks.push(0);
      s.message = `MISSED — ${s.taker.first} ${s.taker.name}`;
      this.banner = { text: b.z > P.GOAL_H ? 'OVER!' : 'WIDE!', sub: `${s.taker.name}`, until: this.t + 1.7 };
      this.push('penaltyMiss', { side, text: `PENALTY ${b.z > P.GOAL_H ? 'skied over the bar' : 'wide'} — ${s.taker.first} ${s.taker.name}` });
      return;
    }
    b.x = gx + dir * 1.0;
    b.vx *= 0.2;
    b.vy *= 0.4;
    s.outcome = 'goal';
    this.stats[side].onTarget += 1;
    this.pens[side].score += 1;
    this.pens[side].taken += 1;
    this.pens[side].kicks.push(1);
    this.netRipple = 1;
    this.shake = 0.55;
    this.flash = 0.6;
    s.message = `GOAL — ${this.teams[side].team.name}`;
    this.banner = { text: 'SCORED', sub: `${s.taker.first} ${s.taker.name}`, until: this.t + 1.6 };
    this.push('penaltyGoal', { side, scorer: `${s.taker.first} ${s.taker.name}`, text: `PENALTY SCORED — ${this.teams[side].team.name}` });
    for (const p of this.teams[side].players) p.celebrate = 1.0 + this.rand() * 0.6;
  }

  pensCheck() {
    const s = this.pens;
    const H = s.home;
    const A = s.away;
    if (s.sudden) return H.taken === A.taken && H.score !== A.score;
    const remH = Math.max(0, 5 - H.taken);
    const remA = Math.max(0, 5 - A.taken);
    if (H.taken >= 5 && A.taken >= 5) {
      if (!s.suddenAnnounced) {
        s.suddenAnnounced = true;
        s.sudden = true;
        this.push('sudden', { text: 'Sudden death!' });
        this.banner = { text: 'SUDDEN DEATH', sub: 'Next miss ends it', until: this.t + 2 };
      }
      return H.score !== A.score && H.taken === A.taken;
    }
    if (H.score > A.score + remA) return true;
    if (A.score > H.score + remH) return true;
    return false;
  }

  pensFinish() {
    const s = this.pens;
    const winner = s.home.score > s.away.score ? 'home' : 'away';
    const loser = winner === 'home' ? 'away' : 'home';
    s.winner = winner;
    this.pens[loser] = this.pens[loser] || { score: 0 };
    this.push('shootoutWon', {
      side: winner,
      text: `${this.teams[winner].team.name} win the shootout ${s[winner].score}-${s[loser].score}`,
    });
    this.banner = {
      text: 'THROUGH',
      sub: `${this.teams[winner].team.name} win ${s[winner].score}-${s[loser].score} on penalties`,
      side: winner,
      until: this.t + 3.2,
    };
    for (const p of this.teams[winner].players) p.celebrate = 3;
    this.finishMatch();
  }

  // ===========================================================================
  //  goals
  // ===========================================================================
  goalFor(side) {
    const conceding = this.other(side).side;
    this.score[side] += 1;
    const sc = this.lastScorer;
    const minute = Math.max(1, Math.floor(this.minute));
    const entry = {
      side,
      scorer: sc ? `${sc.first} ${sc.name}`.replace(/^undefined /, '') : this.teams[side].team.name,
      num: sc ? sc.num : 0,
      minute,
      ownGoal: this.lastOwnGoal,
      penalty: false,
    };
    this.scorers.push(entry);
    this.stats[side].goalsFor += 1;
    this.stats[conceding].goalsAgainst += 1;
    this.lastGoal = entry;
    this.state = 'celebrate';
    this.stateTimer = 2.6;
    this.shake = 1;
    this.flash = 1;
    this.celebrationSide = side;
    const span = this.half >= 3 ? 15 : MIN_PER_SEC_BASE;
    this.pendingHalfEnd = this.minute >= span;
    this.banner = {
      text: this.lastOwnGoal ? 'OWN GOAL' : 'GOAL!',
      sub: `${entry.scorer} ${minute}'  ·  ${this.score.home}-${this.score.away}`,
      side,
      until: this.t + 2.9,
    };
    this.push('goal', {
      side,
      scorer: entry.scorer,
      minute,
      ownGoal: entry.ownGoal,
      text: this.lastOwnGoal ? `Own goal! ${entry.scorer} puts it past his own keeper` : `GOOOAAAL! ${entry.scorer} ${minute}' — ${this.score.home}-${this.score.away}`,
    });
    for (const p of this.teams[side].players) p.celebrate = 1.4 + this.rand() * 1.3;
    for (let i = 0; i < 46; i++) {
      this.fx.push({
        kind: 'confetti',
        x: -P.HL + this.rand() * P.L,
        y: -P.HW + this.rand() * P.W,
        vx: (this.rand() - 0.5) * 4,
        vy: -1 - this.rand() * 3,
        t: -this.rand() * 1.2,
        life: 3,
        c: ['#ffd54a', '#ffffff', this.teams[side].team.colors.jersey, this.teams[side].team.trim][(i * 5) % 4],
      });
    }
    this.kickoffTo = conceding;
    this.lastScorer = null;
    this.lastOwnGoal = false;
  }

  // ===========================================================================
  //  misc
  // ===========================================================================
  possessionPct() {
    const h = this.stats.home.possession;
    const a = this.stats.away.possession;
    const tot = h + a;
    if (tot < 0.5) return 50;
    return (h / tot) * 100;
  }

  shotMapFor(side) {
    return this.fx.filter((f) => f.kind === 'shot');
  }

  scoreline() {
    const p = this.pens ? ` ${this.pens.home.score}-${this.pens.away.score} PEN` : '';
    return `${this.score.home}-${this.score.away}${p}`;
  }
}

function emptyStats() {
  return {
    goalsFor: 0,
    goalsAgainst: 0,
    shots: 0,
    onTarget: 0,
    offTarget: 0,
    blocks: 0,
    saves: 0,
    corners: 0,
    fouls: 0,
    yellow: 0,
    red: 0,
    passTry: 0,
    passOk: 0,
    blocked: 0,
    throughTry: 0,
    throughOk: 0,
    possession: 0,
    setPieces: 0,
  };
}
export { clamp, lerp, dist, nearest, laneClarity, bestMate, match_speed, GRAVITY };
