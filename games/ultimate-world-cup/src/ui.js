// =============================================================================
//  UI: plain DOM screens over the canvas. No framework, no build step.
//  Every screen is a function that returns an element; `App.go()` swaps them.
// =============================================================================
import { TEAMS, TEAM_BY_ID, STADIUMS, squadFor } from './teams.js';
import { drawFlag } from './flags.js';
import { DIFFICULTY } from './match.js';
import { STAGE_LABEL, groupTable, topScorer } from './tournament.js';
import { sound } from './audio.js';

export function h(tag, props = {}, ...kids) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(props || {})) {
    if (v === null || v === undefined || v === false) continue;
    if (k === 'class') el.className = v;
    else if (k === 'html') el.innerHTML = v;
    else if (k === 'text') el.textContent = v;
    else if (k.startsWith('on') && typeof v === 'function') el.addEventListener(k.slice(2).toLowerCase(), v);
    else el.setAttribute(k, v);
  }
  for (const kid of kids.flat()) {
    if (kid === null || kid === undefined || kid === false) continue;
    el.append(kid.nodeType ? kid : document.createTextNode(String(kid)));
  }
  return el;
}

export function flagCanvas(team, w = 46, hgt = 30) {
  const c = h('canvas', { width: Math.round(w * 2), height: Math.round(hgt * 2), class: 'flag' });
  c.style.width = w + 'px';
  c.style.height = hgt + 'px';
  const ctx = c.getContext('2d');
  ctx.scale(2, 2);
  drawFlag(ctx, team.flag, 0.5, 0.5, w - 1, hgt - 1, { radius: 3 });
  return c;
}

const btn = (label, fn, cls = 'btn') =>
  h('button', {
    class: cls,
    onclick: (e) => {
      e.preventDefault();
      sound.start();
      sound.ui(true);
      fn && fn(e);
    },
  }, label);

export class UI {
  constructor(root, game) {
    this.root = root;
    this.game = game;
    this.screens = {};
    this.current = null;
  }

  go(name, props = {}) {
    const build = this.screens[name];
    if (!build) return;
    const el = build(props);
    el.classList.add('screen', 'in');
    if (this.current) this.current.remove();
    this.root.append(el);
    this.current = el;
    this.name = name;
    this.root.dataset.screen = name;
    return el;
  }

  clear() {
    if (this.current) this.current.remove();
    this.current = null;
    this.name = '';
    this.root.dataset.screen = '';
  }

  // =========================================================================
  //  Title
  // =========================================================================
  title() {
    this.screens.title = ({ attract = true } = {}) =>
      h('div', { class: 'title-screen' },
        h('div', { class: 'logo' },
          h('div', { class: 'logo-kicker' }, 'FIFA-STYLE ARCADE FOOTBALL · BROWSER NATIVE'),
          h('h1', {}, h('span', { class: 'l1' }, 'ULTIMATE'), h('span', { class: 'l2' }, 'WORLD CUP')),
          h('p', { class: 'tag' }, '32 nations · one trophy · group stage to final, extra time and penalties you actually take.')
        ),
        h('div', { class: 'menu' },
          btn('ROAD TO THE FINAL', () => this.game.pickTeam('tournament'), 'btn btn-primary btn-lg'),
          h('div', { class: 'menu-row' },
            btn('QUICK MATCH', () => this.game.pickTeam('quick')),
            btn('PENALTY SHOOTOUT', () => this.game.pickTeam('pens')),
            btn('WATCH THE AI', () => this.game.watchCpu())
          ),
          h('div', { class: 'menu-row' },
            btn('HOW TO PLAY', () => this.go('help')),
            btn('SETTINGS', () => this.go('settings'))
          )
        ),
        h('div', { class: 'title-foot' },
          h('span', {}, 'Built with the universal-modder pipeline · no assets, no build step · works with a gamepad'),
          this.game.settings.lastTournament
            ? btn('RESUME TOURNAMENT', () => this.game.resumeTournament(), 'btn btn-tiny')
            : null
        )
      );
    return this.go('title');
  }

  help() {
    this.screens.help = () =>
      h('div', { class: 'panel card' },
        h('h2', {}, 'HOW TO PLAY'),
        h('div', { class: 'help-grid' },
          this.keyRow('WASD / Arrows / Left stick', 'Move your player'),
          this.keyRow('Space (hold)', 'Charge a shot — release to strike'),
          this.keyRow('J', 'Pass to the best option in front of you'),
          this.keyRow('K', 'Through ball / lob over the top'),
          this.keyRow('L', 'Slide tackle (or a block, when you have no ball)'),
          this.keyRow('Q', 'Switch to the player nearest the ball'),
          this.keyRow('E', 'Second man presses / team pushes up'),
          this.keyRow('Shift', 'Sprint (costs stamina)'),
          this.keyRow('Esc / P', 'Pause'),
          h('div', { class: 'help-note' },
            h('p', {}, 'You always control the player closest to the ball. When a set piece is yours, aim with the stick, then Space to take it (a direct strike from a dangerous free kick), J to play it short, or K to whip it into the box.'),
            h('p', {}, 'In a shootout you aim with the stick and power it with Space. If the other side is kicking, you control the keeper: move to guess a side, then press Space to dive.'),
            h('p', {}, 'Defending: hold L to lunge at the ball, and stand your ground to charge players off it. Foul in your own box and you concede a penalty.')
          )
        ),
        btn('BACK', () => this.title(), 'btn btn-ghost')
      );
    return this.go('help');
  }

  keyRow(k, v) {
    return h('div', { class: 'keyrow' }, h('kbd', {}, k), h('span', {}, v));
  }

  settings() {
    const s = this.game.settings;
    const row = (label, opts, key, fmt = (x) => x) =>
      h('div', { class: 'setting' },
        h('label', {}, label),
        h('div', { class: 'seg' }, ...opts.map((o) =>
          h('button', {
            class: s[key] === o ? 'on' : '',
            onclick: () => {
              if (typeof o === 'number' || typeof o === 'string') s[key] = o;
              else s[key] = o.value;
              this.game.saveSettings();
              this.settings();
              sound.select();
            },
          }, typeof o === 'object' ? o.label : fmt(o))
        ))
      );
    this.screens.settings = () =>
      h('div', { class: 'panel card narrow' },
        h('h2', {}, 'SETTINGS'),
        row('Difficulty', Object.keys(DIFFICULTY), 'difficulty', (k) => DIFFICULTY[k].label),
        row('Half length', [{ value: 45, label: 'Blitz · 45s' }, { value: 90, label: 'Standard · 90s' }, { value: 150, label: 'Pro · 150s' }], 'halfSeconds'),
        row('Match speed', [0.75, 1, 1.35, 1.8], 'speed'),
        row('Crowd & sound', ['on', 'off'], 'sound'),
        row('Touch controls', ['auto', 'on', 'off'], 'touch'),
        row('Commentary', ['on', 'off'], 'commentary'),
        btn('DONE', () => this.game.settingsChanged(), 'btn btn-primary')
      );
    return this.go('settings');
  }

  // =========================================================================
  //  Team select
  // =========================================================================
  pickTeam(mode) {
    this.screens.pick = () => {
      const chosen = { home: null, away: null };
      const wrap = h('div', { class: 'panel pick-screen' });
      const title = {
        tournament: 'CHOOSE YOUR NATION',
        quick: 'QUICK MATCH — PICK BOTH TEAMS',
        pens: 'PENALTY SHOOTOUT — PICK BOTH TEAMS',
      }[mode];
      const head = h('div', { class: 'pick-head' },
        h('h2', {}, title),
        h('p', { class: 'sub' }, mode === 'tournament' ? 'Seeded into pots by squad strength. You play every match of your group and the knockouts that follow.' : 'You are the home side. Hold Space to shoot, J to pass.'),
        h('div', { class: 'picks' })
      );
      const grid = h('div', { class: 'teamgrid' });
      const detail = h('div', { class: 'team-detail' });
      const picks = head.querySelector('.picks');

      const renderPicks = () => {
        picks.innerHTML = '';
        for (const [side, label] of [['home', 'YOU'], [mode === 'tournament' ? null : 'away', 'OPPONENT']]) {
          if (!side) continue;
          const t = chosen[side] ? TEAM_BY_ID[chosen[side]] : null;
          picks.append(
            h('div', { class: 'pick-slot' + (t ? ' filled' : '') },
              h('span', { class: 'pick-label' }, label),
              t ? flagCanvas(t, 54, 36) : h('span', { class: 'pick-empty' }, '·'),
              h('span', { class: 'pick-name' }, t ? t.name : mode === 'tournament' && side === 'home' ? 'Select a nation' : 'vs'),
              t && chosen.away ? h('span', { class: 'pick-ovr' }, `${t.ovr} · ${TEAM_BY_ID[chosen.away] ? TEAM_BY_ID[chosen.away].ovr : ''}`) : null
            )
          );
        }
      };

      const showDetail = (team) => {
        detail.innerHTML = '';
        const squad = squadFor(team, 'preview');
        detail.append(
          flagCanvas(team, 74, 48),
          h('div', { class: 'td-body' },
            h('h3', {}, team.name, h('span', { class: 'nick' }, team.nick)),
            h('div', { class: 'bars' },
              this.bar('ATT', team.ratings.att),
              this.bar('MID', team.ratings.mid),
              this.bar('DEF', team.ratings.def),
              this.bar('GK', team.ratings.gk)
            ),
            h('div', { class: 'squad' }, ...squad.slice(0, 11).map((p) => h('span', { class: 'pill' }, `${p.num} ${p.name}`))),
            h('div', { class: 'td-meta' }, `${team.confed} · Pot ${team.pot} · Overall ${team.ovr}`)
          )
        );
      };

      for (const team of [...TEAMS].sort((a, b) => a.pot - b.pot || b.ovr - a.ovr)) {
        const card = h('button', {
          class: 'teamcard',
          onclick: () => {
            if (!chosen.home || chosen.away) {
              chosen.home = team.id;
              chosen.away = null;
            } else {
              if (team.id === chosen.home) return;
              chosen.away = team.id;
            }
            showDetail(team);
            renderPicks();
            go.disabled = !(chosen.home && (mode === 'tournament' || chosen.away));
            sound.select();
          },
        },
          flagCanvas(team, 44, 29),
          h('span', { class: 'tname' }, team.id),
          h('span', { class: 'tovr' }, String(team.ovr))
        );
        card.addEventListener('mouseenter', () => showDetail(team));
        grid.append(card);
      }
      const go = btn(mode === 'tournament' ? 'START TOURNAMENT' : mode === 'pens' ? 'START SHOOTOUT' : 'KICK OFF',
        () => {
          if (!chosen.home) return;
          const away = chosen.away || TEAMS[Math.floor(Math.random() * TEAMS.length)].id;
          if (mode === 'tournament') this.game.startTournament(chosen.home);
          else if (mode === 'pens') this.game.startMatch({ home: chosen.home, away, shootoutOnly: true });
          else this.game.startMatch({ home: chosen.home, away, knockout: true });
        }, 'btn btn-primary btn-wide');
      go.disabled = true;
      showDetail(TEAMS[0]);
      renderPicks();
      wrap.append(head, grid, detail, h('div', { class: 'pick-foot' }, go, btn('BACK', () => this.title(), 'btn btn-ghost')));
      return wrap;
    };
    return this.go('pick');
  }

  bar(label, val) {
    return h('div', { class: 'bar' }, h('span', {}, label), h('i', { style: `width:${Math.round(((val - 60) / 40) * 100)}%` }), h('b', {}, String(val)));
  }

  // =========================================================================
  //  Tournament hub
  // =========================================================================
  tournament() {
    this.screens.tournament = () => {
      const t = this.game.tournament;
      const me = t.championTeam;
      const panel = h('div', { class: 'panel tour-screen' });
      panel.append(h('div', { class: 'tour-head' },
        h('div', {},
          h('h2', {}, 'FIFA WORLD CUP 26 · ROAD TO THE FINAL'),
          h('p', { class: 'sub' }, `${STAGE_LABEL[t.stage]} · Matchday ${t.matchday} · ${t.venues || '8 host cities'}`)
        ),
        h('div', { class: 'me' },
          flagCanvas(TEAM_BY_ID[me], 46, 30),
          h('span', {}, TEAM_BY_ID[me].name)
        )
      ));

      const f = t.nextFixture();
      if (f && !t.done) {
        const home = TEAM_BY_ID[f.home];
        const away = TEAM_BY_ID[f.away];
        const venue = t.venueFor(f);
        panel.append(h('div', { class: 'next-match card' },
          h('div', { class: 'nm-label' }, t.stage === 'groups' ? `GROUP ${f.group} · MATCHDAY ${f.matchday}` : STAGE_LABEL[t.stage].toUpperCase()),
          h('div', { class: 'nm-teams' },
            this.nmSide(home, 'HOME'),
            h('div', { class: 'nm-vs' }, 'VS'),
            this.nmSide(away, 'AWAY')
          ),
          h('div', { class: 'nm-venue' }, `${venue.name}, ${venue.city} · ${venue.cap.toLocaleString()} capacity`),
          h('div', { class: 'nm-actions' },
            btn('PLAY MATCH', () => this.game.playUserFixture(), 'btn btn-primary btn-lg'),
            t.stage === 'groups' ? btn('SIMULATE MATCHDAY', () => this.game.simulateRound(), 'btn btn-ghost') : null,
            btn('SETTINGS', () => this.go('settings'), 'btn btn-ghost')
          )
        ));
      } else if (t.done) {
        panel.append(h('div', { class: 'next-match card' },
          h('div', { class: 'nm-label' }, 'TOURNAMENT COMPLETE'),
          h('div', { class: 'champion-line' },
            flagCanvas(TEAM_BY_ID[t.champion], 60, 40),
            h('div', {}, h('strong', {}, `${TEAM_BY_ID[t.champion].name} are world champions`),
              h('span', {}, `Beat ${t.runnerUp ? TEAM_BY_ID[t.runnerUp].name : '—'} in the final`))
          ),
          h('div', { class: 'nm-actions' }, btn('TROPHY CEREMONY', () => this.ceremony(), 'btn btn-primary'), btn('MAIN MENU', () => this.title(), 'btn btn-ghost'))
        ));
      } else {
        panel.append(h('div', { class: 'next-match card' },
          h('div', { class: 'nm-label' }, 'ELIMINATED'),
          h('p', { class: 'sub' }, 'Your nation has finished its World Cup. Follow the rest of the tournament or start again.'),
          h('div', { class: 'nm-actions' },
            btn('SIMULATE TO THE END', () => this.game.simulateTo('done'), 'btn btn-primary'),
            btn('NEW TOURNAMENT', () => this.pickTeam('tournament'), 'btn btn-ghost')
          )
        ));
      }

      // tables + results
      if (t.stage === 'groups') {
        const tables = h('div', { class: 'tables' });
        for (const g of t.groups) {
          const rows = groupTable(t, g.id);
          tables.append(h('div', { class: 'table-card' + (g.teamIds.includes(me) ? ' mine' : '') },
            h('h4', {}, `GROUP ${g.id}`),
            h('table', {},
              h('tr', {}, h('th', {}, '#'), h('th', {}, 'Team'), h('th', {}, 'P'), h('th', {}, 'W'), h('th', {}, 'D'), h('th', {}, 'L'), h('th', {}, 'GD'), h('th', {}, 'Pts')),
              ...rows.map((r, i) => h('tr', { class: (i < 2 ? 'q ' : '') + (r.id === me ? 'me' : '') },
                h('td', {}, String(i + 1)),
                h('td', { class: 'cell-team' }, flagCanvas(r.team, 20, 13), r.team.name),
                h('td', {}, String(r.played)), h('td', {}, String(r.w)), h('td', {}, String(r.d)), h('td', {}, String(r.l)),
                h('td', {}, (r.gd > 0 ? '+' : '') + r.gd), h('td', { class: 'pts' }, String(r.pts))
              ))
            )
          ));
        }
        panel.append(h('div', { class: 'section-head' }, h('h3', {}, 'GROUP STAGE')), tables);
      } else {
        panel.append(h('div', { class: 'section-head' }, h('h3', {}, 'KNOCKOUT BRACKET')), this.bracket(t));
      }
      const results = t.log.slice(-8).reverse();
      panel.append(h('div', { class: 'recent' },
        h('h4', {}, 'RECENT RESULTS'),
        h('ul', {}, ...results.map((r) => h('li', {}, h('span', { class: 'rd' }, STAGE_LABEL[r.stage] || ''), r.line)))
      ));
      panel.append(h('div', { class: 'tour-foot' },
        h('div', {}, 'Records', h('strong', {}, (() => {
          const ts = topScorer(t);
          return ts ? `Top scorer so far: ${ts.name} — ${ts.goals} goals` : 'Golden boot race is still open';
        })())),
        btn('MAIN MENU', () => this.game.toTitle(), 'btn btn-ghost')
      ));
      return panel;
    };
    return this.go('tournament');
  }

  nmSide(team, label) {
    return h('div', { class: 'nm-side' },
      flagCanvas(team, 60, 40),
      h('strong', {}, team.name),
      h('span', {}, `${label} · OVR ${team.ovr}`)
    );
  }

  bracket(t) {
    const rounds = [['r16', 'ROUND OF 16'], ['qf', 'QUARTER-FINALS'], ['sf', 'SEMI-FINALS'], ['final', 'FINAL']];
    const wrap = h('div', { class: 'bracket' });
    for (const [key, label] of rounds) {
      const col = h('div', { class: 'br-round' }, h('h4', {}, label));
      for (const f of t.bracket[key] || []) {
        const mine = f.home === t.championTeam || f.away === t.championTeam;
        col.append(h('div', { class: 'br-match' + (mine ? ' mine' : '') + (f.played ? ' done' : '') },
          this.brSide(t, f, 'home'),
          this.brSide(t, f, 'away'),
          f.pens ? h('span', { class: 'pens' }, `pens ${f.pens.home}-${f.pens.away}`) : null
        ));
      }
      if (key === 'sf') {
        const tf = (t.bracket.third || [])[0];
        if (tf) col.append(h('div', { class: 'br-match third' }, h('h5', {}, 'THIRD PLACE'), this.brSide(t, tf, 'home'), this.brSide(t, tf, 'away')));
      }
      wrap.append(col);
    }
    return wrap;
  }

  brSide(t, f, which) {
    const id = f[which];
    const team = id ? TEAM_BY_ID[id] : null;
    const score = f.played ? (which === 'home' ? f.h : f.a) : null;
    const won = f.played && (which === 'home' ? f.h > f.a || (f.h === f.a && f.pens && f.pens.home > f.pens.away) : f.a > f.h || (f.h === f.a && f.pens && f.pens.away > f.pens.home));
    return h('div', { class: 'br-side' + (won ? ' won' : '') },
      team ? flagCanvas(team, 22, 15) : h('i', {}, '?'),
      h('span', {}, team ? team.name : 'TBC'),
      h('b', {}, score === null ? '' : String(score))
    );
  }

  ceremony() {
    this.screens.ceremony = () => {
      const t = this.game.tournament;
      const champ = TEAM_BY_ID[t.champion];
      if (!champ) return this.hubCard('NO CEREMONY YET', 'The final has not been played. Finish the cup first.');
      const el = h('div', { class: 'ceremony' },
        h('div', { class: 'trophy' }, h('div', { class: 'cup' }), h('div', { class: 'base' })),
        h('h1', {}, 'WORLD CHAMPIONS'),
        h('div', { class: 'champ' }, flagCanvas(champ, 96, 62), h('div', {}, h('strong', {}, champ.name), h('span', {}, champ.nick))),
        h('div', { class: 'awards' },
          this.award('🏆', 'Winner', `${champ.name}`),
          this.award('🥈', 'Runners-up', t.runnerUp ? TEAM_BY_ID[t.runnerUp].name : '—'),
          this.award('🥉', 'Third place', t.third ? TEAM_BY_ID[t.third].name : '—'),
          this.award('⚽', 'Golden boot', (() => {
            const ts = topScorer(t);
            return ts ? `${ts.name} · ${ts.goals} goals` : '—';
          })())
        ),
        h('div', { class: 'cer-actions' },
          btn('NEW TOURNAMENT', () => this.pickTeam('tournament'), 'btn btn-primary'),
          btn('MAIN MENU', () => this.title(), 'btn btn-ghost')
        )
      );
      return el;
    };
    return this.go('ceremony');
  }

  hubCard(title, body) {
    return h('div', { class: 'card narrow center' }, h('h2', {}, title), h('p', { class: 'sub' }, body), h('div', { class: 'overlay-actions' }, btn('BACK', () => this.tournament(), 'btn btn-primary')));
  }

  award(icon, label, value) {
    return h('div', { class: 'award' }, h('span', { class: 'ico' }, icon), h('div', {}, h('em', {}, label), h('strong', {}, value)));
  }

  // =========================================================================
  //  Match HUD (persistent DOM over the canvas)
  // =========================================================================
  buildHud(host) {
    this.hudHost = host;
    const scoreSide = (side) => {
      const el = h('div', { class: 'sc-side' }, h('canvas', { class: 'flag', width: 52, height: 34 }), h('span', { class: 'sc-ab' }, ''), h('b', { class: 'sc-pts' }, '0'));
      el.dataset.side = side;
      return el;
    };
    const hud = h('div', { class: 'hud' },
      h('div', { class: 'scorebug' },
        scoreSide('home'),
        h('div', { class: 'sc-mid' }, h('span', { class: 'sc-clock' }, "0'"), h('span', { class: 'sc-state' }, 'KICK OFF'), h('span', { class: 'sc-pens' })),
        scoreSide('away')
      ),
      h('div', { class: 'poss' }, h('i', { class: 'poss-home' }), h('i', { class: 'poss-away' })),
      h('div', { class: 'chips' },
        this.chip('shots', 'SH'), this.chip('onTgt', 'ON T'), this.chip('corners', 'COR'), this.chip('fouls', 'FOL'), this.chip('cards', 'CAR')
      ),
      h('div', { class: 'stamina' }, h('span', {}, 'STAMINA'), h('i', {})),
      h('div', { class: 'ticker' }),
      h('div', { class: 'legend' },
        h('kbd', {}, 'SPACE'), h('span', {}, 'shoot'), h('kbd', {}, 'J'), h('span', {}, 'pass'), h('kbd', {}, 'K'), h('span', {}, 'through'), h('kbd', {}, 'L'), h('span', {}, 'tackle'), h('kbd', {}, 'Q'), h('span', {}, 'switch'), h('kbd', {}, 'SHIFT'), h('span', {}, 'sprint'),
        h('button', { class: 'icon', title: 'sound', onclick: () => this.game.toggleSound() }, '🔊')
      )
    );
    host.append(hud);
    this.hud = hud;
    this.scoreEls = { home: hud.querySelector('[data-side=home]'), away: hud.querySelector('[data-side=away]') };
    this.clockEl = hud.querySelector('.sc-clock');
    this.stateEl = hud.querySelector('.sc-state');
    this.possEl = hud.querySelector('.poss');
    this.tickerEl = hud.querySelector('.ticker');
    this.stamEl = hud.querySelector('.stamina i');
    this.pensEl = hud.querySelector('.sc-pens');
    hud.style.display = 'none';
    return hud;
  }

  chip(key, label) {
    const el = h('div', { class: 'chip' }, h('span', {}, label), h('b', {}, '0-0'));
    el.dataset.chip = key;
    return el;
  }

  showHud(on, match) {
    if (!this.hud) return;
    this.hud.style.display = on ? '' : 'none';
    if (on && match) {
      for (const side of ['home', 'away']) {
        const box = this.scoreEls[side];
        const team = match.teams[side].team;
        const c = box.querySelector('canvas');
        const ctx = c.getContext('2d');
        ctx.setTransform(2, 0, 0, 2, 0, 0);
        ctx.clearRect(0, 0, 26, 17);
        drawFlag(ctx, team.flag, 0.5, 0.5, 25, 16, { radius: 2 });
        box.querySelector('.sc-ab').textContent = team.id;
      }
    }
  }

  updateHud(match) {
    if (!this.hud || this.hud.style.display === 'none') return;
    this.scoreEls.home.querySelector('.sc-pts').textContent = String(match.score.home);
    this.scoreEls.away.querySelector('.sc-pts').textContent = String(match.score.away);
    this.clockEl.textContent = match.state === 'shootout' ? 'PENS' : match.clockLabel();
    if (this.pensEl) this.pensEl.textContent = match.pens ? `PENS ${match.pens.home.score}-${match.pens.away.score}` : '';
    const st = match.state;
    this.stateEl.textContent = st === 'play'
      ? (match.pens ? 'PENALTIES' : match.half >= 3 ? 'EXTRA TIME' : match.half === 2 ? '2ND HALF' : '1ST HALF')
      : st === 'celebrate' ? 'GOAL!' : st === 'restart' ? (match.restart ? match.restart.kind.replace('throwin', 'throw-in').toUpperCase() : 'SET PIECE')
      : st === 'halftime' ? 'HALF TIME' : st === 'kickoff' ? 'KICK OFF' : st === 'shootout' ? 'SHOOTOUT' : st.toUpperCase();
    const ph = match.stats.home.possession;
    const pa = match.stats.away.possession;
    const tot = Math.max(0.001, ph + pa);
    this.possEl.querySelector('.poss-home').style.width = `${(ph / tot) * 100}%`;
    this.possEl.querySelector('.poss-away').style.width = `${(pa / tot) * 100}%`;
    const set = (key, v) => {
      const el = this.hud.querySelector(`[data-chip=${key}]`);
      if (el) el.querySelector('b').textContent = v;
    };
    set('shots', `${match.stats.home.shots}-${match.stats.away.shots}`);
    set('onTgt', `${match.stats.home.onTarget}-${match.stats.away.onTarget}`);
    set('corners', `${match.stats.home.corners}-${match.stats.away.corners}`);
    set('fouls', `${match.stats.home.fouls}-${match.stats.away.fouls}`);
    set('cards', `${match.stats.home.yellow + match.stats.home.red}-${match.stats.away.yellow + match.stats.away.red}`);
    if (match.active) this.stamEl.style.width = `${Math.round(match.active.stamina * 100)}%`;
  }

  pushCommentary(text, minute) {
    if (!this.tickerEl || this.game.settings.commentary === 'off') return;
    const line = h('div', { class: 'tick' }, h('b', {}, `${minute}'`), h('span', {}, text));
    this.tickerEl.prepend(line);
    while (this.tickerEl.children.length > 5) this.tickerEl.lastChild.remove();
    line.animate([{ opacity: 0, transform: 'translateY(6px)' }, { opacity: 1, transform: 'none' }], { duration: 260, easing: 'ease-out' });
  }

  // =========================================================================
  //  Overlays: pause / half time / full time
  // =========================================================================
  overlay(builder, opts = {}) {
    this.game.paused = true;
    const box = h('div', { class: 'overlay' + (opts.dismissable ? ' dismissable' : '') }, builder());
    this.game.stage.append(box);
    this.overlayEl = box;
    return box;
  }

  closeOverlay() {
    this.game.paused = false;
    if (this.overlayEl) {
      this.overlayEl.remove();
      this.overlayEl = null;
    }
  }

  pause(match) {
    const box = h('div', { class: 'card pause-card' },
      h('h2', {}, 'PAUSED'),
      h('div', { class: 'pause-teams' },
        h('div', {}, flagCanvas(match.teams.home.team, 52, 34), h('span', {}, match.teams.home.team.name)),
        h('b', {}, match.scoreline()),
        h('div', {}, flagCanvas(match.teams.away.team, 52, 34), h('span', {}, match.teams.away.team.name))
      ),
      h('div', { class: 'overlay-actions' },
        btn('RESUME', () => this.closeOverlay(), 'btn btn-primary'),
        btn('SETTINGS', () => this.go('settings'), 'btn btn-ghost'),
        btn('QUIT MATCH', () => this.game.quitMatch(), 'btn btn-ghost')
      )
    );
    return box;
  }

  statsCard(match, title, sub) {
    const rows = (label, key, pct = false) => {
      const a = match.stats.home[key];
      const b = match.stats.away[key];
      const tot = a + b || 1;
      return h('div', { class: 'stat-row' },
        h('b', {}, pct ? `${Math.round((a / tot) * 100)}%` : String(a)),
        h('span', { class: 'stat-label' }, label),
        h('b', {}, pct ? `${Math.round((b / tot) * 100)}%` : String(b))
      );
    };
    const scorers = match.scorers.length
      ? h('div', { class: 'scorers' }, ...match.scorers.map((s) =>
          h('div', { class: 'scorer' }, h('span', {}, s.minute + "'"), h('b', {}, s.scorer), h('em', { class: s.side }, s.side === 'home' ? match.teams.home.team.id : match.teams.away.team.id), s.ownGoal ? h('i', {}, '(og)') : null)))
      : h('div', { class: 'scorers' }, h('em', { class: 'none' }, 'No goals'));
    const el = h('div', { class: 'card stats-card' },
      h('h2', {}, title),
      sub ? h('p', { class: 'sub' }, sub) : null,
      h('div', { class: 'big-score' },
        h('div', {}, flagCanvas(match.teams.home.team, 58, 38), h('span', {}, match.teams.home.team.name)),
        h('b', {}, match.pens ? `${match.score.home}-${match.score.away}` : `${match.score.home}-${match.score.away}`),
        h('div', {}, flagCanvas(match.teams.away.team, 58, 38), h('span', {}, match.teams.away.team.name))
      ),
      match.pens ? h('div', { class: 'pens-line' }, `Penalties ${match.pens.home.score}-${match.pens.away.score}`) : null,
      h('div', { class: 'stats-grid' }, rows('Possession', 'possession', true), rows('Shots', 'shots'), rows('On target', 'onTarget'), rows('Blocks', 'blocked'), rows('Corners', 'corners'), rows('Fouls', 'fouls')),
      scorers,
      h('div', { class: 'overlay-actions' }, ...this.statsActions(match))
    );
    return el;
  }

  statsActions(match) {
    const g = this.game;
    const out = [];
    if (g.isTournament && g.matchKind === 'user' && match.half === 1) out.push(btn('CONTINUE → SECOND HALF', () => g.continueMatch(), 'btn btn-primary'));
    else out.push(btn('CONTINUE', () => g.continueMatch(), 'btn btn-primary'));
    out.push(btn('RESTART MATCH', () => g.restartMatch(), 'btn btn-ghost'));
    return out;
  }

  halfTime(match) {
    return this.overlay(() => this.statsCard(match, 'HALF TIME', `${match.teams.home.team.name} v ${match.teams.away.team.name} · ${match.venue ? match.venue.name : ''}`)), { dismissable: true };
  }

  fullTime(match) {
    const res = match.result || { winner: 'draw' };
    const g = this.game;
    const title = g.matchKind === 'user' && g.isTournament ? (g.tournamentDone ? 'TOURNAMENT OVER' : 'FULL TIME') : 'FULL TIME';
    const sub = res.winner === 'draw' ? 'Honours even' : `${res.winner === 'home' ? match.teams.home.team.name : match.teams.away.team.name} prevail`;
    return this.overlay(() => {
      const card = this.statsCard(match, title, sub);
      card.querySelector('.overlay-actions').innerHTML = '';
      card.querySelector('.overlay-actions').append(
        ...g.matchEndButtons(match)
      );
      if (res.pom) card.append(h('div', { class: 'pom' }, '⭐ Player of the match: ', h('strong', {}, `${res.pom.name} (${res.pom.team})`)));
      return card;
    });
  }

  matchEndButtons() {
    return this.game.matchEndButtons();
  }

  // =========================================================================
  //  Touch controls
  // =========================================================================
  buildTouch(host) {
    const stick = h('div', { class: 't-stick' }, h('i', {}));
    const knob = stick.querySelector('i');
    const buttons = h('div', { class: 't-btns' },
      this.tBtn('shoot', 'SHOOT'), this.tBtn('pass', 'PASS'), this.tBtn('through', 'THROUGH'), this.tBtn('tackle', 'TACKLE'), this.tBtn('sprint', 'SPRINT')
    );
    const wrap = h('div', { class: 'touch' }, stick, buttons);
    host.append(wrap);
    this.touchEl = wrap;
    let active = false;
    let origin = { x: 0, y: 0 };
    const id = (e) => e.pointerId;
    stick.addEventListener('pointerdown', (e) => {
      active = id(e);
      const r = stick.getBoundingClientRect();
      origin = { x: r.left + r.width / 2, y: r.top + r.height / 2 };
      stick.setPointerCapture(e.pointerId);
      this.moveStick(e);
    });
    stick.addEventListener('pointermove', (e) => {
      if (active === id(e)) this.moveStick(e);
    });
    const end = (e) => {
      if (active !== id(e)) return;
      active = false;
      knob.style.transform = 'translate(0,0)';
      this.game.input.setStick(0, 0, false);
    };
    stick.addEventListener('pointerup', end);
    stick.addEventListener('pointercancel', end);
    this.touchStickEl = stick;
    return wrap;
  }

  moveStick(e) {
    const r = this.touchStickEl.getBoundingClientRect();
    const c = { x: r.left + r.width / 2, y: r.top + r.height / 2 };
    let dx = e.clientX - c.x;
    let dy = e.clientY - c.y;
    const max = r.width * 0.38;
    const m = Math.hypot(dx, dy);
    if (m > max) {
      dx = (dx / m) * max;
      dy = (dy / m) * max;
    }
    this.touchStickEl.querySelector('i').style.transform = `translate(${dx}px,${dy}px)`;
    this.game.input.setStick(dx / max, dy / max, true);
  }

  tBtn(action, label) {
    return h('button', {
      class: 't-btn t-' + action,
      onpointerdown: (e) => {
        e.preventDefault();
        this.game.input.setButton(action, true);
      },
      onpointerup: (e) => {
        e.preventDefault();
        this.game.input.setButton(action, false);
      },
      onpointercancel: () => this.game.input.setButton(action, false),
      onpointerleave: () => this.game.input.setButton(action, false),
    }, label);
  }

  setTouchVisible(on) {
    if (this.touchEl) this.touchEl.classList.toggle('on', !!on);
  }
}

export { STADIUMS };
