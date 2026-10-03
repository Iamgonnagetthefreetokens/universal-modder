"""run_demo — one command that runs the whole merge in the sandbox and checks its oracles.

    python3 fakes/run_demo.py                 # ~25 s, writes fakes/out/
    python3 fakes/run_demo.py --duration 40 --focus   # longer run, then hand input to the guest

Starts the fake guest (RecWars' role) and the fake host (Veloren's role), lets them exchange a camera,
a terrain export, state at 30 Hz and frames at 15 Hz, renders the host's view of the merge, and then
asserts the things that must be true if the bridge works:

  * the guest committed state and frames, and the host parsed them without a torn read;
  * the host applied guest events (explosions) to its own world;
  * guest entities were actually drawn in the host's world (sprites_drawn > 0);
  * the two games agree on where things are (mapping error below a threshold);
  * the terrain flowed host -> guest and the guest rebuilt its map from it.

Exit code 0 = all oracles passed. `fakes/out/summary.md` records the numbers.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import signal
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import bridge as B  # noqa: E402

try:
    import numpy as np
except ImportError:  # pragma: no cover
    np = None

try:
    from PIL import Image, ImageDraw
except ImportError:  # pragma: no cover
    Image = ImageDraw = None

THRESHOLDS = {
    "min_state_reads": 50,
    "min_frame_reads": 40,
    "min_events_applied": 1,
    "min_sprites_drawn": 1,
    "min_frames_with_sprites": 5,
    "max_projection_err_px": 3.0,
    "min_projected_sprites": 20,
    "min_terrain_revision": 2,
    "max_torn": 0,
}


def projection_deviation(samples: list[dict]) -> tuple[float | None, int, int, int]:
    """Independently re-derive where each guest sprite should have been drawn.

    Everything here is written from the documentation, not from fake_host.py: the guest->host mapping
    from docs/MAPPING.md §3 (`host_x = origin_x + gx*U2M`, `host_y = origin_y_max - gy*U2M`, with
    origin_y_max = origin_y + ny*cell_m) and the camera from docs/ARCHITECTURE.md §3 (Veloren yaw:
    forward = (-sin yaw, cos yaw), right = (cos yaw, sin yaw), pitch rotating forward towards up,
    pinhole projection with tan(fovx/2) = aspect * tan(fov/2)).

    The host records, for a handful of frames, the camera pose, the grid in force and the pixel the
    renderer actually used for every sprite it drew. If the host's mapping or camera convention drifts
    from the docs, the two disagree by tens to hundreds of pixels; if they agree, by truncation only.
    """
    worst = None
    total = 0
    frames = 0
    # sprites the renderer drew but the documented mapping says are not even in view: that is a mapping
    # or camera-convention mismatch, not a rounding difference, and it is reported as such
    offscreen = 0
    for sample in samples:
        cam_x, cam_y, cam_z, yaw, pitch, fov = sample["cam"]
        rw, rh = sample["viewport"]
        ox, oy, cell_m, nx, ny = sample["mapping"]
        origin_y_max = oy + ny * cell_m
        fwd = (-math.sin(yaw), math.cos(yaw), 0.0)
        right = (math.cos(yaw), math.sin(yaw), 0.0)
        cp, sp = math.cos(pitch), math.sin(pitch)
        f = (fwd[0] * cp, fwd[1] * cp, fwd[2] * cp + sp)
        u = (-fwd[0] * sp, -fwd[1] * sp, -fwd[2] * sp + cp)
        tanx = math.tan(math.atan(math.tan(fov / 2) * (rw / rh)))
        tany = math.tan(fov / 2)
        for sprite in sample["sprites"]:
            hx = ox + sprite["gx"] * B.U2M
            hy = origin_y_max - sprite["gy"] * B.U2M
            d = (hx - cam_x, hy - cam_y, sprite["zw"] - cam_z)
            zc = d[0] * f[0] + d[1] * f[1] + d[2] * f[2]
            if zc <= 0.4:
                offscreen += 1
                continue
            ux = (d[0] * right[0] + d[1] * right[1]) / (zc * tanx)
            uy = (d[0] * u[0] + d[1] * u[1] + d[2] * u[2]) / (zc * tany)
            if abs(ux) > 1.4 or abs(uy) > 1.4:
                offscreen += 1
                continue
            want_x = (ux * 0.5 + 0.5) * rw
            want_y = (0.5 - uy * 0.5) * rh
            err = math.hypot(want_x - sprite["px"], want_y - sprite["py"])
            worst = err if worst is None else max(worst, err)
            total += 1
        frames += 1
    return (round(worst, 3) if worst is not None else None), total, frames, offscreen


def run(args: argparse.Namespace) -> int:
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    run_dir = out / "run"
    if run_dir.exists():
        for p in run_dir.glob("*.region"):
            p.unlink()
    frames_dir = out / "frames"
    frames_dir.mkdir(exist_ok=True)

    port = args.port
    guest_cmd = [sys.executable, str(HERE / "fake_guest.py"), "--run-dir", str(run_dir),
                 "--port", str(port), "--duration", str(args.duration + 6),
                 "--stats-out", str(out / "guest_stats.json")]
    host_cmd = [sys.executable, str(HERE / "fake_host.py"), "--run-dir", str(run_dir), "--out",
                str(frames_dir), "--port", str(port), "--duration", str(args.duration),
                "--fps", str(args.fps), "--save-every", str(args.save_every),
                "--stats-out", str(out / "host_stats.json")]
    if args.gif:
        host_cmd.append("--gif")
    if not args.focus:
        host_cmd.append("--script")
    if args.focus:
        host_cmd += ["--focus", "--keys", "forward"]   # drive the guest tank from Veloren's side
    if args.width:
        host_cmd += ["--width", str(args.width), "--height", str(args.height)]

    print(f"[demo] run dir: {run_dir}")
    print(f"[demo] guest: {' '.join(guest_cmd[1:])}")
    print(f"[demo] host:  {' '.join(host_cmd[1:])}")
    guest_log = open(out / "guest.log", "w")
    host_log = open(out / "host.log", "w")
    guest = subprocess.Popen(guest_cmd, stdout=guest_log, stderr=subprocess.STDOUT, cwd=str(HERE))
    time.sleep(1.0)
    host = subprocess.Popen(host_cmd, stdout=host_log, stderr=subprocess.STDOUT, cwd=str(HERE))
    try:
        rc_host = host.wait(timeout=args.duration + 60)
        rc_guest = guest.wait(timeout=30)
    except subprocess.TimeoutExpired:
        host.send_signal(signal.SIGTERM)
        guest.send_signal(signal.SIGTERM)
        host.wait(timeout=10)
        guest.wait(timeout=10)
        rc_host = rc_guest = -1
    guest_log.close()
    host_log.close()
    print(f"[demo] exit codes: host={rc_host} guest={rc_guest}")

    host_stats = json.loads((out / "host_stats.json").read_text()) if (out / "host_stats.json").exists() else {}
    guest_stats = json.loads((out / "guest_stats.json").read_text()) if (out / "guest_stats.json").exists() else {}

    # ---- side-by-side image: what the guest shows, and what the host shows ----
    composite = None
    if Image is not None and np is not None:
        latest = sorted(frames_dir.glob("host_*.png"))
        guest_png = None
        try:
            r = B.Region(B.frame_path(run_dir), B.REGION_FRAME, 0, writer=False)
            res = r.read()
            if res:
                hdr, px = B.unpack_frame(res.payload)
                arr = np.frombuffer(px, dtype=np.uint8).reshape(hdr.height, hdr.stride // 4, 4)[:, :hdr.width]
                guest_png = Image.fromarray(arr[..., [2, 1, 0]], "RGB")
        except (B.RegionError, OSError):
            pass
        if latest:
            host_img = Image.open(latest[-1]).convert("RGB")
            if guest_png is None:
                composite = host_img
            else:
                gw, gh = guest_png.size
                scale = host_img.height / gh
                guest_scaled = guest_png.resize((int(gw * scale), host_img.height))
                composite = Image.new("RGB", (guest_scaled.width + host_img.width, host_img.height))
                composite.paste(guest_scaled, (0, 0))
                composite.paste(host_img, (guest_scaled.width, 0))
                d = ImageDraw.Draw(composite)
                for x, label in ((6, "RecWars (guest) - the match, as the tank game shows it"),
                                 (guest_scaled.width + 6,
                                  "Veloren (host) - the same match inside the voxel world")):
                    d.rectangle([x - 2, host_img.height - 16, x + 330, host_img.height - 2],
                                fill=(16, 18, 22))
                    d.text((x + 2, host_img.height - 14), label, fill=(230, 235, 240))
                d.text((guest_scaled.width + 6, host_img.height - 30),
                       "holotable = live guest framebuffer; sprites = guest vehicles and shells",
                       fill=(20, 20, 20))
            composite.save(out / "merge_frame.png")
            latest[-1].replace(out / "host_view.png")

    # ---- oracles ----
    checks: list[tuple[str, bool, str]] = []

    def chk(name: str, ok: bool, detail: str) -> None:
        checks.append((name, bool(ok), detail))

    chk("guest committed state", host_stats.get("state_reads", 0) >= THRESHOLDS["min_state_reads"],
        f"state_reads={host_stats.get('state_reads')}")
    chk("guest committed frames", host_stats.get("frame_reads", 0) >= THRESHOLDS["min_frame_reads"],
        f"frame_reads={host_stats.get('frame_reads')}")
    chk("no torn region reads", host_stats.get("torn", 1) <= THRESHOLDS["max_torn"],
        f"torn={host_stats.get('torn')}")
    chk("guest events reached the host", host_stats.get("events_applied", 0) >= THRESHOLDS["min_events_applied"],
        f"events_applied={host_stats.get('events_applied')}")
    chk("guest entities drawn in the host world",
        host_stats.get("sprites_drawn_max", 0) >= THRESHOLDS["min_sprites_drawn"],
        f"sprites_drawn_max={host_stats.get('sprites_drawn_max')}")
    chk("entities visible on most frames",
        host_stats.get("frames_with_sprites", 0) >= THRESHOLDS["min_frames_with_sprites"],
        f"frames_with_sprites={host_stats.get('frames_with_sprites')}")
    px_worst, px_sprites, px_frames, px_offscreen = projection_deviation(
        host_stats.get("projection_samples") or [])
    chk("guest sprites land where the docs say",
        px_offscreen == 0 and px_sprites >= THRESHOLDS["min_projected_sprites"]
        and px_worst is not None and px_worst <= THRESHOLDS["max_projection_err_px"],
        f"worst={px_worst} px off by, {px_offscreen} drawn-but-offscreen, over {px_sprites} sprites in "
        f"{px_frames} frames (limit {THRESHOLDS['max_projection_err_px']} px)")
    chk("terrain flowed host -> guest",
        host_stats.get("terrain_revision", 0) >= THRESHOLDS["min_terrain_revision"],
        f"terrain_revision={host_stats.get('terrain_revision')}, guest_revision={guest_stats.get('revision')}")
    chk("guest rebuilt its map from host terrain",
        guest_stats.get("revision", 0) >= THRESHOLDS["min_terrain_revision"],
        f"guest rev={guest_stats.get('revision')}")
    age = host_stats.get("frame_age_ms_avg")
    chk("guest frames are fresh enough to look live", age is not None and age < 250.0,
        f"frame_age_avg={age} ms")
    chk("rtt measured", host_stats.get("rtt_us") is not None, f"rtt_us={host_stats.get('rtt_us')}")

    # ---- summary ----
    passed = all(ok for _, ok, _ in checks)
    lines = [
        "# Merge demo — results", "",
        f"Ran `fakes/run_demo.py` for {host_stats.get('duration_s', '?')} s in this sandbox "
        f"(no GPU, no Rust toolchain: both games are stand-ins, the bridge is real).", "",
        "## Oracles", "",
        "| Check | Result | Detail |", "|---|---|---|",
    ]
    for name, ok, detail in checks:
        lines.append(f"| {name} | {'PASS' if ok else 'FAIL'} | {detail} |")
    lines += [
        "", "## Bridge numbers", "",
        f"- guest state commits read by the host: **{host_stats.get('state_reads')}**",
        f"- guest frames read: **{host_stats.get('frame_reads')}**, average age "
        f"**{age:.1f} ms** (max {host_stats.get('frame_age_ms_max')} ms)",
        f"- torn/corrupt region reads: **{host_stats.get('torn')}**",
        f"- guest events applied to the host world: **{host_stats.get('events_applied')}**",
        f"- guest entities drawn in the host's world: up to "
        f"**{host_stats.get('sprites_drawn_max')}** per frame",
        f"- sprite positions re-derived from the docs: worst **{px_worst} px** over {px_sprites} sprites",
        f"- terrain exports (region writes): **{host_stats.get('terrain_revision')}**; guest map "
        f"rebuilds: **{guest_stats.get('revision')}**",
        f"- round-trip control latency: **{host_stats.get('rtt_us')}** µs",
        f"- host frames rendered: **{host_stats.get('frames_rendered')}**", "",
        "## Artifacts", "",
        "- `merge_frame.png` — guest view (left) beside the host's merged view (right)",
        "- `host_view.png` — the host's view alone",
        "- `frames/merge.gif` — the merge running",
        "- `host.log`, `guest.log`, `host_stats.json`, `guest_stats.json` — raw runs",
        "", f"**{'All oracles passed.' if passed else 'SOME ORACLES FAILED.'}**", "",
    ]
    (out / "summary.md").write_text("\n".join(lines))
    print(f"[demo] wrote {out / 'summary.md'}")
    for name, ok, detail in checks:
        print(f"[demo]   {'PASS' if ok else 'FAIL'}  {name}: {detail}")
    if composite is not None:
        print(f"[demo] wrote {out / 'merge_frame.png'} ({composite.size[0]}x{composite.size[1]})")
    return 0 if passed else 1


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__,
                                formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--out", default=str(HERE / "out"))
    p.add_argument("--port", type=int, default=B.DEFAULT_PORT)
    p.add_argument("--duration", type=float, default=25.0)
    p.add_argument("--fps", type=float, default=10.0)
    p.add_argument("--width", type=int, default=640)
    p.add_argument("--height", type=int, default=360)
    p.add_argument("--save-every", type=int, default=8)
    p.add_argument("--gif", action="store_true", default=True)
    p.add_argument("--no-gif", dest="gif", action="store_false")
    p.add_argument("--focus", action="store_true",
                   help="give the guest input focus and drive it (adds --keys)")
    return run(p.parse_args())


if __name__ == "__main__":
    sys.exit(main())
