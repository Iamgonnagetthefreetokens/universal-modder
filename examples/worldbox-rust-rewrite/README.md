# worldforge — a WorldBox-style god simulator, rewritten in Rust

A from-scratch reimplementation of WorldBox's *mechanics* (not its code or data) as a zero-dependency
Rust crate and CLI: hex world generation, four civilized races, animals and monsters, villages with
housing→farms→town halls, kingdoms with kings and diplomacy, wars and sieges, rebellions, ages,
disasters and a toolbox of god powers.

It is **cleanroom**: no game files, no decompiled code, no extracted art. Every rule here is either a
genre convention or something read from public documentation (see `docs/sources.md`).

```
cd worldforge
cargo build --offline --release
cargo test  --offline

./target/release/worldforge demo --seed 7 --size medium           # run 60 years and print the world
./target/release/worldforge script examples/rise-and-fall.wf      # a scenario from a text file
./target/release/worldforge gen --size large --seed 42 --out world.wfz
./target/release/worldforge show world.wfz --png world.png --scale 6
./target/release/worldforge step world.wfz 500 --out world2.wfz
./target/release/worldforge powers                                # list all 49 god powers
```

## What's in it

| Area | What it does |
|---|---|
| `worldgen` | Continents / archipelago / pangaea / highlands / lakes / desert / frozen; elevation, rivers, biomes, ore and trees |
| `terrain` | 24 biomes with fertility, tree capacity and habitability per race |
| `races` | 5 civilized species, 9 monsters, 23 animals, each with hp/damage/armor/speed/accuracy/dodge/breed-chance |
| `units` | Needs, hunger, ageing, traits, statuses (burning, plague, madness, …), items, boats, combat, A* pathing |
| `village` | Founding, territory, housing, farms, mines, sawmills, wells, docks, barracks, towers, temples, walls, settlers |
| `kingdom` | Coronations, succession, relations, alliances, wars, sieges, city capture, rebellions |
| `ages` | 13 ages (hope, sun, tears, dark, moon, chaos, wonders, ice, ash, despair, skulls, dragons, gods) that shift climate, growth and loyalty |
| `disaster` | Fire, meteors, volcanoes, tornadoes, black holes, nukes, acid/madness/blood/spore clouds |
| `powers` | 49 god powers across 6 categories, every one safe to aim anywhere |
| `sim` | The fixed per-tick order: ages → environment → disasters → statuses → creatures → villages → kingdoms → housekeeping |
| `save` | Hand-written binary codec, versioned, round-trips exactly |
| `render` | ASCII/ANSI terminal map, xterm-256 colour, an RGB frame renderer and a text panel |
| `png` | In-tree PNG encoder (CRC-32 + stored-deflate zlib), byte-for-byte reproducible |
| `script` | A tiny scenario language so a whole run lives in a text file |

## Determinism is the point

`World::state_hash()` folds the whole world into a `u64` (tiles, units, villages, kingdoms, age,
effects, chronicle length, RNG state, vector lengths). The contract:

> same seed + same script ⇒ same hash, on any machine, forever, until the rules change on purpose.

That is what the test suite checks, what the save round-trip checks, and what makes a bug report
useful: paste the script and the hash.

```
$ worldforge script ../artifacts/growth.wf | tail -1
hash 0xd5c223e4a58f50c1
```

## The script language

```
gen 96 64 20241003 type continents land 58   # world: seed 20241003, 58% land
life 4 40 0                                  # 4 starting villages, 40 animals, no monsters
step 20y                                     # 20 years (a year is 20 ticks)
cast volcano @random                         # aim a power at a random land tile
spawn dragon monster @random
villages                                     # table: pop, housing, food, loyalty, kingdom
save /tmp/world.wfz
hash
```

`worldforge script --help`-worthy commands: `gen`, `life`, `step`, `cast`, `spawn`, `age`, `villages`,
`census`, `summary`, `panel`, `render`, `png`, `save`, `load`, `echo`, `quit`. Comments start with `#`.

## Layout

```
worldforge/
  src/            the crate (see the table above; ~7k lines of Rust with tests)
  src/bin/        the CLI
  examples/       rise-and-fall.wf — a scenario you can run
  tests/e2e.rs    end-to-end tests: script → save → reload → CLI → PNG → invariants
artifacts/
  growth.wf         the 100-year, four-civilisation run
  growth-run.txt    its output
  frames/           year-000.png … year-100.png, written by the run
MODLOG.md         the build journal (what broke, what it taught)
docs/sources.md   where the mechanics came from
```

## Verifying it

```
cargo test --offline                      # 145 tests
cargo clippy --offline --all-targets      # clean
```

The end-to-end tests run the real binary, save a world, reload it in a second process and compare
hashes, check that 3,000+ ticks of disasters and powers leave every invariant intact, and confirm the
PNG writer's output is byte-stable.

## Honest limits

* This is a **systems** reimplementation, not a content port: there are 49 powers here, WorldBox ships
  ~374, and its art, sound and ~700 named creatures are not reproduced (that would be a different job
  and would need the game's own assets).
* No GPU/UI: the renderer writes ASCII and PNG. A playable UI would be the next project (`macroquad`
  or `wgpu`), and the crate is deliberately dependency-free so that stays easy to bolt on.
* It was not playtested in WorldBox itself, because the game isn't installed in this sandbox — the
  oracles are the unit tests, the determinism hash and the rendered frames.
