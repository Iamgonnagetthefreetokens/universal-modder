"""fake_host — a Veloren-shaped host for the um-bridge demo.

Not the game: a stand-in that has the same *shape* as Veloren's client (a z-up voxel-ish world in
metres, a first-person camera that walks on the terrain, a scene that draws guest entities at world
positions with the host's depth buffer, and a quad in the world showing the guest's framebuffer), so the
whole merge can be watched in a sandbox with no GPU and no Rust. Every hook it stands in for is marked
`# veloren: voxygen/src/...` with the real file:line.

Run standalone:  python3 fakes/fake_host.py --run-dir /tmp/um-demo --out /tmp/um-out
"""

from __future__ import annotations

import argparse
import json
import math
import os
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

LOG_PREFIX = "[host]"
SEA_LEVEL = 0.0
EYE = 1.7


def log(*a) -> None:
    print(LOG_PREFIX, *a, flush=True)


# ================================================================ world (veloren: world/src/lib.rs)
def terrain_height(x: float, y: float) -> float:
    """Deterministic stand-in for Veloren's worldgen: rolling hills, a ridge, a lake, a plateau.

    Sea level is 0.0, so the base is lifted: most of the map is walkable land, the lake is the only
    water, and the ridge in the north is a cliff wall for the guest's vehicles.
    """
    h = (14.0 * math.sin(x / 190.0) * math.cos(y / 160.0)
         + 7.0 * math.sin((x + y) / 88.0)
         + 3.5 * math.sin(x / 41.0) * math.sin(y / 57.0)
         + 1.6 * math.sin((x - y) / 17.0)
         + 2.2 * math.sin(x / 23.0) * math.cos(y / 19.0)
         + 0.9 * math.sin(x / 9.5) * math.cos(y / 11.5))
    ridge = 46.0 * math.exp(-((y - 230.0) / 80.0) ** 2) * (1.0 + 0.35 * math.sin(x / 60.0))
    lake = -22.0 * math.exp(-((x + 130.0) ** 2 + (y + 90.0) ** 2) / (2 * 130.0 ** 2))
    plateau = 26.0 / (1.0 + math.exp(-(x - 260.0) / 45.0)) / (1.0 + math.exp(-(y + 60.0) / 70.0))
    return h + ridge + lake + plateau + 9.0


def terrain_height_np(x, y):
    """Vectorised twin of `terrain_height` (the tests assert they agree)."""
    return (14.0 * np.sin(x / 190.0) * np.cos(y / 160.0)
            + 7.0 * np.sin((x + y) / 88.0)
            + 3.5 * np.sin(x / 41.0) * np.sin(y / 57.0)
            + 1.6 * np.sin((x - y) / 17.0)
            + 2.2 * np.sin(x / 23.0) * np.cos(y / 19.0)
            + 0.9 * np.sin(x / 9.5) * np.cos(y / 11.5)
            + 46.0 * np.exp(-((y - 230.0) / 80.0) ** 2) * (1.0 + 0.35 * np.sin(x / 60.0))
            - 22.0 * np.exp(-((x + 130.0) ** 2 + (y + 90.0) ** 2) / (2 * 130.0 ** 2))
            + 26.0 / (1.0 + np.exp(-(x - 260.0) / 45.0)) / (1.0 + np.exp(-(y + 60.0) / 70.0))
            + 9.0)


def terrain_slope(x: float, y: float, d: float = 8.0) -> float:
    return max(abs(terrain_height(x + d, y) - terrain_height(x - d, y)),
               abs(terrain_height(x, y + d) - terrain_height(x, y - d))) / (2 * d)


@dataclass
class Prop:
    x: float
    y: float
    z: float
    kind: int          # 0 tree, 1 rock


def make_prop_sprites():
    """Small RGBA sprites for the props, built once (stand-ins for Veloren's voxel models)."""
    tree = np.zeros((26, 18, 4), np.uint8)
    canopy = np.zeros((26, 18), bool)
    yy, xx = np.mgrid[0:26, 0:18]
    canopy |= (((xx - 9) / 7.5) ** 2 + ((yy - 9) / 8.5) ** 2) <= 1.0
    trunk = (np.abs(xx - 9) <= 1) & (yy >= 16)
    tree[canopy] = (44, 92, 40, 255)
    tree[(canopy) & (((xx + yy) % 5) == 0)] = (58, 112, 50, 255)
    tree[trunk] = (72, 52, 32, 255)
    rock = np.zeros((14, 18, 4), np.uint8)
    yy, xx = np.mgrid[0:14, 0:18]
    body = (((xx - 9) / 8.0) ** 2 + ((yy - 9) / 4.6) ** 2) <= 1.0
    rock[body] = (118, 114, 108, 255)
    rock[body & (((xx * 3 + yy) % 7) == 0)] = (96, 92, 88, 255)
    return {0: tree, 1: rock}


PROP_SPRITES = None


def world_props(x0: float, y0: float, radius: float = 300.0, spacing: float = 30.0) -> list:
    """Deterministic scatter of props around a point (the host would have these chunks loaded)."""
    props = []
    n = int(2 * radius / spacing)
    for i in range(n):
        for j in range(n):
            x = x0 - radius + i * spacing + ((i * 37 + j * 91) % 13) - 6.0
            y = y0 - radius + j * spacing + ((i * 17 + j * 53) % 11) - 5.0
            h = terrain_height(x, y)
            if h < 1.4 or terrain_slope(x, y) > 0.5:
                continue
            if h > 55.0:
                kind = 1
            elif h > 25.0:
                kind = 1 if ((i * 7 + j * 3) % 2 == 0) else 0
            else:
                kind = 0 if ((i * 5 + j * 11) % 7 != 0) else 1
            props.append(Prop(x, y, h, kind))
    return props


@dataclass
class GuestEntity:
    id: int
    kind: int
    team: int
    flags: int
    x: float          # guest units
    y: float
    z: float
    angle: float
    turret: float
    vx: float
    vy: float
    hp: float
    # filled by the host each frame
    hx: float = 0.0
    hy: float = 0.0
    hz: float = 0.0
    yaw: float = 0.0
    tx: float = 0.0
    ty: float = 0.0
    tz: float = 0.0
    alive: bool = True
    born: float = 0.0


@dataclass
class Burst:
    x: float
    y: float
    z: float
    t: float = 0.0
    life: float = 0.9
    power: float = 1.0
    seeds: object = None


class FakeHost:
    def __init__(self, run_dir: Path, port: int, width: int = 640, height: int = 360,
                 render_scale: int = 2, seed: int = 3):
        self.run_dir = run_dir
        self.port = port
        self.w, self.h = width, height
        self.rw, self.rh = width // render_scale, height // render_scale
        self.rng = np.random.default_rng(seed) if np is not None else None
        self.peer: B.ControlPeer | None = None
        self.watchdog = B.Watchdog()
        self.state_reader: B.Region | None = None
        self.frame_reader: B.Region | None = None
        self.terrain_writer: B.Region | None = None
        self.guest_frame: np.ndarray | None = None
        self.guest_frame_hdr: B.FrameHeader | None = None
        self.guest_frame_age_ms: float = 0.0
        self.entities: dict[int, GuestEntity] = {}
        self.last_state: B.StateHeader | None = None
        self.last_state_seq = 0
        self.bursts: list[Burst] = []
        self.events_applied = 0
        self.seen_event_keys: set[tuple] = set()

        # camera (veloren: voxygen/src/scene/camera.rs:40)
        self.cam_x, self.cam_y = 50.0, 50.0
        self.cam_z = terrain_height(self.cam_x, self.cam_y) + EYE
        self.yaw = math.radians(30.0)
        self.pitch = math.radians(-4.0)
        self.fov = B.FOV_REF

        self.terrain_origin = (0.0, 0.0)
        self.terrain_center = (0.0, 0.0)
        self.terrain_rev = 0
        self.next_terrain_check = 0.0
        self.focus_on = False
        self.keys: dict[str, bool] = {}
        self.mouse = [0.0, 0.0]

        self.props = world_props(self.cam_x, self.cam_y, radius=290.0, spacing=26.0)
        self.props_center = (self.cam_x, self.cam_y)
        self.render_no = 0
        # Projection samples: what the renderer *drew* this frame, kept so the demo can check it
        # against the documented mapping + camera from the outside (fakes/run_demo.py, oracle "guest
        # sprites land where the docs say"). Each sample holds the camera pose, the terrain grid the
        # mapping used, and the projected centre of every sprite drawn.
        self.projection_samples: list[dict] = []
        self.frames_out: list[Path] = []
        self.stats = {"cam": 0, "map": 0, "state_reads": 0, "frame_reads": 0, "torn": 0,
                      "events": 0, "ticks": 0, "frames_rendered": 0, "frame_ages_ms": [],
                      "entity_counts": [], "rtt_us": None}
        self.log_path = run_dir / "host.log"

    # ------------------------------------------------------------ control (veloren: session/mod.rs:583)
    def connect(self) -> None:
        self.peer = B.connect_host(self.port)
        self.peer.send("HELLO", v=B.PROTOCOL_VERSION, role="host", app="veloren", pid=os.getpid(),
                       proto=1, run_dir=str(self.run_dir))
        log(f"connected to guest on 127.0.0.1:{self.port}")

    def send_cam(self) -> None:      # veloren: voxygen/src/scene/camera.rs:492 dependents
        if not self.peer or self.peer.closed:
            return
        self.peer.send("CAM", t=int(time.monotonic() * 1000), x=round(self.cam_x, 3),
                       y=round(self.cam_y, 3), z=round(self.cam_z, 3), yaw=round(self.yaw, 5),
                       pitch=round(self.pitch, 5), fov=round(self.fov, 5),
                       sx=self.w, sy=self.h)
        self.stats["cam"] += 1

    def export_terrain(self) -> None:   # veloren: client.state().terrain() sampling, session/mod.rs:633
        nx = ny = 64
        cell = B.TILE_M
        ox = math.floor((self.cam_x - nx * cell / 2) / cell) * cell
        oy = math.floor((self.cam_y - ny * cell / 2) / cell) * cell
        cells: list[B.Cell] = []
        for j in range(ny):
            for i in range(nx):
                x, y = ox + (i + 0.5) * cell, oy + (j + 0.5) * cell
                cells.append(B.Cell(terrain_height(x, y), B.CELL_UNKNOWN, 0))
        # classify in two passes so slope/cliff uses the exported neighbourhood, exactly as the guest
        # would receive it (docs/MAPPING.md §5)
        for j in range(ny):
            for i in range(nx):
                kind = B.classify_cell(cells, i, j, nx, ny, SEA_LEVEL, cell)
                slope = terrain_slope(ox + (i + 0.5) * cell, oy + (j + 0.5) * cell)
                flags = 1 if (kind in (B.CELL_GRASS, B.CELL_SAND, B.CELL_SNOW) and slope < 0.35
                              and ((i * 7 + j * 13) % 32 == 0)) else 0
                cells[j * nx + i] = B.Cell(cells[j * nx + i].height_m, kind, flags)
        self.terrain_rev += 1
        flags = 1 | (2 if (ox < -1200 or oy < -1200 or ox > 1200 or oy > 1200) else 0)
        hdr = B.TerrainHeader(ox, oy, cell, nx, ny, B.TILE_M, SEA_LEVEL, self.terrain_rev, flags)
        if self.terrain_writer is None:
            self.terrain_writer = B.Region(B.terrain_path(self.run_dir), B.REGION_TERRAIN,
                                           B.TERRAIN_HDR.size + nx * ny * B.CELL.size, writer=True)
        self.terrain_writer.write(B.pack_terrain(hdr, cells))
        self.terrain_origin = (ox, oy)
        self.terrain_center = (ox + nx * cell / 2, oy + ny * cell / 2)
        self.stats["map"] += 1
        if self.peer and not self.peer.closed:
            self.peer.send("MAP", rev=self.terrain_rev, cells=nx, cell_m=cell, origin_x=ox,
                           origin_y=oy)
        log(f"exported terrain rev={self.terrain_rev} origin=({ox:.0f},{oy:.0f})")

    def read_guest(self) -> None:      # veloren: bridge.read_state() in Session::tick
        if self.state_reader is None:
            p = B.state_path(self.run_dir)
            if p.exists():
                try:
                    self.state_reader = B.Region(p, B.REGION_STATE, 32 + B.MAX_ENTITIES * 40 +
                                                 B.MAX_EVENTS * 24, writer=False)
                except (B.RegionError, OSError):
                    return
            else:
                return
        res = self.state_reader.read()
        if res is None:
            return
        self.stats["state_reads"] += 1
        self.stats["torn"] = self.state_reader.torn
        hdr, ents, events = B.unpack_state(res.payload)
        self.last_state, self.last_state_seq = hdr, res.seq
        self.watchdog.beat()
        self.stats["entity_counts"].append(len(ents))
        if self.last_state.flags & 2:
            self.focus_on = True
        # update the entity table, keeping interpolation state
        seen = set()
        for e in ents:
            seen.add(e.id)
            ge = self.entities.get(e.id)
            if ge is None:
                ge = GuestEntity(e.id, e.kind, e.team, e.flags, e.x, e.y, e.z, e.angle,
                                 e.turret_angle, e.vx, e.vy, e.hp_frac, born=time.monotonic())
                self.entities[e.id] = ge
            ge.kind, ge.team, ge.flags = e.kind, e.team, e.flags
            ge.x, ge.y, ge.z = e.x, e.y, e.z
            ge.angle, ge.turret = e.angle, e.turret_angle
            ge.vx, ge.vy, ge.hp = e.vx, e.vy, e.hp_frac
            ge.alive = bool(e.flags & 1)
        for dead in [k for k in self.entities if k not in seen]:
            del self.entities[dead]
        # events: apply once, keyed by (t_ms, kind, x, y) since the ring republishes
        for ev in events:
            key = (ev.t_ms, ev.kind, round(ev.x, 1), round(ev.y, 1))
            if key in self.seen_event_keys:
                continue
            self.seen_event_keys.add(key)
            self.events_applied += 1
            self.stats["events"] += 1
            if ev.kind == B.EV_EXPLOSION:
                hx, hy = B.guest_to_host(ev.x, ev.y, self.terrain_header())
                self.bursts.append(Burst(hx, hy, terrain_height(hx, hy), power=max(0.4, ev.arg),
                                         seeds=self.rng.normal(0, 1, (28, 3))))
                self.log_line(f"EVT explosion guest=({ev.x:.0f},{ev.y:.0f}) host=({hx:.1f},{hy:.1f})")
            elif ev.kind == B.EV_KILL:
                self.log_line(f"EVT kill at guest=({ev.x:.0f},{ev.y:.0f})")
            elif ev.kind == B.EV_FIRE:
                pass
        if len(self.seen_event_keys) > 4096:
            self.seen_event_keys.clear()
        self.read_frame()

    def read_frame(self) -> None:      # veloren: guest texture upload in the scene pass
        if self.frame_reader is None:
            p = B.frame_path(self.run_dir)
            if not p.exists():
                return
            try:
                self.frame_reader = B.Region(p, B.REGION_FRAME, 0, writer=False)
            except (B.RegionError, OSError):
                return
        res = self.frame_reader.read()
        if res is None:
            return
        hdr, px = B.unpack_frame(res.payload)
        self.stats["frame_reads"] += 1
        self.guest_frame_hdr = hdr
        self.guest_frame_age_ms = res.age_ms
        self.stats["frame_ages_ms"].append(res.age_ms)
        w, h = hdr.width, hdr.height
        if self.guest_frame is None or self.guest_frame.shape[:2] != (h, w):
            self.guest_frame = np.zeros((h, w, 3), dtype=np.uint8)
        arr = np.frombuffer(px, dtype=np.uint8).reshape(h, hdr.stride // 4, 4)[:, :w, :]
        self.guest_frame = arr[..., [2, 1, 0]]  # BGRA on the wire -> RGB in the render buffer

    def terrain_header(self) -> B.TerrainHeader:
        return B.TerrainHeader(self.terrain_origin[0], self.terrain_origin[1], B.TILE_M, 64, 64,
                               B.TILE_M, SEA_LEVEL, self.terrain_rev)

    # ------------------------------------------------------------ input (veloren: window.rs:83 Event)
    def set_focus(self, on: bool) -> None:
        self.focus_on = on
        if self.peer and not self.peer.closed:
            self.peer.send("FOCUS", on=1 if on else 0)
        self.log_line(f"focus -> {'guest' if on else 'host'}")

    def forward_input(self, action: str, down: bool) -> None:
        if self.peer and not self.peer.closed and self.focus_on:
            self.peer.send("IN", a=action, d=1 if down else 0, t=int(time.monotonic() * 1000))

    # ------------------------------------------------------------ host-side state
    def update_entities(self, dt: float) -> None:
        """Guest units -> host metres (docs/MAPPING.md §3), plus the terrain height and yaw."""
        t = self.terrain_header()
        for ge in self.entities.values():
            hx, hy = B.guest_to_host(ge.x, ge.y, t)
            ge.hx, ge.hy = hx, hy
            ge.hz = terrain_height(hx, hy)
            ge.yaw = B.guest_angle_to_yaw(ge.angle)
            ge.tx = B.guest_angle_to_yaw(ge.angle + ge.turret)   # turret is relative to the hull
            ge.ty = ge.yaw
        for b in self.bursts:
            b.t += dt
        self.bursts = [b for b in self.bursts if b.t < b.life]

    def action_point(self):
        """Where the guest's action is, in host metres (docs/MAPPING.md §3)."""
        if not self.entities:
            return None
        # prefer the guest's own local player: a stable id that a player (or the demo walk) can follow
        for ge in self.entities.values():
            if ge.flags & 4 and ge.kind in (B.ENTITY_TANK, B.ENTITY_HOVER, B.ENTITY_HUMMER,
                                            B.ENTITY_PLAYER):
                return (ge.hx, ge.hy)
        vehicles = [g for g in self.entities.values()
                    if g.kind in (B.ENTITY_TANK, B.ENTITY_HOVER, B.ENTITY_HUMMER, B.ENTITY_BOT)]
        if not vehicles:
            vehicles = list(self.entities.values())
        geo = min(vehicles, key=lambda g: (g.hx - self.cam_x) ** 2 + (g.hy - self.cam_y) ** 2)
        return (geo.hx, geo.hy)

    @staticmethod
    def yaw_to_point(dx: float, dy: float) -> float:
        """Veloren yaw convention: the direction for yaw φ is (-sin φ, cos φ)."""
        return math.atan2(-dx, dy)

    def mapping_error_deg(self) -> float | None:
        """Angle between the host's view direction and the direction to the guest's player entity.

        A direct check that the guest->host mapping is right: if the two sides disagree, this grows.
        """
        p = self.action_point()
        if p is None:
            return None
        want = self.yaw_to_point(p[0] - self.cam_x, p[1] - self.cam_y)
        diff = (want - self.yaw + math.pi) % (2 * math.pi) - math.pi
        return math.degrees(abs(diff))

    def face_action(self, dead_zone: float = 0.06) -> None:
        """Turn (not walk) to keep the guest's action in view while the guest is being driven."""
        p = self.action_point()
        if p is None:
            return
        want = self.yaw_to_point(p[0] - self.cam_x, p[1] - self.cam_y)
        diff = (want - self.yaw + math.pi) % (2 * math.pi) - math.pi
        self.keys = {"right": diff > dead_zone, "left": diff < -dead_zone}

    def script_walk(self, radius: float = 26.0) -> None:
        p = self.action_point()
        if p is None:
            self.keys = {"forward": True}
            return
        dx, dy = p[0] - self.cam_x, p[1] - self.cam_y
        dist = math.hypot(dx, dy)
        want = self.yaw_to_point(dx, dy)
        diff = (want - self.yaw + math.pi) % (2 * math.pi) - math.pi
        # if the ground ahead is too steep, back off instead of walking into a cliff
        ahead_x = self.cam_x + math.cos(self.yaw) * 12.0
        ahead_y = self.cam_y + math.sin(self.yaw) * 12.0
        steep = terrain_slope(ahead_x, ahead_y) > 0.55
        # Steering is *never* gated on the ground: backing away from a cliff while still turning is what
        # keeps the guest in view, and it is what makes the mapping oracle meaningful (a host that backs
        # up without turning would report a 180 deg "mapping error" that is really just bad driving).
        self.keys = {"right": diff > 0.08, "left": diff < -0.08,
                     "forward": dist > radius and not steep, "back": steep or dist < radius * 0.6}

    def walk(self, dt: float) -> None:
        keys = self.keys
        fwd = (keys.get("forward", False) or keys.get("w", False)) - (keys.get("back", False) or keys.get("s", False))
        turn = (keys.get("right", False) or keys.get("d", False)) - (keys.get("left", False) or keys.get("a", False))
        if self.focus_on:
            fwd = turn = 0                     # keys belong to the guest while it has focus
        self.yaw += turn * 1.8 * dt
        speed = 6.2 if fwd > 0 else (-4.0 if fwd < 0 else 0.0)
        nx = self.cam_x + math.cos(self.yaw) * speed * dt
        ny = self.cam_y + math.sin(self.yaw) * speed * dt
        # veloren is z-up; our stand-in's yaw 0 = +x, matching Dir::forward() = +y after the pi/2 offset
        nx2 = self.cam_x + (-math.sin(self.yaw)) * speed * dt
        ny2 = self.cam_y + math.cos(self.yaw) * speed * dt
        if abs(terrain_slope(nx2, ny2)) < 1.0:
            self.cam_x, self.cam_y = nx2, ny2
        self.cam_z = terrain_height(self.cam_x, self.cam_y) + EYE
        # head bob-free, but keep the pitch slightly down so the ground and the table are both visible
        self.pitch = math.radians(-4.0)
        # rescatter props when the player has moved a long way (chunk streaming, in miniature)
        if math.hypot(self.cam_x - self.props_center[0], self.cam_y - self.props_center[1]) > 120.0:
            self.props = world_props(self.cam_x, self.cam_y, radius=290.0, spacing=26.0)
            self.props_center = (self.cam_x, self.cam_y)

    # ------------------------------------------------------------ render (veloren: scene/mod.rs:1552)
    def render(self) -> "np.ndarray":
        self.render_no += 1
        rw, rh = self.rw, self.rh
        yy, xx = np.mgrid[0:rh, 0:rw]
        aspect = rw / rh
        fovx = 2 * math.atan(math.tan(self.fov / 2) * aspect)
        # camera basis, veloren convention: yaw 0 -> +y, ccw
        cy, sy = math.cos(self.yaw), math.sin(self.yaw)
        fwd = np.array([-sy, cy, 0.0])
        right = np.array([cy, sy, 0.0])
        up = np.array([0.0, 0.0, 1.0])
        pitch_c, pitch_s = math.cos(self.pitch), math.sin(self.pitch)
        dirs = np.empty((rh, rw, 3), dtype=np.float32)
        sx = (2 * (xx + 0.5) / rw - 1) * math.tan(fovx / 2)
        sy_ = (1 - 2 * (yy + 0.5) / rh) * math.tan(self.fov / 2)
        f = fwd * pitch_c + up * pitch_s
        u = -fwd * pitch_s + up * pitch_c
        for k in range(3):
            dirs[..., k] = right[k] * sx + u[k] * sy_ + f[k]
        dirs /= np.linalg.norm(dirs, axis=2, keepdims=True)

        image = np.zeros((rh, rw, 3), dtype=np.float32)
        # sky: gradient + sun glow
        t_sky = (yy / rh).astype(np.float32)
        horizon = np.array([0.65, 0.76, 0.90], np.float32)
        zenith = np.array([0.24, 0.44, 0.78], np.float32)
        # t_sky = 0 at the top of the frame: zenith up, horizon down
        sky = zenith[None, None, :] * (1 - t_sky[..., None]) + horizon[None, None, :] * t_sky[..., None]
        image[:] = sky
        sun = 1.0 / (1.0 + 260 * ((xx - rw * 0.30) ** 2 + (yy - rh * 0.16) ** 2) / (rw * rh))
        image += sun[..., None] * np.array([1.0, 0.95, 0.8], np.float32) * 0.28

        # terrain: march the heightfield (veloren would already have these chunks loaded)
        origin = (self.cam_x, self.cam_y, self.cam_z)
        tmax = 260.0
        steps = 84
        hit_t = np.full((rh, rw), np.inf, dtype=np.float32)
        ground = np.zeros((rh, rw), dtype=bool)
        for s in range(steps):
            t0 = 0.7 + (s / steps) ** 1.6 * tmax
            t1 = 0.7 + ((s + 1) / steps) ** 1.6 * tmax
            tm = (t0 + t1) * 0.5
            px = origin[0] + dirs[..., 0] * tm
            py = origin[1] + dirs[..., 1] * tm
            pz = origin[2] + dirs[..., 2] * tm
            # analytic stand-in for chunk lookup: our terrain is a function
            hs = terrain_height_np(px, py)
            below = (pz <= hs) & ~ground
            if below.any():
                hit_t = np.where(below, tm, hit_t)
                ground |= below
            if ground.all():
                break

        depth = hit_t
        gx = origin[0] + dirs[..., 0] * np.where(np.isfinite(hit_t), hit_t, 0.0)
        gy = origin[1] + dirs[..., 1] * np.where(np.isfinite(hit_t), hit_t, 0.0)
        gh = np.where(np.isfinite(hit_t), origin[2] + dirs[..., 2] * hit_t, 0.0)
        # ground colour by height/water/slope, with distance fog
        slope = (np.abs(np.sin(gx / 190.0) * np.cos(gy / 160.0))
                 + 0.3 * np.abs(np.sin(gx / 41.0) * np.sin(gy / 57.0)))
        col = np.zeros((rh, rw, 3), np.float32)
        col[...] = np.array([0.34, 0.44, 0.22], np.float32)          # grass
        col[gh > 26.0] = np.array([0.42, 0.40, 0.36], np.float32)     # rock
        col[gh > 48.0] = np.array([0.82, 0.84, 0.88], np.float32)     # snow
        col[gh < 1.2] = np.array([0.76, 0.70, 0.46], np.float32)      # sand
        col[gh < 0.4] = np.array([0.13, 0.30, 0.52], np.float32)      # water
        shade = 0.66 + 0.34 * np.clip(1.0 - slope * 0.35, 0, 1)
        fog = np.clip(hit_t / 300.0, 0, 1) ** 1.7
        sky_col = horizon[None, None, :] * 0.92 + image * 0.08
        terrain_col = col * shade[..., None]
        image = np.where(ground[..., None], terrain_col * (1 - fog[..., None]) + sky_col * fog[..., None], image)

        # host props (trees/rocks), then guest entities depth-tested against what is already there
        self._draw_props(image, depth, f, u)
        self._draw_entities(image, depth, dirs, f, u)

        # the holotable: a quad in the world showing the guest's framebuffer
        self._draw_holotable(image, depth, dirs, f, u)

        # explosion bursts (host-side, from guest events -> host particles)
        for b in self.bursts:
            self._draw_burst(image, depth, dirs, f, u, b)
        return image

    # -- helpers -----------------------------------------------------------------
    def fovx(self) -> float:
        return 2 * math.atan(math.tan(self.fov / 2) * (self.rw / self.rh))

    def _basis(self):
        cy, sy = math.cos(self.yaw), math.sin(self.yaw)
        fwd = np.array([-sy, cy, 0.0])
        right = np.array([cy, sy, 0.0])
        up = np.array([0.0, 0.0, 1.0])
        pc, ps = math.cos(self.pitch), math.sin(self.pitch)
        f = fwd * pc + up * ps
        u = -fwd * ps + up * pc
        return fwd, right, f, u

    def _draw_entities(self, image, depth, dirs, f, u) -> None:
        _, right, _, _ = self._basis()
        tanx = math.tan(self.fovx() / 2)
        tany = math.tan(self.fov / 2)
        drawn = 0
        # one projection sample every few frames (enough to score, small enough to keep in stats)
        record = self.render_no % 5 == 1 and len(self.projection_samples) < 24
        sample: dict | None = None
        if record:
            ox, oy = self.terrain_origin
            t = self.terrain_header()
            sample = {
                "cam": [round(self.cam_x, 3), round(self.cam_y, 3), round(self.cam_z, 3),
                        round(self.yaw, 6), round(self.pitch, 6), round(self.fov, 6)],
                "viewport": [self.rw, self.rh],
                # the guest->host mapping in force when these sprites were placed (docs/MAPPING.md §3)
                "mapping": [ox, oy, t.cell_m, t.nx, t.ny],
                "sprites": [],
            }
        for ge in self.entities.values():
            if ge.kind in (B.ENTITY_PROJECTILE,):
                size, col, lift = 0.16, np.array([1.0, 0.88, 0.35], np.float32), 0.2
            elif ge.kind == B.ENTITY_EXPLOSION:
                size, col, lift = 0.9, np.array([1.0, 0.55, 0.2], np.float32), 0.3
            elif ge.kind == B.ENTITY_HOVER:
                size, col, lift = 1.1, np.array([0.45, 0.6, 0.9], np.float32), 0.45
            elif ge.kind == B.ENTITY_HUMMER:
                size, col, lift = 1.2, np.array([0.85, 0.3, 0.28], np.float32), 0.5
            else:
                size, col, lift = 1.35, (np.array([0.95, 0.9, 0.35], np.float32) if (ge.flags & 4)
                                         else np.array([0.35, 0.55, 0.3], np.float32)), 0.5
            z_world = ge.hz + lift
            d = np.array([ge.hx - self.cam_x, ge.hy - self.cam_y, z_world - self.cam_z], np.float32)
            zc = float(np.dot(d, f))
            if zc <= 0.4 or zc > 200.0:
                continue
            ux = float(np.dot(d, right)) / (zc * tanx)
            uy = float(np.dot(d, u)) / (zc * tany)
            if abs(ux) > 1.4 or abs(uy) > 1.4:
                continue
            cx = int((ux * 0.5 + 0.5) * self.rw)
            cy_ = int((0.5 - uy * 0.5) * self.rh)
            half = max(2, int(size * self.rh / (2 * zc * tany)))
            half = min(half, 90)
            y0, y1 = max(0, cy_ - half), min(self.rh, cy_ + half)
            x0, x1 = max(0, cx - half), min(self.rw, cx + half)
            if y1 <= y0 or x1 <= x0:
                continue
            if sample is not None:
                # exactly what the mapping produced and where the renderer put it: the demo re-derives
                # both from the docs and compares (a flipped mapping or a wrong camera convention shows
                # up here as a sprite drawn hundreds of pixels from where the docs say it belongs)
                sample["sprites"].append({
                    "id": ge.id, "kind": ge.kind, "gx": round(ge.x, 3), "gy": round(ge.y, 3),
                    "zw": round(z_world, 3), "px": cx, "py": cy_, "r": half,
                })
            sub = depth[y0:y1, x0:x1]
            visible = (sub > zc - 0.3) | ~np.isfinite(sub)
            yy, xx = np.mgrid[y0:y1, x0:x1]
            body = ((xx - cx) ** 2 / max(1, (half * 0.62)) ** 2
                    + (yy - cy_) ** 2 / max(1, (half * 0.42)) ** 2) <= 1.0
            mask = visible & body
            image[y0:y1, x0:x1][mask] = col
            drawn += 1
            # a barrel, pointing along the turret yaw, drawn as a short line
            bx = cx + int(math.cos(ge.tx) * half * 1.5)
            by = cy_ - int(math.sin(ge.tx) * half * 1.5)
            for t in np.linspace(0.0, 1.0, 6):
                px = int(cx + (bx - cx) * t)
                py = int(cy_ + (by - cy_) * t)
                if 0 <= px < self.rw and 0 <= py < self.rh and (depth[py, px] > zc - 0.3
                                                               or not np.isfinite(depth[py, px])):
                    image[py, px] = col * 0.5
        self.sprites_drawn = drawn
        if sample is not None and sample["sprites"]:
            self.projection_samples.append(sample)

    def _draw_props(self, image, depth, f, u) -> None:
        global PROP_SPRITES
        if PROP_SPRITES is None:
            PROP_SPRITES = make_prop_sprites()
        _, right, _, _ = self._basis()
        tanx = math.tan(self.fovx() / 2)
        tany = math.tan(self.fov / 2)
        rw, rh = self.rw, self.rh
        for p in self.props:
            dx, dy = p.x - self.cam_x, p.y - self.cam_y
            dist2 = dx * dx + dy * dy
            if dist2 > 240.0 ** 2:
                continue
            d = np.array([dx, dy, p.z - self.cam_z], np.float32)
            zc = float(np.dot(d, f))
            if zc <= 0.5:
                continue
            ux = float(np.dot(d, right)) / (zc * tanx)
            uy = float(np.dot(d, u)) / (zc * tany)
            if abs(ux) > 1.15 or abs(uy) > 1.15:
                continue
            sprite = PROP_SPRITES[p.kind]
            sh, sw = sprite.shape[:2]
            px_h = max(2, min(240, int((6.5 if p.kind == 0 else 2.4) * rh / (2 * zc * tany))))
            px_w = max(1, int(px_h * sw / sh))
            # nearest-neighbour resize to the on-screen size, then paste with a per-pixel depth test
            iy = (np.arange(px_h) * sh // px_h).clip(0, sh - 1)
            ix = (np.arange(px_w) * sw // px_w).clip(0, sw - 1)
            small = sprite[iy][:, ix]
            cx = int((ux * 0.5 + 0.5) * rw)
            cy_ = int((0.5 - uy * 0.5) * rh)
            x0r, y0r = cx - px_w // 2, cy_ - px_h
            x0, x1 = max(0, x0r), min(rw, x0r + px_w)
            y0, y1 = max(0, y0r), min(rh, y0r + px_h)
            if x1 <= x0 or y1 <= y0:
                continue
            sub = small[y0 - y0r:y1 - y0r, x0 - x0r:x1 - x0r]
            alpha = sub[..., 3] > 90
            if not alpha.any():
                continue
            alpha = alpha & ((depth[y0:y1, x0:x1] > zc - 0.35) | ~np.isfinite(depth[y0:y1, x0:x1]))
            window = image[y0:y1, x0:x1]
            window[alpha] = sub[..., :3][alpha].astype(np.float32) / 255.0

    def _draw_holotable(self, image, depth, dirs, f, u) -> None:
        if self.guest_frame is None:
            return
        _, right, _, _ = self._basis()
        tanx = math.tan(self.fovx() / 2)
        tany = math.tan(self.fov / 2)
        # a 2.7 m x 1.5 m panel, 3.4 m in front of the eye, tilted back a little
        fwd_w = np.array([-math.sin(self.yaw), math.cos(self.yaw), 0.0])
        cx_ = self.cam_x + fwd_w[0] * 4.2
        cy_ = self.cam_y + fwd_w[1] * 4.2
        # a holotable floats: never let it clip into the ground in front of the player
        center = np.array([cx_, cy_,
                           max(self.cam_z - 0.35, terrain_height(cx_, cy_) + 1.0)], np.float32)
        half_w, half_h = 1.5, 0.84
        tilt = math.radians(18.0)
        panel_up = np.array([0.0, 0.0, math.cos(tilt)], np.float32) + fwd_w * math.sin(tilt)
        panel_right = right.astype(np.float32)
        corners = [center - panel_right * half_w - panel_up * half_h,
                   center + panel_right * half_w - panel_up * half_h,
                   center + panel_right * half_w + panel_up * half_h,
                   center - panel_right * half_w + panel_up * half_h]
        proj = []
        for c in corners:
            d = c - np.array([self.cam_x, self.cam_y, self.cam_z], np.float32)
            zc = float(np.dot(d, f))
            if zc <= 0.2:
                return
            ux = float(np.dot(d, right)) / (zc * tanx)
            uy = float(np.dot(d, u)) / (zc * tany)
            proj.append(((ux * 0.5 + 0.5) * self.rw, (0.5 - uy * 0.5) * self.rh, zc))
        xs = [p[0] for p in proj]
        ys = [p[1] for p in proj]
        x0, x1 = int(max(0, min(xs))), int(min(self.rw, max(xs)) + 1)
        y0, y1 = int(max(0, min(ys))), int(min(self.rh, max(ys)) + 1)
        if x1 <= x0 or y1 <= y0:
            return
        # inverse bilinear: for each pixel in the bounding box, solve for (s, t) in the quad
        yy, xx = np.mgrid[y0:y1, x0:x1]
        p0, p1 = (proj[0][0], proj[0][1]), (proj[1][0], proj[1][1])
        zc_panel = sum(p[2] for p in proj) / 4.0
        # parallelogram approximation is exact enough for a screen that is nearly axis aligned in view
        ex = np.array([p1[0] - p0[0], p1[1] - p0[1]], np.float32)
        ey = np.array([proj[3][0] - p0[0], proj[3][1] - p0[1]], np.float32)
        det = ex[0] * ey[1] - ex[1] * ey[0]
        if abs(det) < 1e-6:
            return
        rx = (xx - p0[0]).astype(np.float32)
        ry = (yy - p0[1]).astype(np.float32)
        s = (rx * ey[1] - ry * ey[0]) / det
        t = (-rx * ex[1] + ry * ex[0]) / det
        inside = (s >= 0) & (s <= 1) & (t >= 0) & (t <= 1)
        fh, fw = self.guest_frame.shape[:2]
        fxs = np.clip((s * fw).astype(int), 0, fw - 1)
        fys = np.clip((t * fh).astype(int), 0, fh - 1)
        tex = self.guest_frame[fys, fxs].astype(np.float32) / 255.0   # 0..255 -> 0..1 float image
        depth_ok = (depth[y0:y1, x0:x1] > zc_panel - 0.6) | ~np.isfinite(depth[y0:y1, x0:x1])
        mask = inside & depth_ok
        sub = image[y0:y1, x0:x1]
        sub[mask] = tex[mask]
        # bezel: a bright frame around the panel, drawn as a one-pixel border of the quad
        border = inside & ~((s > 0.02) & (s < 0.98) & (t > 0.03) & (t < 0.97))
        sub[border] = np.array([0.85, 0.92, 1.0], np.float32)

    def _draw_burst(self, image, depth, dirs, f, u, b: Burst) -> None:
        _, right, _, _ = self._basis()
        tanx = math.tan(self.fovx() / 2)
        tany = math.tan(self.fov / 2)
        frac = b.t / b.life
        n = 26
        offsets = b.seeds[:n] if b.seeds is not None and len(b.seeds) >= n else np.zeros((n, 3))
        for i in range(n):
            ox, oy, oz = offsets[i] * (0.6 + 2.4 * frac)
            wx = b.x + float(ox)
            wy = b.y + float(oy)
            wz = b.z + max(0.0, 2.6 * frac - 1.2 * frac ** 2) + float(oz) * 0.4
            d = np.array([wx - self.cam_x, wy - self.cam_y, wz - self.cam_z], np.float32)
            zc = float(np.dot(d, f))
            if zc <= 0.4:
                continue
            ux = float(np.dot(d, right)) / (zc * tanx)
            uy = float(np.dot(d, u)) / (zc * tany)
            px = int((ux * 0.5 + 0.5) * self.rw)
            py = int((0.5 - uy * 0.5) * self.rh)
            if 0 <= px < self.rw and 0 <= py < self.rh:
                if not np.isfinite(depth[py, px]) or depth[py, px] > zc - 0.5:
                    c = np.array([1.0, 0.65 - 0.4 * frac, 0.15], np.float32)
                    image[py, px] = c
                    if px + 1 < self.rw:
                        image[py, px + 1] = c * 0.7

    # ------------------------------------------------------------ HUD + output
    def compose(self, image_f32) -> "np.ndarray":
        img = np.clip(image_f32, 0, 1)
        img = (img ** (1 / 1.05) * 255).astype(np.uint8)
        big = np.repeat(np.repeat(img, self.h // self.rh, axis=0), self.w // self.rw, axis=1)
        guest_ok = self.guest_frame is not None and self.watchdog.state() != "dead"
        if Image is not None:
            im = Image.fromarray(big, "RGB")
            d = ImageDraw.Draw(im)
            state = self.watchdog.state()
            d.rectangle([4, 4, 360, 62], fill=(10, 12, 16))
            d.text((10, 8), f"Veloren (host, fake)  rev={self.terrain_rev}  "
                            f"pos=({self.cam_x:.0f},{self.cam_y:.0f})  z={self.cam_z:.1f} m",
                   fill=(220, 235, 255))
            d.text((10, 22), f"guest: {'live' if guest_ok else 'offline'}  "
                             f"frame_age={self.guest_frame_age_ms:5.0f} ms  "
                             f"seq={self.last_state_seq}  entities={len(self.entities)}",
                   fill=(180, 220, 180) if guest_ok else (255, 150, 150))
            d.text((10, 36), f"events applied={self.events_applied}  watchdog={state}  "
                             f"focus={'guest' if self.focus_on else 'host'}  "
                             f"'F6' toggles, WASD moves",
                   fill=(255, 235, 180))
            d.text((10, 50), f"holotable: guest framebuffer "
                             f"({self.guest_frame_hdr.width if self.guest_frame_hdr else 0}x"
                             f"{self.guest_frame_hdr.height if self.guest_frame_hdr else 0} BGRA)",
                   fill=(200, 200, 255))
            big = np.asarray(im)
        return big

    def log_line(self, line: str) -> None:
        with open(self.log_path, "a") as f:
            f.write(f"{time.monotonic():.3f} {line}\n")


def run(args: argparse.Namespace) -> int:
    if np is None:
        log("numpy is required for the fakes: pip install numpy")
        return 2
    run_dir = B.open_run_dir(args.run_dir)
    out = Path(args.out) if args.out else run_dir / "frames"
    out.mkdir(parents=True, exist_ok=True)
    host = FakeHost(run_dir, args.port, args.width, args.height, args.render_scale, args.seed)
    host.connect()
    host.export_terrain()
    host.send_cam()
    if args.focus:
        host.set_focus(True)
        for a in [k.strip() for k in args.keys.split(",") if k.strip()]:
            host.forward_input(a, True)
        log(f"guest has input focus; holding: {args.keys or '(nothing)'}")

    stop = {"now": False}

    def on_signal(*_):
        stop["now"] = True

    signal.signal(signal.SIGTERM, on_signal)
    signal.signal(signal.SIGINT, on_signal)

    started = time.monotonic()
    last_cam = last_terrain = last_render = last_ping = 0.0
    frames = []
    gif_frames = []
    tick = 0
    log(f"rendering {args.width}x{args.height} (internal {host.rw}x{host.rh}) to {out}")
    last = time.monotonic()
    while not stop["now"]:
        now = time.monotonic()
        dt = now - last
        last = now
        if args.duration and now - started > args.duration:
            break
        if host.peer:
            for msg in host.peer.pump():
                host.watchdog.beat()
                if msg.type == "WELCOME":
                    log(f"guest: app={msg.s('app')} pid={msg.i('pid')} "
                        f"frame={msg.s('frame')} tick_hz={msg.i('tick_hz')}")
                elif msg.type == "NOTIFY":
                    pass
                elif msg.type == "PONG":
                    host.stats["rtt_us"] = host.peer.rtt_us
                elif msg.type == "BYE":
                    log(f"guest said BYE: {msg.s('reason')}")
        # scripted walk for the standalone demo (the live server drives keys instead):
        # head for where the guest's action is, which is what a player would do
        if args.script:
            host.script_walk()
        elif args.focus:
            host.face_action()
        host.walk(dt if 0 < dt < 0.5 else 0.05)
        tick += 1
        host.stats["ticks"] = tick
        if now - last_cam > 1.0 / 20.0:
            last_cam = now
            host.send_cam()
        if now - last_ping > 1.0 and host.peer and not host.peer.closed:
            last_ping = now
            host.peer.send("PING", t_us=B.monotonic_us())
        if now - last_terrain > args.terrain_period:
            last_terrain = now
            # the arena is re-centred on the player every period, and immediately when the player has
            # walked more than two cells out of the old centre (docs/ARCHITECTURE.md, frame budget)
            host.export_terrain()
        host.read_guest()
        host.update_entities(dt if 0 < dt < 0.5 else 0.05)
        err = host.mapping_error_deg()
        if err is not None:
            host.stats.setdefault("mapping_error_deg", []).append(round(err, 2))
        if now - last_render > 1.0 / args.fps:
            last_render = now
            image = host.render()
            composed = host.compose(image)
            host.stats["frames_rendered"] += 1
            host.stats.setdefault("sprites_drawn", []).append(getattr(host, "sprites_drawn", 0))
            if args.gif and tick % 2 == 0:
                gif_frames.append(Image.fromarray(composed, "RGB").resize(
                    (composed.shape[1] // 2, composed.shape[0] // 2)))
            if tick % args.save_every == 0:
                p = out / f"host_{tick:05d}.png"
                if Image is not None:
                    Image.fromarray(composed, "RGB").save(p)
                    frames.append(p)
        time.sleep(0.002)
    if args.gif and gif_frames:
        gif_path = out / "merge.gif"
        gif_frames[0].save(gif_path, save_all=True, append_images=gif_frames[1:],
                           duration=int(1000 / 12), loop=0)
        log(f"wrote {gif_path} ({len(gif_frames)} frames)")
    if host.peer:
        host.peer.close("host shutdown")
    if host.terrain_writer:
        host.terrain_writer.close()
    errs = host.stats.pop("mapping_error_deg", [])
    host.stats["mapping_error_deg_max"] = max(errs) if errs else None
    host.stats["mapping_error_deg_median"] = (
        sorted(errs)[len(errs) // 2] if errs else None)
    host.stats["mapping_error_deg_avg_when_facing"] = (
        round(sum(e for e in errs if e < 45) / max(1, len([e for e in errs if e < 45])), 2)
        if errs else None)
    ages = host.stats.pop("frame_ages_ms", [])
    ents = host.stats.pop("entity_counts", [])
    host.stats["projection_samples"] = host.projection_samples
    sprites = host.stats.pop("sprites_drawn", [])
    host.stats.update({
        "events_applied": host.events_applied,
        "sprites_drawn_max": max(sprites) if sprites else 0,
        "frames_with_sprites": sum(1 for s_ in sprites if s_ > 0),
        "frame_age_ms_avg": (sum(ages) / len(ages)) if ages else None,
        "frame_age_ms_max": max(ages) if ages else None,
        "entities_avg": (sum(ents) / len(ents)) if ents else None,
        "frames_saved": len(frames),
        "duration_s": round(time.monotonic() - started, 2),
        "camera": {"x": host.cam_x, "y": host.cam_y, "z": host.cam_z, "yaw": host.yaw},
        "terrain_revision": host.terrain_rev,
        "regions": {"state": str(B.state_path(run_dir)), "frame": str(B.frame_path(run_dir)),
                    "terrain": str(B.terrain_path(run_dir))},
    })
    if args.stats_out:
        Path(args.stats_out).write_text(json.dumps(host.stats, indent=2) + "\n")
        log(f"wrote {args.stats_out}")
    log(f"bye: {json.dumps(host.stats)[:400]}")
    log(f"SUMMARY states={host.stats['state_reads']} frames={host.stats['frame_reads']} "
        f"events_applied={host.events_applied} sprites_drawn_max={host.stats['sprites_drawn_max']} "
        f"frames_with_sprites={host.stats['frames_with_sprites']}/{host.stats['frames_rendered']} "
        f"mapping_err_avg={host.stats['mapping_error_deg_avg_when_facing']} "
        f"frame_age_avg_ms={host.stats['frame_age_ms_avg']:.1f} torn={host.stats['torn']}")
    return 0


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--run-dir", default=None)
    p.add_argument("--out", default=None)
    p.add_argument("--port", type=int, default=B.DEFAULT_PORT)
    p.add_argument("--width", type=int, default=640)
    p.add_argument("--height", type=int, default=360)
    p.add_argument("--render-scale", type=int, default=2, help="internal resolution divisor")
    p.add_argument("--fps", type=float, default=12.0)
    p.add_argument("--duration", type=float, default=0.0)
    p.add_argument("--terrain-period", type=float, default=2.0)
    p.add_argument("--save-every", type=int, default=6)
    p.add_argument("--gif", action="store_true")
    p.add_argument("--script", action="store_true", help="walk around on a canned path")
    p.add_argument("--focus", action="store_true", help="give input focus to the guest at startup")
    p.add_argument("--keys", default="", help="comma-separated guest actions to hold while focused")
    p.add_argument("--seed", type=int, default=3)
    p.add_argument("--stats-out", default=None)
    return run(p.parse_args())


if __name__ == "__main__":
    sys.exit(main())
