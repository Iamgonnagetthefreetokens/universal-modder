// =============================================================================
//  ULTIMATE WORLD CUP - application shell.
//  Owns the loop, the screens, the sound cues and the tie-in with the
//  tournament. The match itself lives entirely in src/match.js.
// =============================================================================
import { Match } from './match.js';
import { Renderer } from './render.js';
import { UI, h } from './ui.js';
import { Input } from './input.js';
import { sound } from './audio.js';
import { TEAMS, TEAM_BY_ID, STADIUMS } from './teams.js';
import { buildTournament, playTournamentMatch, applyFixture, topScorer } from './tournament.js';

const STEP = 1 / 60;
/** what a CPU-only match is fed: nothing, the engine ignores it */
const EMPTY_INPUT = Object.freeze({ mx: 0, my: 0, sprint: false, shoot: false, pass: false, through: false, tackle: false, sw: false, press: false });
const SAVE_KEY = 'uwc26.settings';
const TOUR_KEY = 'uwc26.tournament';

export class Game {
  constructor({ canvas, stage, uiRoot, hudHost }) {
    this.canvas = canvas;
    this.stage = stage;
    this.renderer = new Renderer(canvas);
    this.input = new Input();
    this.ui = new UI(uiRoot, this);
    this.ui.buildHud(hudHost);
    this.ui.buildTouch(hudHost);
    this.settings = {
      difficulty: 'pro',
      halfSeconds: 90,
      speed: 1,
      sound: 'on',
      touch: 'auto',
      commentary: 'on',
      lastTournament: false,
    };
    this.loadSettings();
    this.match = null;
    this.tournament = null;
    this.isTournament = false;
    this.matchKind = 'watch';
    this.paused = false;
    this.acc = 0;
    this.lastTs = 0;
    this.fps = 60;
    this.mode = 'boot';
    this.crowdLevel = 0.12;
    this.lastMatchConfig = null;

    window.addEventListener('resize', () => this.renderer.resize());
    this.input.onPause = () => {
      if (this.mode === 'match' && this.match) this.togglePause();
    };
    this.input.onAnyKey = (code) => {
      // any key press unlocks WebAudio in browsers that need a gesture
      sound.start();
    };
    if (this.settings.sound === 'off') sound.setMuted(true);
    this.applyTouchMode();
  }

  // -------------------------------------------------------------- persistence
  loadSettings() {
    try {
      const raw = localStorage.getItem(SAVE_KEY);
      if (raw) Object.assign(this.settings, JSON.parse(raw));
    } catch (e) {}
  }

  saveSettings() {
    try {
      localStorage.setItem(SAVE_KEY, JSON.stringify(this.settings));
    } catch (e) {}
  }

  saveTournament() {
    try {
      if (!this.tournament) return;
      const t = this.tournament;
      const played = t.fixtures.concat(...Object.values(t.bracket))
        .filter((f) => f && f.played)
        .map((f) => ({ id: f.id, h: f.h, a: f.a, pens: f.pens || null, scorers: f.scorers || [] }));
      localStorage.setItem(TOUR_KEY, JSON.stringify({ seed: t.seed, championTeam: t.championTeam, matchday: t.matchday, stage: t.stage, played, goals: t.records.goals, log: t.log.slice(-40) }));
      this.settings.lastTournament = true;
      this.saveSettings();
    } catch (e) {}
  }

  resumeTournament() {
    let data = null;
    try {
      data = JSON.parse(localStorage.getItem(TOUR_KEY) || 'null');
    } catch (e) {}
    if (!data) {
      this.pickTeam('tournament');
      return;
    }
    this.startTournament(data.championTeam, data);
  }

  touchWanted() {
    return this.settings.touch === 'on' || (this.settings.touch === 'auto' && matchMedia('(pointer: coarse)').matches);
  }

  applyTouchMode() {
    this.ui.setTouchVisible(this.touchWanted());
  }

  toggleSound() {
    this.settings.sound = this.settings.sound === 'on' ? 'off' : 'on';
    sound.setMuted(this.settings.sound === 'off');
    this.saveSettings();
  }

  // ------------------------------------------------------------------- boot
  boot() {
    this.ui.title();
    this.mode = 'title';
    this.startAttract();
    requestAnimationFrame((ts) => this.loop(ts));
  }

  toTitle() {
    this.match = null;
    this.paused = false;
    this.ui.closeOverlay();
    this.mode = 'title';
    this.startAttract();
    this.ui.showHud(false);
    this.ui.title();
  }

  reopen() {
    if (this.isTournament && this.tournament) this.ui.tournament();
    else if (this.mode === 'title') this.ui.title();
    else this.ui.tournament();
  }

  /** leaving the settings screen: back to a live match, the cup hub, or the title */
  settingsChanged() {
    this.applyTouchMode();
    if (this.mode === 'match' && this.match) {
      if (this.ui.overlayEl) this.ui.closeOverlay();
      else this.togglePause();
      return;
    }
    this.reopen();
  }

  // -------------------------------------------------------------- attract mode
  startAttract() {
    const a = TEAMS[Math.floor(Math.random() * TEAMS.length)];
    let b = TEAMS[Math.floor(Math.random() * TEAMS.length)];
    while (b.id === a.id) b = TEAMS[Math.floor(Math.random() * TEAMS.length)];
    const m = this.startMatch({ home: a.id, away: b.id, kind: 'watch', halfSeconds: 240, quiet: true });
    if (m) m.attract = true;
    this.attractMatch = m;
    this.ui.showHud(false);
  }

  watchCpu() {
    const a = TEAMS[Math.floor(Math.random() * TEAMS.length)];
    let b = TEAMS[Math.floor(Math.random() * TEAMS.length)];
    while (b.id === a.id) b = TEAMS[Math.floor(Math.random() * TEAMS.length)];
    this.startMatch({ home: a.id, away: b.id, kind: 'watch', knockout: true });
    this.ui.showHud(true, this.match);
    this.mode = 'match';
  }

  // -------------------------------------------------------------- match setup
  startMatch(cfg) {
    const home = typeof cfg.home === 'string' ? TEAM_BY_ID[cfg.home] : cfg.home;
    const away = typeof cfg.away === 'string' ? TEAM_BY_ID[cfg.away] : cfg.away;
    const humanTeam = cfg.kind === 'watch' ? null : cfg.humanTeam || 'home';
    const venue = cfg.venue ? STADIUMS.find((s) => s.id === cfg.venue) || STADIUMS[0] : STADIUMS[Math.floor(Math.random() * STADIUMS.length)];
    const seed = cfg.seed ?? Math.floor(Math.random() * 1e9);
    const match = new Match({
      home,
      away,
      seed,
      difficulty: this.settings.difficulty,
      halfSeconds: cfg.halfSeconds || this.settings.halfSeconds,
      knockout: !!cfg.knockout,
      humanTeam,
      shootoutUserKeeper: true,
      venue,
      attendance: venue.cap,
    });
    match.venue = venue;
    match.onMatchEnd = cfg.onMatchEnd || null;
    this.match = match;
    this.matchKind = cfg.kind || 'user';
    this.lastMatchConfig = cfg;
    if (cfg.shootoutOnly) {
      match.half = 5;
      match.startShootout();
    }
    this.paused = false;
    if (!cfg.quiet) {
      this.mode = 'match';
      this.ui.showHud(true, match);
      this.ui.clear();
    }
    this.ui.setTouchVisible(match.humanTeam !== null && this.touchWanted());
    sound.start();
    return match;
  }

  restartMatch() {
    const cfg = this.lastMatchConfig;
    if (!cfg) return;
    this.ui.closeOverlay();
    this.startMatch({ ...cfg, seed: Math.floor(Math.random() * 1e9) });
  }

  quitMatch() {
    this.ui.closeOverlay();
    this.match = null;
    if (this.isTournament) {
      this.mode = 'hub';
      this.startAttract();
      this.ui.tournament();
      return;
    }
    this.toTitle();
  }

  togglePause() {
    if (this.mode !== 'match') return;
    if (this.ui.overlayEl) {
      this.ui.closeOverlay();
      return;
    }
    this.paused = true;
    this.ui.overlay(() => this.ui.pause(this.match), { dismissable: true });
    sound.crowdLevel(0.05);
  }

  // -------------------------------------------------------------- tournament
  startTournament(teamId, restore = null) {
    const t = buildTournament({ champion: teamId, seed: restore ? restore.seed : Math.floor(Math.random() * 99999) });
    if (restore && restore.played) this.replayResults(t, restore);
    this.tournament = t;
    this.isTournament = true;
    this.matchKind = 'hub';
    this.tournamentTeamId = teamId;
    this.mode = 'hub';
    this.match = null;
    this.ui.showHud(false);
    this.saveTournament();
    this.ui.tournament();
    return t;
  }

  /**
   * Rebuilding a cup from localStorage: replay each stored result through the same applier the
   * simulation uses, opening the next round whenever the stored results empty the current one.
   */
  replayResults(t, restore) {
    const lookup = (id) =>
      t.fixtures.find((f) => f.id === id) ||
      Object.values(t.bracket)
        .flat()
        .find((f) => f && f.id === id) ||
      null;
    for (const r of restore.played || []) {
      const f = lookup(r.id);
      if (!f || f.played) continue;
      applyFixture(t, f, { home: r.h, away: r.a, scorers: r.scorers || [], pens: r.pens || null, user: false });
      let guard = 0;
      while (!t.roundFixtures().length && !t.done && guard++ < 12) t.advance();
    }
    for (const [id, n] of Object.entries(restore.goals || {})) t.records.goals[id] = Math.max(t.records.goals[id] || 0, n);
    t.topScorer = topScorer(t);
  }

  playUserFixture() {
    const t = this.tournament;
    const f = t.nextFixture();
    if (!f) return;
    const userHome = f.home === t.championTeam;
    const knockout = f.stage !== 'groups';
    this.startMatch({
      home: f.home,
      away: f.away,
      kind: 'user',
      knockout,
      venue: f.venue,
      seed: hashId(f.id) + t.seed,
      humanTeam: userHome ? 'home' : 'away',
      fixtureId: f.id,
    });
  }

  simulateRound() {
    const t = this.tournament;
    const f = t.nextFixture();
    if (f) {
      playTournamentMatch(t, { simulate: true });
      this.match = null;
      this.saveTournament();
      this.ui.tournament();
      this.checkTourDone();
    }
  }

  simulateTo(stage) {
    const t = this.tournament;
    let guard = 0;
    while (!t.done && guard++ < 200) {
      for (const f of t.roundFixtures()) t.simulate(f, {});
      t.advance();
    }
    this.saveTournament();
    if (stage === 'done') this.ui.ceremony();
    else this.ui.tournament();
  }

  checkTourDone() {
    const t = this.tournament;
    if (t.done) {
      this.paused = true;
      this.ui.overlay(() =>
        h(
          'div',
          { class: 'card narrow center' },
          h('h2', {}, t.champion === t.championTeam ? 'WORLD CHAMPIONS!' : 'TOURNAMENT OVER'),
          h('p', { class: 'sub' }, t.champion === t.championTeam ? 'Your nation lifted the trophy.' : `${TEAM_BY_ID[t.champion] ? TEAM_BY_ID[t.champion].name : 'Another nation'} won it.`),
          h('div', { class: 'overlay-actions' }, h('button', { class: 'btn btn-primary', onclick: () => { this.ui.closeOverlay(); this.ui.ceremony(); } }, 'TROPHY CEREMONY'))
        )
      );
    }
  }

  // ------------------------------------------------------------ match endgame
  matchEndButtons() {
    const out = [];
    if (this.isTournament && this.matchKind === 'user') {
      out.push(h('button', { class: 'btn btn-primary', onclick: () => this.applyMatchToTournament() }, this.tournament && !this.tournament.done ? 'CONTINUE THE CUP' : 'SEE THE FINAL STANDINGS'));
      out.push(h('button', { class: 'btn btn-ghost', onclick: () => this.quitMatch() }, 'BACK TO TOURNAMENT'));
    } else if (this.matchKind === 'pens') {
      out.push(h('button', { class: 'btn btn-primary', onclick: () => this.startMatch({ ...this.lastMatchConfig, seed: Math.floor(Math.random() * 1e9) }) }, 'AGAIN'));
      out.push(h('button', { class: 'btn btn-ghost', onclick: () => this.toTitle() }, 'MAIN MENU'));
    } else if (this.matchKind === 'user') {
      out.push(h('button', { class: 'btn btn-primary', onclick: () => this.startMatch({ ...this.lastMatchConfig, seed: Math.floor(Math.random() * 1e9) }) }, 'REMATCH'));
      out.push(h('button', { class: 'btn btn-ghost', onclick: () => this.pickTeam('quick') }, 'CHANGE TEAMS'));
      out.push(h('button', { class: 'btn btn-ghost', onclick: () => this.toTitle() }, 'MAIN MENU'));
    } else {
      out.push(h('button', { class: 'btn btn-primary', onclick: () => this.toTitle() }, 'MAIN MENU'));
    }
    return out;
  }

  applyMatchToTournament() {
    const m = this.match;
    const t = this.tournament;
    this.ui.closeOverlay();
    if (m && t) {
      const f = t.fixtures.concat(...Object.values(t.bracket)).find((x) => x.id === (this.lastMatchConfig && this.lastMatchConfig.fixtureId));
      const mine = f && !f.played ? f : t.nextFixture();
      const result = {
        home: m.result ? m.result.home : m.score.home,
        away: m.result ? m.result.away : m.score.away,
        scorers: (m.scorers || []).map((s) => ({ side: s.side, team: s.side === 'home' ? (mine ? mine.home : 'home') : (mine ? mine.away : 'away'), player: s.scorer, minute: s.minute, penalty: s.penalty })),
        pens: m.pens ? { home: m.pens.home.score, away: m.pens.away.score } : null,
      };
      if (mine) {
        applyFixture(t, mine, { ...result, user: true });
        playTournamentMatch(t, { simulate: false, result: null, restOnly: true });
      }
    }
    this.saveTournament();
    this.match = null;
    this.mode = 'hub';
    this.startAttract();
    this.ui.tournament();
    this.checkTourDone();
  }

  continueMatch() {
    this.ui.closeOverlay();
    if (!this.match) return;
    if (this.match.state === 'halftime') this.match.resume();
    if (this.match.state === 'results') this.onMatchEnd(this.match);
  }

  onMatchEnd(m) {
    if (m.state !== 'results') return;
    if (m.onMatchEnd) {
      m.onMatchEnd(m);
      return;
    }
    if (this.matchKind === 'watch') {
      this.toTitle();
      return;
    }
    // show the scorecard; its primary button is what applies the result
    if (!this.ui.overlayEl) this.ui.fullTime(m);
  }

  // ---------------------------------------------------------------- loop
  step() {
    const m = this.match;
    if (!m) return;
    const demo = !!m.attract;
    const inp = demo ? EMPTY_INPUT : this.input.poll();
    m.update(STEP, inp);
    m.consumeEdges();
    if (!demo) {
      this.handleEvents(m);
      this.crowd(m);
    }
    if (m.state === 'halftime') {
      if (demo) m.resume();
      else if (!this.ui.overlayEl) this.ui.halfTime(m);
    }
    if (m.state === 'results' && !this.ui.overlayEl) {
      if (demo) this.startAttract();
      else this.onMatchEnd(m);
    }
  }

  handleEvents(m) {
    if (!m.events.length) return;
    for (const e of m.events.splice(0)) {
      switch (e.type) {
        case 'goal':
          sound.goal();
          sound.blip({ freq: 880, type: 'triangle', dur: 0.5, gain: 0.14, slide: 240 });
          break;
        case 'save':
          sound.save();
          break;
        case 'whistle':
          sound.whistle(e.why === 'full time' || e.why === 'half 1 ended');
          break;
        case 'post':
          sound.post();
          break;
        case 'blocked':
          sound.noise({ dur: 0.16, freq: 700, q: 0.8, gain: 0.14, sweep: -400 });
          break;
        case 'shotKick':
          sound.kick(0.85);
          break;
        case 'kick':
          if (e.kind === 'shot' || e.kind === 'freekick' || e.kind === 'penalty') sound.kick(0.75);
          else if (e.kind === 'cross' || e.kind === 'through') sound.kick(0.45);
          else sound.pass();
          break;
        case 'yellow':
        case 'red':
          sound.card();
          break;
        case 'corner':
          sound.blip({ freq: 520, type: 'triangle', dur: 0.1, gain: 0.07 });
          break;
        case 'penaltyGoal':
          sound.goal();
          break;
        case 'penaltySave':
        case 'penaltyMiss':
          sound.blip({ freq: 240, type: 'sawtooth', dur: 0.3, gain: 0.1, slide: -120 });
          break;
        default:
          break;
      }
      if (e.text) this.ui.pushCommentary(e.text, e.minute || Math.round(m.minute));
    }
  }

  crowd(m) {
    const b = m.ball;
    const near = Math.min(Math.abs(b.x - P_HL), Math.abs(b.x + P_HL));
    const box = near < 20 ? 0.55 : 0.15;
    const level = m.state === 'celebrate' ? 1 : m.state === 'results' ? 0.8 : m.state === 'shootout' ? 0.5 : box + (b.owner ? 0.12 : 0);
    this.crowdLevel += (level - this.crowdLevel) * 0.05;
    sound.crowdLevel(this.crowdLevel);
  }

  loop(ts) {
    const raw = Math.min(0.1, (ts - this.lastTs) / 1000 || 0);
    this.lastTs = ts;
    this.fps = this.fps * 0.9 + (raw > 0 ? 1 / raw : 60) * 0.1;
    if (this.mode !== 'match' || !this.paused) {
      this.acc += raw * (this.mode === 'match' ? Number(this.settings.speed) || 1 : 1);
      let guard = 0;
      while (this.acc >= STEP && guard++ < 5) {
        this.step();
        this.acc -= STEP;
      }
      if (guard >= 5) this.acc = 0;
    }
    if (this.match) {
      this.renderer.draw(this.match, { speedLabel: `${Number(this.settings.speed).toFixed(2)}×  ${Math.round(this.fps)}fps` });
      this.ui.updateHud(this.match);
    } else {
      this.renderer.ctx.save();
      this.renderer.ctx.setTransform(1, 0, 0, 1, 0, 0);
      this.renderer.ctx.fillStyle = '#05070c';
      this.renderer.ctx.fillRect(0, 0, this.canvas.width, this.canvas.height);
      this.renderer.ctx.restore();
    }
    requestAnimationFrame((t) => this.loop(t));
  }
}

const P_HL = 52.5;

function hashId(str) {
  let v = 2166136261;
  for (let i = 0; i < str.length; i++) {
    v ^= str.charCodeAt(i);
    v = Math.imul(v, 16777619);
  }
  return v >>> 0;
}
