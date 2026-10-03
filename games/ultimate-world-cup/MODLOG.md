# MODLOG — Ultimate World Cup 26

Working folder: `games/ultimate-world-cup/`. Journal of how this mod was built, in the order it happened,
including the parts that failed. Anything not written here was lost to context compaction.

## 0. Intake / routing

- Task: "create a ultimate world cup soccer game". No installed game to mod was found (`um scan` has nothing to
  point at: no installed PC games in the sandbox, no root `package.json`, repo is the universal-modder toolkit
  itself).
- Route chosen: **build the game from scratch as a zero-dependency browser game** (HTML5 canvas + vanilla ES
  modules + WebAudio). Rationale: it is the only target that runs in this sandbox, is verifiable headlessly, and
  keeps the repo's "no game files, no binaries, no secrets" publish rules intact.
- Source of truth: `src/*.js` only. No asset files at all — flags, kits, crowd and sound are procedural, so
  there is nothing to extract, convert or gitignore.

## 1. Contract established early (do not break it silently)

- Units: metres, origin on the centre spot, pitch 105 × 68, goals at `x = ±52.5`, home attacks `+x`
  (`teams.home.dir = 1`). `match.goalX(side)` / `ownGoalX(side)` / `inBoxAt()`.
- Engine surface used by everything else: `state ∈ {intro, kickoff, play, restart, celebrate, halftime, shootout,
  results}`, `update(dt, input)`, `consumeEdges()`, `resume()`, `stats`, `events`, `commentary`, `fx`, `banner`,
  `scoreline()`, `clockLabel()`, `possessionPct()`, `result`, `pens`, `venue`.
- Caller loop is fixed-step: `match.update(1/60, input.poll()); match.consumeEdges(); if (state==='halftime') match.resume();`
- Renderer surface: `new Renderer(canvas)`, `resize()`, `draw(match, view)`, `pitchRect()`, `grassRect()`,
  `cameraFor()`, `X()/Y()`, sprite floor via `px(n)` (screen-pixel minimum so sprites never vanish at 390 px).
- Input map (single source: `src/input.js` KEYMAP): WASD/arrows move, Space shoot (hold ≤0.9 s), J pass,
  K through, L tackle/slide, Q switch, E press, Shift sprint, Esc/P pause. Gamepad: RT shoot, LT/RB sprint,
  A pass, X through, B tackle, Y switch.

## 2. What bit me (each of these cost a full debugging cycle)

1. **`export const` re-exported at the bottom of `match.js`** → `SyntaxError: Duplicate export`. `node --check`
   passes; the *import* fails. Always run an import test, not just a parse test.
2. **Every half ended 0-0** for three separate reasons stacked on top of each other:
   - `aiKeeper` used `incoming = b.owner.side === k.side`, so the keeper punched away her own defender's touch
     every frame (~150 phantom passes per half).
   - `updateBall` never applied ground friction because micro-bounces kept `vz` non-zero.
   - capture reach exceeded the release check (`d2 > 7.2`), so every grab was cancelled the next frame — proved
     by a histogram showing possession stints under 6 frames.
3. **`p.tackleCool` was never initialised** → `undefined <= 0` is `false` → the AI never slide-tackled and
   `fouls` was *literally zero* across all seeds. Rule learned: when a stat is exactly 0 on every seed, check the
   gate, not the tuning.
4. **Block detection missed fast shots** (0.33 m/frame tunnelling). Fix: swept point-to-segment distance against
   the segment the ball crossed this frame, not the ball's current position.
5. **A harness that never calls `resume()` at halftime** silently reports half-scores as full-time scores.
6. **ESM harnesses must live in `tools/`** — `/tmp` files cannot `import './src/...'`, and `/tmp` is not persisted.
7. **`TEAMS[i].flag` used `F[id] || F.CRC` while `F` has no `CRC` key** → `undefined` reached `drawFlag`, which
   threw only in the UI (the engine never draws). Now every nation has a spec plus a `FALLBACK_FLAG`.
8. **`P.CC_R` did not exist** (centre-circle radius used by the renderer). Found by rendering to a real canvas,
   not by the stub — a stub canvas swallows bad numbers, `@napi-rs/canvas` throws `Failed to convert napi value
   Undefined into f64`. That error mode is why `tools/preview.mjs` exists.
9. **`pickPenaltyTaker` was referenced but never written** → the shootout crashed on the first `pensPlace()`.
10. **`m.ball.owner === m.active` when both are `null`** is `true` → renderer crash on charge-ring drawing.
11. **Held shoot button + a human set piece**: the shoot edge used to *shoot* instead of taking the restart, and
    the auto-take timeout was 14 s, so an idle human stalled the whole match. Set pieces now have their own
    vocabulary (Space take / J short / K into the box) and the timeout is 6.5 s.
12. The presentation test hung because the shootout left `input.buttons.shoot === true` *and* a stale full-time
    overlay: a leftover overlay blocks `onMatchEnd`, and a stuck button rewires every subsequent decision.

## 3. Balance, as measured

`tools/smoke.mjs full` (16 pairs, full matches incl. extra time and shootouts) and a 6-seed BRA-v-GER aggregate:

```
goals 5.50 · shots 23.2 · on target 19.0 · blocked 1.0 · corners 1.0 · fouls 4.8 · yellow 0.7 · red 0.0
home possession 57%   ·   engine cost ~180 ms per match pair (node, single thread)
```

- Failed tuning theories, do not retry: random "loses control" on every fast pass; capping shot power; counting
  possession inside `tickClock`/top of `updateBall` (frame order reads 0); `checkPossession` first-player-wins;
  a 5%/frame dribble knock-on.
- Honest caveat: on-target ratio (~80%) is arcade-high because accuracy is predicted at the kick, not after
  deflection chaos. Left as a design choice, documented in the README.
- Deliberate omissions: offside, crossbar collision (only the post bounce), substitutions, throw-in animation.

13. **Shootouts gave one nation every kick.** `pensPlace()` read `s.turn` and nothing ever flipped it, so the
    same side kept stepping up until `pensCheck()` called the shootout over: a headless run finished **6-0 with
    `away.kicks` empty**. `pensPlace` now advances the turn and derives `s.round` from the kicks actually taken.
    `scoreline()` had a matching sin — it interpolated the `pens.away` *object*, so the pause card read
    "0-0 6-[object Object] pens". Shootout conversion is now measured in the harness (75% over 8 seeded
    shootouts, sudden death reached at 8-7 in the shell test) instead of assumed.
14. **Settings did nothing when you pressed DONE.** `ui.settings()` called `game.go(game.lastScreen)`, a method
    that does not exist, reading a field that is never assigned — the pattern of a name that looks plausible but
    was never wired. Replaced with `Game.settingsChanged()`: close the pause overlay if one is open, otherwise
    re-render the hub or title, and re-apply the touch mode either way.
15. **The attract match could raise a half-time card over a menu.** `step()` opened `ui.halfTime()` for any match
    in `halftime`; demo matches now just resume, since there is no HUD to resume *from*.
16. **Nations that wear the same colour** (BEL/IRN, USA/CRC, ESP/MAR all read as one mass on a 8 px sprite).
    `resolveKitClash()` in the Match constructor compares both jersey colours by weighted RGB distance
    (threshold 0.22) and gives the away side the most contrasting of white / dark / trim / socks / gold. The first
    attempt compared *luminance* only: it passed two dark blues as distinct and flagged red against blue.
    Distance, not brightness, is the test. Rendering reads `team.kit`, and `banner.side` / `sideOf()` read
    `teams[side].team` — a re-kit must never be able to break the goal banner.
17. **Restoring a saved cup invented its own bookkeeping.** `replayResults` re-derived table rows by hand and then
    *forced* `t.stage` and `t.matchday` from storage, which could park the hub on a round whose fixtures did not
    exist. It now replays each stored result through `applyFixture` (the same applier the live game uses) and
    advances only when a round is genuinely empty. The harness saves at group matchday 3, drops the object,
    restores and compares table, stage and "next fixture" strings.
18. **Portrait framing fought the thumb pad.** The fit kept landscape margins (11 m) on phones, so a 390x844
    viewport showed a tiny pitch floating in a black bowl — and with full-bleed margins the pad sat *over* the
    players. Portrait now uses 1.5 m side margins and puts the pitch origin at 42% of the height, leaving the
    bottom third to the thumbs.

## 4. Presentation decisions

- Dark broadcast look: `#05070c` stage, lime `#B6FF3B` accent, gold for awards; HUD and menus are DOM (crisp text,
  free accessibility), the canvas draws only the world plus the goal banner.
- Crowd is a cached offscreen "bowl": dots are only painted within a limited depth around the turf, otherwise a
  390 × 844 phone became a wall of dots with a stamp in the middle.
- Portrait: pitch fills the width and rides at 42 % height so the thumb pad owns the bottom third.
- Shootout and in-play penalties zoom to the penalty area (`cameraFor`) — otherwise you watch a 7 px ball from
  52 m away.
- Squad names are generated per nation: a shuffled per-country given-name pool extended by the confederation's,
  so a squad never repeats a first name while it can avoid it (previous global pool produced "Koji Lautaro").
- Flags/kits are verified visually through `tools/preview.mjs` sheets rather than assumed.

## 5. How to verify

```bash
node tools/smoke.mjs quick       # engine contract: 240 frames, state machine, stats keys
node tools/smoke.mjs full        # engine + set pieces + shootouts, 16 pairs, balance metrics
node tools/smoke.mjs tourney     # whole tournament: groups → final, bracket integrity, top scorer
node tools/render-smoke.mjs      # ~80 assertions on the DOM shell: every screen, HUD wiring, kit clashes,
                                 # save/restore, touch, resize, a cup played by a scripted synthetic
                                 # international, and a tied knockout that goes to penalties
node tools/preview.mjs preview   # real PNG frames via @napi-rs/canvas (dev-only dependency)
bin/um publish check .           # repo gate: PASS, 246 files, 0 failures, 0 warnings
python3 -m http.server 8000      # then open http://localhost:8000
```

`render-smoke.mjs` is the one that earns its keep: it drives `Game.step()` and clicks the DOM with real input
objects, so it catches unwired buttons, stale overlays and screen crashes that no amount of reading the source
would show. One rule it enforces for future sessions: **any section that opens a card must close it before the
next section**, or `step()` stops opening new cards and every later `clickPrimary()` silently re-clicks a dead
button.

No browser exists in this sandbox (the Playwright Chromium download is blocked, and the Debian mirrors are
unreachable for `--with-deps`), so `tools/domstub.mjs` + `@napi-rs/canvas` are the screenshot substitute. If you
have a real browser, the things left to confirm by eye are: audio unlock on first click, gamepad polling, and
pointer-capture on the touch stick.

## 6. Next steps if this keeps going

- Away-player AI assist when the human idles (a stalled human carrier currently lets the CPU score at will —
  visible in headless runs where nobody presses anything).
- Offside: the only laws-of-the-game gap that changes how the game *feels*, and the engine already tracks the
  data for it — it needs a linesman flag and an indirect free kick, nothing more.
- A "create nation" screen — the flag DSL and the `RAW` row already cover everything it needs.
- Match-engine difficulty is a single knob; a "realism" toggle that scales shot accuracy and keeper reach would
  let the arcade numbers (≈80% of shots on target) be traded away without rebalancing everything else.
