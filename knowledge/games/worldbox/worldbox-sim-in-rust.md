---
kind: game
title: "WorldBox rewritten as a deterministic Rust simulation (no game files): patterns for god-game mechanics"
game: "WorldBox - God Simulator"
games_also: []
game_version: "public docs as of 2026-10 (0.21+ mechanics: ages, alliances, plots); no install used"
platform: other
engine: unity-mono
route: reimplementation
tools: ["Rust 1.88 (offline toolchain)", "um kb", "um scan", "cargo test", "cargo clippy"]
anti_cheat: "none (single player, offline; nothing from the game was used or shipped)"
status: working
agents: ["Arena Agent Mode"]
humans: []
date: 2026-10-03
links: ["https://github.com/rehan-remade/universal-modder/tree/main/examples/worldbox-rust-rewrite"]
tags: [simulation, god-game, cleanroom, determinism, hex-grid, agents, kingdoms, disasters, zero-dependency, png, save-format]
---

# WorldBox rewritten as a deterministic Rust simulation (no game files)

> A cleanroom reimplementation of WorldBox's *mechanics* as a zero-dependency Rust crate and CLI:
> hex worldgen, four civilized races, villages that grow from fireplace to walls, kingdoms with kings,
> diplomacy, wars and rebellions, 13 ages, disasters and 49 god powers. No game files, no decompiled
> code, no assets. The oracle is a state hash: same seed + same script ⇒ same world, forever.
> Deliberately **not** verified in the real game (WorldBox is not installed in the build sandbox).

## Setup
- Rust 1.88, `cargo build --offline` / `cargo test --offline`. The sandbox had **no crates.io access**,
  so the crate had to be genuinely dependency-free: in-tree PCG32 RNG, save codec, PNG encoder and
  ASCII renderer. Any agent in a locked-down environment can copy that trick.
- No WorldBox install, no `um scan` hit: this is route `reimplementation` from public documentation
  only (three fan wikis + Steam announcements; links in `examples/worldbox-rust-rewrite/docs/sources.md`).
- `um kb` needed PyYAML; `pip install --break-system-packages` is blocked on Debian images, so:
  `python3 -m venv --system-site-packages ~/.cache/um-venv && ~/.cache/um-venv/bin/pip install pyyaml`,
  then `PYTHONPATH=$PWD ~/.cache/um-venv/bin/python -m um kb check …`.

## Route and why
`reimplementation` (see `skills/mod-any-game` route table). The alternatives were wrong here: a Unity
loader mod would need the game and would only run on the user's Windows box; `decomp-recomp` is
explicitly excluded because no game files or decompiled code may be used. Reimplementing the *rules*
from public docs gives something the human can build, run and modify without owning anything.

## How the game works (what we had to learn)
Enough to drive a simulation, from public docs only:
- **Settlement order**: claim territory → fireplace → town hall → ~3–5 houses → gathering (trees, ore,
  farms). The hall (max tier 3) gates house upgrades.
- **Kingdoms**: the capital is the first village of a colour, and its leader becomes king. Villages
  split off when crowded (also forced by Inspiration / Clone Rain). Low loyalty ⇒ revolt, which drags
  1–6 neighbours; expansion sends settlers above a population threshold.
- **Ages** (0.21+): hope, sun, dark, tears, moon, chaos, wonders, ice, ash, despair — each biases biome
  growth and mood for many years. The Rust build ships 13 (adds skulls, dragons, gods).
- **Powers**: ~374 across 8 tabs. The tab structure (`PowerCategory`) is the useful part; the crate
  implements 49 representative powers.
- **Stats that are documented**: dwarf 200 hp / 22 dmg / 4 armor / 30 speed / 80% accuracy / 4 diplomacy.
  Everything else (costs, siege rates, war thresholds) is a tuned invention.

## Build steps
1. `git clone` the toolkit, `cd examples/worldbox-rust-rewrite/worldforge`.
2. `cargo build --offline --release && cargo test --offline` (185 tests with the Minecraft bridge).
3. `./target/release/worldforge demo --seed 7 --size medium` to watch a world run.
4. `./target/release/worldforge script ../artifacts/growth.wf` writes `frames/year-0*.png`.
5. Field note + PR via the toolkit: `um kb check`, `um kb index`, `um kb pr`.

## Verification
- **Determinism oracle**: `World::state_hash()` (FNV-1a over tiles, units, villages, kingdoms, age,
  effects, chronicle length, RNG state, vector lengths). Same script ⇒ same hash, asserted across
  runs, across a save/load round-trip, and across processes (the e2e test writes a save, then runs the
  real binary in a second process and compares hashes).
- **Invariants after chaos**: 3,000+ ticks with 12 powers cast at random tiles, then every tile in
  range, every owner pointing at a live village, every unit's village/kingdom existing, every kingdom's
  cities and king alive.
- **Save codec**: byte-stable encoding, truncated/corrupted saves return `SaveError` instead of
  panicking, and a loaded world keeps evolving identically for 300 more ticks.
- **PNG writer**: byte-for-byte deterministic; header/CRC vectors checked against known values.
- **NOT verified**: in-game behaviour. WorldBox was never launched. Nothing here claims to match its
  balance, numbers or feel — only its shape.

## Gotchas
1. **Borrow checker as a determinism tool.** Every E0499/E0502 in the first build was resolved by
   hoisting the RNG roll out of the block that held `tile_mut`/`unit_mut`. That is the same discipline
   that makes a hash a valid oracle: no hidden state, no order dependence inside a system.
2. **Death slots must not be recycled within the same tick.** Push ids to a `graveyard` and flush before
   allocating, or a dying unit's slot is taken by a newborn mid-tick and stale copies (combat targets,
   kingdom kings, siege references) point at the wrong creature.
3. **Compaction and the graveyard must agree.** `compact()` has to flush the graveyard first, otherwise
   the alive/dead mask it builds is wrong and id→slot remapping silently corrupts references.
4. **Cap the *list*, trust the *counters*.** A 40-building cap per village is necessary for save size,
   but if `has(TownHall)` reads the list it starts lying at scale. Counters (`town_hall_level`, `walls`)
   are the authority.
5. **AI states need an exit condition.** A villager who enters `Attack` and never leaves walks at a wolf
   forever, and if the AI re-orders a move every tick the population explodes (44 → 1,133 units in
   300 ticks, 40 s per 100 ticks). Add give-up distance, homesickness and breeding caps early.
6. **Quadratic falloff for blasts, per-neighbour rolls for fire.** A linear falloff lets a unit three
   tiles from a nuke shrug; one shared "spread direction" roll lets a forest fire die out immediately.
7. **Food must be a real economy.** "Produced every tick, eaten `pop/3` per tick" starves a hamlet
   before its first farm. Reap per tick, eat one meal per villager every four ticks, and let good land
   feed people — otherwise every village dies at year 10 and it looks like a bug in the village code.
8. **Zero dependencies is a feature, not a handicap.** An in-tree PNG encoder is ~200 lines (CRC-32
   table + stored-deflate zlib), and it makes "the renderer is deterministic" a one-line test.

## Assets
None. The PNG writer draws flat-colour hex-ish cells; there is no art to ship and none was generated.

## Cost and time
One agent session. Four `cargo test` iterations on the simulation's *feel* (starvation, stone),
one performance hunt (the 1,133-unit spiral), then the toolkit wrap-up (journal, README, sources,
this note).

## Follow-on: the Minecraft bridge
The same crate now publishes itself for Minecraft (`worldforge serve` + a Fabric mod); that half has its
own note: [A WorldBox world as Minecraft visuals](../minecraft-java/worldbox-in-minecraft.md)
(`examples/worldbox-in-minecraft`). The simulation stayed the brain; the mapping lives in
`src/bridge.rs` with unit tests, so the Minecraft side decides nothing.

## Open questions
- Whether the human wants their own install read (save files, data tables) — that would be a separate
  opt-in reader on their machine, in the `data`/`asset-only` family, not part of this crate.
- A windowed UI (`macroquad`) over the same `World` is the obvious next layer; the script/CLI layer
  already supplies the game loop.
- Powers coverage: 49 of ~374. The enum, category set and `cast` safety contract scale, the per-power
  behaviour does not exist yet.
