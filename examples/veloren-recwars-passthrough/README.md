# Tanks of Veloren — a passthrough merge of two Rust games

Two Rust games, running side by side on one machine, wired into each other over loopback:

* **[Veloren](https://github.com/veloren/veloren)** — a multiplayer voxel RPG. Here it is the **host**:
  the world, the camera, the renderer, the player on the ground.
* **[RecWars](https://github.com/martin-t/rec-wars)** — a 2D tank battle game (an open-source Rust port
  of the Windows game *RecWar*). Here it is the **guest**: its own match keeps running with its own
  bots, its own HUD and its own window — and Veloren can see it, drive it, and stand on its ground.

Veloren exports the terrain around you; RecWars rebuilds its arena from that heightfield, so its tanks
drive on the ground Veloren is standing on. Veloren receives the tank match's state and frames, draws
the vehicles as depth-tested sprites at their mapped world positions, and shows the guest's actual
framebuffer on a framed quad — a "holotable" in the world, like a screen you can walk up to. Hold
focus and Veloren's camera flies the tanks; let go and RecWars' own bots take the wheel back.

This is the [mashup-mods **pattern 2 (passthrough)**](../../skills/mashup-mods/SKILL.md) flavour of a
merge: neither game is ported into the other, both keep running as themselves, and state, frames and
input cross between them. It is the same shape as the
[Minecraft × GTA V reference](../../examples/minecraft-gta5-passthrough/): two games, one shared
world, one of them driving.

```
        Veloren (host)                                  RecWars (guest)
  ┌───────────────────────────┐                  ┌───────────────────────────┐
  │ world, camera, renderer   │   terrain  ──►   │ map built from Veloren's  │
  │ sprite pass: guest tanks  │   control  ──►   │ heightfield (tanks drive  │
  │ holotable: guest screen   │   input    ──►   │ on Veloren's ground)      │
  │ one hook in Session::tick │   state    ◄──   │ vehicles, shots, kills    │
  └───────────────────────────┘   frames   ◄──   │ every 3rd frame, scaled   │
             ▲                    127.0.0.1       └───────────────────────────┘
             └───────────────────── :47811 ──────────────────────────────────┘
```

## How it works

**One crate, both games.** `bridge/` is `um-bridge`, a `std`-only Rust crate with no dependencies. Both
games compile the same crate, so the wire format cannot drift: the host writes a message, the guest
reads it with the same code. It is copied into each repo as `um-bridge/` by the patches, so neither
game's dependency graph changes.

**Three regions, one control socket.** The two processes share a run directory:

| Region | Direction | Contents |
|---|---|---|
| `terrain` | host → guest | 64×64 grid of 8 m cells, one cell per guest tile, plus sea level and spawn flags |
| `state` | guest → host | vehicles and projectiles (32-byte header, 40-byte entities), plus a 64-entry event ring |
| `frame` | guest → host | BGRA8 frame of the real tank match, scaled down for the trip |

Each region is a small file with a double-slot header and a single 28-byte commit record written last:
a reader only ever trusts a slot whose CRC matches, so no reader can see a half-written frame. The
control socket carries lines like `HELLO`, `CAM`, `FOCUS`, `IN`, `HB`; a peer that goes quiet for two
seconds is declared dead and the merge degrades to "each game plays on alone" instead of hanging.
Formats, limits and the exact byte layouts are in [`docs/PROTOCOL.md`](docs/PROTOCOL.md).

**Mapping is a definition, not a guess.** Guest tile units become metres at 1 unit = 0.125 m, guest
`(0,0)` is Veloren's south-west corner of the exported grid, and yaw converts as
`yaw = normalise(−θ − π/2)`. Cell classification (water → shallow → cliff → sand → snow → rock →
grass) is a pure function of the heightfield, so both sides can compute it and a test can pin it.
The worked vectors are in [`docs/MAPPING.md`](docs/MAPPING.md).

**Frame budget.** RecWars' readback is the expensive part (10-20 ms at 1600×900, by the game's own
comment), so the guest publishes every 3rd frame at half resolution — ~20 Hz of a 640×360 screen, which
is what Veloren's holotable shows. Veloren uploads it with `queue.write_texture` plus one quad.
Terrain re-exports every 2 s, or as soon as the player walks more than two cells. Numbers and the
reasoning are in [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

**Nothing is a fork.** Each side is one new module and one call site. Both are off unless
`UM_BRIDGE=1` / `RCW_BRIDGE=1` (or `--bridge`) is set: with the bridge off, you are playing the
upstream games.

## Files

```
README.md               this file
MODLOG.md               journal: what was done, what is tested, what is unverified
docs/PROTOCOL.md        wire format: control lines, region layouts, limits, failure behaviour
docs/MAPPING.md         coordinates, yaw, cell classification, worked vectors
docs/ARCHITECTURE.md    where the hooks go in each game, frame budget, phase 2 drawing
bridge/                 um-bridge: the crate both games compile (std only)
  src/{lib,protocol,region,control,watchdog,crc32,mapping}.rs
  tests/conformance.rs  runs the shared test vectors
  testdata/vectors.{json,rs}  generated by fakes/test_bridge.py --write-vectors
fakes/                  everything that runs in this sandbox, with no GPU and no toolchain
  bridge.py             reference implementation of the protocol (same wire format as the crate)
  test_bridge.py        119 checks; also emits the crate's test vectors
  fake_host.py          a Veloren-shaped host: analytic heightfield, raymarched terrain, sprites,
                        holotable, HUD, mapping oracle
  fake_guest.py         a RecWars-shaped guest: tile arena from the host's heightfield, tanks, bots,
                        BGRA frames, HUD
  run_demo.py           one command: both fakes, 11 oracles, summary.md, PNGs and a GIF
  live_server.py        both roles in one process, MJPEG in the browser + a control pad
patches/                reference patches against the pinned upstream commits
  veloren-host.patch    new voxygen/src/bridge/mod.rs + one hook in Session::tick
  recwars-guest.patch   new src/bridge.rs, one Map constructor, three call sites in main.rs
  make_patches.py       generates and verifies both patches (git apply --check)
  README.md             what is verified, what is not, and the rough edges to expect
```

## Requirements

| For | You need |
|---|---|
| the fakes and the demo (this repo, right now) | Python 3, `pillow`, `numpy` |
| the live browser preview | the same, and a browser |
| the real games | Rust (Veloren pins its toolchain with `rust-toolchain`), a Vulkan-capable GPU and drivers, and a checkout of each game at the pinned commit |

Nothing in this example ships game code, assets or saves. The recon clones are what the patches are
written against; the games are built from their own repositories.

## Build, install, run

### The fakes (what you can run here, today)

```sh
cd fakes
python3 run_demo.py --duration 22
# → out/summary.md (11/11 oracles), out/merge_frame.png, out/host_view.png, out/frames/merge.gif
# exit code 0 means every oracle passed

python3 test_bridge.py            # 119 checks of the protocol itself
python3 live_server.py --port 8090   # then open the preview: side-by-side MJPEG + a control pad
```

`live_server.py` runs both roles in one process (in separate threads: the host is single-threaded and
would otherwise starve the guest's simulation) and serves `/`, `/stats`, `/cmd?k=forward&d=1`,
`/focus?on=1`. It binds `0.0.0.0` so the sandbox preview can reach it; everything it talks to is still
loopback-only.

### The real games

```sh
# 1. RecWars (guest) — start it first, it listens on 127.0.0.1:47811
git clone https://github.com/martin-t/rec-wars.git && cd rec-wars
git checkout 201690250a9a04ea9b9eb36dbec3bd84c211b0cd
cp -r <this-example>/bridge ./um-bridge
git apply <this-example>/patches/recwars-guest.patch
RCW_BRIDGE=1 cargo run --release -- local     # or: cargo run --release -- local --bridge

# 2. Veloren (host)
git clone https://github.com/veloren/veloren.git && cd veloren
git checkout 585a91b4a76fcf5df7a4851127cf5907a3ce34df
cp -r <this-example>/bridge ./um-bridge
git apply <this-example>/patches/veloren-host.patch
UM_BRIDGE=1 cargo run --release -p veloren-voxygen
```

Both patches apply cleanly to those commits (`git apply --check`, verified — see
[`patches/README.md`](patches/README.md)). **Neither has been compiled**: the machine that wrote them
had no Rust toolchain and no GPU. Treat the first build as a debugging session, and read the "known
rough edges" section before you start.

Environment, if you want to change something:

| Variable | Side | Default | Meaning |
|---|---|---|---|
| `UM_BRIDGE` | host | unset → off | enable the host side |
| `RCW_BRIDGE` | guest | unset → off | enable the guest side (or pass `--bridge`) |
| `UM_BRIDGE_PORT` / `RCW_BRIDGE_PORT` | both | `47811` | control port (must match) |
| `UM_BRIDGE_RUN_DIR` / `RCW_BRIDGE_RUN_DIR` | both | `$TMPDIR/um-bridge-<port>` | run directory (must match) |
| `UM_BRIDGE_SEA` | host | `0.0` | sea level in metres, for cell classification |
| `RCW_BRIDGE_SCALE` | guest | `2` | publish the frame at 1/`n` resolution |
| `RCW_BRIDGE_EVERY` | guest | `3` | publish every `n`-th frame |

## Controls

| Input | Without focus | With focus (host holds it) |
|---|---|---|
| Veloren movement | moves your character | still moves your character |
| Veloren `Tab` (or the `FOCUS` message) | — | hands the guest's input to Veloren's aim keys |
| left/right | — | turn the tank |
| forward/back | — | drive |
| `Q`/`E` | — | rotate the turret |
| fire / mine | — | shoot / drop a mine |
| mouse | — | turn the turret (deltas) |
| releasing focus | — | clears every held key, so nothing drives itself |

Focus exists so one keyboard can serve two games. Releasing focus (or the host going quiet for two
seconds) clears the held-key set; the guest's own bots keep playing either way.

## Tests without GTA

There is no GPU and no game binary in this sandbox, so the fakes *are* the tests. They are built to
catch the failures a real merge actually has:

* `test_bridge.py` — 119 checks on the wire format: commit records and torn-write detection, CRC,
  ring-buffer wraparound, text-line parsing, and the mapping vectors from `docs/MAPPING.md`.
  `--write-vectors` regenerates `bridge/testdata/vectors.{json,rs}`, which is what
  `bridge/tests/conformance.rs` will check the Rust crate against on a machine that can build it. The
  vectors include the worked examples from `docs/MAPPING.md`, so a mapping regression fails here first.
* `run_demo.py` — runs the two fakes against each other for 22 s of simulated time and asserts 11
  oracles: state and frame reads actually happened, no torn reads, events were applied exactly once,
  sprites were drawn, frames contained sprites, terrain revisions agree, and — the interesting one —
  **every sprite is drawn where the documentation says it belongs**. Last run: **11/11 pass**, 158 state
  reads, 158 frames, 496 events applied, 11 sprites in the busiest frame, terrain revision 12 == guest
  revision 12, and a worst-case sprite error of **1.35 px** across 81 sprites.

That sprite oracle is the one that earns its keep. The host records, for a couple of dozen frames, what
the renderer actually drew: the camera pose, the grid in force, and the pixel every sprite landed on.
`run_demo.py` then re-derives those pixels **from the docs alone** — the mapping formula from
`docs/MAPPING.md`, the camera convention from `docs/ARCHITECTURE.md` — and compares. Host and oracle
share no code, so a drifted convention cannot hide: flipping one sign in the mapping turns the result
into "152 px off, 24 sprites drawn that should not even be in view". That is exactly the failure an
earlier version of this merge had (a yaw convention used the wrong way round — invisible in a
screenshot, a 90° error in the data). The check runs in the fakes; the same convention is pinned by the
unit vectors in `test_bridge.py` for the Rust crate.

## Director and video pipeline

Not used here: this example is about the merge, not about capturing it. The fakes can write PNGs and a
GIF (`run_demo.py`), which is enough to review a frame; the toolkit's `um video` and `um fal` groups
are for turning game footage into clips and are deliberately out of scope.

## Safety

* **Loopback only.** The control socket binds `127.0.0.1`, and there is no authentication. This is for
  two games on one machine. Do not port-forward it, do not expose it to a LAN.
* **Single-player and offline.** Both games are run locally, without their official servers, without
  any account, and without touching anyone else's session. Veloren's bridge is client-side only.
* **Own copies only.** The patches are applied to your own clones; the games are built from their own
  repositories. This example redistributes no game code, no assets, and no saves.
* **No anti-cheat, no DRM, nothing bypassed.** Neither game has either, and the merge does not go
  through any online service.
* **Reversible.** Both sides are off by default and are one module plus one call site; `git apply -R`
  puts you back exactly where the pinned commit was.
* **Backups.** If you try this against a game that stores saves, Modlog's first rule applies: copy the
  save directory before the first run. Veloren and RecWars have no progression to lose.

## Credits

* **Veloren** — the Veloren developers, GPL-3.0. <https://veloren.net> · <https://github.com/veloren/veloren>
* **RecWars** — Martin T. and contributors, AGPL-3.0-or-later, a Rust port of the original *RecWar*.
  <https://github.com/martin-t/rec-wars>
* **the merge method** — universal-modder's `skills/mashup-mods/SKILL.md`, pattern 2 (passthrough),
  and the [Minecraft × GTA V reference example](../../examples/minecraft-gta5-passthrough/).
* The bridge crate, the fakes, the patches and the docs in this directory were written for
  universal-modder. No upstream source was copied into this example; the patches are diffs against
  upstream commits, and the fakes are original code shaped like each game rather than code taken from it.

## Lessons

* **The fakes are the merge.** Writing a second, tiny implementation of each game's *interface* (what
  the bridge touches: camera, terrain, entities, frames, input) is what made the protocol correct in an
  afternoon. The real games then only have to match an interface that is already exercised.
* **An oracle that shares code with what it tests tests nothing.** The first version of the position
  check compared the host's steering against the host's own mapping, so it passed even with the mapping
  deliberately flipped. The version that stayed re-derives the answer from the docs, with no shared
  code, and was verified by breaking the mapping on purpose and watching it fail.
* **Channel order and yaw conventions are where passthrough merges die.** Both were wrong at least
  twice here and both were invisible in a screenshot. Every convention in `docs/MAPPING.md` has a test
  vector now, and the demo's oracle checks the *drawn* position against the *stated* one.
* **A shared crate beats a shared document.** The host and the guest compiling the same `um-bridge`
  crate means a protocol change cannot compile on one side only, which is exactly the class of bug that
  a written spec does not catch.
* **Commit records, not locks.** A 28-byte record written last, with a CRC and a sequence number, gives
  a reader a consistent snapshot with no locks, no mutexes across processes, and no doubt about whether
  the game can afford to skip a publish.
* **Say what you did not run.** The patches here apply and are reviewed; they have not been compiled.
  Stating that plainly in the patch header, the README and the MODLOG is cheaper than a user discovering
  it, and it is what makes the rest of the numbers trustworthy.
