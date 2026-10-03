# Architecture — Tanks of Veloren

## 1. Processes and threads

```
┌────────────────────────── Veloren (host) ──────────────────────────┐
│ voxygen: main thread (winit)                                        │
│   Session::tick ──┬─ bridge.send_cam(camera)          → TCP        │
│                   ├─ bridge.export_terrain(terrain)   → region     │
│                   ├─ bridge.read_state()  → Vec<GuestEntity/Event>  │
│                   └─ focus mode: swallow input events → TCP         │
│   Session::render ─── scene first pass:                            │
│                        · guest entities as sprites (host depth)     │
│                        · holotable quad textured with frame.region  │
│ wrapper (event loop): BridgeHandle owns the socket + regions        │
└────────────────────────────────────────────────────────────────────┘
        ▲ 127.0.0.1:47811 (TCP, line protocol)      ▲ state.region / frame.region
        │                                          │ (files, double-buffered)
        ▼                                          │
┌────────────────────────── rec-wars (guest) ────────────────────────┐
│ macroquad main loop (src/main.rs:401)                               │
│   cl_input ──► Client::cl_input  ◄── bridge input injection         │
│   update   ──► simulation (untouched)                               │
│   render   ──► macroquad draw calls (untouched)                     │
│   bridge.tick ─┬─ publish state.region (entities + events)          │
│                ├─ publish frame.region (throttled, scaled)          │
│                ├─ read terrain.region → Map::from_heightfield       │
│                └─ service the control socket (non-blocking)         │
└────────────────────────────────────────────────────────────────────┘
```

- **No second thread in either game.** The bridge is polled from the loops both games already run:
  `Session::tick` in Veloren, the `next_frame().await` loop body in RecWars. Rationale: the loads are
  small (a heightfield every few seconds, ≤ 1 MB frames at ≤ 20 Hz, a few kB/s of control lines), and both
  engines are single-threaded at the points we touch. A dedicated thread per side is the documented upgrade
  if a 1080p60 frame channel is ever wanted (that is what the Minecraft×GTA V reference did, with shared
  memory + a producer thread).
- **Socket is non-blocking** on both sides (`set_nonblocking`); the bridge keeps a read buffer and never
  blocks a frame on the peer. A full write buffer drops control lines older than the newest of their kind
  (only `CAM`, `NOTIFY`, `IN` are coalescable; `HELLO`/`BYE` are not dropped, they are retried).
- **Ordering rule:** a `NOTIFY kind=state seq=N` is only read as a hint — the host always reads the region
  and uses the header's `seq`, so a lost notification delays a frame at most until the next one, and a
  duplicated notification is harmless.

## 2. Frame flow (guest → host), and the budget

The guest's own renderer is the source of the pixels; the host does not render RecWars, it *displays* it.

| Step | Cost | Note |
|---|---|---|
| `get_screen_data()` (macroquad readback) | **10–20 ms at 1600×900** | measured by the game's author, `src/client.rs:314-321` — the single most important number in this mod |
| CPU downscale + row copy into the region (BGRA) | ~2–4 ms for 640×360 | box filter, `rayon`-free |
| region commit (one `pwrite` of ≤ 1 MB) | < 1 ms on tmpfs/`%LOCALAPPDATA%` SSD | |
| host: `queue.write_texture` (0.9 MB) | < 1 ms | wgpu upload |
| host: 1 quad draw + 1 sprite batch | < 0.5 ms | |

So a publish **must not happen every frame**. Defaults: publish every 3rd frame (`bridge.frame_every = 3`,
≈20 Hz at 60 fps), at `scale = 2` (half resolution, `640×360` from a `1280×720` window; `FRAME` carries it).
The documented *better* path, which the patch leaves a hook for, is to draw the guest's world into an
offscreen `RenderTarget` at publish resolution and read that back with `get_texture_data` — macroquad then
reads 0.9 MB instead of 4.6 MB, and the cost scales with publish size instead of window size. It is behind
the same `bridge.frame_target` cvar because the two paths must produce identical region bytes.

Consequences the host must handle, and does:
- **The guest's frame can be 1–2 guest frames stale.** Interpolate entity sprites (they come from
  `state.region` at 30 Hz with velocities), and never let the holotable's pixels drive gameplay.
- **The guest's window can be resized at any time.** `FRAME` precedes the first commit in a new geometry;
  the host recreates its texture on that line, and ignores a region whose `width/height` disagrees with the
  last `FRAME`.
- **The guest can be minimised** (macroquad stops presenting). The watchdog freezes the holotable on the
  last frame and the host logs `guest stalled`; entity sprites keep updating from the state region, which is
  still being written (the sim keeps running).

## 3. Terrain flow (host → guest)

- The host samples its own loaded terrain (`client.state().terrain()`, a `TerrainGrid` of chunks the client
  already has — no world generation, no extra chunks loaded) on a **64 × 64 grid at 8 m spacing** centred on
  the player, every `bridge.terrain_period = 2 s`, or immediately when the player moves more than
  `cell_m * 2` from the last export's centre.
- 4096 samples at ~1 µs each = a few ms once every 2 s; the region is 4096 × 8 B + 32 B = 32 800 B.
- `revision` bumps per export; the guest rebuilds its `Map` only when `revision` changes (a map rebuild
  reallocates a 64 × 64 tile grid in RecWars — cheap, but it also resets nothing: the guest moves vehicles
  to the nearest non-wall tile after a rebuild, and if a vehicle ends up inside a wall it is nudged out
  along the shallowest gradient).
- **This is the "collision back-channel"** of Pattern 2: Veloren's ground becomes the guest's ground, so
  the merge is not just a picture-in-picture. Water, cliffs and snow come along as surfaces the guest
  already understands.

## 4. Input and focus

- `FOCUS on=1` is host-initiated (default key `F6` in the host patch). While it is on, the host stops
  feeding its own input mapping and forwards `IN a=<action> d=<0|1>` + `MOUSE dx dy`; the guest replaces its
  `ClientInput` (`src/input.rs:15`) with the accumulated bridge state for that frame.
- Focus is **one-way at a time**: the guest's own window still receives input if the user clicks it, so the
  guest also stops reading its own keyboard while `FOCUS on=1` (`bridge.exclusive_input = true`), otherwise
  two input sources fight.
- Any of these ends focus: `FOCUS on=0`, 2 s of silence from the host (watchdog), or `BYE`. Releasing focus
  releases all bridge-held actions, so a tank never drives away on its own.
- The mouse is sent as **deltas**, never absolute pixels: the two windows have different sizes, and the host
  grabs the cursor while focused anyway.

## 5. Safety, determinism and resource use

- **Offline only.** The control socket binds `127.0.0.1` and hard-refuses a non-loopback `HELLO`. The bridge
  never touches RecWars' game netcode (`src/net.rs`), never injects into an online match, and the host patch
  is inert until `--bridge` is passed.
- **Regions are validated, never trusted:** `magic`, `version`, `kind`, size caps (`ent_count ≤ 512`,
  `nx,ny ≤ 128`, `width*height ≤ 1920*1200`), CRC and re-read. Bad data = `Busy`/`Corrupt`, and the bridge
  degrades to "no guest", never to a panic in the render thread.
- **No game files move.** The patches ship code and shaders; the guest's textures stay in the guest's
  install (`um publish check --game` clean). The host draws guest *simulation state*, not guest art: in v1
  the tanks are Veloren-side sprites chosen by kind, so no AGPL asset ever crosses the bridge.
- **Costs are bounded:** frame region is allocated once at `scale` resolution; the point light / particle
  budget of the host is unaffected because the guest adds at most ~128 sprites + 1 textured quad.
- **No locking anywhere.** One writer and one reader per region, enforced by the writer-alive bit.

## 6. Optional: the server hears about it too

Client-side effects are cosmetic. If the guest's explosions should *really* happen in Veloren's world
(server-authoritative), the route is: guest → bridge → voxygen → a server command. Veloren's server has a
WASM plugin API (`plugin/wit/veloren.wit`; loaded at `server/src/lib.rs:300`, dispatched through
`PluginMgr::command_event` at `server/src/lib.rs:1412`) and plugins have no sockets — but the *client* can
send a command, and a plugin can react to it. So the patch ships a thin `chat`-command relay
(`/tank <event> <x> <y> <z>`) that a 40-line plugin turns into real server-side entity/effect spawns. It is
off by default: it is a second moving part and the vertical slice does not need it.

## 7. What is deliberately not in v1

| Not done | Why | Where it would go |
|---|---|---|
| Guest art on the host (real tank sprites) | assets are AGPL and must come from the user's install, converted at runtime | a converter step that reads the guest's `data/vehicles/*.png` and builds a host atlas; `asset-pipeline` skill |
| Depth compositing (guest depth vs host depth) | the guest is 2D; the holotable is a textured world quad, so host depth already orders it correctly | if the guest ever renders 3D, publish depth in a second region slot |
| Host weapon fire → guest damage | the merge runs one way for combat in the slice (guest → host) | `IN a=fire` already exists; a host raycast would send `EVT`-equivalent `HIT` |
| `mmap` instead of positioned reads | correctness first; the frame path is the only one where it matters | `Region::map()` with `MmapMut` behind the same API |
| Multiple guests | one region set = one guest; two guests need a run-dir per guest | a `run_dir` per pair, already a parameter |
