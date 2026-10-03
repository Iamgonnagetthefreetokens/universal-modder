// =============================================================================
//  The tournament: draw, group stage, knockout bracket, results, records.
//  Pure data + maths, no DOM: the browser shell reads this, the headless harness
//  (../tools/smoke.mjs) runs a whole World Cup through it.
// =============================================================================
import { TEAMS, TEAM_BY_ID, squadFor, xgFor, STADIUMS, teamStrength } from './teams.js';
import { mulberry32 } from './match.js';

const clamp = (v, a, b) => Math.max(a, Math.min(b, v));

export const STAGES = ['groups', 'r16', 'qf', 'sf', 'third', 'final', 'done'];
export const STAGE_LABEL = {
  groups: 'Group stage',
  r16: 'Round of 16',
  qf: 'Quarter-final',
  sf: 'Semi-final',
  third: 'Third place',
  final: 'FINAL',
  done: 'Trophy',
};

/**
 * Draw 32 nations into 8 groups of four, one per pot, with the chosen team seeded in
 * Group A as the host. Returns a plain object, so it can be saved to localStorage.
 */
export function buildTournament({ champion, seed = 2026, teams = TEAMS, userSide = 'home' } = {}) {
  const rng = mulberry32(((seed | 0) ^ 0x51ed270b) >>> 0);
  const ids = teams.map((t) => t.id);
  const pots = [1, 2, 3, 4].map((p) => ids.filter((id) => TEAM_BY_ID[id].pot === p).slice());
  for (const pot of pots) shuffle(pot, rng);
  const groups = [];
  const champ = champion && TEAM_BY_ID[champion] ? champion : ids[0];
  for (const pot of pots) {
    const i = pot.indexOf(champ);
    if (i > 0) [pot[0], pot[i]] = [pot[i], pot[0]];
  }
  for (let g = 0; g < 8; g++) {
    groups.push({ id: 'ABCDEFGH'[g], teamIds: [pots[0][g], pots[1][g], pots[2][g], pots[3][g]] });
  }
  const t = {
    seed,
    championTeam: champ,
    userSide,
    rng,
    stage: 'groups',
    matchday: 1,
    groups,
    bracket: { r16: [], qf: [], sf: [], third: [], final: [] },
    records: { goals: {}, apps: {} },
    done: false,
    eliminated: false,
    champion: null,
    runnerUp: null,
    third: null,
    log: [],
    _rng: rng,
    form: Object.fromEntries(ids.map((id) => [id, 0])),
    stats: Object.fromEntries(ids.map((id) => [id, { played: 0, w: 0, d: 0, l: 0, gf: 0, ga: 0, pts: 0 }])),
  };
  buildGroupFixtures(t);
  t.roundFixtures = () => roundFixtures(t);
  t.nextFixture = () => nextFixture(t);
  t.advance = () => advance(t);
  t.table = (gid) => groupTable(t, gid);
  t.simulate = (f, o) => simulateFixture(t, f, o);
  t.playMatch = (o) => playTournamentMatch(t, o);
  t.venueFor = (f) => fixtureVenue(t, f);
  t.isAlive = () => userAlive(t);
  return t;
}

function shuffle(a, rng) {
  for (let i = a.length - 1; i > 0; i--) {
    const j = Math.floor(rng() * (i + 1));
    [a[i], a[j]] = [a[j], a[i]];
  }
  return a;
}

// group matchdays: MD1 1v2 3v4, MD2 1v3 4v2, MD3 1v4 2v3 (last round simultaneous)
const GROUP_PAIRS = [
  [[0, 1], [2, 3]],
  [[0, 2], [3, 1]],
  [[0, 3], [1, 2]],
];

function buildGroupFixtures(t) {
  t.fixtures = [];
  for (let md = 0; md < 3; md++) {
    for (const g of t.groups) {
      for (const [a, b] of GROUP_PAIRS[md]) {
        t.fixtures.push({
          id: `${g.id}${md}${a}`,
          stage: 'groups',
          group: g.id,
          matchday: md + 1,
          home: g.teamIds[a],
          away: g.teamIds[b],
          played: false,
          h: 0,
          a: 0,
          pens: null,
          scorers: [],
          venue: STADIUMS[Math.floor(t._rng() * STADIUMS.length)].id,
          user: false,
        });
      }
    }
  }
}

export function fixtureVenue(t, f) {
  return STADIUMS.find((s) => s.id === f.venue) || STADIUMS[0];
}

export function groupTable(t, groupId) {
  const g = t.groups.find((x) => x.id === groupId);
  const rows = g.teamIds.map((id) => ({
    id,
    team: TEAM_BY_ID[id],
    played: 0,
    w: 0,
    d: 0,
    l: 0,
    gf: 0,
    ga: 0,
    pts: 0,
    form: [],
  }));
  const byId = Object.fromEntries(rows.map((r) => [r.id, r]));
  for (const f of t.fixtures) {
    if (f.stage !== 'groups' || f.group !== groupId || !f.played) continue;
    const h = byId[f.home];
    const a = byId[f.away];
    h.played++;
    a.played++;
    h.gf += f.h;
    h.ga += f.a;
    a.gf += f.a;
    a.ga += f.h;
    if (f.h > f.a) {
      h.w++;
      a.l++;
      h.pts += 3;
      h.form.push('W');
      a.form.push('L');
    } else if (f.h < f.a) {
      a.w++;
      h.l++;
      a.pts += 3;
      a.form.push('W');
      h.form.push('L');
    } else {
      h.d++;
      a.d++;
      h.pts++;
      a.pts++;
      h.form.push('D');
      a.form.push('D');
    }
  }
  rows.sort((x, y) => y.pts - x.pts || y.gf - y.ga - (x.gf - x.ga) || y.gf - x.gf || x.id.localeCompare(y.id));
  rows.forEach((r) => (r.gd = r.gf - r.ga));
  return rows;
}

/** Fixtures of the current matchday / round. */
export function roundFixtures(t) {
  if (t.stage === 'groups') return t.fixtures.filter((f) => f.matchday === t.matchday && !f.played);
  return (t.bracket[t.stage] || []).filter((f) => !f.played);
}

export function nextFixture(t) {
  const list = roundFixtures(t);
  const mine = list.find((f) => f.home === t.championTeam || f.away === t.championTeam);
  if (mine) {
    mine.user = true;
    return mine;
  }
  return list[0] || null;
}

/** Poisson-ish scoreline for anything the human is not playing. */
export function simulateFixture(t, f, { user = false } = {}) {
  const home = TEAM_BY_ID[f.home];
  const away = TEAM_BY_ID[f.away];
  const sh = teamStrength(home);
  const sa = teamStrength(away);
  const formH = (t.form[f.home] || 0) * 0.12;
  const formA = (t.form[f.away] || 0) * 0.12;
  const xh = xgFor(sh, sa, { neutral: true, form: formH });
  const xa = xgFor(sa, sh, { neutral: true, form: formA });
  const h = poisson(xh, t._rng);
  const a = poisson(xa, t._rng);
  const result = {
    home: h,
    away: a,
    scorers: scorersFor(t, f, h, a),
    pens: null,
    user,
  };
  if (f.stage !== 'groups' && h === a) {
    // knockout: somebody has to win the shootout
    const pH = clamp(0.5 + (sh - sa) * 1.6, 0.3, 0.72);
    const pens = shootout(t, pH);
    result.pens = pens;
    if (pens.home > pens.away) result.home = h + 1;
    else result.away = a + 1;
  }
  applyFixture(t, f, result);
  return result;
}

export function applyFixture(t, f, result) {
  f.played = true;
  f.h = result.home;
  f.a = result.away;
  f.scorers = result.scorers;
  f.pens = result.pens || null;
  const hs = t.stats[f.home];
  const as = t.stats[f.away];
  if (hs) {
    hs.played++;
    hs.gf += result.home;
    hs.ga += result.away;
    hs.w += result.home > result.away ? 1 : 0;
    hs.d += result.home === result.away ? 1 : 0;
    hs.l += result.home < result.away ? 1 : 0;
    hs.pts += result.home > result.away ? 3 : result.home === result.away ? 1 : 0;
    t.form[f.home] = clamp((t.form[f.home] || 0) + (result.home > result.away ? 1 : result.home === result.away ? 0 : -1), -3, 3);
  }
  if (as) {
    as.played++;
    as.gf += result.away;
    as.ga += result.home;
    as.w += result.away > result.home ? 1 : 0;
    as.d += result.away === result.home ? 1 : 0;
    as.l += result.away < result.home ? 1 : 0;
    as.pts += result.away > result.home ? 3 : result.away === result.home ? 1 : 0;
    t.form[f.away] = clamp((t.form[f.away] || 0) + (result.away > result.home ? 1 : result.away === result.home ? 0 : -1), -3, 3);
  }
  for (const s of result.scorers || []) {
    t.records.goals[s.player] = (t.records.goals[s.player] || 0) + 1;
  }
  t.log.push({ fixture: f.id, stage: f.stage, line: `${f.home} ${result.home}-${result.away} ${f.away}` });
}

function poisson(mean, rng) {
  const L = Math.exp(-mean);
  let k = 0;
  let p = 1;
  do {
    k++;
    p *= rng();
  } while (p > L && k < 12);
  return k - 1;
}

function shootout(t, pHome) {
  let h = 0;
  let a = 0;
  for (let i = 0; i < 5; i++) {
    if (t._rng() < pHome * 0.78) h++;
    if (t._rng() < (1 - pHome) * 0.78) a++;
  }
  let guard = 0;
  while (h === a && guard++ < 12) {
    const xh = t._rng() < pHome * 0.78 ? 1 : 0;
    const xa = t._rng() < (1 - pHome) * 0.78 ? 1 : 0;
    h += xh;
    a += xa;
    if (xh !== xa) break;
  }
  if (h === a) h += t._rng() < 0.5 ? 1 : 0;
  return { home: h, away: a };
}

function scorersFor(t, f, h, a) {
  const out = [];
  const push = (id, n, side) => {
    const team = TEAM_BY_ID[id];
    if (!team) return;
    const squad = squadFor(team, String(t.seed));
    const attackers = squad.filter((p) => ['ST', 'RW', 'LW', 'AM', 'CM'].includes(p.slot));
    const pool = attackers.length ? attackers : squad;
    for (let i = 0; i < n; i++) {
      const p = pool[Math.floor(t._rng() * pool.length)];
      out.push({
        side,
        team: id,
        player: `${p.first} ${p.name}`.trim(),
        minute: 2 + Math.floor(t._rng() * 88),
      });
    }
  };
  push(f.home, h, 'home');
  push(f.away, a, 'away');
  return out.sort((x, y) => x.minute - y.minute);
}

/** Advance the tournament after a matchday has been fully played. */
export function advance(t) {
  if (t.stage === 'groups') {
    const remaining = roundFixtures(t);
    if (remaining.length) return { ok: true, waiting: remaining.length };
    if (t.matchday < 3) {
      t.matchday++;
      markUserFixtures(t);
      return { ok: true, matchday: t.matchday };
    }
    qualify(t);
    return { ok: true, stage: 'r16' };
  }
  if (t.stage === 'r16' || t.stage === 'qf' || t.stage === 'sf') {
    const round = t.bracket[t.stage];
    if (round.some((f) => !f.played)) return { ok: true, waiting: true };
    nextRound(t);
    return { ok: true, stage: t.stage };
  }
  if (t.stage === 'third') {
    t.stage = 'final';
    if (t.bracket.final.every((f) => f.played)) crown(t);
    return { ok: true, stage: t.stage };
  }
  if (t.stage === 'final') {
    crown(t);
    return { ok: true, stage: 'done' };
  }
  t.done = true;
  return { ok: true };
}

function markUserFixtures(t) {
  for (const f of roundFixtures(t)) f.user = f.home === t.championTeam || f.away === t.championTeam;
}

function qualify(t) {
  const winners = [];
  const runners = [];
  for (const g of t.groups) {
    const table = groupTable(t, g.id);
    winners.push(table[0]);
    runners.push(table[1]);
  }
  t.qualified = {
    winners: winners.map((r) => r.id),
    runners: runners.map((r) => r.id),
    order: ['A', 'B', 'C', 'D', 'E', 'F', 'G', 'H'].map((id) => id),
  };
  const userRank = groupTable(t, t.groups.find((g) => g.teamIds.includes(t.championTeam)).id).findIndex((r) => r.id === t.championTeam);
  const advances = winners.map((w) => w.id).concat(runners.map((r) => r.id));
  if (!advances.includes(t.championTeam)) t.eliminated = true;
  t.userGroupPos = userRank + 1;
  // R16: winner of A vs runner-up B, etc. (cross-group so the same group cannot meet again)
  const w = winners.map((r) => r.id);
  const ru = runners.map((r) => r.id);
  const pairs = [
    [0, 1],
    [2, 3],
    [4, 5],
    [6, 7],
    [1, 0],
    [3, 2],
    [5, 4],
    [7, 6],
  ];
  t.bracket.r16 = pairs.map(([wi, ri], i) => ({
    id: `r16${i}`,
    stage: 'r16',
    matchday: 1,
    home: w[wi],
    away: ru[ri],
    played: false,
    h: 0,
    a: 0,
    pens: null,
    scorers: [],
    venue: STADIUMS[(i * 3 + 1) % STADIUMS.length].id,
  }));
  t.stage = 'r16';
  markUserFixtures(t);
}

function nextRound(t) {
  const feed = (from, to) => {
    const src = t.bracket[from] || [];
    t.bracket[to] = [];
    for (let i = 0; i < src.length; i += 2) {
      const a = src[i];
      const b = src[i + 1];
      t.bracket[to].push({
        id: `${to}${i / 2}`,
        stage: to,
        matchday: 1,
        home: winnerOf(a),
        away: winnerOf(b),
        played: false,
        h: 0,
        a: 0,
        pens: null,
        scorers: [],
        venue: STADIUMS[(i * 5 + 2) % STADIUMS.length].id,
        feeds: [a.id, b.id],
      });
    }
  };
  if (t.stage === 'r16') {
    feed('r16', 'qf');
    t.stage = 'qf';
  } else if (t.stage === 'qf') {
    feed('qf', 'sf');
    t.stage = 'sf';
  } else if (t.stage === 'sf') {
    const losers = t.bracket.sf.map(loserOf);
    t.bracket.final = [
      {
        id: 'final0',
        stage: 'final',
        matchday: 1,
        home: winnerOf(t.bracket.sf[0]),
        away: winnerOf(t.bracket.sf[1]),
        played: false,
        h: 0,
        a: 0,
        pens: null,
        scorers: [],
        venue: 'metlife',
      },
    ];
    t.bracket.third = [
      {
        id: 'third0',
        stage: 'third',
        matchday: 1,
        home: losers[0],
        away: losers[1],
        played: false,
        h: 0,
        a: 0,
        pens: null,
        scorers: [],
        venue: 'hardrock',
      },
    ];
    t.stage = 'third';
  } else if (t.stage === 'third') {
    t.stage = 'final';
  }
  markUserFixtures(t);
}

function winnerOf(f) {
  if (!f || !f.played) return null;
  if (f.h !== f.a) return f.h > f.a ? f.home : f.away;
  if (f.pens) return f.pens.home > f.pens.away ? f.home : f.away;
  return f.home;
}
function loserOf(f) {
  const w = winnerOf(f);
  if (!w) return null;
  return w === f.home ? f.away : f.home;
}

function crown(t) {
  const f = t.bracket.final[0];
  if (!f || !f.played) return false;
  t.champion = winnerOf(f);
  t.runnerUp = loserOf(f);
  const tf = t.bracket.third[0];
  t.third = tf && tf.played ? winnerOf(tf) : tf ? null : null;
  t.fourth = tf && tf.played ? loserOf(tf) : null;
  t.topScorer = topScorer(t);
  t.done = true;
  t.stage = 'done';
  return true;
}

/** Does the human's team still have a game to play? */
export function userAlive(t) {
  if (t.done) return false;
  const list = roundFixtures(t);
  return list.some((f) => f.home === t.championTeam || f.away === t.championTeam);
}

/** One-stop helper used by the menus: play/simulate everything the human is not playing. */
export function playTournamentMatch(t, { simulate = false, result = null, restOnly = false } = {}) {
  if (restOnly) {
    // the human's fixture was already applied by the match engine: play the rest, move on
    for (const other of roundFixtures(t)) if (!other.played) simulateFixture(t, other, { user: false });
    return { fixture: null, result: null, advance: advance(t), text: '' };
  }
  const f = nextFixture(t);
  if (!f) {
    advance(t);
    return { text: 'nothing scheduled', done: t.done };
  }
  let res;
  if (simulate) res = simulateFixture(t, f, { user: true });
  else if (result) {
    res = { home: result.home, away: result.away, scorers: result.scorers || [], pens: result.pens || null, user: true };
    applyFixture(t, f, res);
  } else res = simulateFixture(t, f, { user: false });
  // the rest of the matchday happens without the human
  for (const other of roundFixtures(t)) if (!other.played) simulateFixture(t, other, { user: false });
  const advanced = advance(t);
  return { fixture: f, result: res, advance: advanced, text: `${f.home} ${res.home}-${res.away} ${f.away}${res.pens ? ` (pens ${res.pens.home}-${res.pens.away})` : ''}` };
}

/** Golden boot / golden glove helpers for the results screen. */
export function topScorer(t) {
  let best = null;
  for (const [name, goals] of Object.entries(t.records.goals)) if (!best || goals > best.goals) best = { name, goals };
  return best;
}

export function teamRecord(t, id) {
  return t.stats[id] || { played: 0, w: 0, d: 0, l: 0, gf: 0, ga: 0, pts: 0 };
}

export { groupTable as tableFor };
