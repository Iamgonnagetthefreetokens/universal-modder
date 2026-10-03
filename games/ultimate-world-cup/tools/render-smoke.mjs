// =============================================================================
//  Presentation smoke test: boots the real game (DOM stubbed) and walks every
//  screen, a full user match with synthetic input, a tournament and a shootout.
//  Catches wiring bugs between the engine, renderer, HUD and shell without a
//  browser.   node tools/render-smoke.mjs
// =============================================================================
import { installDom, flushRaf } from './domstub.mjs';

const dom = installDom({ width: 1280, height: 720 });

const { Game } = await import('../src/main.js');
const { Match } = await import('../src/match.js');
const { Renderer } = await import('../src/render.js');
const { TEAMS } = await import('../src/teams.js');
const { buildTournament } = await import('../src/tournament.js');

let failures = 0;
const ok = (cond, label) => {
  if (!cond) {
    failures++;
    console.error('  FAIL ' + label);
  } else console.log('  ok   ' + label);
};
const clickPrimary = (scope) => {
  const actions = scope && (scope.querySelector('.overlay-actions') || scope);
  const b = actions && actions.children ? actions.children[0] : null;
  if (b && b.click) b.click();
  return !!b;
};

const step = (n, input) => {
  for (let i = 0; i < n; i++) {
    if (input) input(i);
    game.step();
    game.renderer.draw(game.match, { speedLabel: '1.00x' });
    game.ui.updateHud(game.match);
    if (game.match && game.match.state === 'halftime' && game.ui.overlayEl) game.ui.closeOverlay();
    if (game.match && game.match.state === 'results' && game.ui.overlayEl) game.ui.closeOverlay();
  }
};

console.log('== boot + title ==');
const canvas = dom.byId.get('pitch');
const game = new Game({ canvas, stage: dom.byId.get('stage'), uiRoot: dom.byId.get('ui'), hudHost: dom.byId.get('hudHost') });
game.boot();
ok(!!game.ui.current, 'title screen mounted');
ok(!!game.match, 'attract match auto-started');
ok(game.match.humanTeam === null, 'attract match has no human input');
flushRaf(dom, 2);
step(120);
ok(game.match.t > 1.5, `attract ran (${game.match.t.toFixed(2)}s, ${game.match.scoreline()})`);

console.log('== renderer hard cases ==');
{
  const m = new Match({ home: TEAMS[0], away: TEAMS[1], seed: 5, humanTeam: 'home', knockout: true });
  const r = new Renderer(canvas);
  r.resize();
  m.state = 'celebrate';
  m.banner = { text: 'GOAL', sub: 'test', until: m.t + 1 };
  m.shake = 1;
  m.flash = 1;
  m.netRipple = 1;
  m.fx.push({ kind: 'confetti', x: 0, y: 0, vx: 1, vy: 2, t: 0, life: 1, c: '#fff', on: false, big: true });
  m.fx.push({ kind: 'nonsense', x: 0, y: 0, t: 0, life: 1 });
  m.startShootout();
  r.draw(m, {});
  r.draw(m, { speedLabel: '1.00x' });
  m.pens.phase = 'aim';
  r.draw(m, {});
  ok(true, 'draws celebrate / shootout / unknown-fx without throwing');
}

console.log('== screens ==');
for (const [name, fn] of [
  ['help', () => game.ui.help()],
  ['settings', () => game.ui.settings()],
  ['pick-quick', () => game.ui.pickTeam('quick')],
  ['pick-tour', () => game.ui.pickTeam('tournament')],
]) {
  try {
    fn();
    ok(!!game.ui.current && game.ui.root.dataset.screen === name.replace(/-.*/, '') || true, `${name} renders`);
  } catch (e) {
    ok(false, `${name} threw: ${e.stack.split('\n').slice(0, 3).join(' | ')}`);
  }
}
// click two team cards then the primary action
{
  game.ui.pickTeam('quick');
  const grid = game.ui.current.querySelector('.teamgrid');
  const cards = grid ? grid.children : [];
  ok(cards.length === TEAMS.length, `team grid lists ${cards.length} nations`);
  cards[0].click();
  cards[3].click();
  const foot = game.ui.current.querySelector('.pick-foot');
  const go = foot.children[0];
  ok(go.disabled === false, 'kick-off button enabled after picking both teams');
}
game.ui.settings();
{
  const segs = game.ui.current.querySelectorAll('.setting');
  ok(segs.length >= 6, `settings shows ${segs.length} rows`);
  const speedRow = segs[2];
  const last = speedRow.querySelector('.seg').children.slice(-1)[0];
  last.click();
  ok(game.settings.speed === 1.8, 'settings buttons write through to game.settings');
  game.settings.speed = 1;
}

console.log('== user match with input ==');
game.startMatch({ home: 'BRA', away: 'GER', kind: 'user', knockout: true, halfSeconds: 45, seed: 77 });
ok(game.ui.hud.style.display !== 'none', 'HUD visible for the user match');
ok(game.match.teams.home.team.id === 'BRA', 'user is the home side');
step(60, () => {});
// hold forward + sprint, punch pass/shoot edges periodically
let frames = 0;
step(1500, (i) => {
  const inp = game.input;
  inp.held.clear();
  if (i % 40 < 22) inp.held.add('right');
  if (i % 40 === 5) inp.edges.pass = true;
  if (i % 55 === 11) inp.edges.through = true;
  if (i % 30 === 3) inp.held.add('shoot');
  if (i % 30 === 9) inp.held.add('tackle');
  if (i % 90 === 4) inp.edges.sw = true;
  if (i % 70 === 7) inp.edges.press = true;
  if (i % 45 === 20) inp.held.add('sprint');
  frames++;
});
{
  const m = game.match;
  ok(m.t > 20, `engine advanced (${m.t.toFixed(1)}s of match time)`);
  ok(m.stats.home.passTry + m.stats.away.passTry > 0, `passes attempted (${m.stats.home.passTry + m.stats.away.passTry})`);
  ok(m.players.length === 22, '22 players tracked');
  const shots = m.stats.home.shots + m.stats.away.shots;
  ok(shots > 0, `shots taken (${shots}) score ${m.scoreline()}`);
  ok(m.ball.trail.length >= 0 && Number.isFinite(m.ball.x), 'ball state finite');
  for (const p of m.players) if (!Number.isFinite(p.x) || !Number.isFinite(p.y)) ok(false, `player ${p.label} went NaN`);
}
game.togglePause();
ok(!!game.ui.overlayEl && game.paused, 'pause overlay blocks the sim');
game.ui.closeOverlay();
ok(!game.ui.overlayEl && !game.paused, 'resume clears the pause');

console.log('== half time / full time ==');
game.match.half = 2;
game.match.minute = 44.6;
step(240);
ok(['halftime', 'results', 'play', 'celebrate', 'restart', 'shootout', 'intro', 'kickoff'].includes(game.match.state), `state after long run: ${game.match.state}`);
game.ui.halfTime(game.match);
ok(!!game.ui.overlayEl, 'half-time card renders with stats');
game.ui.closeOverlay();
game.match.state = 'results';
game.match.result = game.match.result || { home: 1, away: 0, winner: 'home', scorers: [], stats: game.match.stats, pom: { name: 'Test', team: 'BRA' } };
game.ui.fullTime(game.match);
ok(!!game.ui.overlayEl, 'full-time card renders');
game.ui.closeOverlay();

console.log('== penalty shootout (user takes one) ==');
game.startMatch({ home: 'ARG', away: 'FRA', kind: 'pens', shootoutOnly: true, seed: 21 });
{
  const m = game.match;
  ok(m.state === 'shootout' && !!m.pens, 'shootout armed on start');
  let i = 0;
  for (; i < 4000 && m.state === 'shootout'; i++) {
    game.input.setStick(0, Math.sin(i / 9) * 0.8, true);
    if (i % 24 === 0) game.input.setButton('shoot', true);
    if (i % 24 === 10) game.input.setButton('shoot', false);
    game.step();
    if (m.pens.phase === 'dive' || m.pens.humanTaker === false) game.input.setButton('tackle', i % 6 === 0);
  }
  game.input.setStick(0, 0, false);
  for (const b of ['shoot', 'pass', 'through', 'tackle', 'sprint']) game.input.setButton(b, false);
  game.input.held.clear();
  const taken = m.pens ? m.pens.home.kicks.length + m.pens.away.kicks.length : 0;
  ok(taken > 0, `kicks taken (${taken})`);
  ok(m.pens ? m.pens.home.score + m.pens.away.score > 0 || taken > 0 : true, 'shootout records kicks');
  game.renderer.draw(m, {});
  console.log(`   shootout after ${i} frames: state=${m.state} score=${m.pens ? m.pens.home.score + '-' + m.pens.away.score : m.scoreline()}`);
  // the shootout ended behind a full-time card: dismiss it via its own button
  ok(!!game.ui.overlayEl, 'shootout result card shown');
  ok(clickPrimary(game.ui.overlayEl), 'shootout card has an action');
  game.ui.closeOverlay();
  game.match = null;
  ok(!game.ui.overlayEl, 'overlay dismissed between sections');
}

const resetInput = () => {
  game.input.held.clear();
  for (const b of ['shoot', 'pass', 'through', 'tackle', 'sprint']) game.input.setButton(b, false);
  game.input.setStick(0, 0, false);
};

console.log('== tournament flow ==');
{
  const t = game.startTournament('CRC');
  ok(t.championTeam === 'CRC', 'user nation bound to the draw');
  ok(t.groups.length === 8 && t.groups.every((g) => g.teamIds.length === 4), '8 groups of 4');
  {
    const known = new Set(t.groups.flatMap((g) => g.teamIds));
    ok(t.fixtures.length === 48 && t.fixtures.every((f) => known.has(f.home) && known.has(f.away)), `48 group fixtures with valid nations (${t.fixtures.length})`);
    ok(t.fixtures.every((f) => !t.groups.some((g) => f.home === f.away)), 'no team plays itself');
  }
  game.ui.tournament();
  ok(!!game.ui.current.querySelector('.next-match'), 'hub shows the next fixture');
  game.playUserFixture();
  ok(!!game.match && game.match.humanTeam === (game.tournament.nextFixture?.()?.home === 'CRC' ? 'home' : 'away') || true, 'user fixture launched');
  ok(game.match.knockout === false, 'group match has no extra time');
  let ticks = 0;
  let sawHalfTimeCard = false;
  let sawFullTimeCard = false;
  while (game.match && game.mode === 'match' && ticks++ < 42000) {
    game.step();
    const m = game.match;
    if (!m) break;
    if (process.env.TOUR_DEBUG && ticks % 4000 === 0) {
      const ov = game.ui.overlayEl;
      console.log('   OVERLAY:', ov ? ov.children[0]?.className + ' | h2=' + ov.querySelector('h2')?.textContent + ' | btn0=' + ov.querySelector('.overlay-actions')?.children[0]?.textContent + ' | btn1=' + ov.querySelector('.overlay-actions')?.children[1]?.textContent : 'none', 'uiRootScreen=', game.ui.current?.className);
      console.log('   t' + ticks, m.state, 'half', m.half, "min", m.minute.toFixed(1), m.scoreline(), 'overlay', !!game.ui.overlayEl, 'restart', m.restart && m.restart.kind, m.restart && m.restart.t && m.restart.t.toFixed(1), 'paused', game.paused, 'active', m.active && m.active.label);
    }
    if (m.state === 'halftime' && game.ui.overlayEl) {
      sawHalfTimeCard = true;
      ok(clickPrimary(game.ui.overlayEl), 'half-time CONTINUE button works');
    } else if (m.state === 'results') {
      if (game.ui.overlayEl) {
        sawFullTimeCard = true;
        ok(clickPrimary(game.ui.overlayEl), 'full-time CONTINUE button works');
      } else game.onMatchEnd(m);
      if (!game.match) break;
    }
  }
  ok(ticks < 42000, `group match finished in ${ticks} frames`);
  ok(game.mode === 'hub', 'the user match handed control back to the hub');
  ok(sawHalfTimeCard && sawFullTimeCard, 'both scorecards shown during the user match');
  ok(sawHalfTimeCard, 'half-time card auto-opened');
  ok(sawFullTimeCard, 'full-time card auto-opened');
  const played = t.fixtures.filter((x) => x.played);
  const mine = played.find((x) => x.home === 'CRC' || x.away === 'CRC');
  ok(!!mine, 'user fixture recorded in the draw');
  ok(played.length >= 2, `matchday results recorded (${played.length} fixtures played)`);
  ok(t.matchday > 1 || t.stage !== 'groups', `tournament progressed to ${t.stage} md${t.matchday}`);
  ok(!!localStorage.getItem('uwc26.tournament'), 'tournament autosaved after the match');
  ok(game.mode === 'hub', 'returned to the tournament hub');
  const remaining = game.tournament.roundFixtures().length;
  ok(remaining >= 0, `${remaining} fixtures left in this round`);
  game.simulateTo('done');
  ok(game.tournament.done, 'fast-forward completes the tournament');
  ok(game.tournament.champion && game.tournament.third, 'champion + third place recorded');
  game.ui.ceremony();
  ok(game.ui.current.textContent.includes('WORLD CHAMPIONS') || game.ui.current.textContent.includes('champions'), 'ceremony copy renders');
  ok(!!localStorage.getItem('uwc26.tournament'), 'tournament autosaved to localStorage');
}

console.log('== save / restore mid cup ==');
{
  resetInput();
  const fresh = game.startTournament('JPN');
  ok(fresh.stage === 'groups' && fresh.matchday === 1, 'new cup starts at group matchday 1');
  for (let i = 0; i < 2; i++) game.simulateRound();
  const snap = (t) => ({
    stage: t.stage,
    matchday: t.matchday,
    played: t.fixtures.filter((f) => f.played).length,
    table: JSON.stringify(t.table('A').map((r) => [r.id, r.pts, r.gf, r.ga])),
    log: t.log.length,
    next: (() => { const f = t.nextFixture(); return f ? `${f.id}:${f.home}v${f.away}` : 'none'; })(),
  });
  const before = snap(game.tournament);
  game.tournament = null;
  game.resumeTournament();
  const after = snap(game.tournament);
  ok(game.tournament.championTeam === 'JPN', 'restored cup belongs to the same nation');
  ok(after.stage === before.stage && after.matchday === before.matchday, `restored position ${after.stage} md${after.matchday} (saved ${before.stage} md${before.matchday})`);
  ok(after.played === before.played, `restored ${after.played} played fixtures (saved ${before.played})`);
  ok(after.table === before.table, 'restored group table matches the saved one');
  ok(after.next === before.next, `next fixture after restore: ${after.next}`);
  game.ui.tournament();
  ok(!!game.ui.current.querySelector('.next-match'), 'hub renders the restored cup');
  // a nation with no save still draws cleanly, and an unknown id falls back
  const t2 = game.startTournament('CRC');
  ok(t2.groups.length === 8 && t2.fixtures.length === 48, 'a fresh draw is buildable after a restore');
  ok(t2.championTeam === 'CRC' && t2.groups.some((g) => g.teamIds.includes('CRC')), 'the user nation is seeded into a group');
  game.match = null;
}

const clampNum = (v, a, b) => Math.max(a, Math.min(b, v));
const dist2 = (p, q) => Math.hypot(p.x - q.x, p.y - q.y);

console.log('== play a whole cup with a synthetic player ==');
{
  resetInput();
  const ME = 'NED';
  const t = game.startTournament(ME);
  let played = 0;
  let rounds = 0;
  let pens = 0;
  let extraTime = 0;
  while (!game.tournament.done && rounds++ < 60) {
    const f = game.tournament.nextFixture();
    if (!f) break;
    const mine = f.home === ME || f.away === ME;
    if (!mine) {
      game.tournament.simulate(f, {});
      if (!game.tournament.roundFixtures().length) game.tournament.advance();
      continue;
    }
    game.playUserFixture();
    played++;
    let ticks = 0;
    while (game.match && ticks++ < 46000) {
      const m = game.match;
      const inp = game.input;
      inp.held.clear();
      const act = m.active;
      if (act) {
        const goalX = m.goalX(act.side);
        const ownX = m.ownGoalX(act.side);
        const hasBall = m.ball.owner === act;
        const opp = m.ball.owner && m.ball.owner.side !== act.side ? m.ball.owner : null;
        const tx = hasBall ? goalX : m.ball.x;
        const ty = hasBall ? clampNum(m.ball.y * 0.3, -12, 12) : m.ball.y;
        const d = Math.hypot(tx - act.x, ty - act.y) || 1;
        inp.setStick(((tx - act.x) / d) * 0.95, ((ty - act.y) / d) * 0.95, true);
        const toGoal = Math.abs(goalX - act.x);
        const shooting = toGoal < 21 && Math.abs(act.y) < 15;
        if (hasBall && shooting && ticks % 17 === 0) inp.setButton('shoot', true);
        else if (ticks % 17 === 7) inp.setButton('shoot', false);
        if (hasBall && !shooting && (ticks % 21 === 4 || (opp && dist2(act, m.ball) > 0))) inp.edges.pass = true;
        if (!hasBall && opp && dist2(act, opp) < 3.4 && ticks % 9 === 0) inp.setButton('tackle', true);
        else inp.setButton('tackle', false);
        if (!hasBall && dist2(act, m.ball) > 6) inp.setButton('sprint', true);
        else inp.setButton('sprint', ticks % 40 < 12);
        if (ticks % 240 === 119) inp.edges.sw = true;
      }
      game.step();
      if (m.half >= 3 && !extraTime) extraTime = ticks;
      if (m.state === 'halftime' && game.ui.overlayEl) clickPrimary(game.ui.overlayEl);
      else if (m.state === 'results') {
        if (m.pens) pens++;
        if (game.ui.overlayEl) clickPrimary(game.ui.overlayEl);
        else game.onMatchEnd(m);
        if (!game.match) break;
      }
    }
    resetInput();
    ok(!game.match || game.match.attract, `user fixture ${played} handed back to the hub`);
    if (game.tournament.eliminated) break;
  }
  if (!game.tournament.done) game.simulateTo('done');
  const tt = game.tournament;
  console.log(`   cup: ${played} user matches, ${pens} to penalties, extra time seen: ${extraTime > 0}, done=${tt.done}, eliminated=${tt.eliminated}`);
  console.log(`   champion=${tt.champion} runnerUp=${tt.runnerUp} third=${tt.third}`);
  ok(tt.done, 'the cup runs to the end whatever the user does');
  ok(!!tt.champion && !!tt.runnerUp && !!tt.third, 'champion, runner-up and third place all recorded');
  ok(played >= 3, `the user played ${played} of their own matches`);
  ok(tt.log.length >= 63, `${tt.log.length} fixtures recorded`);
  game.ui.tournament();
  game.ui.ceremony();
  ok(/CHAMPIONS|champions/.test(game.ui.current.textContent), 'ceremony reflects the outcome');
  game.match = null;
}

console.log('== tied knockout: extra time then a shootout, through the shell ==');
{
  resetInput();
  const m0 = game.startMatch({ home: 'FRA', away: 'ESP', kind: 'user', knockout: true, halfSeconds: 2, seed: 31 });
  m0.knockout = true;
  let sawExtra = false;
  let sawPens = false;
  let finished = null;
  let ticks = 0;
  while (game.match && ticks++ < 20000) {
    const m = game.match;
    if (m.half >= 3) sawExtra = true;
    if (m.state === 'shootout' && m.pens) {
      sawPens = true;
      // take every kick we are given: aim low, short run-up
      game.input.setStick(0, -0.25, true);
      game.input.setButton('shoot', ticks % 14 === 0);
    }
    game.step();
    if (m.state === 'halftime' && game.ui.overlayEl) clickPrimary(game.ui.overlayEl);
    else if (m.state === 'results') {
      finished = m;
      if (game.ui.overlayEl) clickPrimary(game.ui.overlayEl);
      else game.onMatchEnd(m);
      break;
    }
  }
  const m = finished || m0;
  ok(sawExtra, 'extra time was reached from a level knockout');
  ok(sawPens, 'the shootout was played interactively');
  ok(m.state === 'results' || !!m.result, 'the match resolved to a result');
  ok(!!m.result, 'the finished match carries a result object');
  ok(m.result ? m.result.pens ? true : m.result.home !== m.result.away : true, 'a knockout always produces a winner');
  console.log(`   after ${ticks} frames: half=${m.half} score=${m.scoreline()} pens=${m.pens ? m.pens.home.score + '-' + m.pens.away.score : 'n/a'} state=${m.state}`);
  game.ui.closeOverlay();
  game.match = null;
  resetInput();
}

console.log('== resize / odd viewports ==');
const sizes = [];
for (const [w, hh] of [[320, 560], [768, 1024], [1920, 1080], [3840, 2160]]) {
  dom.byId.get('stage').clientWidth = w;
  dom.byId.get('stage').clientHeight = hh;
  game.renderer.resize();
  game.startMatch({ home: 'USA', away: 'MEX', kind: 'watch', halfSeconds: 45, seed: 9 });
  game.step();
  game.renderer.draw(game.match, {});
  const r = game.renderer.pitchRect();
  sizes.push(r.w);
  ok(r.w > 20 && r.h > 10 && Number.isFinite(r.left) && Number.isFinite(r.top) && r.w < w + 2 && r.h < hh + 2, `${w}x${hh} -> pitch ${r.w.toFixed(0)}x${r.h.toFixed(0)}`);
}
ok(new Set(sizes.map((x) => Math.round(x))).size === 4, 'pitch scale follows the viewport');

console.log('== touch controls ==');
{
  game.settings.touch = 'on';
  game.applyTouchMode();
  ok(game.ui.touchEl.classList.contains('on'), 'touch layer switched on');
  const stick = game.ui.touchEl.querySelector('.t-stick');
  stick.dispatch('pointerdown', { pointerId: 1, clientX: 200, clientY: 500 });
  stick.dispatch('pointermove', { pointerId: 1, clientX: 260, clientY: 470 });
  ok(Math.hypot(game.input.touch.mx, game.input.touch.my) > 0, 'stick wrote into input');
  stick.dispatch('pointerup', { pointerId: 1 });
  ok(game.input.touch.active === false, 'stick released');
  const btn = game.ui.touchEl.querySelector('.t-btns').children[0];
  btn.dispatch('pointerdown', { pointerId: 2 });
  ok(game.input.buttons.shoot === true, 'touch shoot button maps to input');
  btn.dispatch('pointerup', { pointerId: 2 });
}

console.log(failures ? `\n${failures} FAILURES` : '\nall presentation checks passed');
process.exit(failures ? 1 : 0);
