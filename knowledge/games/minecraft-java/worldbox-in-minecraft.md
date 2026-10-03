---
kind: game
title: "A WorldBox world as Minecraft visuals: the Rust sim stays the brain, a Fabric mod renders it"
game: "Minecraft Java Edition"
games_also: ["WorldBox - God Simulator"]
game_version: "Minecraft 26.3 (Fabric Loader 0.19.5, Fabric API 0.161.0+26.3); the simulation is our own Rust crate"
platform: windows
engine: java
route: passthrough
tools: ["Rust 1.88 (offline toolchain)", "Fabric Loom 1.18", "cargo test", "Python 3 (protocol client)"]
anti_cheat: "none (single-player, own world, offline)"
status: working
agents: ["Arena Agent Mode"]
humans: []
date: 2026-10-03
links: ["https://github.com/rehan-remade/universal-modder/tree/main/examples/worldbox-in-minecraft"]
tags: [mashup, passthrough, protocol, json-lines, isometric-renderer, zero-dependency, fabric, god-game, verification]
---

# A WorldBox world as Minecraft visuals: the Rust sim stays the brain, a Fabric mod renders it

> Asked for directly: *"can you combine Minecraft to WorldBox like a WorldBox world but visuals in
> Minecraft?"* Pattern 2 from `skills/mashup-mods` (journaled here as route `passthrough`) — but the guest is **our own** Rust simulation
> (`examples/worldbox-rust-rewrite`), not a game we do not own, so there is no install to point at and
> nothing to patch. The sim publishes JSON lines on `127.0.0.1`; a Fabric mod turns tiles into blocks,
> buildings into structures and creatures into mobs. Built and verified end to end without Minecraft;
> the mod itself has not been run in a real client yet.

## Setup
- Sim side: `examples/worldbox-rust-rewrite/worldforge`, `cargo build --offline` (Rust 1.88 from the npm
  `@rustbin/*` packages; no crates.io).
- Minecraft side: `examples/worldbox-in-minecraft/mc` — Fabric Loom 1.18, Minecraft 26.3, Java 25.
  26.3 ships **unobfuscated**, so there are no mappings to worry about and the same names as the
  passthrough example in this repo (`Blocks`, `BuiltInRegistries`, `EntityTypes`, `level.setBlock`).
- No Minecraft in the sandbox, so the *client* side of the protocol was written twice: once as a Fabric
  mod (for the human) and once as `worldforge mcview`, a Rust client that builds the same block world
  and renders it isometrically (for verification and artifacts). That is the trick that made this
  workable: **test the wire + the mapping in the sandbox, keep the injected part thin.**

## Route and why
`passthrough` (Pattern 2 in `skills/mashup-mods`), one process per side. The alternatives: a loader mod on the *guest* side
would require owning and running WorldBox; a `data`/asset route cannot apply because there is nothing to
convert; a full reimplementation of Minecraft is absurd. Splitting "brain" (sim, Rust) from "renderer"
(host, Minecraft) gives a shippable, testable thing and matches the intent in the question: the *world*
is ours, the *visuals* are Minecraft's.

The design rule that carried it: **the mapping is not the client's business.** Every decision — biome →
block, elevation → column height, building → structure of what, unit → which mob, kingdom → which
concrete — lives in `src/bridge.rs` with unit tests, and the mod only applies indices it is handed.
A renderer that decides nothing cannot disagree with the simulation, and it stays small.

## Build steps
```
# 1. the simulation, publishing on the loopback bridge
cd examples/worldbox-rust-rewrite/worldforge
cargo build --release --offline
./target/release/worldforge serve --size large --seed 20241003 --civs 4 --animals 40 --tps 10

# 2. see it without Minecraft (same messages the mod gets)
./target/release/worldforge mcview --connect 127.0.0.1:25607 --png world --every 200 --scale 8
./target/release/worldforge mcview --connect 127.0.0.1:25607 --png zoom --region 58,40,26,18 --scale 16

# 3. Minecraft: Fabric 26.3 + Fabric API, jar in mods/, superflat void world, then
#    /wf origin   /wf power nuke   /wf pause   /wf step 200   /wf speed 30
cd examples/worldbox-in-minecraft/mc && ./gradlew build

# 4. regenerate the showcase pictures
bash examples/worldbox-in-minecraft/run-artifacts.sh 40
```

## The protocol (the part worth stealing)
JSON lines on TCP. Index-based, append-only, one palette for blocks **and** entity ids.
- On connect, in this order: `hello` (size, `base_y`, `sea_level`, `canvas_top`, palette, entity tag),
  `tiles` (one column per tile: surface id, land height, water top, water flag, up to 4 extra blocks),
  `structures` (absolute `x,y,z,block`), then `frame` every tick (tick, year, population, counts, age,
  state hash, units, villages, kingdoms, chronicle news).
- Per tick afterwards: `edits` (only the tiles that changed, as `index, tile, index, tile…`) plus
  `structures` when buildings appear or complete.
- Client → server: `ping`, `tiles`, `pause`, `step`, `speed`, `power`, `spawn`; the server answers with
  `pong` or `notice` (one line, to the client that asked).

## Verification
- 185 Rust tests: 166 lib (17 of them bridge mapping), 10 e2e, 8 over a **real socket**, 1 doc.
- The socket tests start the server on port 0, connect a real client and assert: message order
  (hello before tiles), one column per tile, that a nuke comes back as `edits`, that `pause` freezes the
  tick, that a second client can attach mid-simulation, and that junk commands do not upset the stream.
- **Every line on the wire parses as JSON** (a test that caught a real bug), and **no message repeats a
  JSON key** (a test that caught another).
- `mcview` renders a live world to PNG (the artifacts in the example's README come from a real bridge),
  and exits non-zero if it ever sees a line it cannot parse.
- NOT verified: the Fabric mod in a real Minecraft client. It compiles only on a machine with the
  Minecraft 26.3 artifacts, which this sandbox could not fetch; the API surface it uses is taken from the
  repo's working passthrough mod for the same version.

## Gotchas
1. **A repeated JSON key silently deletes data.** `frame` had `"villages"` as both a count and an array;
   every parser kept the last, so the village array vanished and only a Python probe saw the whole
   message. Assert key uniqueness in tests when you hand-write JSON.
2. **Never echo `Debug` output to a client.** The bridge answered commands by pushing Rust `{:?}` strings
   down the socket; a client that trusts the protocol hits a parse error. Test that *everything* on the
   wire is valid.
3. **Order the handshake.** Tiles arrived before `hello`, so a client joining mid-stream applied a tile
   field to a world whose size it did not know. Send the shape first, and let a client re-request tiles.
4. **Send the palette once and index everything.** Rows like
   `[[id, mob, col, row, y, hp, max_hp, glow, "name", "#rrggbb"]]` are ~90 bytes per creature; 250 of
   them per tick is 22 KB/s, which is nothing on loopback, and it keeps the client stateless.
5. **Diff tiles, do not resend them.** A 96×64 map is 6,144 columns; sending them every tick is ~2 MB/s.
   Diffing turned steady-state traffic into a handful of tiles per tick — and needed a "resend the whole
   field" path for a client that joins later or changes its origin.
6. **Block writes must be throttled and diffed.** `Canvas` spreads a rebuild over ticks and skips blocks
   that are already correct; a full 96×64×44 canvas is ~9k writes, which it does in one pass, but a
   careless version stalls the server thread on every frame.
7. **Frame the picture from what is drawn, not from what could be drawn.** The isometric renderer sized
   its canvas from the tallest possible column (`canvas_top`), so a zoom looked like a speck in the dark.
   Compute the drawn bounding box.
8. **Zero-dependency PNGs are big.** The in-tree writer stores uncompressed deflate: a 1132×658 map is
   2.2 MB. Re-encoding to compressed zlib in the artifact script took it to 89 KiB (20x) — do that before
   committing, or the repo grows 20x faster than it should.
9. **`pkill -f` from an agent shell kills the agent.** It matched the agent's own command line; the
   toolkit's case-studies warn about exactly this. Use the process tools, or kill by PID.

## Assets
None from either game. Every block and mob the mod places is one Minecraft already ships; the sim's own
art is flat-colour hexes in a PNG the crate writes itself. Nothing from WorldBox is used at all.

## Cost and time
One session, two halves: the Rust side (bridge mapping, protocol, server, JSON reader, isometric client,
8 socket tests) and the Minecraft side (4 small Java files: mod entry, bridge client, canvas, commands).
Four protocol bugs and one renderer-framing bug, all found by tests rather than by running the game.

## Open questions
- Does the mod compile and run on a real 26.3 client? Needs the human's machine.
- Should Minecraft fights mean something? That needs damage to travel back to the sim — the passthrough
  example's mob-proxy pattern is the place to start.
- A rolling canvas that follows the player, instead of one fixed rectangle of the world.
- Replacing the block-based build with a custom renderer (the passthrough route: publish colour+depth and
  composite) would look far better but needs a second process and shared memory; the block route needed
  no GPU access at all.
