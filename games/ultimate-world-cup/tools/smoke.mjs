// Headless harness: runs whole matches with no DOM, so the engine can be checked in CI or in
// one command while you work on the rendering layer.
//   node tools/smoke.mjs            quick 20-minute sanity match
//   node tools/smoke.mjs full       a full match per pair of nations, 8 of them
//   node tools/smoke.mjs tourney    a whole tournament, matches simulated
import { Match, DIFFICULTY } from '../src/match.js';
import { TEAMS, TEAM_BY_ID, STADIUMS } from '../src/teams.js';
import { buildTournament, simulateFixture, playTournamentMatch } from '../src/tournament.js';

const mode = process.argv[2] || 'quick';
const dt = 1 / 60;

function runMatch(home, away, opts = {}) {
  const m = new Match({
    home: TEAM_BY_ID[home],
    away: TEAM_BY_ID[away],
    seed: opts.seed ?? 2026,
    difficulty: opts.difficulty || 'world',
    halfSeconds: opts.halfSeconds ?? 60,
    humanTeam: opts.humanTeam ?? null,
    knockout: opts.knockout ?? true,
  });
  const maxFrames = opts.maxFrames ?? 60 * 60 * 4;
  let frames = 0;
  const t0 = Date.now();
  while (frames < maxFrames) {
    const input = { mx: 0, my: 0 };
    if (opts.humanTeam) {
      // pretend a human exists: wiggle the stick, press things at random
      const t = frames / 60;
      input.mx = Math.sin(t * 1.3);
      input.my = Math.cos(t * 0.9);
      input.sprint = Math.sin(t) > 0.7;
      input.shoot = frames % 90 < 12;
      input.pass = frames % 47 === 0;
      input.through = frames % 131 === 0;
      input.tackle = frames % 61 === 0;
      input.sw = frames % 211 === 0;
    }
    m.update(dt, input);
    m.consumeEdges();
    frames++;
    if (m.state === 'halftime') m.resume();
    if (m.state === 'results') break;
  }
  const ms = Date.now() - t0;
  return {
    match: m,
    frames,
    ms,
    mspf: (ms / frames).toFixed(3),
    goals: m.events.filter((e) => e.type === 'goal').length,
    events: m.events.length,
    text: `${home} ${m.score.home}-${m.score.away} ${away}` + (m.pens ? ` (pens ${m.pens.home.score}-${m.pens.away.score})` : '') + `  half=${m.half} state=${m.state}`,
  };
}

let failures = 0;
function check(cond, label, extra = '') {
  if (!cond) {
    failures++;
    console.log(`  FAIL  ${label} ${extra}`);
  } else console.log(`  ok    ${label} ${extra}`);
}

if (mode === 'quick' || mode === 'full') {
  const r = runMatch('BRA', 'GER', { halfSeconds: 45, seed: 11 });
  console.log(r.text);
  console.log(`  frames=${r.frames} engine=${r.mspf}ms/frame  events=${r.events} goals=${r.goals}`);
  const m = r.match;
  check(m.state === 'results' || m.state === 'celebrate', 'match terminates', m.state);
  check(r.goals > 0 && r.goals < 12, 'goal count plausible', String(r.goals));
  check(m.players.every((p) => Math.abs(p.x) < 60 && Math.abs(p.y) < 40), 'players stay on the pitch');
  check(Number.isFinite(m.ball.x) && Number.isFinite(m.ball.y), 'ball is finite');
  check(m.stats.home.shots + m.stats.away.shots > 0, 'shots happened', `${m.stats.home.shots}/${m.stats.away.shots}`);
  console.log('  commentary:', m.commentary.slice(0, 5).map((c) => `${c.minute}' ${c.text}`).join('  |  '));

  // one match per nation pair, look for blowouts, crashes, stuck clocks
  const pairs = [];
  for (let i = 0; i < TEAMS.length; i += 2) pairs.push([TEAMS[i].id, TEAMS[i + 1].id]);
  if (mode === 'full') {
    let worst = 0;
    let worstText = '';
    let totalMs = 0;
    for (const [a, b] of pairs) {
      const x = runMatch(a, b, { halfSeconds: 40, seed: a.charCodeAt(0) * 31 + b.charCodeAt(2) });
      totalMs += x.ms;
      const g = x.match.score.home + x.match.score.away;
      if (g > worst) {
        worst = g;
        worstText = x.text;
      }
      if (x.match.state !== 'results') console.log('  !! did not finish:', x.text);
      console.log('  ', x.text, `(${x.frames} frames)`);
    }
    console.log(`  highest scoring game: ${worst} goals - ${worstText}`);
    console.log(`  engine cost per match pair avg: ${(totalMs / pairs.length).toFixed(0)} ms`);
  }
}

if (mode === 'tourney') {
  const t = buildTournament({ champion: 'USA', seed: 99 });
  const out = playTournamentMatch(t, { simulate: true });
  console.log('  groups built:', t.groups.length, 'groups; next fixture:', t.nextFixture() && t.nextFixture().home, 'v', t.nextFixture() && t.nextFixture().away);
  console.log('  ', out.text || '(none)');
  // simulate the entire tournament quickly with the scoreline model
  const t2 = buildTournament({ champion: 'ARG', seed: 5 });
  let guard = 0;
  while (!t2.done && guard++ < 400) {
    for (const f of t2.roundFixtures()) {
      const res = simulateFixture(t2, f, { user: f.home === 'ARG' || f.away === 'ARG' });
      if (res && res.scorers && res.scorers.some((x) => x.player.includes('undefined'))) check(false, 'scorer name', JSON.stringify(res.scorers));
      if (!Number.isFinite(res.home + res.away)) {
        check(false, 'finite scoreline', JSON.stringify(res));
        break;
      }
    }
    t2.advance();
    if (t2.stage === 'groups' && !t2.roundFixtures().length && t2.matchday === 3) t2.advance();
  }
  check(t2.done, 'tournament completes', `stage=${t2.stage} guard=${guard}`);
  console.log('  champion:', t2.champion, ' runner-up:', t2.runnerUp, ' third:', t2.third, ' fourth:', t2.fourth);
  console.log('  bracket sizes: r16', t2.bracket.r16.length, 'qf', t2.bracket.qf.length, 'sf', t2.bracket.sf.length, 'final', t2.bracket.final.length);
  console.log('  top scorer:', t2.topScorer ? `${t2.topScorer.name} (${t2.topScorer.goals})` : 'n/a');
}

console.log(failures ? `\n${failures} check(s) failed` : '\nall checks passed');
process.exit(failures ? 1 : 0);
