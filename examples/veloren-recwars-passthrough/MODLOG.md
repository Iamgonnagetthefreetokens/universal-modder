# MODLOG — Tanks of Veloren (Veloren × RecWars passthrough)

Journal for this mod, in the order things were learned. Every claim about a game's code carries a
`file:line` reference so the next agent can re-verify in one grep. Written during an agent session on
2026-10-02; the sandbox it ran in could **not** build Rust (see "Environment" at the bottom), so read the
status column of every claim.

## 0. The ask
"Use this [universal-modder] to merge 2 rust games together."

Decisions taken with the user before work started:
- **Games:** the agent picks two open-source Rust games (so both are reachable as source).
- **Pattern:** passthrough — both games run at once and exchange state, render and collision
  (`skills/mashup-mods/SKILL.md`, Pattern 2).
- **Where it lands:** in this repo as a runnable-here-as-possible example: fakes + protocol that execute in
  the sandbox, plus reference patches for the real games.

## 1. Search the knowledge base first
```
$ bin/um kb search "rust"     # nothing
$ bin/um kb search "veloren"  # nothing
```
No prior art. `knowledge/INDEX.md` had no Rust game, so this is new ground — and the field note at the end
of this directory is the first `veloren` note in the base. (The `um` CLI needed `pyyaml`, `pillow`,
`numpy`; installed with `pip install --break-system-packages`.)

## 2. Choosing the pair
Constraint that decided everything: **a passthrough mod needs to own a renderer on both sides.** Closed
Rust games (Tiny Glade, Hydrofoil Generation) are out — no source, no shader hook. So both games are
open source, and we picked by *role*:

| Role | Pick | Why |
|---|---|---|
| Host (the "GTA V" role: big 3D world, host-side plugin draws the guest into its own pass) | **Veloren** `veloren/veloren` @ `585a91b4a76fcf5df7a4851127cf5907a3ce34df` (2026-09-30, mirror of gitlab; GPL-3.0) | Real 3D voxel RPG, wgpu renderer, live upstream, hand-written engine → clean, readable pipelines. 7.6k★ |
| Guest (the "Minecraft" role: owns a simulation, publishes frames + state) | **RecWars** `martin-t/rec-wars` @ `201690250a9a04ea9b9eb36dbec3bd84c211b0cd` (2024-12-28, AGPL-3.0) | A real top-down vehicle shooter (tanks, hovercraft, 8 weapons, bots), macroquad 0.4.13, native + WASM, explicitly built to be moddable, and small (~2.8 MB checkout) |

Also considered: `anima-libera/qwy3` (Minecraft-like, wgpu, 45★ — same *kind* of world as the host, so the
mashup is less interesting), `Technici4n/voxel-rs` (unplayable engine, dead since 2020), `veloren/airshipper`
(launcher, not a game), `esp-rs`… no. Veloren × RecWars wins because the two games are *complementary*: a
first-person voxel world (z-up, metres, terrain chunks) and a top-down 2D arena (y-down, 64-unit tiles).
Everything interesting in the mod is that mismatch.

## 3. Recon — Veloren (host)
Where the host hooks go, with evidence:

| Need | Where | Note |
|---|---|---|
| Per-frame client tick that already computes the camera | `voxygen/src/session/mod.rs:583` `fn tick(&mut self, global_state, events: Vec<Event>)`; camera computed at `:633` `camera.compute_dependents(&client.state().terrain())` | the exact place to send `CAM` and to sample terrain |
| Camera pose for the guest | `voxygen/src/scene/camera.rs:40` `pub struct Camera`, dependents (`view_mat_inv`, `proj_mat`, `cam_pos`, `cam_dir`, …) `:20-36` | `cam_pos` + `cam_dir` are all the guest needs for a focus point and a view cone |
| Player entity / ECS | `client.entity()` (`voxygen/src/session/mod.rs:610`), storages via `client.state().ecs()` | `comp::Pos`, `comp::Ori` if a player-anchored placement is wanted |
| Terrain sampling | `client.state().terrain()` (`voxygen/src/session/mod.rs:633`) → `common::terrain::TerrainGrid` of *loaded* chunks | heights around the player; `TerrainChunkSize::RECT_SIZE = 32×32` blocks (`common/src/terrain/mod.rs:49`, `TERRAIN_CHUNK_BLOCKS_LG = 5` at `:45`), 1 block = 1 m in world units |
| World generator (fallback / offline) | `world/src/lib.rs:304` `pub fn sample_columns()` → `Sampler<Sample = Option<ColumnSample>>` | for a server-side or headless export |
| Render pass to draw the guest into | `voxygen/src/scene/mod.rs:1552` `if let Some(mut first_pass) = drawer.first_pass()` inside `Scene::render`; sprite/particle/tether drawers follow | the guest quad goes in here, after sprites, so it is depth-tested against terrain and figures |
| First-pass drawer | `voxygen/src/render/renderer/drawer.rs:311` `fn first_pass()`, `:1023` `pub struct FirstPassDrawer`, `:1268` `SpriteDrawer`, `:1428` `draw_ui()` | a new `draw_guest_screen()` sits next to these |
| Pipeline registration | `voxygen/src/render/pipelines/mod.rs` (one `pub mod` per pipeline), `voxygen/src/render/renderer/pipeline_creation.rs:474` (`InterfacePipelines`) / `:528` (`IngameAndShadowPipelines`), `Renderer` struct at `voxygen/src/render/renderer/mod.rs:136` | add `guest_screen.rs` + one field on `Renderer` |
| Shaders | `assets/voxygen/shaders/*.glsl` (e.g. `sprite-vert.glsl`, `sprite-frag.glsl`), compiled through `voxygen/src/render/renderer/compiler.rs` | new pair of GLSL files ship with the patch |
| Input events to route | `voxygen/src/window.rs:83` `pub enum Event` — `InputUpdate(GameInput, bool)`, `MouseButton(_, PressState)`, `CursorPan(Vec2<f32>)`, `AnalogGameInput(_)`; consumed in a `for event in events` loop at `voxygen/src/session/mod.rs:769`, "pass all other events to the scene" at `:1367` | bridge focus mode intercepts here |
| Server-side reactions (optional, later) | `pluginexamples` + `plugin/wit/veloren.wit` (`server-events`, `actions`, `information` interfaces); loader at `server/src/lib.rs:300` (`PluginMgr::from_asset_or_default`, feature `plugins`) | plugins are **WASM, server-side, no sockets** → guest events reach the server by the client sending a server command (`server/src/lib.rs:1412` `plugin_manager.command_event`), not by the plugin dialing the bridge |

Two Veloren facts that shape the design:
- **Nothing in Veloren loads third-party native code.** Plugins are WASM and server-side. So the host mod is
  a *source patch to voxygen*, not a plugin (same as the Minecraft×GTA V reference: the host side there was
  a ReShade add-on + ASI, i.e. code injected into the host process).
- **The client's camera is driven by the player character.** You cannot teleport the Veloren camera to a
  guest tank without moving the player, so the guest is told where the player is and *renders that area*
  instead (see `docs/MAPPING.md`, "camera focus").

## 4. Recon — RecWars (guest)
| Need | Where | Note |
|---|---|---|
| Main loop | `src/main.rs:401-416` — `cl_input` → `update` → `render` → `console.update` → `post_render` → `next_frame().await` | one insert: `bridge.tick(...)` between `render` and `next_frame` |
| Frame readback, proven | `src/client.rs:310` `fn save_screenshot` uses `get_screen_data()`; comment at `:314-321`: **"get_screen_data() takes between 10 and 20 ms at 1600x900"** and macroquad issue #655 (leaks) | the guest's frame publish must be throttled and/or rendered at a lower res — this comment is the budget |
| Input struct + injection | `src/input.rs:15` `pub struct ClientInput { left, right, up, down, turret_left, turret_right, prev_weapon, next_weapon, fire, mine, self_destruct, horn, chat, pause }`; `ClientGame { input1, input2, .. }` (`src/client.rs:54+`); gathered in `Client::cl_input` (`src/client.rs:283`) | host-routed input replaces `self.cg.input1` here |
| State to publish | `src/game_state.rs:15` `GameState { players: Arena<Player>, vehicles: Arena<Vehicle>, projectiles: Arena<Projectile>, ais: Arena<Ai>, .. }`; `Client::init_explosion` (`src/client.rs:448`) | entity + event export |
| Map / units | `src/map.rs:9` `pub const TILE_SIZE: f64 = 64.0`; `Map { surfaces: Vec<Surface>, tiles: Vec<Vec<Tile>>, spawns, bases }` (`:13`), `Surface { name, kind, friction, speed }` (`:281`), `SurfaceKind { Normal, Spawn, Wall, Water, Snow, Base }` (`:305`); tile index/offset math `Map::tile_pos` (`:80`) | the guest's ground truth; a host heightfield becomes one of these maps |
| Where a remote map would be built | `parse_map` / `parse_texture_list` (`src/map.rs:321`, `:348`) are the only constructors → the patch adds `Map::from_heightfield` | |
| Existing network layer | `src/net.rs`, `src/net_messages.rs` (serde + bincode over UDP/WebRTC) | precedent for message design, and proof the game is already split client/server — but the bridge deliberately does **not** reuse it: it must not touch the game's netcode or its online path |

## 5. The design that came out of it
**"Tanks of Veloren":** RecWars keeps running its own window and simulation; Veloren's client renders that
simulation **inside its own 3D world** — tanks, shells and explosions appear as sprites standing on
Veloren's real terrain at the right world positions, and the guest's live framebuffer is drawn on a framed
"holotable" quad in front of the player. Collision flows the other way: Veloren exports the heightfield
around the player and RecWars builds its battleground from it, so the tanks are driving on the hill you are
standing on. A focus toggle hands the keyboard to RecWars, so you can drive a tank from inside Veloren.

- Transport: control channel = TCP line protocol on `127.0.0.1`; bulk state = file-backed shared regions
  with a seqlock-style header. Rationale and exact bytes: `docs/PROTOCOL.md`.
- Units/coords: `docs/MAPPING.md`. Threading, frame budget, watchdog: `docs/ARCHITECTURE.md`.
- Both games are Rust, so the bridge is **one crate, `um-bridge`, linked into both patches** — that is the
  thing that makes a Rust×Rust merge cheaper than a Rust×C++ one, and it is why the protocol is defined in
  Rust types and mirrored by a Python reference implementation (`fakes/bridge.py`) for the tests and fakes.

## 6. What runs where (honesty section)
| Artifact | State |
|---|---|
| `docs/*`, `README.md` | written from the recon above; `README.md` covers the fakes, the real games, the controls and the limits |
| `fakes/bridge.py` (protocol reference) + `fakes/test_bridge.py` | **runs here**; conformance vectors in `bridge/testdata/vectors.json` are generated by it |
| `fakes/fake_guest.py`, `fakes/fake_host.py`, `fakes/run_demo.py` | **runs here**; produces composite frames, a GIF and `stats.json` in `fakes/out/` |
| `fakes/live_server.py` | **runs here**; browser preview of the same traffic (no GPU needed) |
| `bridge/` (`um-bridge` crate) | written against the vectors; **not compiled here** — no Rust toolchain in the sandbox (all toolchain mirrors blocked). `cargo test -p um-bridge` on a real machine is the first thing to run |
| `patches/veloren-host.patch`, `patches/recwars-guest.patch` | **`git apply --check` verified** against the pinned upstream commits; **not compiled**, no GPU/game present |

## 7. Environment (why nothing Rust was compiled)
The sandbox has `python3`, `node`, `npm`, `gcc`, `g++`, `make`, `git` and network access to
`pypi.org`, `registry.npmjs.org` and `api.github.com`/`github.com` git. It does **not** have, and cannot
fetch: `rustc`/`cargo` (`static.rust-lang.org`, `rsproxy.cn`, USTC/Tsinghua/SJTU mirrors, Debian/Ubuntu
mirrors, conda, GitHub release assets — all unreachable), a GPU, either game, or its assets. So: everything
that needs Rust or a GPU is a reviewed reference implementation, everything else is executed and its output
is committed under `fakes/out/`.

## 8. Next steps for a human with a real machine
1. `git clone` both games at the pinned commits, apply the patches (`patches/README.md`), build
   (`cargo build --release`; Veloren needs its asset download and a GPU).
2. Run `recwars local` with `bridge on`, then run Veloren with `--bridge`; expect the holotable in front of
   the player and a heightfield exported within ~2 s.
3. Verify the two oracles the reference example used: (a) `MAP` revision changes when the player crosses a
   chunk border, (b) a guest explosion raises the host's event counter and the local particle burst.
4. Record a 20–45 s clip (`um win record`), then `um kb pr` the field note.

## 9. The oracle that had to be replaced (and why)

The demo's position check started as "the host's camera yaw versus the direction to the guest's mapped
player": cheap, and it caught a real bug (the host steering with the math convention while Veloren's
yaw is `(-sin, cos)`). But when the demo was re-run at the end of the session it flipped between
medians of 8° and 73° — because (a) the host's walk script stopped steering whenever the ground ahead
was steep, so it backed up without turning, and (b) the guest's idle local player is regularly shot by
the bots and respawns across the arena, so the target teleports away from anything the host can track.

Fixing (a) was a one-line fix in `fake_host.script_walk` (steering is never gated on the ground). For
(b) it became clear the check was also *circular*: the host steers with the same mapping it was being
checked against, so a deliberately flipped mapping still scored near zero. It was replaced by the sprite
oracle: the host records the camera pose, the grid in force and the pixel each sprite was drawn at, and
`run_demo.py` re-derives those pixels from the documentation alone. Verified the honest way — by
sabotaging `guest_to_host` (one sign) and watching it fail with "152 px off, 24 drawn-but-offscreen",
then restoring it and watching 11/11 pass twice.

The old yaw metric is still recorded in `host_stats.json` (`mapping_error_deg_*`) as a diagnostic, but
it no longer gates the demo: it measures tracking lag, not correctness.

## 10. Final status (end of the session)

Everything below was executed in the sandbox, on the pinned commits and the commits noted. Anything not run
says so.

| Artifact | Verified how | Result |
|---|---|---|
| `fakes/bridge.py` + `fakes/test_bridge.py` | `python3 fakes/test_bridge.py` | **119/119 checks pass** (commit records, torn-read detection, CRC, ring wraparound, text parsing, mapping vectors) |
| `fakes/fake_host.py` + `fakes/fake_guest.py` + `fakes/run_demo.py` | `python3 fakes/run_demo.py --duration 22`, run twice back to back | **11/11 oracles pass both times**: 158-167 state reads, 158-167 frame reads, 0 torn reads, ~500 events applied exactly once, 9-11 sprites max, 137-138 frames containing sprites, terrain revision 12 == guest revision 12, RTT ~64-68 ms, and worst-case sprite placement error **1.28-1.35 px** over 71-81 sprites (limit 3 px, 0 drawn off-view) |
| `fakes/live_server.py` | `python3 fakes/live_server.py --port 8090`, then HTTP probes | `/` 200 (4.6 kB HTML), both MJPEG streams (1.35 MB guest / 0.53 MB host in a 3 s grab), `/stats` JSON with live counters (`state_reads=72`, `frame_reads=71`, `torn=0`, terrain rev 7 and climbing), `/focus?on=1` + `/cmd?k=forward&d=1` reaching the guest (`focus_on: true`, `keys.forward: true`), `errors: []` |
| `bridge/testdata/vectors.{json,rs}` | written by `test_bridge.py --write-vectors` | 63-line `vectors.rs` for `cargo test`, so the Rust crate is checked against the same vectors the Python reference passes |
| `bridge/` crate | brace/paren balance after stripping comments and strings; read against the pinned sources | **never compiled** — no toolchain in the sandbox. Ships with `tests/conformance.rs` for a real machine |
| `patches/veloren-host.patch` (4 files) | `python3 patches/make_patches.py` → `git apply --check` against `veloren@585a91b` | **applies cleanly**; **never compiled** |
| `patches/recwars-guest.patch` (4 files) | same, against `rec-wars@20169025` | **applies cleanly**; **never compiled** |
| the games themselves | — | never built or run here: no Rust toolchain, no GPU, no assets, neither game installed |
| the sprite oracle itself | `fakes/bridge.py` sabotaged (one sign flipped in `guest_to_host`), demo re-run | **failed loudly** (152 px off, 24 drawn-but-offscreen), then passed again after restoring — so the oracle is known to detect a broken mapping, not just to print numbers |

Re-run everything that runs:

```sh
cd examples/veloren-recwars-passthrough
python3 fakes/test_bridge.py                 # 119 checks
python3 fakes/run_demo.py --duration 22      # 11 oracles, exit 0 = pass, artifacts in fakes/out/
python3 patches/make_patches.py              # regenerate + verify both patches
python3 fakes/live_server.py --port 8090     # browser preview of the live merge
```

Artifacts from the last full run: `fakes/out/summary.md` (the oracle table above), `fakes/out/merge_frame.png`
(the composite the oracles score), `fakes/out/host_view.png` (the host's own view: terrain, occluding props,
guest sprites, holotable), plus `fakes/out/frames/merge.gif`, `fakes/out/{host,guest}.log` and
`fakes/out/{host,guest}_stats.json`. The summary and the two stills are committed; the GIF, the logs, the
stats and the region scratch are `.gitignore`d because one command rebuilds them.

### What a human has to do next, in order

1. `cargo test -p um-bridge` (in `bridge/`) — the crate is the only untested-shape code left; expected to
   need small compile fixes.
2. Apply both patches and build. Fix the two added modules (the rough edges are listed in
   `patches/README.md`); nothing else in either game is touched.
3. Add the host's drawing half: `Scene::render`'s first pass (`voxygen/src/scene/mod.rs:1552`) — sprites
   from `bridge::guest_entities()` and one quad from `bridge::guest_frame()`. The fakes implement exactly
   this and are the reference for how the pixels should look (`fakes/fake_host.py`); the projection
   convention the real renderer has to match is the one `run_demo.py` checks independently.
4. Only then judge the merge: the protocol, mapping and failure behaviour are already exercised, so any
   surprise left is in the two games' own code.
