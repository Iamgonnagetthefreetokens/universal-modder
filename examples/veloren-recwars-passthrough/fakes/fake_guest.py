"""fake_guest — a RecWars-shaped guest for the um-bridge demo.

Not the game: a small stand-in that has the same *shape* as RecWars (a y-down tile arena in 64-unit
tiles, tanks with hulls and turrets, projectiles and explosions, a local player, a fixed tick loop) so
that the protocol, the mapping and the host-side integration can be exercised in a sandbox with no GPU
and no Rust. The hooks it stands in for are marked `# recwars: src/...` with the real file:line.

Run standalone:  python3 fakes/fake_guest.py --run-dir /tmp/um-demo --port 47811
"""

from __future__ import annotations

import argparse
import math
import os
import random
import signal
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import bridge as B  # noqa: E402

try:
    import numpy as np
except ImportError:  # pragma: no cover
    np = None

try:
    from PIL import Image, ImageDraw
except ImportError:  # pragma: no cover
    Image = ImageDraw = None

LOG_PREFIX = "[guest]"


def log(*a) -> None:
    print(LOG_PREFIX, *a, flush=True)


@dataclass
class Vehicle:
    id: int
    kind: int
    x: float
    y: float
    angle: float = 0.0
    turret: float = 0.0
    vx: float = 0.0
    vy: float = 0.0
    hp: float = 1.0
    team: int = 0
    is_player: bool = False
    cool: float = 0.0
    waypoint: tuple[float, float] | None = None

    @property
    def speed(self) -> float:
        return math.hypot(self.vx, self.vy)


@dataclass
class Projectile:
    id: int
    x: float
    y: float
    vx: float
    vy: float
    life: float = 2.5
    owner: int = 0
    weapon: int = 1


@dataclass
class Boom:
    x: float
    y: float
    r: float
    t: float = 0.0


class GuestWorld:
    """The stand-in simulation."""

    MAX_SPEED = 150.0        # units/s  (150 * 0.125 = 18.75 m/s)
    ACCEL = 320.0
    TURN = 2.6               # rad/s
    TURRET_TURN = 3.4
    TILE = B.TILE_UNITS      # 64.0  # recwars: src/map.rs:9 TILE_SIZE
    WALL = B.CELL_CLIFF

    def __init__(self, seed: int = 1, bots: int = 4):
        self.rng = random.Random(seed)
        self.tick = 0
        self.time = 0.0
        self.vehicles: list[Vehicle] = []
        self.projectiles: list[Projectile] = []
        self.booms: list[Boom] = []
        self.events: list[B.Event] = []
        self.next_id = 1
        self.map: list[list[int]] = []      # [row][col] of cell kinds
        self.spawn_point = (0.0, 0.0)
        self.terrain: B.TerrainHeader | None = None
        self.revision = -1
        self.focus: tuple[float, float] | None = None   # host camera position, guest units
        self.focus_input: dict[str, bool] = {}
        self.mouse = [0.0, 0.0]
        self.focus_on = False
        self.frame_size = (480, 270)
        self.publish_ts = 0
        self.bot_count = bots

    # ------------------------------------------------------------ map (recwars: src/map.rs)
    def apply_terrain(self, header: B.TerrainHeader, cells: list[B.Cell]) -> None:
        self.terrain = header
        self.revision = header.revision
        self.map = [[cells[j * header.nx + i].kind for i in range(header.nx)]
                    for j in range(header.ny)]
        self.map_w, self.map_h = header.nx, header.ny
        # keep vehicles out of walls after a rebuild
        for v in self.vehicles:
            if self.is_wall(v.x, v.y):
                for dx, dy in ((0, -1), (0, 1), (-1, 0), (1, 0), (1, 1), (-1, -1)):
                    nx_, ny_ = v.x + dx * self.TILE, v.y + dy * self.TILE
                    if not self.is_wall(nx_, ny_):
                        v.x, v.y = nx_, ny_
                        break
        self.spawn_vehicles()
        log(f"map rebuilt: rev={header.revision} {header.nx}x{header.ny} "
            f"origin=({header.origin_x:.1f},{header.origin_y:.1f})")

    def cell_kind(self, x: float, y: float) -> int:
        if not self.map:
            return B.CELL_GRASS
        c = int(x // self.TILE)
        r = int(y // self.TILE)
        if c < 0 or r < 0 or c >= self.map_w or r >= self.map_h:
            return self.WALL
        return self.map[r][c]

    def is_wall(self, x: float, y: float) -> bool:
        return self.cell_kind(x, y) in (self.WALL, B.CELL_UNKNOWN)

    def water(self, x: float, y: float) -> bool:
        return self.cell_kind(x, y) in (B.CELL_WATER, B.CELL_SHALLOW)

    def nonwall_center(self) -> tuple[float, float]:
        for _ in range(200):
            c = self.rng.randrange(1, max(2, self.map_w - 1))
            r = self.rng.randrange(1, max(2, self.map_h - 1))
            if self.map and self.map[r][c] not in (self.WALL, B.CELL_UNKNOWN):
                return (c * self.TILE + self.TILE / 2, r * self.TILE + self.TILE / 2)
        return (self.map_w * self.TILE / 2, self.map_h * self.TILE / 2)

    def spawn_vehicles(self) -> None:
        if self.vehicles or not self.map:
            return
        px, py = self.nonwall_center()
        self.spawn_point = (px, py)
        self.vehicles.append(Vehicle(self.next_id, B.ENTITY_TANK, px, py, is_player=True))
        self.next_id += 1
        for i in range(self.bot_count):
            bx, by = self.nonwall_center()
            kind = (B.ENTITY_TANK, B.ENTITY_HOVER, B.ENTITY_HUMMER)[i % 3]
            self.vehicles.append(Vehicle(self.next_id, kind, bx, by, team=(i % 2) + 1,
                                         angle=self.rng.uniform(-math.pi, math.pi)))
            self.next_id += 1
        self.events.append(B.Event(B.EV_SPAWN, t_ms=int(self.time * 1000), x=px, y=py))
        log(f"spawned {len(self.vehicles)} vehicles")

    # ------------------------------------------------------------ sim (recwars: src/systems.rs)
    def step(self, dt: float) -> None:
        self.tick += 1
        self.time += dt
        player = next((v for v in self.vehicles if v.is_player), None)
        for v in self.vehicles:
            drive_fwd = drive_back = turn_l = turn_r = 0.0
            if v.is_player:
                if self.focus_on:
                    inp = self.focus_input
                    drive_fwd = bool(inp.get("forward"))
                    drive_back = bool(inp.get("back"))
                    turn_l = bool(inp.get("left"))
                    turn_r = bool(inp.get("right"))
                    tgt = math.atan2(self.mouse[1], self.mouse[0]) if any(self.mouse) else None
                    if tgt is not None:
                        v.turret = tgt
                else:
                    # nobody is driving (the bridge is off, or the host has not taken focus):
                    # idle, drifting the turret around, exactly like a parked tank in the real game
                    drive_fwd = drive_back = turn_l = turn_r = 0.0
                    v.turret = v.angle + math.sin(self.time * 0.4) * 1.1
            else:
                self.bot_ai(v, dt, player)
                drive_fwd = 1.0
                turn_l, turn_r = self._bot_turn(v, dt)

            if turn_l:
                v.angle -= self.TURN * dt
            if turn_r:
                v.angle += self.TURN * dt
            ax = math.cos(v.angle)
            ay = math.sin(v.angle)
            accel = self.ACCEL * (1.0 if drive_fwd else (-0.8 if drive_back else 0.0))
            v.vx += ax * accel * dt
            v.vy += ay * accel * dt
            # friction, and water slows harder
            fr = 2.2 if self.water(v.x, v.y) else 0.9
            v.vx -= v.vx * fr * dt
            v.vy -= v.vy * fr * dt
            sp = v.speed
            if sp > self.MAX_SPEED:
                v.vx *= self.MAX_SPEED / sp
                v.vy *= self.MAX_SPEED / sp
            nx = v.x + v.vx * dt
            ny = v.y + v.vy * dt
            if v.is_player and not self.focus_on and not self.terrain is None:
                # gentle spring back to the spawn area so the demo does not walk to the map edge
                sx, sy = self.spawn_point
                d = math.hypot(v.x - sx, v.y - sy)
                if d > 180.0:
                    v.vx += (sx - v.x) / d * 60.0 * dt
                    v.vy += (sy - v.y) / d * 60.0 * dt
            if self.is_wall(nx, v.y):
                v.vx = -v.vx * 0.2
            else:
                v.x = nx
            if self.is_wall(v.x, ny):
                v.vy = -v.vy * 0.2
            else:
                v.y = ny
            v.cool = max(0.0, v.cool - dt)
            if v.is_player and not self.focus_on:
                v.turret = v.angle

        # projectiles
        keep: list[Projectile] = []
        for p in self.projectiles:
            p.x += p.vx * dt
            p.y += p.vy * dt
            p.life -= dt
            hit = None
            if self.is_wall(p.x, p.y):
                hit = (p.x, p.y)
            else:
                for v in self.vehicles:
                    if v.id == p.owner:
                        continue
                    if math.hypot(v.x - p.x, v.y - p.y) < 26.0:
                        hit = (p.x, p.y)
                        v.hp = max(0.0, v.hp - 0.34)
                        self.events.append(B.Event(B.EV_HIT, t_ms=int(self.time * 1000),
                                                   x=p.x, y=p.y, arg=v.hp))
                        if v.hp <= 0.0:
                            self.events.append(B.Event(B.EV_KILL, t_ms=int(self.time * 1000),
                                                       x=v.x, y=v.y, arg=float(p.owner)))
                            v.hp = 1.0
                            v.x, v.y = self.nonwall_center()
                        break
            if hit is not None:
                self.booms.append(Boom(hit[0], hit[1], 48.0))
                self.events.append(B.Event(B.EV_EXPLOSION, arg16=48, t_ms=int(self.time * 1000),
                                           x=hit[0], y=hit[1], arg=0.7))
            elif p.life > 0.0:
                keep.append(p)
        self.projectiles = keep

        self.booms = [b for b in self.booms if b.t < 0.6]
        for b in self.booms:
            b.t += dt
        if len(self.events) > B.MAX_EVENTS:
            self.events = self.events[-B.MAX_EVENTS:]

        # fire (player + bots)  # recwars: src/client.rs:283 cl_input -> ClientInput.fire
        fire = bool(self.focus_input.get("fire")) if (self.focus_on and player) else False
        if player and (fire or (self.tick % 90 == 0)):
            self.fire(player)
        for v in self.vehicles:
            if not v.is_player and v.cool <= 0.0 and self.rng.random() < 0.02:
                self.fire(v)

        if self.tick % 300 == 0:
            self.events.append(B.Event(B.EV_SPAWN, t_ms=int(self.time * 1000),
                                       x=self.vehicles[-1].x, y=self.vehicles[-1].y))

    def _bot_turn(self, v: Vehicle, dt: float) -> tuple[float, float]:
        wx, wy = v.waypoint or (v.x, v.y)
        want = math.atan2(wy - v.y, wx - v.x)
        diff = (want - v.angle + math.pi) % (2 * math.pi) - math.pi
        if abs(diff) < 0.1:
            return 0.0, 0.0
        return (1.0, 0.0) if diff < 0 else (0.0, 1.0)

    def bot_ai(self, v: Vehicle, dt: float, player: Vehicle | None) -> None:
        if v.waypoint is None or math.hypot(v.waypoint[0] - v.x, v.waypoint[1] - v.y) < 48.0:
            v.waypoint = self.nonwall_center()
        if player is not None:
            d = math.hypot(player.x - v.x, player.y - v.y)
            if d < 700.0:
                v.turret = math.atan2(player.y - v.y, player.x - v.x)
                if v.cool <= 0.0:
                    self.fire(v)

    def fire(self, v: Vehicle) -> None:
        if v.cool > 0.0:
            return
        v.cool = 0.55
        speed = 620.0
        px = v.x + math.cos(v.turret) * 20.0
        py = v.y + math.sin(v.turret) * 20.0
        self.projectiles.append(Projectile(self.next_id, px, py,
                                           math.cos(v.turret) * speed, math.sin(v.turret) * speed,
                                           owner=v.id))
        self.next_id += 1
        self.events.append(B.Event(B.EV_FIRE, arg16=1, t_ms=int(self.time * 1000), x=px, y=py))

    # ------------------------------------------------------------ export (recwars: src/game_state.rs:15)
    def entities(self) -> list[B.Entity]:
        out = []
        for v in self.vehicles:
            flags = 1 | (2 if v.cool > 0.5 else 0) | (4 if v.is_player else 0)
            out.append(B.Entity(v.id, v.kind, v.team, flags, v.x, v.y, 0.0, v.angle, v.turret,
                                v.vx, v.vy, v.hp))
        for p in self.projectiles:
            out.append(B.Entity(p.id, B.ENTITY_PROJECTILE, 0, 1, p.x, p.y, 0.0,
                                math.atan2(p.vy, p.vx), 0.0, p.vx, p.vy, 1.0))
        for i, b in enumerate(self.booms):
            out.append(B.Entity(0x40000000 + i, B.ENTITY_EXPLOSION, 0, 1, b.x, b.y, 0.0,
                                0.0, 0.0, 0.0, 0.0, 1.0 - b.t / 0.6))
        return out

    def state_header(self) -> B.StateHeader:
        player = next((v for v in self.vehicles if v.is_player), None)
        cam = self.focus if (self.focus_on and self.focus) else ((player.x, player.y) if player
                                                                 else (0.0, 0.0))
        return B.StateHeader(
            tick=self.tick,
            game_time_ms=int(self.time * 1000),
            focus_ent=player.id if player else 0xFFFF,
            flags=(1 if player and player.hp > 0 else 0) | (2 if self.focus_on else 0),
            player_x=player.x if player else 0.0,
            player_y=player.y if player else 0.0,
            cam_x=cam[0],
            cam_y=cam[1],
        )

    # ------------------------------------------------------------ render (recwars: src/rendering.rs:17)
    def render_frame(self) -> bytes:
        """Top-down view, centred on the bridge focus (or the local player)."""
        w, h = self.frame_size
        img = np.zeros((h, w, 4), dtype=np.uint8)
        img[..., 3] = 255
        if not self.map:
            return img.tobytes()

        player = next((v for v in self.vehicles if v.is_player), None)
        if self.focus_on and self.focus:
            cx, cy = self.focus
        else:
            cx, cy = (player.x, player.y) if player else (self.map_w * self.TILE / 2,
                                                           self.map_h * self.TILE / 2)
        px_per_unit = 1.0 / 6.5                      # 64 units (1 tile) -> ~10.7 px
        half_w, half_h = w / 2 / px_per_unit, h / 2 / px_per_unit
        palette = {
            B.CELL_UNKNOWN: (24, 24, 28), B.CELL_WATER: (38, 78, 148), B.CELL_SHALLOW: (56, 112, 176),
            B.CELL_SAND: (196, 178, 120), B.CELL_GRASS: (86, 132, 66), B.CELL_ROCK: (120, 118, 112),
            B.CELL_CLIFF: (70, 66, 64), B.CELL_SNOW: (226, 230, 236),
        }
        # terrain: sample the tile grid at a coarse resolution, then upscale (cheap, chunky, fine)
        step = 10                                   # pixels per sample
        gw, gh = w // step + 1, h // step + 1
        gx = (np.arange(gw)[None, :] - gw / 2) * step / px_per_unit
        gy = (np.arange(gh)[:, None] - gh / 2) * step / px_per_unit
        cols = np.clip(((cx + gx) // self.TILE).astype(int), 0, max(0, self.map_w - 1))
        rows = np.clip(((cy + gy) // self.TILE).astype(int), 0, max(0, self.map_h - 1))
        kinds = np.array(self.map, dtype=np.uint8)[rows, cols] if self.map else np.zeros((gh, gw), np.uint8)
        lut = np.zeros((8, 3), dtype=np.uint8)
        for k, c in palette.items():
            lut[k] = c
        small = lut[kinds]
        big = np.repeat(np.repeat(small, step, axis=0), step, axis=1)[:h, :w]
        img[..., :3] = big
        # grid lines (tile borders)
        origin_x = cx - half_w
        for c in range(int((cx - half_w) // self.TILE), int((cx + half_w) // self.TILE) + 2):
            x = int((c * self.TILE - origin_x) * px_per_unit)
            if 0 <= x < w:
                img[:, x, :3] = (img[:, x, :3] * 3 // 4)
        for r in range(int((cy - half_h) // self.TILE), int((cy + half_h) // self.TILE) + 2):
            y = int((r * self.TILE - (cy - half_h)) * px_per_unit)
            if 0 <= y < h:
                img[y, :, :3] = (img[y, :, :3] * 3 // 4)

        def to_px(x, y):
            return int((x - (cx - half_w)) * px_per_unit), int((y - (cy - half_h)) * px_per_unit)

        # vehicles as little rotated tanks
        for v in self.vehicles:
            vx, vy = to_px(v.x, v.y)
            size = 14
            yy, xx = np.mgrid[-size:size + 1, -size:size + 1]
            ca, sa = math.cos(v.angle), math.sin(v.angle)
            lx = xx * ca + yy * sa
            ly = -xx * sa + yy * ca
            body = (np.abs(lx) <= size * 0.62) & (np.abs(ly) <= size * 0.42)
            tread = (np.abs(lx) <= size * 0.62) & (np.abs(ly) <= size * 0.62) & (np.abs(ly) > size * 0.32)
            turret = (lx ** 2 + ly ** 2 <= 16) | ((lx >= 0) & (lx <= size * 0.9) & (np.abs(ly) <= 1.5))
            base = (110, 150, 90) if v.team == 0 else ((168, 92, 84) if v.team == 1 else (92, 116, 168))
            if v.is_player:
                base = (240, 220, 90)
            for mask, col in ((tread, (40, 40, 44)), (body, base), (turret, (30, 30, 32))):
                ys, xs = np.nonzero(mask)
                py_, px_ = ys + vy - size, xs + vx - size
                ok = (py_ >= 0) & (py_ < h) & (px_ >= 0) & (px_ < w)
                img[py_[ok], px_[ok], :3] = col
        for p in self.projectiles:
            x, y = to_px(p.x, p.y)
            if 0 <= x < w and 0 <= y < h:
                img[max(0, y - 1):y + 2, max(0, x - 1):x + 2, :3] = (255, 226, 120)
        for b in self.booms:
            x, y = to_px(b.x, b.y)
            r = int(b.r * px_per_unit * (0.4 + 1.6 * b.t))
            yy, xx = np.mgrid[-r:r + 1, -r:r + 1]
            mask = (xx ** 2 + yy ** 2) <= r * r
            ys, xs = np.nonzero(mask)
            py_, px_ = ys + y - r, xs + x - r
            ok = (py_ >= 0) & (py_ < h) & (px_ >= 0) & (px_ < w)
            fade = 1.0 - b.t / 0.6
            img[py_[ok], px_[ok], :3] = (255, int(150 * fade + 60), 40)
        # host camera marker (where the player is looking in Veloren)
        if self.focus is not None:
            fx, fy = to_px(*self.focus)
            if 0 <= fx < w and 0 <= fy < h:
                img[max(0, fy - 8):fy + 9, max(0, fx - 1):fx + 2, :3] = (255, 80, 220)
                img[max(0, fy - 1):fy + 2, max(0, fx - 8):fx + 9, :3] = (255, 80, 220)

        if Image is not None:
            im = Image.fromarray(img[..., :3], "RGB")
            d = ImageDraw.Draw(im)
            focus = next((v for v in self.vehicles if v.is_player), None)
            d.text((6, 4), f"RecWars (guest)  t={self.time:6.1f}s  tick={self.tick}  "
                           f"rev={self.revision}  {'FOCUS' if self.focus_on else 'auto'}",
                   fill=(255, 255, 255))
            d.text((6, 16), f"host_at={self.focus[0]:.0f},{self.focus[1]:.0f} u" if self.focus
                   else "host_at=none", fill=(255, 200, 255))
            if focus:
                d.text((6, h - 14), f"drive: {focus.speed * B.U2M:.1f} m/s  hp={focus.hp:.2f}",
                       fill=(255, 255, 255))
            img[..., :3] = np.asarray(im)
        # publish BGRA8 as the protocol requires (docs/PROTOCOL.md §2.5); internally everything is RGB
        return img[..., [2, 1, 0, 3]].tobytes()


def run(args: argparse.Namespace) -> int:
    if np is None:
        log("numpy is required for the fakes: pip install numpy")
        return 2
    run_dir = B.open_run_dir(args.run_dir)
    port = args.port
    server = B.GuestServer(port)
    log(f"listening on 127.0.0.1:{server.port}  run_dir={run_dir}")

    world = GuestWorld(seed=args.seed, bots=args.bots)
    world.frame_size = (args.frame_width, args.frame_height)
    state_region = B.Region(B.state_path(run_dir), B.REGION_STATE, 32 + B.MAX_ENTITIES * 40
                            + B.MAX_EVENTS * 24, writer=True)
    frame_region = B.Region(B.frame_path(run_dir), B.REGION_FRAME,
                            B.FRAME_HDR.size + args.frame_width * args.frame_height * 4, writer=True)
    watchdog = B.Watchdog()
    peer: B.ControlPeer | None = None
    terrain_reader: B.Region | None = None
    started = time.monotonic()
    last_state = 0.0
    last_frame = 0.0
    last_cam = 0.0
    stats = {"frames": 0, "states": 0, "cam_msgs": 0, "in_msgs": 0, "events": 0, "input": 0}

    stop = {"now": False}

    def on_signal(*_):
        stop["now"] = True

    signal.signal(signal.SIGTERM, on_signal)
    signal.signal(signal.SIGINT, on_signal)

    pending_cam: dict | None = None
    while not stop["now"]:
        now = time.monotonic()
        if args.duration and now - started > args.duration:
            break
        if peer is None:
            peer = server.accept()
        if peer is not None:
            for msg in peer.pump():
                watchdog.beat()
                if msg.type == "HELLO":
                    peer.send("WELCOME", v=B.PROTOCOL_VERSION, role="guest", app="recwars",
                              pid=os.getpid(), tick_hz=args.hz,
                              state_region=str(B.state_path(run_dir)),
                              frame=f"{args.frame_width}x{args.frame_height}")
                    log(f"host connected: app={msg.s('app')} pid={msg.i('pid')}")
                elif msg.type == "CAM":
                    stats["cam_msgs"] += 1
                    pending_cam = {"x": msg.f("x"), "y": msg.f("y"), "z": msg.f("z"),
                                   "yaw": msg.f("yaw"), "pitch": msg.f("pitch"), "fov": msg.f("fov", B.FOV_REF)}
                    host = msg.f("x"), msg.f("y")
                    if world.terrain is not None:
                        world.focus = B.clamp_to_region(*B.host_to_guest(host[0], host[1], world.terrain),
                                                        world.terrain)
                elif msg.type == "MAP":
                    try:
                        if terrain_reader is None:
                            terrain_reader = B.Region(B.terrain_path(run_dir), B.REGION_TERRAIN, 0,
                                                      writer=False)
                        r = terrain_reader.read()
                        if r is not None:
                            hdr, cells = B.unpack_terrain(r.payload)
                            world.apply_terrain(hdr, cells)
                    except (B.RegionError, OSError) as e:
                        log(f"terrain read failed: {e}")
                elif msg.type == "FOCUS":
                    world.focus_on = bool(msg.i("on"))
                    if not world.focus_on:
                        world.focus_input.clear()
                        world.mouse = [0.0, 0.0]
                    log(f"input focus {'to guest' if world.focus_on else 'to host'}")
                elif msg.type == "IN":
                    stats["in_msgs"] += 1
                    world.focus_input[msg.s("a")] = bool(msg.i("d"))
                elif msg.type == "MOUSE":
                    world.mouse[0] += msg.f("dx")
                    world.mouse[1] += msg.f("dy")
                elif msg.type == "PING":
                    watchdog.beat()
                elif msg.type == "BYE":
                    log(f"host said BYE reason={msg.s('reason')}")
                    peer.closed = True
            if peer.closed:
                peer = None
                world.focus_on = False
                world.focus_input.clear()
            elif watchdog.state() == "dead":
                log("host watchdog: dead -> focus off, input released")
                peer.close("watchdog")
                peer = None
                world.focus_on = False
                world.focus_input.clear()

        if world.map:
            dt = 1.0 / args.hz
            world.step(dt)
            stats["events"] += len(world.events)
            if now - last_state >= 1.0 / args.state_hz:
                last_state = now
                blob = B.pack_state(world.state_header(), world.entities(), world.events)
                seq = state_region.write(blob)
                stats["states"] += 1
                world.events = []
                if peer is not None and not peer.closed:
                    peer.send("NOTIFY", kind="state", seq=seq, tick=world.tick)
            if now - last_frame >= 1.0 / args.frame_hz:
                last_frame = now
                pixels = world.render_frame()
                seq = frame_region.write(B.pack_frame(*world.frame_size, pixels, game_tick=world.tick,
                                                      scale_permille=int(1000 * 0.3)))
                stats["frames"] += 1
            if peer is not None and not peer.closed and now - last_cam > 1.0:
                last_cam = now
                peer.send("PING", t_us=B.monotonic_us())
        time.sleep(0.001)

    if peer is not None:
        peer.close("guest shutdown")
    state_region.close()
    frame_region.close()
    server.close()
    log(f"bye: {stats}")
    if args.stats_out:
        import json
        Path(args.stats_out).write_text(json.dumps({"guest": stats, "revision": world.revision,
                                                    "ticks": world.tick}, indent=2) + "\n")
    return 0


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--run-dir", default=None)
    p.add_argument("--port", type=int, default=B.DEFAULT_PORT)
    p.add_argument("--duration", type=float, default=0.0, help="seconds; 0 = until killed")
    p.add_argument("--hz", type=float, default=30.0, help="simulation rate")
    p.add_argument("--state-hz", type=float, default=30.0)
    p.add_argument("--frame-hz", type=float, default=15.0)
    p.add_argument("--frame-width", type=int, default=480)
    p.add_argument("--frame-height", type=int, default=270)
    p.add_argument("--bots", type=int, default=4)
    p.add_argument("--seed", type=int, default=7)
    p.add_argument("--stats-out", default=None)
    return run(p.parse_args())


if __name__ == "__main__":
    sys.exit(main())
