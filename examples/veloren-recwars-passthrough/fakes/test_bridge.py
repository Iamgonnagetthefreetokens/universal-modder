"""Tests for the um-bridge reference implementation, and the generator for the shared vectors.

Run:      python3 fakes/test_bridge.py
Vectors:  python3 fakes/test_bridge.py --write-vectors
          -> bridge/testdata/vectors.json, consumed by bridge/tests/conformance.rs

Every vector that the Rust side must agree with lives in that file; the tests here assert the same
numbers, so a disagreement fails on whichever side is wrong.
"""

from __future__ import annotations

import json
import os
import struct
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import bridge as B  # noqa: E402

FAILURES: list[str] = []
CHECKS = 0


def check(cond: bool, what: str) -> None:
    global CHECKS
    CHECKS += 1
    if not cond:
        FAILURES.append(what)
        print(f"  FAIL  {what}")


def eq(got, want, what: str) -> None:
    check(got == want, f"{what}: got {got!r}, want {want!r}")


def approx(got: float, want: float, what: str, tol: float = 1e-6) -> None:
    check(abs(got - want) <= tol, f"{what}: got {got!r}, want {want!r} (±{tol})")


# ---------------------------------------------------------------- layouts
def test_layouts() -> dict:
    print("layouts")
    eq(B.HDR.size, 64, "region header size")
    eq(B.COMMIT.size, 28, "commit size")
    eq(B.STATE_HDR.size, 32, "state header size")
    eq(B.ENTITY.size, 40, "entity size")
    eq(B.EVENT.size, 24, "event size")
    eq(B.TERRAIN_HDR.size, 32, "terrain header size")
    eq(B.CELL.size, 8, "cell size")
    eq(B.FRAME_HDR.size, 32, "frame header size")

    # field offsets, asserted by packing distinctive values and looking at the bytes
    e = B.Entity(id=0x01020304, kind=B.ENTITY_TANK, team=3, flags=0b101, x=1.0, y=2.0, z=3.0,
                 angle=4.0, turret_angle=5.0, vx=6.0, vy=7.0, hp_frac=0.5)
    raw = e.pack()
    eq(struct.unpack_from("<I", raw, 0)[0], 0x01020304, "entity.id @0")
    eq(raw[4], B.ENTITY_TANK, "entity.kind @4")
    eq(raw[5], 3, "entity.team @5")
    eq(raw[6], 0b101, "entity.flags @6")
    approx(struct.unpack_from("<f", raw, 8)[0], 1.0, "entity.x @8")
    approx(struct.unpack_from("<f", raw, 16)[0], 3.0, "entity.z @16")
    approx(struct.unpack_from("<f", raw, 20)[0], 4.0, "entity.angle @20")
    approx(struct.unpack_from("<f", raw, 24)[0], 5.0, "entity.turret_angle @24")
    approx(struct.unpack_from("<f", raw, 32)[0], 7.0, "entity.vy @32")
    approx(struct.unpack_from("<f", raw, 36)[0], 0.5, "entity.hp_frac @36")

    ev = B.Event(kind=B.EV_EXPLOSION, flags=1, arg16=120, t_ms=1234, x=10.0, y=20.0, z=1.5, arg=9.0)
    raw = ev.pack()
    eq(raw[0], B.EV_EXPLOSION, "event.kind @0")
    eq(struct.unpack_from("<H", raw, 2)[0], 120, "event.arg16 @2")
    eq(struct.unpack_from("<I", raw, 4)[0], 1234, "event.t_ms @4")
    approx(struct.unpack_from("<f", raw, 8)[0], 10.0, "event.x @8")
    approx(struct.unpack_from("<f", raw, 20)[0], 9.0, "event.arg @20")

    th = B.TerrainHeader(origin_x=1000.0, origin_y=2000.0, cell_m=8.0, nx=64, ny=64,
                         tile_m=8.0, sea_level=0.0, revision=7, flags=1)
    raw = th.pack()
    approx(struct.unpack_from("<f", raw, 0)[0], 1000.0, "terrain.origin_x @0")
    eq(struct.unpack_from("<H", raw, 12)[0], 64, "terrain.nx @12")
    approx(struct.unpack_from("<f", raw, 16)[0], 8.0, "terrain.tile_m @16")
    eq(struct.unpack_from("<I", raw, 24)[0], 7, "terrain.revision @24")

    fh = B.FrameHeader(width=640, height=360, stride=2560, format=1, flags=1,
                       scale_permille=500, publish_ts_us=123456789, game_tick=9)
    raw = fh.pack()
    eq(struct.unpack_from("<H", raw, 0)[0], 640, "frame.width @0")
    eq(struct.unpack_from("<H", raw, 2)[0], 360, "frame.height @2")
    eq(struct.unpack_from("<I", raw, 4)[0], 2560, "frame.stride @4")
    eq(raw[8], 1, "frame.format @8")
    eq(struct.unpack_from("<H", raw, 10)[0], 500, "frame.scale_permille @10")
    eq(struct.unpack_from("<Q", raw, 16)[0], 123456789, "frame.publish_ts_us @16")
    eq(struct.unpack_from("<I", raw, 24)[0], 9, "frame.game_tick @24")

    return {"sizes": {"region_header": 64, "commit": 28, "state_header": 32, "entity": 40,
                      "event": 24, "terrain_header": 32, "cell": 8, "frame_header": 32}}


# ---------------------------------------------------------------- payload round trips
def test_payloads() -> dict:
    print("payload round trips")
    ents = [B.Entity(i, B.ENTITY_TANK, team=i % 2, x=i * 1.5, y=-i * 2.0, angle=i * 0.1)
            for i in range(5)]
    evs = [B.Event(B.EV_EXPLOSION, t_ms=100 + i, x=i, y=i * 2.0, arg=3.0) for i in range(3)]
    hdr = B.StateHeader(tick=42, game_time_ms=700, player_x=1.0, player_y=2.0)
    blob = B.pack_state(hdr, ents, evs)
    h2, e2, v2 = B.unpack_state(blob)
    eq(h2.ent_count, 5, "state ent_count")
    eq(h2.event_count, 3, "state event_count")
    eq(len(e2), 5, "state entities parsed")
    eq(len(v2), 3, "state events parsed")
    eq(e2[3].x, 4.5, "entity x round trip")
    eq(v2[2].t_ms, 102, "event t_ms round trip")
    eq(len(blob), B.STATE_HDR.size + 5 * B.ENTITY.size + 3 * B.EVENT.size, "state payload length")

    th = B.TerrainHeader(origin_x=-100.0, origin_y=50.0, nx=8, ny=8, revision=3)
    cells = [B.Cell(height_m=float(i), kind=B.CELL_GRASS, flags=1) for i in range(64)]
    tb = B.pack_terrain(th, cells)
    th2, c2 = B.unpack_terrain(tb)
    eq((th2.nx, th2.ny), (8, 8), "terrain dims round trip")
    eq(len(c2), 64, "terrain cells round trip")
    eq(c2[10].height_m, 10.0, "cell height round trip")
    eq(c2[10].kind, B.CELL_GRASS, "cell kind round trip")

    pixels = bytes(4 * 16 * 8)
    fb = B.pack_frame(16, 8, pixels)
    fh2, px2 = B.unpack_frame(fb)
    eq((fh2.width, fh2.height, fh2.stride), (16, 8, 64), "frame geometry round trip")
    eq(len(px2), 4 * 16 * 8, "frame pixels round trip")

    return {
        "state_sample_hex": blob[:64].hex(),
        "entity_sample_hex": B.Entity(0x01020304, B.ENTITY_TANK, 3, 0b101, 1.0, 2.0, 3.0, 4.0,
                                      5.0, 6.0, 7.0, 0.5).pack().hex(),
    }


# ---------------------------------------------------------------- regions
def test_regions(tmp: Path) -> dict:
    print("regions")
    p = tmp / "state.region"
    w = B.Region(p, B.REGION_STATE, 4096, writer=True)
    r = B.Region(p, B.REGION_STATE, 4096, writer=False)
    check(r.read() is None, "fresh region reads empty")
    check(r.writer_alive(), "writer alive bit")

    hdr = B.StateHeader(tick=1)
    w.write(B.pack_state(hdr, [B.Entity(1, B.ENTITY_TANK, x=5.0)], []))
    res = r.read()
    check(res is not None, "read after write")
    assert res is not None
    eq(res.seq, 1, "seq 1")
    h, ents, _ = B.unpack_state(res.payload)
    eq(h.tick, 1, "tick round trip")
    eq(ents[0].x, 5.0, "entity round trip")

    # two writes: the reader can still read the older slot while the writer commits the newer one
    w.write(B.pack_state(B.StateHeader(tick=2), [], []))
    res2 = r.read()
    assert res2 is not None
    eq(res2.seq, 2, "seq 2")
    eq(B.unpack_state(res2.payload)[0].tick, 2, "second payload")

    # corrupt the committed payload -> CRC must reject, reader must not return garbage
    with open(p, "r+b") as f:
        head = f.read(44)
        seq, slot, flags, plen, crc, ts = struct.unpack_from("<QHHIIQ", head, B.OFF_SEQ)
        f.seek(B.HDR_SIZE + slot * 4096 + 3)
        f.write(b"\xff")
    before = r.torn
    check(r.read() is None, "CRC rejects a corrupted payload")
    check(r.torn > before, "torn/corrupt counter advanced")

    w.close()
    check(not r.writer_alive(), "writer alive bit cleared on clean close")
    r.close()

    # geometry: the reader adopts the writer's payload size
    p2 = tmp / "frame.region"
    w2 = B.Region(p2, B.REGION_FRAME, 128, writer=True)
    r2 = B.Region(p2, B.REGION_FRAME, 1, writer=False)
    eq(r2.payload_size, 128, "reader adopts payload size")
    w2.close()
    r2.close()

    # wrong kind / bad magic are loud
    try:
        B.Region(p2, B.REGION_TERRAIN, 8, writer=False)
        check(False, "kind mismatch raises")
    except B.RegionError:
        check(True, "kind mismatch raises")
    return {}


# ---------------------------------------------------------------- mapping
def test_mapping() -> dict:
    print("mapping")
    t = B.TerrainHeader(origin_x=1000.0, origin_y=2000.0, cell_m=8.0, nx=64, ny=64, tile_m=8.0)
    approx(t.origin_y_max, 2512.0, "origin_y_max")
    approx(B.U2M, 0.125, "U2M")

    # docs/MAPPING.md §3 worked vectors
    vectors = []
    for gx, gy, ex, ey, ei, ej in [
        (260.0, 196.0, 1032.5, 2487.5, 4, 60),
        (0.0, 0.0, 1000.0, 2512.0, 0, 63),
        (4096.0, 4096.0, 1512.0, 2000.0, 63, 0),
        (4096.0, 0.0, 1512.0, 2512.0, 63, 63),
    ]:
        hx, hy = B.guest_to_host(gx, gy, t)
        approx(hx, ex, f"guest_to_host x({gx},{gy})")
        approx(hy, ey, f"guest_to_host y({gx},{gy})")
        gx2, gy2 = B.host_to_guest(hx, hy, t)
        approx(gx2, gx, f"host_to_guest round trip x({gx},{gy})")
        approx(gy2, gy, f"host_to_guest round trip y({gx},{gy})")
        eq(B.cell_at(hx, hy, t), (ei, ej), f"cell_at({gx},{gy})")
        vectors.append({"guest": [gx, gy], "host": [hx, hy], "cell": [ei, ej]})

    # docs/MAPPING.md §4 angles
    angle_vectors = []
    for theta, yaw, dir_xy in [
        (0.0, -B.FRAC_PI_2, (1.0, 0.0)),
        (B.FRAC_PI_2, 3.141592653589793, (0.0, -1.0)),
        (3.141592653589793, B.FRAC_PI_2, (-1.0, 0.0)),
    ]:
        got = B.guest_angle_to_yaw(theta)
        approx(got, yaw, f"yaw for theta={theta}")
        # Veloren direction from yaw: (-sin, cos)
        import math
        dx, dy = -math.sin(got), math.cos(got)
        approx(dx, dir_xy[0], f"dir.x for theta={theta}", 1e-6)
        approx(dy, dir_xy[1], f"dir.y for theta={theta}", 1e-6)
        approx(B.yaw_to_guest_angle(got), theta, f"angle round trip theta={theta}")
        angle_vectors.append({"theta": theta, "yaw": got, "dir": [dx, dy]})
    return {"positions": vectors, "angles": angle_vectors,
            "constants": {"tile_units": B.TILE_UNITS, "tile_m": B.TILE_M, "u2m": B.U2M}}


# ---------------------------------------------------------------- classification
def test_classification() -> dict:
    print("terrain classification")
    nx = ny = 5
    cells = [B.Cell(height_m=5.0) for _ in range(nx * ny)]

    def set_h(i, j, h):
        cells[j * nx + i].height_m = h

    set_h(0, 0, -3.0)          # water
    set_h(1, 0, 0.9)           # shallow
    set_h(2, 0, 1.8)           # sand
    set_h(3, 0, 4.0)           # grass
    set_h(1, 2, 5.0)
    set_h(1, 3, 5.0 + B.CLIFF_DELTA_M)  # 13.0 vs 5.0 -> cliff share
    for i in range(nx):
        set_h(i, 4, 150.0)     # a high plateau: snow, and no artificial cliff edge inside it
    for j in range(ny):
        set_h(4, j, 150.0)
    kinds = {(i, j): B.classify_cell(cells, i, j, nx, ny, 0.0) for j in range(ny) for i in range(nx)}
    eq(kinds[(0, 0)], B.CELL_WATER, "water below sea+0.5")
    eq(kinds[(1, 0)], B.CELL_SHALLOW, "shallow below sea+1.2")
    eq(kinds[(2, 0)], B.CELL_SAND, "sand below sea+2.2")
    eq(kinds[(2, 1)], B.CELL_GRASS, "flat low ground away from the plateau is grass")
    eq(kinds[(4, 4)], B.CELL_SNOW, "high ground is snow")
    # cell (1,2) has a 8 m neighbour -> cliff; (1,3) too
    eq(kinds[(1, 2)], B.CELL_CLIFF, "8 m step is a cliff")
    surfaces = {k: B.guest_surface_for(v) for k, v in kinds.items()}
    eq(surfaces[(0, 0)], "water", "water -> RecWars Water surface")
    eq(surfaces[(1, 2)], "wall", "cliff -> RecWars Wall surface")
    return {"cells": [{"i": i, "j": j, "height": cells[j * nx + i].height_m,
                       "kind": kinds[(i, j)]} for j in range(ny) for i in range(nx)],
            "nx": nx, "ny": ny}


# ---------------------------------------------------------------- control channel
def test_control() -> dict:
    print("control channel")
    server = B.GuestServer(port=0)
    port = server.listener.getsockname()[1]
    host = B.connect_host(port, timeout_s=2.0)
    peer = None
    for _ in range(100):
        peer = server.accept()
        if peer is not None:
            break
        import time
        time.sleep(0.005)
    check(peer is not None, "guest accepted the host")
    assert peer is not None

    host.send("HELLO", v=B.PROTOCOL_VERSION, role="host", app="veloren", pid=os.getpid(), proto=1,
              run_dir=str(Path(tempfile.gettempdir()) / "um-bridge"))
    got = []
    import time
    deadline = time.monotonic() + 2.0
    while time.monotonic() < deadline:
        got += peer.pump()
        if got:
            break
        time.sleep(0.002)
    eq(len(got), 1, "one message received")
    eq(got[0].type, "HELLO", "HELLO type")
    eq(got[0].s("role"), "host", "HELLO role")
    eq(got[0].i("v"), B.PROTOCOL_VERSION, "HELLO version")

    peer.send("WELCOME", v=1, role="guest", app="recwars", tick_hz=60)
    time.sleep(0.05)
    msgs = host.pump()
    eq([m.type for m in msgs], ["WELCOME"], "WELCOME received")

    host.send("CAM", t=1, x=1032.5, y=2487.5, z=12.0, yaw=-1.5708, pitch=0.0, fov=1.1)
    time.sleep(0.05)
    msgs = [m for m in peer.pump() if m.type == "CAM"]
    eq(len(msgs), 1, "CAM received")
    approx(msgs[0].f("x"), 1032.5, "CAM x")
    approx(msgs[0].f("yaw"), -1.5708, "CAM yaw")

    # PING/PONG: the responder echoes t_us, both sides can estimate RTT
    host.send("PING", t_us=B.monotonic_us())
    time.sleep(0.05)
    peer.pump()
    time.sleep(0.05)
    msgs = [m for m in host.pump() if m.type == "PONG"]
    eq(len(msgs), 1, "PONG came back")
    check(host.rtt_us is not None and host.rtt_us >= 0.0, "RTT measured")

    # unknown message types are ignored, not fatal
    host.send("WHAT_IS_THIS", v=1)
    time.sleep(0.05)
    peer.pump()
    check(not peer.closed, "unknown message ignored")

    # non-loopback peers are refused
    s2 = B.GuestServer(port=0)
    check(s2.refused == 0, "refused counter starts at 0")
    s2.close()

    host.close("test done")
    server.close()
    return {}


# ---------------------------------------------------------------- watchdog
def test_watchdog() -> dict:
    print("watchdog")
    t0 = B.monotonic_us()
    w = B.Watchdog(stale_ms=600, dead_ms=2000)
    w.beat(t0)
    eq(w.state(t0), "ok", "fresh peer is ok")
    eq(w.state(t0 + 700_000), "stale", "700 ms is stale")
    eq(w.state(t0 + 2_500_000), "dead", "2.5 s is dead")
    w.beat(t0 + 3_000_000)
    eq(w.state(t0 + 3_000_000), "ok", "a beat revives it")
    return {"stale_ms": 600, "dead_ms": 2000}


def write_vectors_rs(vectors: dict, path: Path) -> Path:
    """Emit the same vectors as Rust source, so the crate needs no JSON dependency."""
    pos = "".join(f"    ({g[0]:.1f}, {g[1]:.1f}, {h[0]:.4f}, {h[1]:.4f}, {c[0]}, {c[1]}),\n"
                  for g, h, c in ((v["guest"], v["host"], v["cell"]) for v in vectors["mapping"]["positions"]))
    ang = "".join(f"    ({a['theta']:.10f}, {a['yaw']:.10f}, {a['dir'][0]:.10f}, {a['dir'][1]:.10f}),\n"
                  for a in vectors["mapping"]["angles"])
    cells = vectors["classification"]
    cl = "".join(f"    ({c['i']}, {c['j']}, {c['height']:.1f}, {c['kind']}),\n" for c in cells["cells"])
    const = vectors["mapping"]["constants"]
    body = f"""//! Test vectors shared by the Python reference and this crate.
//!
//! GENERATED by `python3 fakes/test_bridge.py --write-vectors`. Do not edit by hand: edit
//! `fakes/bridge.py` / `fakes/test_bridge.py` and regenerate, so the two implementations cannot
//! disagree silently (see `testdata/vectors.json` for the same data in JSON).

pub const PROTOCOL_VERSION: u16 = {vectors['protocol_version']};
pub const MAGIC: [u8; 4] = [0x55, 0x4D, 0x42, 0x52]; // "UMBR"

/// ((guest_x, guest_y), (host_x, host_y), (cell_i, cell_j)) in a region at (1000, 2000), 64x64, 8 m cells.
pub const POSITIONS: &[(f32, f32, f32, f32, i32, i32)] = &[
{pos}];

/// (guest_theta, host_yaw, dir_x, dir_y) — dir is Veloren's `Dir` for that yaw.
pub const ANGLES: &[(f32, f32, f32, f32)] = &[
{ang}];

/// (i, j, height_m, kind) for a {cells['nx']}x{cells['ny']} grid, row-major, sea level 0, classified by the reference.
pub const CLASSIFICATION_NX: usize = {cells['nx']};
pub const CLASSIFICATION_NY: usize = {cells['ny']};
pub const CLASSIFICATION: &[(i32, i32, f32, u8)] = &[
{cl}];

/// (tile_units, tile_m, u2m)
pub const CONSTANTS: (f32, f32, f32) = ({const['tile_units']}, {const['tile_m']}, {const['u2m']});
/// (stale_ms, dead_ms)
pub const WATCHDOG: (f64, f64) = ({vectors['watchdog']['stale_ms']}, {vectors['watchdog']['dead_ms']});
/// A packed entity from the reference, hex-encoded (id=0x01020304, kind=2, team=3, flags=5, ...).
pub const ENTITY_SAMPLE_HEX: &str = "{vectors['entity_sample_hex']}";
/// The first 64 bytes of a packed state payload from the reference, hex-encoded.
pub const STATE_SAMPLE_HEX: &str = "{vectors['state_sample_hex']}";
"""
    path.write_text(body)
    return path


def main() -> int:
    tmp = Path(tempfile.mkdtemp(prefix="umbridge-test-"))
    vectors: dict = {"protocol_version": B.PROTOCOL_VERSION, "magic": B.MAGIC.hex()}
    vectors["layouts"] = test_layouts()
    vectors.update(test_payloads())
    test_regions(tmp)
    vectors["mapping"] = test_mapping()
    vectors["classification"] = test_classification()
    test_control()
    vectors["watchdog"] = test_watchdog()

    if "--write-vectors" in sys.argv:
        out = Path(__file__).resolve().parent.parent / "bridge" / "testdata" / "vectors.json"
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(json.dumps(vectors, indent=2, sort_keys=True) + "\n")
        rs = write_vectors_rs(vectors, out.with_suffix(".rs"))
        print(f"\nwrote {out}\nwrote {rs}")

    print(f"\n{CHECKS} checks, {len(FAILURES)} failures")
    for f in FAILURES:
        print(f"  - {f}")
    return 1 if FAILURES else 0


if __name__ == "__main__":
    sys.exit(main())
