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

## Numbers

* ~7,000 lines of Rust (about 40% of that tests), zero dependencies, `cargo build --offline` clean.
* 145 tests: unit tests per module + `tests/e2e.rs` running the real binary.
* 49 god powers, 24 biomes, 36 species, 13 ages, 13 building kinds.
* 100 years of a 96×64 world (9 villages, 3 kingdoms, 121 people alive) in ~2 s release, ~25 s debug;
  the run's hash is `0xd5c223e4a58f50c1`.

## What is deliberately not here

* No game files, no extracted art, no decompiled code — this is a reimplementation of *mechanics*.
* No UI/GPU: `render` writes ASCII/ANSI and PNG frames.
* No attempt to hit WorldBox's ~374 powers or its exact numbers; where the public docs gave a stat
  (dwarf hp 200 / damage 22 / armor 4 / speed 30 / 80% accuracy), the crate uses it, and everything
  else is a consistent genre-shaped invention.

## Next

* A playable window (`macroquad`) over the same `World` — the CLI/script layer already gives it a brain.
* Port the save format into a "world share" text format so a run can be pasted into a chat.
* If the human wants their own install read: a separate, opt-in tool that reads their WorldBox saves on
  their machine; nothing in this crate needs it.
