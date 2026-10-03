"""um-bridge v1, reference implementation (Python).

This is the executable definition of docs/PROTOCOL.md and docs/MAPPING.md. The Rust crate in
`bridge/` mirrors it, and `bridge/testdata/vectors.json` (written by `test_bridge.py`) is the shared
conformance data so the two implementations cannot drift silently.

Stdlib only, on purpose: the fakes must run anywhere, including a sandbox with no GPU.
"""

from __future__ import annotations

import math
import os
import socket
import struct
import sys
import time
import zlib
from dataclasses import dataclass, field
from pathlib import Path
from typing import Iterable, Optional

PROTOCOL_VERSION = 1
MAGIC = b"UMBR"
HDR_SIZE = 64
SLOTS = 2
DEFAULT_PORT = 47811
DEFAULT_RUN_DIR_NAME = "um-bridge/veloren-recwars"

REGION_STATE = 1
REGION_TERRAIN = 2
REGION_FRAME = 3

# ---------------------------------------------------------------- limits (docs/PROTOCOL.md §2)
MAX_ENTITIES = 512
MAX_EVENTS = 64
MAX_TERRAIN = 128
MAX_FRAME_W, MAX_FRAME_H = 1920, 1200

# ---------------------------------------------------------------- mapping constants (docs/MAPPING.md §2)
TILE_UNITS = 64.0
TILE_M = 8.0
U2M = TILE_M / TILE_UNITS
FOV_REF = 1.1
FRAC_PI_2 = 1.5707963267948966

ENTITY_NONE, ENTITY_PLAYER, ENTITY_TANK, ENTITY_HOVER, ENTITY_HUMMER = 0, 1, 2, 3, 4
ENTITY_BOT, ENTITY_PROJECTILE, ENTITY_EXPLOSION, ENTITY_PICKUP = 5, 6, 7, 8

EV_FIRE, EV_HIT, EV_EXPLOSION, EV_KILL, EV_SPAWN, EV_PICKUP, EV_SELF_DESTRUCT = 1, 2, 3, 4, 5, 6, 7

CELL_UNKNOWN, CELL_WATER, CELL_SHALLOW, CELL_SAND, CELL_GRASS, CELL_ROCK, CELL_CLIFF, CELL_SNOW = (
    0, 1, 2, 3, 4, 5, 6, 7,
)

ACTION_NAMES = (
    "left", "right", "forward", "back", "turret_l", "turret_r",
    "prev_weapon", "next_weapon", "fire", "mine", "self_destruct", "horn",
)

CLIFF_SLOPE = 1.0        # Δheight / cell_m that counts as a wall (45°)
CLIFF_DELTA_M = 8.0      # with cell_m = 8.0


def monotonic_us() -> int:
    """Cross-process comparable on one machine (CLOCK_MONOTONIC / QPC)."""
    return time.monotonic_ns() // 1000


def default_run_dir() -> Path:
    if sys.platform.startswith("win"):
        base = os.environ.get("LOCALAPPDATA") or os.path.expanduser("~")
    else:
        base = os.environ.get("XDG_RUNTIME_DIR") or "/tmp"
    return Path(base) / DEFAULT_RUN_DIR_NAME


# ================================================================ regions
HDR = struct.Struct("<4sHHIHHQHHIIQI16s")
# offsets, kept explicit because the spec is a table of offsets
OFF_MAGIC, OFF_VERSION, OFF_KIND, OFF_PAYLOAD_SIZE = 0, 4, 6, 8
OFF_SLOTS, OFF_HDR_SIZE, OFF_SEQ, OFF_SLOT = 12, 14, 16, 24
OFF_FLAGS, OFF_PAYLOAD_LEN, OFF_CRC, OFF_TS, OFF_PID = 26, 28, 32, 36, 44
COMMIT = struct.Struct("<QHHIIQ")          # seq, slot, flags, payload_len, crc32, write_ts_us = 28 B @ 16
COMMIT_OFF = OFF_SEQ
COMMIT_LEN = 28
FLAG_WRITER_ALIVE = 1 << 0


class RegionError(Exception):
    pass


@dataclass
class ReadResult:
    payload: bytes
    seq: int
    write_ts_us: int
    age_ms: float
    torn: int


class Region:
    """File-backed double-buffered region, single writer / single reader (docs/PROTOCOL.md §2.2)."""

    def __init__(self, path: Path, kind: int, payload_size: int, writer: bool):
        self.path = Path(path)
        self.kind = kind
        self.payload_size = int(payload_size)
        self.writer = writer
        self.seq = 0
        self.slot = 0
        self.torn = 0
        self._fd: Optional[int] = None
        if writer:
            self._create()
        else:
            self._open()

    # -- lifecycle -----------------------------------------------------------------
    def _create(self) -> None:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        flags = FLAG_WRITER_ALIVE
        header = HDR.pack(
            MAGIC, PROTOCOL_VERSION, self.kind, self.payload_size, SLOTS, HDR_SIZE,
            0, 0, flags, 0, 0, monotonic_us(), os.getpid(), b"\0" * 16,
        )
        assert len(header) == HDR_SIZE
        self._fd = os.open(self.path, os.O_RDWR | os.O_CREAT | os.O_TRUNC, 0o600)
        os.pwrite(self._fd, header, 0)
        os.ftruncate(self._fd, HDR_SIZE + SLOTS * self.payload_size)

    def _open(self) -> None:
        self._fd = os.open(self.path, os.O_RDONLY)
        head = os.pread(self._fd, HDR_SIZE, 0)
        if len(head) < HDR_SIZE:
            raise RegionError(f"{self.path}: short header")
        magic, version, kind, payload_size = HDR.unpack(head)[:4]
        if magic != MAGIC:
            raise RegionError(f"{self.path}: bad magic {magic!r}")
        if version != PROTOCOL_VERSION:
            raise RegionError(f"{self.path}: protocol version {version} != {PROTOCOL_VERSION}")
        if kind != self.kind:
            raise RegionError(f"{self.path}: kind {kind} != {self.kind}")
        if payload_size != self.payload_size:
            self.payload_size = payload_size  # reader adopts the writer's geometry

    def close(self, clean: bool = True) -> None:
        if self._fd is None:
            return
        if self.writer and clean:
            try:
                head = os.pread(self._fd, 2, OFF_FLAGS)
                flags = struct.unpack("<H", head)[0] & ~FLAG_WRITER_ALIVE
                os.pwrite(self._fd, struct.pack("<H", flags), OFF_FLAGS)
            except OSError:
                pass
        os.close(self._fd)
        self._fd = None

    def __enter__(self) -> "Region":
        return self

    def __exit__(self, *exc) -> None:
        self.close()

    # -- writer --------------------------------------------------------------------
    def write(self, payload: bytes, ts_us: Optional[int] = None) -> int:
        if not self.writer:
            raise RegionError("not a writer")
        if len(payload) > self.payload_size:
            raise RegionError(f"payload {len(payload)} > slot {self.payload_size}")
        slot = self.slot
        os.pwrite(self._fd, payload, HDR_SIZE + slot * self.payload_size)
        self.seq += 1
        crc = zlib.crc32(payload) & 0xFFFFFFFF
        os.pwrite(
            self._fd,
            COMMIT.pack(self.seq, slot, FLAG_WRITER_ALIVE, len(payload), crc,
                        ts_us if ts_us is not None else monotonic_us()),
            COMMIT_OFF,
        )
        self.slot = slot ^ 1
        return self.seq

    # -- reader --------------------------------------------------------------------
    def read(self, attempts: int = 8) -> Optional[ReadResult]:
        prefix = os.pread(self._fd, 44, 0)
        if len(prefix) < 44:
            return None
        seq = struct.unpack_from("<Q", prefix, OFF_SEQ)[0]
        if seq == 0:
            return None
        for _ in range(attempts):
            seq, slot, _flags, payload_len, crc, ts = struct.unpack_from("<QHHIIQ", prefix, OFF_SEQ)
            if payload_len > self.payload_size:
                raise RegionError(f"payload_len {payload_len} > {self.payload_size}")
            payload = os.pread(self._fd, payload_len, HDR_SIZE + slot * self.payload_size)
            again = os.pread(self._fd, COMMIT_LEN, COMMIT_OFF)
            if again != prefix[OFF_SEQ:OFF_SEQ + COMMIT_LEN]:
                self.torn += 1
                prefix = os.pread(self._fd, 44, 0)
                continue
            if len(payload) != payload_len or (zlib.crc32(payload) & 0xFFFFFFFF) != crc:
                self.torn += 1
                continue
            return ReadResult(payload, seq, ts, age_ms=(monotonic_us() - ts) / 1000.0, torn=self.torn)
        self.torn += 1
        return None

    def writer_alive(self) -> bool:
        flags = struct.unpack("<H", os.pread(self._fd, 2, OFF_FLAGS))[0]
        return bool(flags & FLAG_WRITER_ALIVE)


# ================================================================ payload layouts
STATE_HDR = struct.Struct("<IIHHHHffff")     # 32 B
ENTITY = struct.Struct("<IBBBBffffffff")   # 40 B
EVENT = struct.Struct("<BBHIffff")           # 24 B
TERRAIN_HDR = struct.Struct("<fffHHffII")    # 32 B
CELL = struct.Struct("<fBBH")                # 8 B
FRAME_HDR = struct.Struct("<HHIBBHIQII")     # 32 B

assert STATE_HDR.size == 32, STATE_HDR.size
assert ENTITY.size == 40, ENTITY.size
assert EVENT.size == 24, EVENT.size
assert TERRAIN_HDR.size == 32, TERRAIN_HDR.size
assert CELL.size == 8, CELL.size
assert FRAME_HDR.size == 32, FRAME_HDR.size


@dataclass
class Entity:
    id: int
    kind: int
    team: int = 0
    flags: int = 1
    x: float = 0.0
    y: float = 0.0
    z: float = 0.0
    angle: float = 0.0
    turret_angle: float = 0.0
    vx: float = 0.0
    vy: float = 0.0
    hp_frac: float = 1.0

    def pack(self) -> bytes:
        return ENTITY.pack(self.id, self.kind, self.team, self.flags, 0,
                           self.x, self.y, self.z, self.angle, self.turret_angle,
                           self.vx, self.vy, self.hp_frac)

    @staticmethod
    def unpack(buf: bytes, off: int) -> "Entity":
        vals = ENTITY.unpack_from(buf, off)
        e = Entity(int(vals[0]), int(vals[1]), int(vals[2]), int(vals[3]))
        e.x, e.y, e.z, e.angle, e.turret_angle, e.vx, e.vy, e.hp_frac = (float(v) for v in vals[5:])
        return e


@dataclass
class Event:
    kind: int
    flags: int = 0
    arg16: int = 0
    t_ms: int = 0
    x: float = 0.0
    y: float = 0.0
    z: float = 0.0
    arg: float = 0.0

    def pack(self) -> bytes:
        return EVENT.pack(self.kind, self.flags, self.arg16, self.t_ms,
                          self.x, self.y, self.z, self.arg)

    @staticmethod
    def unpack(buf: bytes, off: int) -> "Event":
        vals = EVENT.unpack_from(buf, off)
        e = Event(int(vals[0]), int(vals[1]), int(vals[2]), int(vals[3]))
        e.x, e.y, e.z, e.arg = (float(v) for v in vals[4:])
        return e


@dataclass
class StateHeader:
    tick: int = 0
    game_time_ms: int = 0
    ent_count: int = 0
    event_count: int = 0
    focus_ent: int = 0xFFFF
    flags: int = 0
    player_x: float = 0.0
    player_y: float = 0.0
    cam_x: float = 0.0
    cam_y: float = 0.0

    def pack(self) -> bytes:
        return STATE_HDR.pack(self.tick, self.game_time_ms, self.ent_count, self.event_count,
                              self.focus_ent, self.flags, self.player_x, self.player_y,
                              self.cam_x, self.cam_y)

    @staticmethod
    def unpack(buf: bytes) -> "StateHeader":
        v = STATE_HDR.unpack_from(buf, 0)
        return StateHeader(int(v[0]), int(v[1]), int(v[2]), int(v[3]), int(v[4]), int(v[5]),
                           float(v[6]), float(v[7]), float(v[8]), float(v[9]))


def pack_state(header: StateHeader, entities: Iterable[Entity], events: Iterable[Event]) -> bytes:
    ents = list(entities)[:MAX_ENTITIES]
    evs = list(events)[-MAX_EVENTS:]
    header.ent_count = len(ents)
    header.event_count = len(evs)
    blob = bytearray(header.pack())
    blob += b"".join(e.pack() for e in ents)
    blob += b"".join(e.pack() for e in evs)
    return bytes(blob)


def unpack_state(payload: bytes) -> tuple[StateHeader, list[Entity], list[Event]]:
    h = StateHeader.unpack(payload)
    if h.ent_count > MAX_ENTITIES or h.event_count > MAX_EVENTS:
        raise RegionError("state counts out of range")
    need = STATE_HDR.size + h.ent_count * ENTITY.size + h.event_count * EVENT.size
    if len(payload) < need:
        raise RegionError(f"state payload {len(payload)} < {need}")
    off = STATE_HDR.size
    ents = [Entity.unpack(payload, off + i * ENTITY.size) for i in range(h.ent_count)]
    off += h.ent_count * ENTITY.size
    evs = [Event.unpack(payload, off + i * EVENT.size) for i in range(h.event_count)]
    return h, ents, evs


@dataclass
class Cell:
    height_m: float = 0.0
    kind: int = CELL_UNKNOWN
    flags: int = 0


@dataclass
class TerrainHeader:
    origin_x: float = 0.0
    origin_y: float = 0.0
    cell_m: float = TILE_M
    nx: int = 64
    ny: int = 64
    tile_m: float = TILE_M
    sea_level: float = 0.0
    revision: int = 0
    flags: int = 0

    @property
    def origin_y_max(self) -> float:
        return self.origin_y + self.ny * self.cell_m

    def pack(self) -> bytes:
        return TERRAIN_HDR.pack(self.origin_x, self.origin_y, self.cell_m, self.nx, self.ny,
                                self.tile_m, self.sea_level, self.revision, self.flags)

    @staticmethod
    def unpack(buf: bytes) -> "TerrainHeader":
        v = TERRAIN_HDR.unpack_from(buf, 0)
        return TerrainHeader(float(v[0]), float(v[1]), float(v[2]), int(v[3]), int(v[4]),
                             float(v[5]), float(v[6]), int(v[7]), int(v[8]))


def pack_terrain(header: TerrainHeader, cells: list[Cell]) -> bytes:
    if len(cells) != header.nx * header.ny:
        raise RegionError(f"cells {len(cells)} != {header.nx}x{header.ny}")
    if header.nx > MAX_TERRAIN or header.ny > MAX_TERRAIN:
        raise RegionError("terrain too large")
    return header.pack() + b"".join(CELL.pack(c.height_m, c.kind, c.flags, 0) for c in cells)


def unpack_terrain(payload: bytes) -> tuple[TerrainHeader, list[Cell]]:
    h = TerrainHeader.unpack(payload)
    if h.nx > MAX_TERRAIN or h.ny > MAX_TERRAIN:
        raise RegionError("terrain too large")
    need = TERRAIN_HDR.size + h.nx * h.ny * CELL.size
    if len(payload) < need:
        raise RegionError(f"terrain payload {len(payload)} < {need}")
    cells = []
    for i in range(h.nx * h.ny):
        height, kind, flags, _pad = CELL.unpack_from(payload, TERRAIN_HDR.size + i * CELL.size)
        cells.append(Cell(float(height), int(kind), int(flags)))
    return h, cells


@dataclass
class FrameHeader:
    width: int = 0
    height: int = 0
    stride: int = 0
    format: int = 1
    flags: int = 0
    scale_permille: int = 1000
    publish_ts_us: int = 0
    game_tick: int = 0

    def pack(self) -> bytes:
        return FRAME_HDR.pack(self.width, self.height, self.stride, self.format, self.flags,
                              self.scale_permille, 0, self.publish_ts_us, self.game_tick, 0)

    @staticmethod
    def unpack(buf: bytes) -> "FrameHeader":
        v = FRAME_HDR.unpack_from(buf, 0)
        return FrameHeader(int(v[0]), int(v[1]), int(v[2]), int(v[3]), int(v[4]), int(v[5]),
                           int(v[7]), int(v[8]))


def pack_frame(width: int, height: int, pixels: bytes, game_tick: int = 0,
               scale_permille: int = 1000, ts_us: Optional[int] = None) -> bytes:
    if width > MAX_FRAME_W or height > MAX_FRAME_H:
        raise RegionError("frame too large")
    h = FrameHeader(width, height, width * 4, 1, 1, scale_permille,
                    ts_us if ts_us is not None else monotonic_us(), game_tick)
    if len(pixels) != height * h.stride:
        raise RegionError(f"pixels {len(pixels)} != {height * h.stride}")
    return h.pack() + pixels


def unpack_frame(payload: bytes) -> tuple[FrameHeader, bytes]:
    h = FrameHeader.unpack(payload)
    if h.width > MAX_FRAME_W or h.height > MAX_FRAME_H:
        raise RegionError("frame too large")
    if h.stride < h.width * 4:
        raise RegionError("bad stride")
    need = FRAME_HDR.size + h.height * h.stride
    if len(payload) < need:
        raise RegionError(f"frame payload {len(payload)} < {need}")
    return h, payload[FRAME_HDR.size:need]


# ================================================================ mapping (docs/MAPPING.md)
def guest_to_host(gx: float, gy: float, t: TerrainHeader) -> tuple[float, float]:
    return (t.origin_x + gx * U2M, t.origin_y_max - gy * U2M)


def host_to_guest(hx: float, hy: float, t: TerrainHeader) -> tuple[float, float]:
    return ((hx - t.origin_x) / U2M, (t.origin_y_max - hy) / U2M)


def clamp_to_region(gx: float, gy: float, t: TerrainHeader) -> tuple[float, float]:
    span = t.nx * TILE_UNITS
    return (min(max(gx, 0.0), span), min(max(gy, 0.0), t.ny * TILE_UNITS))


def normalize_angle(a: float) -> float:
    """Wrap to (-pi, pi] — the range the protocol and docs use."""
    a = math.fmod(a + math.pi, 2.0 * math.pi)
    if a <= 0.0:
        a += 2.0 * math.pi
    return a - math.pi


def guest_angle_to_yaw(theta: float) -> float:
    """docs/MAPPING.md §4: yaw = -theta - pi/2, normalised."""
    return normalize_angle(-theta - FRAC_PI_2)


def yaw_to_guest_angle(yaw: float) -> float:
    return normalize_angle(-yaw - FRAC_PI_2)


def cell_at(hx: float, hy: float, t: TerrainHeader) -> tuple[int, int]:
    i = int((hx - t.origin_x) // t.cell_m)
    j = int((hy - t.origin_y) // t.cell_m)
    return (min(max(i, 0), t.nx - 1), min(max(j, 0), t.ny - 1))


def classify_cell(cells: list[Cell], i: int, j: int, nx: int, ny: int, sea: float,
                  cell_m: float = TILE_M) -> int:
    """docs/MAPPING.md §5, one cell -> a `kind`. `cells` is row-major with j increasing with host y."""
    h = cells[j * nx + i].height_m
    if h < sea + 0.5:
        return CELL_WATER
    if h < sea + 1.2:
        return CELL_SHALLOW
    worst = 0.0
    for di, dj in ((1, 0), (-1, 0), (0, 1), (0, -1)):
        ii, jj = i + di, j + dj
        if 0 <= ii < nx and 0 <= jj < ny:
            worst = max(worst, abs(cells[jj * nx + ii].height_m - h))
    if worst >= CLIFF_DELTA_M:
        return CELL_CLIFF
    if h < sea + 2.2:
        return CELL_SAND
    if h > sea + 120.0:
        return CELL_SNOW
    slope = worst / cell_m
    if slope >= 0.5 or h > sea + 90.0:
        return CELL_ROCK
    return CELL_GRASS


def guest_surface_for(kind: int) -> str:
    """Guest-facing name of the RecWars surface a cell kind becomes (`src/map.rs:305`)."""
    return {
        CELL_UNKNOWN: "wall",
        CELL_WATER: "water",
        CELL_SHALLOW: "water",
        CELL_SAND: "sand",
        CELL_GRASS: "grass",
        CELL_ROCK: "rock",
        CELL_CLIFF: "wall",
        CELL_SNOW: "snow",
    }[kind]


# ================================================================ control channel
class ControlError(Exception):
    pass


@dataclass
class Msg:
    type: str
    fields: dict = field(default_factory=dict)

    def i(self, key: str, default: int = 0) -> int:
        try:
            return int(float(self.fields.get(key, default)))
        except (TypeError, ValueError):
            return default

    def f(self, key: str, default: float = 0.0) -> float:
        try:
            return float(self.fields.get(key, default))
        except (TypeError, ValueError):
            return default

    def s(self, key: str, default: str = "") -> str:
        return self.fields.get(key, default)

    def encode(self) -> bytes:
        parts = [self.type] + [f"{k}={v}" for k, v in self.fields.items()]
        return (" ".join(parts) + "\n").encode("ascii", "replace")


def parse_line(line: str) -> Optional[Msg]:
    line = line.strip()
    if not line:
        return None
    parts = line.split(" ")
    fields: dict = {}
    for p in parts[1:]:
        if "=" in p:
            k, _, v = p.partition("=")
            fields[k] = v
    return Msg(parts[0], fields)


class ControlPeer:
    """Line-protocol TCP peer: never blocks a frame, never raises on a dead socket."""

    def __init__(self, sock: socket.socket):
        self.sock = sock
        self.sock.setblocking(False)
        self._buf = b""
        self.inbox: list[Msg] = []
        self.hello: Optional[Msg] = None
        self.closed = False
        self.sent = 0
        self.received = 0
        self.dropped = 0
        self.last_rx_us = monotonic_us()
        self.last_tx_us = monotonic_us()
        self.rtt_us: Optional[float] = None
        self.clock_offset_us: Optional[float] = None

    # -- io ------------------------------------------------------------------------
    def pump(self) -> list[Msg]:
        """Read everything available; returns newly parsed messages."""
        if self.closed:
            return []
        fresh: list[Msg] = []
        while True:
            try:
                chunk = self.sock.recv(65536)
            except BlockingIOError:
                break
            except OSError:
                self.closed = True
                break
            if not chunk:
                self.closed = True
                break
            self._buf += chunk
        while b"\n" in self._buf:
            raw, _, self._buf = self._buf.partition(b"\n")
            msg = parse_line(raw.decode("ascii", "replace"))
            if msg is not None:
                self.received += 1
                self.last_rx_us = monotonic_us()
                if msg.type == "PING":
                    self.send("PONG", t_us=msg.i("t_us"))
                elif msg.type == "PONG":
                    self.rtt_us = (monotonic_us() - msg.i("t_us")) / 2.0
                self.inbox.append(msg)
                fresh.append(msg)
        return fresh

    def send(self, mtype: str, **fields) -> bool:
        if self.closed:
            return False
        try:
            self.sock.sendall(Msg(mtype, fields).encode())
            self.sent += 1
            self.last_tx_us = monotonic_us()
            return True
        except BlockingIOError:
            self.dropped += 1
            return False
        except OSError:
            self.closed = True
            return False

    def take(self, mtype: str) -> list[Msg]:
        got = [m for m in self.inbox if m.type == mtype]
        self.inbox = [m for m in self.inbox if m.type != mtype]
        return got

    def silent_ms(self) -> float:
        return (monotonic_us() - max(self.last_rx_us, self.last_tx_us)) / 1000.0

    def close(self, reason: str = "normal") -> None:
        if not self.closed:
            self.send("BYE", reason=reason)
            try:
                self.sock.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
        self.sock.close()
        self.closed = True


class GuestServer:
    """Guest side: listens for the host on loopback."""

    def __init__(self, port: int = DEFAULT_PORT):
        self.listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.listener.bind(("127.0.0.1", port))
        self.listener.listen(1)
        self.listener.setblocking(False)
        self.port = port
        self.peer: Optional[ControlPeer] = None
        self.refused = 0

    def accept(self) -> Optional[ControlPeer]:
        if self.peer is not None and not self.peer.closed:
            return self.peer
        try:
            sock, addr = self.listener.accept()
        except BlockingIOError:
            return self.peer
        if addr[0] not in ("127.0.0.1", "::1"):
            sock.close()
            self.refused += 1
            return self.peer
        self.peer = ControlPeer(sock)
        return self.peer

    def close(self) -> None:
        if self.peer:
            self.peer.close("guest shutdown")
        self.listener.close()


def connect_host(port: int = DEFAULT_PORT, timeout_s: float = 5.0) -> ControlPeer:
    deadline = time.monotonic() + timeout_s
    while True:
        try:
            sock = socket.create_connection(("127.0.0.1", port), timeout=1.0)
            return ControlPeer(sock)
        except OSError:
            if time.monotonic() > deadline:
                raise ControlError(f"no guest listening on 127.0.0.1:{port}")
            time.sleep(0.1)


# ================================================================ run-dir helper
def open_run_dir(run_dir: Optional[str | Path] = None) -> Path:
    d = Path(run_dir) if run_dir else default_run_dir()
    d.mkdir(parents=True, exist_ok=True)
    return d


def state_path(run_dir: Path) -> Path:
    return Path(run_dir) / "state.region"


def terrain_path(run_dir: Path) -> Path:
    return Path(run_dir) / "terrain.region"


def frame_path(run_dir: Path) -> Path:
    return Path(run_dir) / "frame.region"


# ================================================================ watchdog
class Watchdog:
    """Peer liveness from "any traffic at all" (docs/PROTOCOL.md §4).

    `ok` < `stale_ms`, `stale` < `dead_ms`, then `dead`. The host freezes the last guest frame on
    `dead`, the guest releases focus and zeroes bridge input on `dead`.
    """

    def __init__(self, stale_ms: float = 600.0, dead_ms: float = 2000.0):
        self.stale_ms = stale_ms
        self.dead_ms = dead_ms
        self.last_activity_us = monotonic_us()

    def beat(self, now_us: Optional[int] = None) -> None:
        self.last_activity_us = now_us if now_us is not None else monotonic_us()

    def age_ms(self, now_us: Optional[int] = None) -> float:
        now = now_us if now_us is not None else monotonic_us()
        return (now - self.last_activity_us) / 1000.0

    def state(self, now_us: Optional[int] = None) -> str:
        age = self.age_ms(now_us)
        if age >= self.dead_ms:
            return "dead"
        if age >= self.stale_ms:
            return "stale"
        return "ok"
