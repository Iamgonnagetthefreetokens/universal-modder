// The field: 32 nations, real kit colours, deterministic squads and a rating block per line.
// Everything here is plain data so the match engine, the tournament and the menus share one
// source of truth (and so it can be unit-tested from node without a DOM).
import { vb, hws, altH, cross } from './flags.js';

const W = '#ffffff';
const B = '#000000';

// --- flag geometry helpers ------------------------------------------------
function checker(x0, y0, w, h, cols, rows, color) {
  const ops = [];
  for (let r = 0; r < rows; r++)
    for (let c = 0; c < cols; c++)
      if ((r + c) % 2 === 0) ops.push(['r', x0 + (c * w) / cols, y0 + (r * h) / rows, w / cols, h / rows, color]);
  return ops;
}

function starGrid(x0, y0, dx, dy, cols, rows, color, r) {
  const ops = [];
  for (let ry = 0; ry < rows; ry++)
    for (let cx = 0; cx < cols; cx++) {
      const off = ry % 2 ? dx / 2 : 0;
      ops.push(['s', x0 + cx * dx + off, y0 + ry * dy, r, 5, color]);
    }
  return ops;
}

function pentagram(cx, cy, r, color) {
  const pts = [];
  for (let i = 0; i < 5; i++) pts.push([cx + Math.sin((i * 4 * Math.PI) / 5) * r, cy - Math.cos((i * 4 * Math.PI) / 5) * r]);
  const ops = [];
  for (let i = 0; i < 5; i++) {
    const a = pts[i];
    const b = pts[(i + 1) % 5];
    ops.push(['l', a[0], a[1], b[0], b[1], color, 0.03]);
  }
  return ops;
}

function arcPoly(cx, cy, r, a0, a1) {
  const pts = [];
  const steps = 26;
  for (let i = 0; i <= steps; i++) pts.push([cx + Math.cos(a0 + ((a1 - a0) * i) / steps) * r, cy + Math.sin(a0 + ((a1 - a0) * i) / steps) * r]);
  return pts;
}

function taegeukBars() {
  const out = [];
  for (const [sx, sy] of [[-1, -1], [1, -1], [-1, 1], [1, 1]]) {
    const a = Math.atan2(sy * 0.62, sx);
    const px = 0.5 + Math.cos(a) * 0.36;
    const py = 0.5 + Math.sin(a) * 0.36;
    out.push(['r', px - 0.09, py - 0.022, 0.18, 0.044, B]);
  }
  return out;
}

function serrated(x, n, color) {
  const pts = [[0, 0], [x, 0]];
  const h = 1 / n;
  for (let i = 0; i < n; i++) {
    pts.push([x + 0.04, h * (i + 0.5)]);
    pts.push([x, h * (i + 1)]);
  }
  pts.push([0, 1]);
  return [['p', pts, color]];
}

function kenteeth(x0, x1, color) {
  const ops = [];
  const n = 12;
  for (let i = 0; i < n; i++) {
    const u = x0 + ((x1 - x0) * i) / (n - 1);
    ops.push(['r', u - 0.008, 0.015, 0.016, 0.02, color]);
    ops.push(['r', u - 0.008, 0.965, 0.016, 0.02, color]);
  }
  return ops;
}

// --- flags ----------------------------------------------------------------
const F = {
  BRA: [['r', 0, 0, 1, 1, '#009c3b'], ['p', [[0.5, 0.08], [0.93, 0.5], [0.5, 0.92], [0.07, 0.5]], '#ffdf00'],
    ['c', 0.5, 0.5, 0.26, '#002776'], ['r', 0.26, 0.44, 0.48, 0.09, W]],
  ARG: [...hws(['#74acdf', 1, W, 1, '#74acdf', 1]), ['c', 0.5, 0.5, 0.11, '#f6b40e'], ['c', 0.5, 0.5, 0.06, '#e8a20c']],
  FRA: vb('#002395', W, '#ed2939'),
  ESP: [...hws(['#aa151b', 1, '#f1bf00', 2, '#aa151b', 1]), ['r', 0.12, 0.3, 0.12, 0.4, '#c60b1e'], ['r', 0.145, 0.4, 0.07, 0.16, '#f1bf00']],
  ENG: [['r', 0, 0, 1, 1, W], ...cross(0.5, 0.5, 0.075, '#ce1124')],
  NED: hb3('#ae1c28', W, '#21468b'),
  POR: [['r', 0, 0, 0.4, 1, '#046a38'], ['r', 0.4, 0, 0.6, 1, '#da291c'], ['c', 0.4, 0.5, 0.15, '#ffcf00'], ['r', 0.33, 0.44, 0.14, 0.12, '#da291c']],
  GER: hb3(B, '#dd0000', '#ffce00'),
  BEL: vb(B, '#fdda25', '#ef3340'),
  CRO: [...hb3('#ff0000', W, '#171796'), ['p', [[0.34, 0.16], [0.66, 0.16], [0.66, 0.5], [0.5, 0.63], [0.34, 0.5]], W],
    ...checker(0.34, 0.16, 0.32, 0.34, 5, 4, '#ff0000')],
  URU: [...altH(9, W, '#0038a8'), ['r', 0, 0, 0.4, 0.55, W], ['c', 0.2, 0.27, 0.1, '#f6b40e']],
  SUI: [['r', 0, 0, 1, 1, '#da291c'], ...cross(0.5, 0.5, 0.075, W)],
  COL: [...hws(['#fcd116', 1, '#003893', 0.5, '#ce1126', 0.5]), ['c', 0.5, 0.78, 0.05, '#fcd116']],
  MEX: [...vb('#006847', W, '#ce1126'), ['c', 0.5, 0.5, 0.11, '#8a6a3a'], ['c', 0.5, 0.5, 0.07, W]],
  USA: [...altH(13, '#b22234', W), ['r', 0, 0, 0.42, 0.54, '#3c3b6e'], ...starGrid(0.04, 0.07, 0.07, 0.1, 5, 5, W, 0.018)],
  CAN: [['r', 0, 0, 0.25, 1, '#d52b1e'], ['r', 0.75, 0, 0.25, 1, '#d52b1e'],
    ['p', [[0.5, 0.2], [0.55, 0.4], [0.64, 0.34], [0.6, 0.48], [0.68, 0.5], [0.56, 0.55], [0.58, 0.72], [0.52, 0.66], [0.5, 0.8], [0.48, 0.66], [0.42, 0.72], [0.44, 0.55], [0.32, 0.5], [0.4, 0.48], [0.36, 0.34], [0.45, 0.4]], '#d52b1e']],
  JPN: [['r', 0, 0, 1, 1, W], ['c', 0.5, 0.5, 0.29, '#bc002d']],
  KOR: [['r', 0, 0, 1, 1, W], ['p', arcPoly(0.5, 0.5, 0.25, 0, Math.PI), '#cd2e3a'], ['p', arcPoly(0.5, 0.5, 0.25, Math.PI, Math.PI * 2), '#0047a0'],
    ['c', 0.5, 0.375, 0.125, '#cd2e3a'], ['c', 0.5, 0.625, 0.125, '#0047a0'], ['c', 0.5, 0.5, 0.25, 'rgba(0,0,0,0)'], ...taegeukBars()],
  AUS: [['r', 0, 0, 1, 1, '#00247d'], ...cross(0.25, 0.25, 0.035, W), ...cross(0.25, 0.25, 0.02, '#cc142b'),
    ['s', 0.25, 0.76, 0.09, 7, W], ...starGrid(0.58, 0.2, 0.2, 0.2, 3, 2, W, 0.045), ['s', 0.83, 0.62, 0.05, 5, W]],
  MAR: [['r', 0, 0, 1, 1, '#c1272d'], ...pentagram(0.5, 0.5, 0.26, '#006233')],
  SEN: [...vb('#00853f', '#fdef42', '#e31b23'), ['s', 0.5, 0.5, 0.13, 5, '#00853f']],
  GHA: [...hws(['#ce1126', 1, '#fcd116', 1, '#006b3f', 1]), ['s', 0.5, 0.5, 0.13, 5, B]],
  CMR: [...vb('#007a5e', '#ce1126', '#fcd116'), ['s', 0.5, 0.5, 0.15, 5, '#fcd116']],
  NGA: vb('#008751', W, '#008751'),
  CIV: vb('#f77f00', W, '#009e49'),
  ECU: [...hws(['#ffd100', 2, '#0057b7', 1, '#ef3340', 1]), ['c', 0.5, 0.5, 0.11, '#b8860b'], ['c', 0.5, 0.5, 0.07, '#ffd100']],
  QAT: [['r', 0, 0, 1, 1, '#8a1538'], ...serrated(0.3, 9, W)],
  KSA: [['r', 0, 0, 1, 1, '#165d31'], ['r', 0.16, 0.55, 0.62, 0.05, W], ['r', 0.62, 0.42, 0.05, 0.17, W], ['c', 0.34, 0.42, 0.05, W]],
  IRN: [...hb3('#239f40', W, '#da0000'), ...kenteeth(0.14, 0.86, W), ['c', 0.5, 0.5, 0.07, '#da0000']],
  TUN: [['r', 0, 0, 1, 1, '#e70013'], ['c', 0.5, 0.5, 0.31, W], ['c', 0.47, 0.5, 0.21, '#e70013'], ['c', 0.56, 0.5, 0.18, W], ['s', 0.56, 0.5, 0.1, 5, '#e70013']],
  DEN: [['r', 0, 0, 1, 1, '#c8102e'], ...cross(0.38, 0.5, 0.075, W)],
  POL: [['h', 0, 0.5, W], ['h', 0.5, 1, '#dc143c']],
  PAN: [['r', 0, 0, 0.5, 0.5, W], ['r', 0.5, 0.5, 0.5, 0.5, W], ['r', 0.5, 0, 0.5, 0.5, '#da121a'], ['r', 0, 0.5, 0.5, 0.5, '#005aa0'],
    ['s', 0.25, 0.25, 0.09, 5, '#005aa0'], ['s', 0.75, 0.75, 0.09, 5, '#da121a']],
  CRC: hws(['#002b7f', 1, W, 1, '#ce1126', 2, W, 1, '#002b7f', 1]),
  MEX2: vb('#006847', W, '#ce1126'),
};

// Every nation without a hand-drawn spec still gets a legible banner.
const FALLBACK_FLAG = [
  ['r', 0, 0, 1, 1, '#eef2f7'],
  ['c', 0.5, 0.5, 0.24, '#0a0d12'],
  ['s', 0.5, 0.5, 0.13, 5, '#b6ff3b'],
];
function hb3(a, b, c) {
  return [['h', 0, 1 / 3, a], ['h', 1 / 3, 2 / 3, b], ['h', 2 / 3, 1, c]];
}

// --- teams ---------------------------------------------------------------
// row: [id, name, nickname, confederation, jersey, shorts, kitPattern, trim, [att,mid,def,gk], surnames]
const RAW = [
  ['ARG', 'Argentina', 'La Albiceleste', 'CONMEBOL', W, W, 'stripes', '#75aadb', [93, 89, 84, 86], ['Messi', 'Lautaro', 'De Paul', 'Enzo', 'Álvarez', 'Di María', 'Martínez', 'Romero']],
  ['FRA', 'France', 'Les Bleus', 'UEFA', '#002395', W, 'plain', '#ed2939', [92, 89, 87, 88], ['Mbappé', 'Griezmann', 'Tchouaméni', 'Camavinga', 'Koundé', 'Saliba', 'Maignan', 'Dembélé']],
  ['ESP', 'Spain', 'La Roja', 'UEFA', '#c60b1e', W, 'plain', '#f1bf00', [89, 93, 86, 85], ['Yamal', 'Pedri', 'Rodri', 'Nico W.', 'Morata', 'Le Normand', 'Simón', 'Olmo']],
  ['BRA', 'Brazil', 'A Seleção', 'CONMEBOL', '#ffdf00', '#133c8b', 'plain', '#009c3b', [92, 89, 84, 85], ['Vinícius', 'Rodrygo', 'Casemiro', 'Bruno G.', 'Endrick', 'Militão', 'Alisson', 'Raphinha']],
  ['ENG', 'England', 'Three Lions', 'UEFA', W, W, 'plain', '#ce1124', [89, 88, 87, 88], ['Kane', 'Bellingham', 'Saka', 'Foden', 'Rice', 'Stones', 'Pickford', 'Palmer']],
  ['POR', 'Portugal', 'Seleção das Quinas', 'UEFA', '#c8102e', '#c8102e', 'halves', '#046a38', [90, 87, 84, 85], ['Ronaldo', 'Bruno F.', 'Bernardo', 'Leão', 'Vitinha', 'Dias', 'Costa', 'Nunes']],
  ['NED', 'Netherlands', 'Oranje', 'UEFA', '#ff6600', W, 'plain', '#21468b', [86, 87, 86, 84], ['Gakpo', 'Simons', 'de Jong', 'Frans', 'van Dijk', 'Ake', 'Verbruggen', 'Malen']],
  ['GER', 'Germany', 'Die Mannschaft', 'UEFA', W, '#1a1a1a', 'sash', '#dd0000', [88, 87, 83, 85], ['Musiala', 'Wirtz', 'Kimmich', 'Havertz', 'Rüdiger', 'Tah', 'Neuer', 'Sané']],
  ['BEL', 'Belgium', 'Rode Duivels', 'UEFA', '#e32a34', '#1a1a1a', 'plain', '#fdda25', [86, 85, 82, 84], ['De Bruyne', 'Doku', 'Openda', 'Tielemans', 'Faes', 'De Cuyper', 'Casteels', 'Trossard']],
  ['CRO', 'Croatia', 'Vatreni', 'UEFA', '#ff0000', B, 'checks', '#171796', [84, 86, 82, 83], ['Modrić', 'Kovačić', 'Pašalić', 'Gvardiol', 'Perišić', 'Sutalo', 'Livaković', 'Baturina']],
  ['MAR', 'Morocco', 'Atlas Lions', 'CAF', '#c1272d', '#006233', 'plain', '#c1272d', [83, 84, 86, 85], ['Hakimi', 'Ziyech', 'Amrabat', 'En-Nesyri', 'Aguerd', 'Mazraoui', 'Bounou', 'Ounahi']],
  ['JPN', 'Japan', 'Samurai Blue', 'AFC', '#0a2f6b', '#0a2f6b', 'plain', '#bc002d', [84, 85, 83, 82], ['Mitoma', 'Kamada', 'Endo', 'Minamino', 'Taniguchi', 'Tomiyasu', 'Suzuki', 'Doan']],
  ['USA', 'United States', 'Stars & Stripes', 'CONCACAF', '#0a2b6b', W, 'plain', '#b22234', [83, 82, 80, 81], ['Pulisic', 'Ream', 'Balogun', 'Musah', 'Yunes', 'Robinson', 'Freese', 'Tillman']],
  ['MEX', 'Mexico', 'El Tri', 'CONCACAF', '#006847', W, 'stripes', '#ce1126', [83, 82, 80, 82], ['Jiménez', 'Lozano', 'F. Álvarez', 'Edson', 'Romero', 'Aguirre', 'Malagón', 'Gallardo']],
  ['SEN', 'Senegal', 'Lions of Teranga', 'CAF', '#00853f', '#fdef42', 'plain', '#e31b23', [84, 82, 83, 81], ['Mané', 'Jackson', 'Diop', 'Gueye', 'Sabaly', 'Camara', 'Mendy', 'Diarra']],
  ['URU', 'Uruguay', 'La Celeste', 'CONMEBOL', '#5fbdf2', B, 'plain', '#0038a8', [85, 81, 81, 82], ['Núñez', 'Valverde', 'Araújo', 'De la Cruz', 'Oliva', 'Cáceres', 'Rochet', 'Díaz']],
  ['COL', 'Colombia', 'Los Cafeteros', 'CONMEBOL', '#fcd116', '#003893', 'plain', '#ce1126', [85, 83, 81, 81], ['Luis Díaz', 'James', 'Córdoba', 'Ríos', 'Arias', 'Lucumí', 'Vargas', 'Muñoz']],
  ['SUI', 'Switzerland', 'La Nati', 'UEFA', '#da291c', W, 'plain', '#fff', [81, 83, 83, 82], ['Xhaka', 'Embolo', 'Akanji', 'Ndoye', 'Freuler', 'Rodríguez', 'Kobel', 'Vargas']],
  ['KOR', 'South Korea', 'Taegeuk Warriors', 'AFC', '#c60c30', B, 'halves', '#0047a0', [84, 81, 79, 80], ['Son', 'Lee K.Y.', 'Hwang', 'Kim M.J.', 'Jung', 'Cho', 'Oh', 'Baik']],
  ['AUS', 'Australia', 'Socceroos', 'AFC', '#00853f', '#ffcd00', 'plain', '#00247d', [79, 80, 82, 81], ['Irwin', 'Mabil', 'Velupillay', 'Souttar', 'Mooy', 'Behich', 'Ryan', 'Duke']],
  ['ECU', 'Ecuador', 'La Tri', 'CONMEBOL', '#ffdd00', '#0057b7', 'plain', '#ef3340', [80, 79, 80, 80], ['En. Valencia', 'Caicedo', 'Pacho', 'Moisés', 'Plata', 'Hincapié', 'Domínguez', 'Yeboah']],
  ['DEN', 'Denmark', 'Danish Dynamite', 'UEFA', '#c8102e', W, 'plain', '#fff', [81, 82, 81, 82], ['Højlund', 'Eriksen', 'Wind', 'Kristensen', 'Vestergaard', 'Andersen', 'Schmeichel', 'Dahls']],
  ['NGA', 'Nigeria', 'Super Eagles', 'CAF', '#008751', W, 'hoops', '#fcd116', [81, 78, 77, 78], ['Osimhen', 'Lookman', 'Iwobi', 'Chukwueze', 'Bassey', 'Omeruo', 'Nwabali', 'Aina']],
  ['CAN', 'Canada', 'Les Rouges', 'CONCACAF', '#d52b1e', W, 'plain', '#d52b1e', [80, 78, 78, 79], ['Davies', 'Jonathan D.', 'Buchanan', 'Eustáquio', 'Shaffelburg', 'Waterman', 'Crépeau', 'Laryea']],
  ['KSA', 'Saudi Arabia', 'Green Falcons', 'AFC', '#165d31', W, 'plain', '#165d31', [76, 77, 78, 79], ['Al-Dawsari', 'Kanno', 'Al-Bulaihi', 'Al-Ghanam', 'Al-Najei', 'Tambakti', 'Al-Owais', 'Abdulhamid']],
  ['CMR', 'Cameroon', 'Indomitable Lions', 'CAF', '#007a5e', '#ce1126', 'sash', '#fcd116', [79, 77, 79, 79], ['Mbeumo', 'Aboubakar', 'Ondoua', 'Kadile', 'Tolo', 'Nkoulou', 'Epassy', 'Bahoken']],
  ['CIV', "Côte d'Ivoire", 'Les Éléphants', 'CAF', '#f77f00', W, 'plain', '#009e49', [80, 79, 79, 79], ['Haller', 'Kessié', 'Doué', 'Guela', 'Kossounou', 'Bailly', 'Fofana', 'Adingra']],
  ['CRC', 'Costa Rica', 'La Sele', 'CONCACAF', '#002ae6', W, 'plain', '#ce1126', [76, 76, 79, 80], ['Navas', 'Borges', 'Bennett', 'Aguilera', 'Calvo', 'Matarrita', 'Sequeira', 'Campos']],
  ['PAN', 'Panama', 'Marea Roja', 'CONCACAF', '#da121a', B, 'checks', '#005aa0', [74, 74, 76, 77], ['Fajardo', 'Carrasquilla', 'Barcenas', 'Murillo', 'Andrada', 'Cummings', 'Mosquera', 'Guerrero']],
  ['QAT', 'Qatar', 'Al-Adoom', 'AFC', '#8a1538', W, 'halves', '#8a1538', [75, 76, 78, 78], ['Afif', 'Akram', 'Hassan', 'Boudiaf', 'Khoukhi', 'Tarek', 'Barsham', 'Muntari']],
  ['IRN', 'Iran', 'Team Melli', 'AFC', '#da0000', W, 'plain', '#239f40', [77, 77, 80, 79], ['Taremi', 'Azmoun', 'Jahanbakhsh', 'Hosseini', 'Kanani', 'Moharrami', 'Beiranvand', 'Ghoddos']],
  ['TUN', 'Tunisia', 'Carthage Eagles', 'CAF', '#e70013', W, 'plain', '#e70013', [75, 76, 79, 78], ['Khazri', 'Sassi', 'Skhiri', 'Maâloul', 'Jaziri', 'Bronn', 'Dammi', 'Chaabane']],
];

export const TEAMS = RAW.map(([id, name, nick, confed, jersey, shorts, pattern, trim, r, surnames]) => ({
  id,
  name,
  nick,
  confed,
  pattern,
  trim,
  colors: { jersey, shorts, socks: pattern === 'plain' ? jersey : W, trim },
  ratings: { att: r[0], mid: r[1], def: r[2], gk: r[3] },
  ovr: Math.round((r[0] * 1.12 + r[1] + r[2] + r[3] * 0.88) / 4.0),
  surnames,
  flag: F[id] || FALLBACK_FLAG,
  pot: 4,
}));

export const TEAM_BY_ID = Object.fromEntries(TEAMS.map((t) => [t.id, t]));

// seed the draw: rank by overall rating, four pots of eight
[...TEAMS]
  .sort((a, b) => b.ovr - a.ovr || a.name.localeCompare(b.name))
  .forEach((t, i) => {
    TEAM_BY_ID[t.id].pot = Math.floor(i / 8) + 1;
  });

export const STADIUMS = [
  { id: 'sofi', name: 'SoFi Stadium', city: 'Inglewood, CA', cap: 70240, turf: '#2f8f3e', crowd: ['#1f2a44', '#c9d3ff', '#4a5b8c'] },
  { id: 'hardrock', name: 'Hard Rock Stadium', city: 'Miami Gardens, FL', cap: 65326, turf: '#2d8c46', crowd: ['#0f3b2e', '#f2d06b', '#2f7d65'] },
  { id: 'arrowhead', name: 'Arrowhead Stadium', city: 'Kansas City, MO', cap: 76416, turf: '#2a8a3c', crowd: ['#e31837', '#1a1a1a', '#ff7f00'] },
  { id: 'bcplace', name: 'BC Place', city: 'Vancouver, BC', cap: 54500, turf: '#389b4c', crowd: ['#0038a8', '#d52b1e', '#cfd6e6'] },
  { id: 'azteca', name: 'Estadio Azteca', city: 'Mexico City, MX', cap: 87523, turf: '#2f8f3e', crowd: ['#006847', '#ce1126', '#f2f2f2'] },
  { id: 'metlife', name: 'MetLife Stadium', city: 'East Rutherford, NJ', cap: 82500, turf: '#2b8540', crowd: ['#2b2f38', '#8f9bb3', '#4a5568'] },
  { id: 'atandt', name: 'AT&T Stadium', city: 'Arlington, TX', cap: 80000, turf: '#34924a', crowd: ['#0a2b6b', '#c60c30', '#d1d5db'] },
  { id: 'lumen', name: 'Lumen Field', city: 'Seattle, WA', cap: 68740, turf: '#27813c', crowd: ['#1a1a1a', '#5fa92d', '#0f4c81'] },
];

const FIRSTS = ['Luca', 'Diogo', 'Mateo', 'Kai', 'Yanis', 'Emre', 'Noah', 'Ilias', 'Rafa', 'Toni', 'Joel', 'Marco', 'Sami', 'Leo', 'Adam', 'Nico', 'Koji', 'Park', 'Moussa', 'Ivan'];

// Given names are picked to match the nation, so a squad reads like a real team
// sheet instead of a random lottery of the world's first names.
const FIRSTS_BY_CONFED = {
  CONMEBOL: ['Lautaro', 'Emiliano', 'Rodrigo', 'Mateo', 'Santiago', 'Facundo', 'Bruno', 'Thiago', 'Nicolás', 'Gonzalo', 'Cristian', 'Alan'],
  UEFA: ['Luca', 'Kai', 'Yanis', 'Rafa', 'Toni', 'Marco', 'Adam', 'Noah', 'Félix', 'David', 'Oskar', 'Emre', 'Bukayo', 'Kevin', 'Denzel', 'Florian'],
  CAF: ['Moussa', 'Ilias', 'Sami', 'Ousmane', 'Achraf', 'Sadio', 'Nabil', 'Youssef', 'Kwame', 'Victor', 'Thomas', 'Bertrand', 'Franck', 'Amad'],
  AFC: ['Koji', 'Yuki', 'Takumi', 'Min-jae', 'Hee-chan', 'Kang-in', 'Sardar', 'Mehdi', 'Ali', 'Alireza', 'Ivan', 'Chanathip'],
  CONCACAF: ['Jesús', 'Andrés', 'Carlos', 'Diego', 'Hirving', 'Raúl', 'Edson', 'Alphonso', 'Jonathan', 'Cyle', 'Christian', 'Weston'],
  OFC: ['Mathew', 'Jackson', 'Ajdin', 'Martin', 'Harry', 'Chris', 'Kosta'],
};
const FIRSTS_BY_TEAM = {
  ARG: ['Lionel', 'Lautaro', 'Julián', 'Enzo', 'Alexis', 'Rodrigo', 'Nicolás', 'Thiago'],
  BRA: ['Vinícius', 'Rodrygo', 'Rafael', 'Bruno', 'Lucas', 'Gabriel', 'Éder', 'Wesley'],
  FRA: ['Kylian', 'Ousmane', 'Aurélien', 'Bradley', 'William', 'Jules', 'Manu', 'Rayan'],
  ESP: ['Pedri', 'Gavi', 'Nico', 'Lamine', 'Alvaro', 'Fermín', 'Dani', 'Mikel'],
  ENG: ['Jude', 'Bukayo', 'Declan', 'Harry', 'Phil', 'Eberechi', 'Trent', 'Cole'],
  POR: ['Bruno', 'Rafael', 'João', 'Vitinha', 'Gonçalo', 'Nuno', 'Diogo', 'Pedro'],
  NED: ['Cody', 'Xavi', 'Virgil', 'Denzel', 'Justin', 'Joshua', 'Quinten', 'Ryan'],
  GER: ['Jamal', 'Florian', 'Joshua', 'Antonio', 'Kai', 'Maximilian', 'Karim', 'Alejandro'],
  JPN: ['Koji', 'Yuki', 'Takumi', 'Daichi', 'Hiroki', 'Ritsu', 'Junya', 'Zion'],
  KOR: ['Min-jae', 'Hee-chan', 'Kang-in', 'Jae-sung', 'Seungwoo', 'Lee', 'Woo-young', 'Hyun'],
  IRN: ['Sardar', 'Mehdi', 'Ali', 'Karim', 'Alireza', 'Mojtaba', 'Shojja', 'Ramin'],
  QAT: ['Akram', 'Almoez', 'Boualem', 'Hassan', 'Karim', 'Pedro', 'Lucas', 'Ahmed'],
  AUS: ['Mathew', 'Jackson', 'Ajdin', 'Martin', 'Harry', 'Keanu', 'Nestory', 'Craig'],
  CAN: ['Alphonso', 'Jonathan', 'Cyle', 'Liam', 'Sam', 'Tajon', 'Richie', 'Junior'],
  USA: ['Christian', 'Weston', 'Tyler', 'Gio', 'Antonee', 'Folarin', 'Malik', 'Johnny'],
  MEX: ['Jesús', 'Raúl', 'Edson', 'Hirving', 'Santiago', 'Guillermo', 'Julián', 'Orbelín'],
  SEN: ['Sadio', 'Ismaila', 'Kalidou', 'Nicolas', 'Iliman', 'Pape', 'Cruzy', 'Habib'],
  MAR: ['Achraf', 'Hakim', 'Youssef', 'Nayef', 'Sofyan', 'Amine', 'Abde', 'Walid'],
  NGA: ['Victor', 'Wilfred', 'Alex', 'Kelechi', 'Ademola', 'Frank', 'Samuel', 'Calvin'],
  CMR: ['Karl', 'André', 'Bryan', 'Jean', 'Georges', 'François', 'Nicolas', 'Oumar'],
  CIV: ['Nicolas', 'Sébastien', 'Franck', 'Yan', 'Amad', 'Simon', 'Jean', 'Oumar'],
  ECU: ['Enner', 'Piero', 'Moisés', 'Romario', 'Gonzalo', 'Alan', 'Jhegson', 'Kendry'],
};

/**
 * Deterministic squad for a nation: 11 starters in a 4-3-3 plus four impact subs.
 * Same seed string always yields the same players, so a nation keeps its squad for a whole
 * tournament (and two machines reproduce the same match).
 */
/**
 * One shuffled run through a nation's given-name pool, extended with the confederation's so
 * a 15-man list never repeats a first name while it can avoid it.
 */
function firstNames(team, rnd) {
  const own = (FIRSTS_BY_TEAM[team.id] || []).slice();
  const pool = own.slice();
  const seen = new Set(pool);
  for (const n of FIRSTS_BY_CONFED[team.confed] || FIRSTS) if (!seen.has(n)) (pool.push(n), seen.add(n));
  for (const n of FIRSTS) if (!seen.has(n) && pool.length < 18) (pool.push(n), seen.add(n));
  const flip = (arr, from, to) => {
    for (let i = to - 1; i > from; i--) {
      const j = from + Math.floor(rnd() * (i - from + 1));
      [arr[i], arr[j]] = [arr[j], arr[i]];
    }
  };
  if (own.length) flip(pool, 0, own.length);
  flip(pool, own.length, pool.length);
  let k = 0;
  return (surname) => {
    for (let g = 0; g < pool.length; g++) {
      const n = pool[(k + g) % pool.length];
      if (n !== surname) {
        k = (k + g + 1) % pool.length;
        return n;
      }
    }
    return pool[0];
  };
}

export function squadFor(team, seedStr = 'wc26') {
  let h = 2166136261;
  for (const ch of String(seedStr) + team.id) {
    h ^= ch.charCodeAt(0);
    h = Math.imul(h, 16777619);
  }
  const rnd = () => {
    h = (h ^ (h >>> 15)) >>> 0;
    h = Math.imul(h, 2246822507) >>> 0;
    h = (h ^ (h >>> 13)) >>> 0;
    h = Math.imul(h, 3266489909) >>> 0;
    return ((h ^ (h >>> 16)) >>> 0) / 4294967296;
  };
  const slots = ['GK', 'RB', 'CB', 'CB', 'LB', 'DM', 'CM', 'AM', 'RW', 'ST', 'LW'];
  const rateKey = (s) => (s === 'GK' ? 'gk' : s[0] === 'D' || s[0] === 'C' || s === 'RB' || s === 'LB' ? 'def' : s === 'CM' || s === 'AM' ? 'mid' : 'att');
  const nums = [1, 2, 4, 5, 3, 6, 8, 10, 7, 9, 11];
  const firstName = firstNames(team, rnd);
  const squad = slots.map((slot, i) => {
    const base = team.ratings[rateKey(slot)];
    const ovr = clamp(Math.round(base + rnd() * 7 - 3.5), 58, 96);
    const surname = team.surnames[i % team.surnames.length];
    return {
      slot,
      num: nums[i],
      name: surname,
      first: firstName(surname),
      ovr,
      pac: clamp(Math.round(ovr + rnd() * 12 - 5 + (slot === 'ST' || slot === 'RW' || slot === 'LW' || slot === 'AM' ? 4 : 0)), 52, 99),
      shoot: clamp(Math.round(ovr + rnd() * 14 - 6 + (slot === 'ST' || slot === 'RW' || slot === 'LW' ? 6 : slot.startsWith('C') || slot === 'RB' || slot === 'LB' ? -7 : 0)), 45, 99),
      pass: clamp(Math.round(ovr + rnd() * 10 - 3 + (slot === 'CM' || slot === 'AM' || slot === 'DM' ? 6 : 0)), 45, 98),
      def: clamp(Math.round(ovr + rnd() * 10 - 3 + (slot === 'GK' ? 10 : slot === 'CB' || slot === 'RB' || slot === 'LB' || slot === 'DM' ? 7 : -10)), 40, 98),
      age: 19 + Math.floor(rnd() * 15),
    };
  });
  for (let i = 0; i < 4; i++) {
    const slot = ['CM', 'ST', 'CB', 'GK'][i];
    const subName = team.surnames[(i + 5) % team.surnames.length];
    squad.push({
      slot,
      num: 12 + i * 6,
      name: subName,
      first: firstName(subName),
      ovr: clamp(Math.round(team.ratings[rateKey(slot)] - 7 + rnd() * 6), 55, 92),
      pac: clamp(Math.round(team.ratings.mid - 4 + rnd() * 12), 55, 96),
      shoot: clamp(Math.round(team.ratings.att - 8 + rnd() * 12), 50, 94),
      pass: clamp(Math.round(team.ratings.mid - 5 + rnd() * 10), 50, 94),
      def: clamp(Math.round(team.ratings.def - 5 + rnd() * 10), 50, 94),
      age: 18 + Math.floor(rnd() * 16),
    });
  }
  return squad.map((p) => ({ ...p, label: `${p.num} ${p.name}` }));
}

const clamp = (v, a, b) => Math.max(a, Math.min(b, v));

export function teamStrength(team) {
  const r = team.ratings;
  return (r.att * 0.36 + r.mid * 0.26 + r.def * 0.26 + r.gk * 0.12) / 100;
}

// home advantage model: expected goals per 90 for a team rated `s` against `o`
export function xgFor(s, o, { neutral = true, form = 0 } = {}) {
  const diff = (s - o) * 8 + (neutral ? 0 : 0.22) + form;
  return clamp(1.35 + diff * 0.42, 0.18, 4.6);
}

export { F as FLAGS };
