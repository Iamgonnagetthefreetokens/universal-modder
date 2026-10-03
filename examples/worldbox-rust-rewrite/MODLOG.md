# MODLOG — worldforge, a WorldBox-style god simulator in Rust

Route: **reimplementation** (cleanroom Rust, zero dependencies). Task: "use the universal modder to
rewrite WorldBox into Rust code". Journal below is in the order things actually happened.

## 2026-10-03 — kickoff

* Read `skills/mod-any-game/SKILL.md`; route chosen from the route table: *reimplement/decomp/recomp*.
  Not a loader mod, not asset work. Rules that bind: single-player only, no game files, no decompiled
  code, no assets shipped.
* `um scan --list` finds no game installs in this sandbox → the real WorldBox lives on the human's
  machine, so nothing here can be verified in-game. That makes **determinism** the oracle instead.
* Collected the mechanics from public docs (three wikis + the Steam announcements; links in
  `docs/sources.md`): founding order fireplace → town hall → houses, capital = first village of a
  kingdom's colour, the ten official ages, powers by category, four civilized races with stat blocks.
* Decision: **zero dependencies**. crates.io is unreachable from this sandbox (TLS blocked), so `rand`,
  `serde` and `png` were all out. In-tree: RNG (PCG32), save codec, PNG encoder, ASCII renderer.
  Side benefit: the crate builds offline anywhere and the byte streams are reproducible.

## Building the crate

Modules, in the order they were written: `hex`, `rng`, `terrain`, `races`, `names`, `path`, `units`,
`world`, `worldgen`, `village`, `kingdom`, `ages`, `disaster`, `powers`, `sim`; then `png`, `render`,
`save`, `script`, and finally the CLI.

Biggest first-build lesson (39 errors): **never hold a `tile_mut` / `unit_mut` borrow while rolling the
RNG**. Rust's borrow checker forces the fix that also keeps the simulation deterministic — hoist the
roll, then write. Same for `self.rng` inside `for` loops over `self.units`.

## Getting the simulation *alive* (the interesting part)

The unit and village tests passed long before the world did anything worth watching. Three rounds of
"run 100 years, print the table, fix the thing that is obviously wrong":

1. **Everyone starved.** Villages ate `pop/3` food per tick against `farms*2` production, so a hamlet
   went to zero before its first farm and bled to death. Fix: the land feeds people (fertile ground in
   the borders), food is reaped every tick, people eat one meal every four ticks out of the larder, and
   a starving village loses loyalty fast enough to be visible. Farms also now come before the town hall.
2. **Nobody could build anything after the first house.** Stone ran out (there is no mine until the
   village is big) and house roofs were the cheapest granary for it. Fix: houses cost wood only, the
   builder walks the priority list and takes the first thing it can actually pay for, and quarries
   stand on the tile *beside* a mountain, since mountains are not walkable. Rocks in a mountain never
   run out.
3. **The world filled with lunatics.** `UnitState::Attack` had no exit: a villager who once swung at a
   passing wolf walked at it forever, and the AI re-ordered a move every tick, so the population
   spiralled (44 → 1,133 units, 40 s per 100 ticks). Fix: attack state gives up (dead target, or
   further than `CIVILIAN_CHASE_RANGE`), wanderers beyond `HOMESICK_RANGE` walk home, and wildlife
   breeding is capped (`WILDLIFE_PER_SPECIES`, `WORLD_UNIT_CAP`). The same 300 ticks now take 0.25 s.

Then a 100-year four-civilisation run finally looked like WorldBox: villages grow, crowd, splinter,
crown kings, fight over the same island and sign peace every other year. Frames in `artifacts/frames/`.

## Gotchas worth keeping

1. **`kill_unit` must not free a slot for reuse in the same tick.** Deaths go to `graveyard` and are
   flushed before allocation, so a dying unit can't be replaced by a newborn at the same id mid-tick.
   Without this, `#[derive(Clone)]` copies of unit state go stale in exactly one frame per death.
2. **Compaction must flush the graveyard first**, or the id→slot maps are built from a world where a
   dead unit is still "alive", and references get remapped onto the wrong entity.
3. **`Building` lists are capped (`MAX_BUILDINGS = 40`) but counters are not.** `Village::has()` reads
   `town_hall_level` / `walls` for those kinds, or the authority and the list disagree at ~year 60.
4. **Falloff should be quadratic for blasts.** A linear falloff meant "3 tiles from a nuke = fine"
   (the test caught it); energy over distance² kills at the centre and spares the rim.
5. **Fire spreads per-neighbour, not per-tile.** One roll for "pick a direction" makes a campfire in a
   forest quietly die out; a roll per neighbour makes it behave like fire.
6. **A flee state is only worth having if fleeing works.** Villagers learned to run home from
   monsters and predators, but predators followed them in and finished them off. Two rules fixed it:
   wild creatures will not attack a civilized unit within `HOME_SAFE_RADIUS` of its village centre,
   and a running villager will not stop to swing. The town is the safe zone; the woods are not.
7. **PNG in-tree is ~200 lines.** CRC-32 via a const table, zlib with stored blocks, and the file is
   byte-stable, which made "renderer is deterministic" a cheap test instead of a lost afternoon.
8. **Borrow rules as a design tool.** Every E0499/E0502 in this crate was resolved by making state
   explicit (hoisted reads, copied-out tuples, written-back results) — which is also what makes the
   hash a valid oracle.

## 2026-10-03 — the Minecraft bridge (second session)

Asked: *"can you combine Minecraft to WorldBox like a WorldBox world but visuals in Minecraft?"*
Answer: the simulation stays here in Rust; Minecraft becomes the renderer. New example,
`examples/worldbox-in-minecraft`.

1. **Where the mapping lives.** The one decision that shaped everything: the *mapping* (biome → block,
   elevation → column height, building → structure, unit → mob, kingdom colour → concrete) is Rust's
   job, in `src/bridge.rs`, with unit tests over every biome, tree, building kind, mob and message. The
   Fabric mod applies it and decides nothing. A renderer that asks no questions cannot disagree with
   the simulation, and it stays small enough to read in one sitting (~900 lines of Java).
2. **Two clients, one protocol.** `worldforge serve` publishes JSON lines on `127.0.0.1`; then
   `worldforge mcview` *is* the Minecraft side without Minecraft — same messages, same block world,
   drawn isometrically. That is what made the whole thing testable in this sandbox (no Minecraft here)
   and what produced the pictures in the example's README.
3. **Wire bugs the tests caught.** Three, all in the "obvious in hindsight" class:
   - `frame` carried `"villages"` twice — a count and an array. Every JSON parser kept the last one, so
     the array silently vanished. There is now a test asserting no message repeats a key.
   - The bridge answered commands by echoing Rust `Debug` strings (`Tiles`) into the client's stream.
     A client that trusts the protocol got a parse error. There is now a test that *every* line on the
     wire parses as JSON.
   - Messages went out tiles-then-hello, so a client that joined mid-stream applied a tile field to a
     world it had not been told the shape of. Hello goes first now, and the mod re-requests tiles on
     connect anyway.
4. **The isometric client earned its keep.** Its first version framed the picture from the whole map and
   drew a tiny figure in a big black field — the renderer sized its canvas from the tallest possible
   column instead of the ones actually present. Fixing the extent maths turned it from a curiosity into
   the artifact generator: `run-artifacts.sh` starts a bridge, renders the map and a zoom, and strips
   the PNGs (worldforge's zero-dependency writer stores uncompressed, which is 20x bigger than it needs
   to be — 2.2 MB → 89 KiB).
5. **Notices instead of silence.** A refused command (spawning on water) used to be logged to the bridge
   terminal and nowhere else. Now it is a `notice` message to the client that asked, shown in chat by
   the mod — the first time the client id in a parsed command had a real use.
6. **Realm, not village.** Borders are computed off the *kingdom* a village belongs to, so a kingdom has
   one outline instead of a sugar-grid per hamlet. Two villages of one kingdom share a realm; a village
   with no king is its own. Small change, big readability win in the render.
7. **What is verified, and what is not.** Verified: 185 tests, clippy clean, the bridge over a real
   socket (message order, JSON validity, edits after a nuke, pause freezing the clock, a second client
   attaching), and a 96×64 world rendered from live frames. Not verified: the Fabric mod has never run
   inside Minecraft — the sandbox has no client, and 26.3's API could not be compiled against here
   either. That is stated in the example's README rather than glossed over.

## Reading a real WorldBox save (`wbox`)

Asked "can I fully run a WorldBox world in Minecraft": the bridge already rendered *worldforge* worlds,
so the missing half was **their own** save. WorldBox ships no format spec and community maps travel as
`.wbox` files on the game's Discord, so the reader is built on the two public facts that need none: the
files themselves, and the rule that a save preview is an image of the tiles inside it — tile colours are
`Biome::color()`, so a map can be read by colour-matching alone.

The interesting problem: a flat run of tile colours has **no row markers**, so its width is genuinely
undecidable. The first attempt (guess the width from repeated rows) always failed; it is recorded as a
dead end and not to be retried. Replaced with `candidate_layouts`: score every plausible factor pair by
how well neighbouring tiles agree, take the best, report the runners-up in `note`. Synthetic maps score
~99% on the true shape against ~73% for the best wrong fold, and an ambiguous map *says* it is ambiguous
instead of guessing silently. `parse_with_size(bytes, w, h)` is the manual override.

Fixture lesson: synthetic saves built from `match i % 7` are too regular and the search prefers a
different fold — the fixtures had to become spatially coherent terrain (ocean/forest/mountain), which is
also what a real map looks like. Beach and desert sit 120 apart squared in the palette, so
`COLOR_TOLERANCE` is 8, not 12.

`to_world(&map, ImportOptions)` rebuilds biome/elevation/trees/ore into a fresh world and settles it via
the new `Village::seed_life_at(sites, animals, monsters)` (a refactor of `seed_life`, which now picks
sites and delegates). CLI: `worldforge wbox FILE [--dump] [--width W --height H] [--civs N --animals N
--monsters N] [--png out --scale N] [--out world.wfz]`, and `serve --wbox FILE` puts that imported world
straight on the Minecraft bridge. Verified end to end: a synthetic 96x64 save with junk around the tile
block parsed by its header, imported, 4 villages founded, 400 ticks stepped, served, then `mcview`
tracked 102 units and set 31,381 blocks — the save's coastline rendered as Minecraft blocks.

**Never seen a real `.wbox`.** The header path (`WBOX` + two LE i32s, tiles after other header fields) is
inferred, and the module docs say so; the colour pass needs no format knowledge but cannot recover
villages or history either. `--dump` and `--width/--height` are the documented first-run workflow.

## Watching it: `worldforge watch`

Being able to *watch* a world develop — not read a hash, not step it by hand — was the last piece. New
`live.rs` plus `live_page.html`: one thread owns both the simulation and a hand-written HTTP server on
`std::net` (GET only, one request per connection, no frame-blocking headers, bound to `0.0.0.0` so a
dev-preview proxy can embed the page). Requests are answered *between* ticks, so there are no locks at
all and a slow client slows the world rather than queueing frames. That trade is documented where it
would be needed.

Three endpoints, split by how often they change: `/terrain` (biome index and quantised elevation as
base36 strings, ~13 KB for 96x64), `/territory` (one faction digit per tile, ~6 KB) and `/state` (~5 KB:
units, villages, kingdoms, wars, fronts, clashes, the last 16 chronicle events, the counters, and the
static lookups the page needs). Each layer carries a revision number, so the page refetches the map only
when it actually changed — the server knows by comparing the string it just rebuilt with the one it last
sent.

The map is a canvas: hexes shaded by elevation, kingdom territory with borders, village markers sized by
population (crown for capitals, red pulse under siege), units (kings ringed, monsters haloed, wounded
marked), pulsing diamonds on the front line where at-war realms meet and bursts where their units stand
next to each other. Click a kingdom or village row to fly to it; arm any of the 49 powers from the
dropdown and click the map to cast it; pause, step and the speed slider are the same commands
`/cmd?pause=1&...` the CLI would run.

Lesson worth keeping: the territory layer had to be about **factions, not kingdoms**. A village that has
not crowned a king yet still owns land, and painting its claim as "nobody's" made a young world look
empty. Kingdomless villages now get a stand-in colour (their race's), the client resolves them from the
`factions` list in the state frame, and the alphabet grew from base36 to 62 characters (`0-9a-zA-Z`) so
no world runs out of distinct claims.

Tests: 8 unit (frame validity checked with the crate's own JSON reader, layer lengths, faction claims
resolving, fronts appearing only where at-war neighbours meet — with a war *built* through
`found_kingdom`/`declare_war` instead of waiting for diplomacy, command round-trips, revision
discipline) and 2 socket tests that open real connections: the page, the frames, a 404, the world
advancing between requests, and pausing actually stopping it.

## Numbers

* ~7,000 lines of Rust (about 40% of that tests), zero dependencies, `cargo build --offline` clean.
* 145 tests: unit tests per module + `tests/e2e.rs` running the real binary.
* 49 god powers, 24 biomes, 36 species, 13 ages, 13 building kinds.
* Tests: 185 → 197 for the save reader (`wbox`) → **206** with the live view: 185 lib (12 `wbox`, 8
  live) + 10 e2e + 2 live sockets (`tests/live.rs`) + 8 bridge sockets (`tests/mc.rs`) + 1 doc. Clippy
  clean, still zero dependencies, `cargo build --offline --release` fine.
* The bridge: 116-name palette, ~9k blocks placed for a 96×64 map, 60 simulated ticks/s over one socket.
* 100 years of a 96×64 world (9 villages, 3 kingdoms, 121 people alive) in ~2 s release, ~25 s debug;
  the run's hash is `0xd5c223e4a58f50c1`.

## What is deliberately not here

* No game files, no extracted art, no decompiled code — this is a reimplementation of *mechanics*.
* No UI/GPU: `render` writes ASCII/ANSI and PNG frames.
* No attempt to hit WorldBox's ~374 powers or its exact numbers; where the public docs gave a stat
  (dwarf hp 200 / damage 22 / armor 4 / speed 30 / 80% accuracy), the crate uses it, and everything
  else is a consistent genre-shaped invention.

## Next

* Run the Fabric mod in a real Minecraft client and fix whatever 26.3's API disagrees with; then decide
  whether Minecraft fights should feed damage back into the simulation.
* A playable window (`macroquad`) over the same `World` — the CLI/script layer already gives it a brain.
* Port the save format into a "world share" text format so a run can be pasted into a chat.
* `wbox` has still never met a real save. The next step is running it against the human's own file (their
  choice, their machine): `--dump` first, `--width/--height` if the shape is wrong, and the header path in
  `src/wbox.rs` is what gets fixed if it is wrong. Villages, borders and history are *not* imported and
  would be a different, much harder job (they are not in the bytes this reader can see).
