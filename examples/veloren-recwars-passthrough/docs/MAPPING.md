# Mapping — Veloren ↔ RecWars

Two games with nothing in common have to agree on metres, axes, angles and what "ground" means. This is
that agreement; `bridge/src/mapping.rs` and `fakes/bridge.py` implement it, `bridge/testdata/vectors.json`
holds the numbers, and the Rust tests + Python tests both check them.

## 1. The two spaces

| | Veloren (host) | RecWars (guest) |
|---|---|---|
| Units | metres (1 block = 1 m) | "units"; **64 units = 1 tile** (`src/map.rs:9` `TILE_SIZE = 64.0`) |
| Axes | `x`, `y` horizontal, `z` up; right-handed | `x` right, `y` **down** (screen/top-left origin: `map.rs:11` "origin in the top-left corner", `col_row(c, r)`) |
| Yaw | radians, **0 = +Y, counter-clockwise**, from `Dir::forward() = +Y` (`common/src/util/dir.rs:122`) and `yawed_left = rotation_z(+θ)` (`common/src/comp/ori.rs:227`) | radians, **0 = +X, clockwise on screen** (macroquad rotates in a y-down space) |
| Angles in the bridge | radians, both | radians, both |
| Ground | voxel terrain, chunk = 32 × 32 blocks (`common/src/terrain/mod.rs:45`) | 2D tile grid, `Surface { name, kind, friction, speed }` (`map.rs:281`) |

## 2. Constants

| Name | Value | Why |
|---|---|---|
| `TILE_UNITS` | 64.0 | RecWars' `TILE_SIZE` |
| `TILE_M` | 8.0 m | a tile is 8 m of Veloren ground; a RecWars vehicle is ≈ 40 units ⇒ **5 m long** in Veloren, i.e. a real tank next to a 1.7 m player |
| `U2M` | `TILE_M / TILE_UNITS = 0.125` m/unit | one conversion factor for lengths, positions and speeds |
| `cell_m` | `= tile_m` (8.0) in v1 | one terrain cell = one guest tile, so no resampling. A `cell_m ≠ tile_m` variant is a v2 feature (resample the heightfield with a box filter; the protocol already carries both fields) |
| `nx`, `ny` | 64 × 64 | a 512 m × 512 m battleground: comfortably inside Veloren's loaded chunks around the player, and 4096 height samples is nothing to re-export on a revision change |

## 3. Positions

Guest row 0 is the **top** of the map (screen top), and the top of a map in Veloren is the **maximum** `y`;
row index increases as Veloren `y` decreases. So with (from `TerrainHeader`)

```
origin_x, origin_y    = min corner in Veloren metres
origin_y_max          = origin_y + ny * cell_m
U2M                   = tile_m / 64.0
```

**guest → host**

```
host_x = origin_x     + gx * U2M
host_y = origin_y_max - gy * U2M
host_z = height_m at the containing cell      (the guest is 2D; its z is empty, the host supplies it)
```

**host → guest**

```
gx = (host_x - origin_x)     / U2M
gy = (origin_y_max - host_y) / U2M
```

and clamping to `[0, nx*64] × [0, ny*64]` units keeps a wandering player inside the battleground.

### Worked vectors (also in `vectors.json`)

Region: `origin = (1000.0, 2000.0)`, `nx = ny = 64`, `cell_m = tile_m = 8.0` ⇒ `origin_y_max = 2512.0`.

| # | guest (units) | host (metres) | cell (i, j) |
|---|---|---|---|
| A | `(260.0, 196.0)` | `(1032.5, 2487.5)` | `(4, 60)` |
| B | `(0.0, 0.0)` (top-left of the map) | `(1000.0, 2512.0)` | `(0, 63)` |
| C | `(4096.0, 4096.0)` (bottom-right corner) | `(1512.0, 2000.0)` | `(63, 0)` |
| D | `(4096.0, 0.0)` | `(1512.0, 2512.0)` | `(63, 63)` |

Cell index for a point: `i = floor((host_x - origin_x) / cell_m)`, `j = floor((host_y - origin_y) / cell_m)`.
Cell index for a guest tile `(c, r)`: `i = c`, `j = ny - 1 - r` (exact, because `cell_m == tile_m`).

## 4. Angles

Guest angle `θ` points along `(cos θ, sin θ)` in a y-down frame. Veloren yaw `φ` points along
`(-sin φ, cos φ)` in a y-up frame. Rewriting `(cos θ, -sin θ)` in Veloren's convention and solving gives a
**quarter-turn offset**, not a mirror:

```
yaw_host  = -theta_guest - FRAC_PI_2      (normalised to (-π, π])
theta_guest = -yaw_host - FRAC_PI_2
```

| # | guest θ | direction on the guest map | host yaw φ | host direction |
|---|---|---|---|---|
| E | `0` | right (east) | `-π/2` | `(+1, 0)` east ✓ |
| F | `π/2` | down (south) | `π` | `(0, -1)` south ✓ |
| G | `π` | left (west) | `π/2` | `(-1, 0)` west ✓ |

Getting this wrong is the classic mashup bug (the Minecraft×GTA V note records `yaw = 180 − heading`;
here it is `−θ − 90°`), so the vector test asserts all three quadrants and the round trip.

## 5. Terrain → tile map (host → guest)

The host fills `terrain.region` with `height_m` per cell and a first-pass `kind`. The guest converts cells
into its own `Map` (`tiles: Vec<Vec<Tile>>` over `surfaces: Vec<Surface>`, `map.rs:13`), keeping every
existing system — collision (`Map::is_wall`), AI, spawns — untouched:

| Cell condition (evaluated in this order) | `kind` | Guest surface | Effect |
|---|---|---|---|
| `h < sea + 0.5` | 1 water | `SurfaceKind::Water` | vehicles slow, side particles (`map.rs:305`) |
| `h < sea + 1.2` | 2 shallow | `SurfaceKind::Water` | as above |
| `maxΔh` to the 4-neighbourhood `≥ 8.0 m` (slope ≥ 45°) | 6 cliff | `SurfaceKind::Wall` | solid for vehicles and most weapons |
| `h < sea + 2.2` | 3 sand | `Normal`, friction 0.8 | beach |
| `h > sea + 120.0` | 7 snow | `SurfaceKind::Snow` | |
| slope `≥ 0.5`, or `h > sea + 90` | 5 rock | `Normal`, friction 1.0 | |
| otherwise | 4 grass | `Normal`, friction 0.9 | |
| `flags & 1` (host says spawnable) on every 32nd such cell (hash of `(i,j)`) | — | `SurfaceKind::Spawn` | gives the match spawn points |

`sea` is Veloren's sea level as the host reports it (its `Globals::view_distance.z`, "minimum height over
any land chunk", `render/pipelines/mod.rs`). The fake host uses `0.0`.

Two deliberate limits: v1 does not export Veloren *blocks* (trees, walls, houses) — only ground height and
the classes above, so a house is a hill to the guest; and it does not export caves at all. Both are visible
in the guest as "the map is smoother than the world". `flags` bit1 (`partial`) is set when the heightfield
touches the edge of the client's loaded chunks, and the guest fills those cells as `kind = 0` (void →
treated as `Wall`) instead of pretending.

## 6. Camera focus (host → guest)

The guest's camera normally follows its own local vehicle. Under the bridge the host sends `CAM` at 20 Hz
and the guest, when `bridge.focus` is on, centres its viewport on the *host's* position instead:

```
gx, gy = host_to_guest(cam.x, cam.y)
zoom   = clamp(fov_ref / fov, 0.5, 4.0)      fov_ref = 1.1 rad (Veloren's default Camera::new, camera.rs:332)
```

`pitch` is ignored (there is no up in a top-down game) and `yaw` is only used when the host wants the guest
to look along the player's heading (`bridge.lock_heading`, default off — a rotating 2D arena is unplayable).
`focus` implies `FOCUS on=1` (input goes to the guest).

## 7. Guest entities → host world (guest → host)

Every entity in `state.region` is placed by the rules above; the host then:

- picks `host_z` by sampling its own terrain at `(host_x, host_y)` and adds a half-height offset per kind
  (tank 0.5 m, projectile 0.2 m, explosion 0.0 m at ground level) — the guest's own `z` is ignored except
  when it is non-zero *and* the host's terrain sample is missing (edge of loaded chunks), which keeps a
  shell from sinking into unloaded ground;
- converts `angle` with §4 to a sprite yaw, and `turret_angle` the same way (it is relative to the hull in
  RecWars, so the host adds the hull yaw first);
- maps `kind` to a host sprite/model: 1,2,3,4,5 → vehicle sprites, 6 → small projectile quad, 7 → expanding
  explosion quad + host particle burst, 8 → pickup glow;
- derives velocity for interpolation from `vx, vy` (× `U2M` for m/s) so 60 Hz host frames between 30 Hz
  guest commits do not stutter.

Events (`kind 3 explosion`, `4 kill`, …) are applied on top: the host spawns its own local burst
(`scene/particle`, non-authoritative) and, if the optional server plugin is installed, forwards the event as
a server command so the *server* reacts as well (see `docs/ARCHITECTURE.md` §6).
