// =============================================================================
//  Headless preview renderer.
//  Real Skia canvas through @napi-rs/canvas, so a match frame - stadium,
//  kits, flags, ball, effects - can be exported to PNG with no browser.
//  Handy for balance eyeballing and for the README screenshots.
//
//    npm i @napi-rs/canvas     (dev only, in a scratch folder)
//    node tools/preview.mjs [outdir]
// =============================================================================
import { createRequire } from 'node:module';
import fs from 'node:fs';
import path from 'node:path';

const require = createRequire(import.meta.url);
let napiCanvas = null;
try {
  napiCanvas = require('@napi-rs/canvas');
} catch {
  try {
    napiCanvas = require(path.join(process.env.HOME || '/home/user', '.verify', 'node_modules', '@napi-rs', 'canvas'));
  } catch {
    console.error('need @napi-rs/canvas:  npm i @napi-rs/canvas');
    process.exit(2);
  }
}

const OUT = process.argv[2] || 'preview';
fs.mkdirSync(OUT, { recursive: true });

// --- a canvas that quacks like the browser's for Renderer ------------------
function realCanvas(w, h) {
  const c = napiCanvas.createCanvas(w, h);
  c.style = {};
  c.clientWidth = w;
  c.clientHeight = h;
  c.parentElement = { clientWidth: w, clientHeight: h };
  c.getBoundingClientRect = () => ({ left: 0, top: 0, right: w, bottom: h, width: w, height: h });
  return c;
}

// --- DOM stub, with <canvas> upgraded to real pixels ------------------------
const { installDom } = await import('./domstub.mjs');
const dom = installDom({ width: 1280, height: 720 });
const stubCreate = dom.document.createElement;
dom.document.createElement = (tag) => {
  if (String(tag).toLowerCase() === 'canvas') return realCanvas(256, 256);
  return stubCreate(tag);
};

const { Match, P } = await import('../src/match.js');
const { Renderer } = await import('../src/render.js');
const { TEAMS, TEAM_BY_ID } = await import('../src/teams.js');
const { drawFlag } = await import('../src/flags.js');

const report = [];
function save(name, canvas) {
  const file = path.join(OUT, name + '.png');
  fs.writeFileSync(file, canvas.toBuffer('image/png'));
  report.push([name + '.png', fs.statSync(file).size]);
  return file;
}

/** run a match to a moment that has something going on */
function scene(cfg, frames, wantState) {
  const m = new Match(cfg);
  let best = null;
  for (let i = 0; i < frames; i++) {
    m.update(1 / 60, i % 37 === 0 ? { mx: 0.6, my: -0.2, sprint: true, shoot: false, pass: true, through: false, tackle: false, sw: false, press: false } : { mx: 0.35, my: 0.1, sprint: false, shoot: i % 90 < 8, pass: false, through: false, tackle: i % 61 === 5, sw: false, press: false });
    m.consumeEdges();
    if (m.state === 'halftime') m.resume();
    if (wantState && m.state === wantState) break;
    if (m.banner || m.state === 'celebrate' || m.state === 'restart') best = i;
  }
  return { m, at: best };
}

function shot(name, m, w = 1280, h = 720, view = {}) {
  const c = realCanvas(w, h);
  const r = new Renderer(c);
  r.resize();
  r.draw(m, view);
  return save(name, c);
}

// 1. open play
{
  const { m } = scene(
    { home: TEAM_BY_ID.BRA, away: TEAM_BY_ID.GER, seed: 1234, halfSeconds: 240, humanTeam: 'home', difficulty: 'world' },
    1500
  );
  shot('01-match', m, 1280, 720, { speedLabel: '1.00×  60fps' });
  console.log(`01-match: ${m.scoreline()} ${m.clockLabel()} state=${m.state}`);
}
// 2. goal celebration (banner + net ripple + confetti)
{
  const { m } = scene({ home: TEAM_BY_ID.ARG, away: TEAM_BY_ID.FRA, seed: 77, halfSeconds: 600, humanTeam: null, difficulty: 'pro' }, 4000, 'celebrate');
  shot('02-goal', m, 1280, 720);
  console.log('02-goal: state=' + m.state, m.banner ? 'banner=' + m.banner.text : 'no banner');
}
// 3. a set piece with the wall
{
  const { m } = scene({ home: TEAM_BY_ID.ESP, away: TEAM_BY_ID.MAR, seed: 91, halfSeconds: 600, humanTeam: 'home', difficulty: 'legend' }, 3000, 'restart');
  shot('03-setpiece', m, 1280, 720);
  console.log('03-setpiece: restart=' + (m.restart ? m.restart.kind : 'none'));
}
// 4. shootout
{
  const m = new Match({ home: TEAM_BY_ID.ENG, away: TEAM_BY_ID.POR, seed: 5, halfSeconds: 30, humanTeam: 'home', difficulty: 'world' });
  m.half = 5;
  m.startShootout();
  for (let i = 0; i < 400; i++) {
    m.update(1 / 60, { mx: 0, my: 0.4, sprint: false, shoot: i % 20 < 8, pass: false, through: false, tackle: false, sw: false, press: false });
    m.consumeEdges();
  }
  shot('04-shootout', m, 1280, 720);
  console.log('04-shootout: phase=' + (m.pens ? m.pens.phase : m.state));
}
// 5. mobile portrait
{
  const { m } = scene({ home: TEAM_BY_ID.USA, away: TEAM_BY_ID.MEX, seed: 42, halfSeconds: 240, humanTeam: 'home', difficulty: 'pro' }, 1200);
  shot('05-mobile', m, 390, 844);
  console.log('05-mobile: 390x844');
}
// 6. flag sheet - every nation, so broken specs are obvious
{
  const cols = 8;
  const rows = Math.ceil(TEAMS.length / cols);
  const cw = 132;
  const chh = 116;
  const short = (n) => (n.length > 13 ? n.slice(0, 12) + '…' : n);
  const fw = cw - 34;
  const fh = fw / 1.5;
  const c = realCanvas(cols * cw, rows * chh);
  const ctx = c.getContext('2d');
  ctx.fillStyle = '#0a0d12';
  ctx.fillRect(0, 0, c.width, c.height);
  TEAMS.forEach((t, i) => {
    const x = (i % cols) * cw;
    const y = Math.floor(i / cols) * chh;
    drawFlag(ctx, t.flag, x + 17, y + 14, fw, fh, { radius: 4, outline: true });
    ctx.fillStyle = '#eaf2ff';
    ctx.font = 'bold 13px sans-serif';
    ctx.fillText(`${short(t.name)} ${t.ovr}`, x + 17, y + 14 + fh + 18);
  });
  save('06-flags', c);
  console.log('06-flags: ' + TEAMS.length + ' nations');
}
// 7. kits sheet - jersey patterns on the pitch colours
{
  const cols = 8;
  const c = realCanvas(cols * 96, Math.ceil(TEAMS.length / cols) * 96);
  const ctx = c.getContext('2d');
  ctx.fillStyle = '#101823';
  ctx.fillRect(0, 0, c.width, c.height);
  TEAMS.forEach((t, i) => {
    const x = (i % cols) * 96 + 20;
    const y = Math.floor(i / cols) * 96 + 14;
    ctx.save();
    ctx.translate(x, y);
    // torso
    ctx.fillStyle = t.colors.jersey;
    ctx.beginPath();
    ctx.moveTo(-16, 0);
    ctx.lineTo(-22, 8);
    ctx.lineTo(-22, 44);
    ctx.lineTo(22, 44);
    ctx.lineTo(22, 8);
    ctx.lineTo(16, 0);
    ctx.quadraticCurveTo(0, 10, -16, 0);
    ctx.closePath();
    ctx.fill();
    if (t.pattern === 'stripes') {
      ctx.fillStyle = t.trim;
      for (let s = -18; s < 18; s += 8) ctx.fillRect(s, 2, 3.4, 42);
    } else if (t.pattern === 'halves') {
      ctx.fillStyle = t.trim;
      ctx.fillRect(-22, 0, 22, 44);
    } else if (t.pattern === 'hoops') {
      ctx.fillStyle = t.trim;
      for (let s = 6; s < 44; s += 9) ctx.fillRect(-22, s, 44, 4);
    }
    ctx.strokeStyle = 'rgba(0,0,0,.45)';
    ctx.stroke();
    ctx.restore();
    ctx.fillStyle = '#cfe0f5';
    ctx.font = 'bold 11px sans-serif';
    ctx.fillText(t.id, x - 20, y + 62);
  });
  save('07-kits', c);
  console.log('07-kits: ' + TEAMS.length + ' strips');
}

console.log('\nwrote to ' + path.resolve(OUT));
for (const [n, sz] of report) console.log(`  ${n.padEnd(16)} ${(sz / 1024).toFixed(1)} KB`);
