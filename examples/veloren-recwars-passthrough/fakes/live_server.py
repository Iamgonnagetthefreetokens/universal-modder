"""live_server — the whole merge, live in your browser, from the sandbox.

    python3 fakes/live_server.py --port 8090

Runs the fake guest and the fake host in one process (real bridge: TCP control channel + region
files), renders both sides, and serves them:

    /                both views side by side, with a control pad
    /stream/guest    MJPEG of what the tank game shows
    /stream/host     MJPEG of the host world with the merge in it
    /stats           JSON of both sides' counters
    /cmd?k=&d=       inject a guest action (what FOCUS + IN do on the wire)

No GPU and no Rust: the stand-in renderers are numpy, the bridge is the real thing. Use it to see the
merge without installing either game.
"""

from __future__ import annotations

import argparse
import io
import json
import math
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlparse

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import bridge as B  # noqa: E402
import fake_guest  # noqa: E402
import fake_host  # noqa: E402

try:
    import numpy as np
except ImportError:
    np = None

try:
    from PIL import Image, ImageDraw
except ImportError:
    Image = ImageDraw = None


class LiveMerge:
    """Both roles in one process, talking over the real bridge."""

    def __init__(self, port: int, run_dir: Path, width: int, height: int, bots: int = 5,
                 terrain_period: float = 2.0):
        self.run_dir = run_dir
        self.port = port
        # the same 2 s cadence the Rust host and fake_host use (docs/ARCHITECTURE.md, frame budget)
        self.terrain_period = terrain_period
        self.width, self.height = width, height
        self.guest = fake_guest.GuestWorld(seed=11, bots=bots)
        self.guest.frame_size = (480, 270)
        self.host = fake_host.FakeHost(run_dir, port, width, height, render_scale=2, seed=5)
        self.lock = threading.Lock()
        self.guest_jpeg: bytes | None = None
        self.host_jpeg: bytes | None = None
        self.guest_frame_no = 0
        self.host_frame_no = 0
        self.threads: list[threading.Thread] = []
        self.stop = threading.Event()
        self.errors: list[str] = []
        self.keys = {"forward": False, "back": False, "left": False, "right": False, "fire": False}
        self.focus = False
        self.started = time.monotonic()
        self.stats = {"state_reads": 0, "frame_reads": 0, "events_applied": 0, "cam": 0, "map": 0,
                      "torn": 0, "sprites_max": 0}

    # ---------------- guest ----------------
    def run_guest(self) -> None:
        try:
            server = B.GuestServer(self.port)
        except OSError as e:
            self.errors.append(f"guest could not listen on {self.port}: {e}")
            return
        state_region = B.Region(B.state_path(self.run_dir), B.REGION_STATE,
                                32 + B.MAX_ENTITIES * 40 + B.MAX_EVENTS * 24, writer=True)
        frame_region = B.Region(B.frame_path(self.run_dir), B.REGION_FRAME,
                                B.FRAME_HDR.size + self.guest.frame_size[0]
                                * self.guest.frame_size[1] * 4, writer=True)
        peer = None
        watchdog = B.Watchdog()
        terrain_reader = None
        last_state = last_frame = 0.0
        dt = 1.0 / 30.0
        print(f"[live] guest listening on 127.0.0.1:{self.port} run_dir={self.run_dir}")
        while not self.stop.is_set():
            now = time.monotonic()
            if peer is None:
                peer = server.accept()
                if peer is not None:
                    print("[live] host connected")
            if peer is not None:
                for msg in peer.pump():
                    watchdog.beat()
                    if msg.type == "HELLO":
                        peer.send("WELCOME", v=B.PROTOCOL_VERSION, role="guest", app="recwars",
                                  pid=0, tick_hz=30, frame=f"{self.guest.frame_size[0]}x"
                                  f"{self.guest.frame_size[1]}")
                    elif msg.type == "CAM":
                        self.stats["cam"] += 1
                        if self.guest.terrain is not None:
                            self.guest.focus = B.clamp_to_region(
                                *B.host_to_guest(msg.f("x"), msg.f("y"), self.guest.terrain),
                                self.guest.terrain)
                    elif msg.type == "MAP":
                        try:
                            if terrain_reader is None:
                                terrain_reader = B.Region(B.terrain_path(self.run_dir),
                                                          B.REGION_TERRAIN, 0, writer=False)
                            r = terrain_reader.read()
                            if r is not None:
                                hdr, cells = B.unpack_terrain(r.payload)
                                with self.lock:
                                    self.guest.apply_terrain(hdr, cells)
                        except (B.RegionError, OSError) as e:
                            self.errors.append(f"terrain: {e}")
                    elif msg.type == "FOCUS":
                        self.guest.focus_on = bool(msg.i("on"))
                        if not self.guest.focus_on:
                            self.guest.focus_input.clear()
                    elif msg.type == "IN":
                        self.guest.focus_input[msg.s("a")] = bool(msg.i("d"))
                    elif msg.type == "MOUSE":
                        self.guest.mouse[0] += msg.f("dx")
                        self.guest.mouse[1] += msg.f("dy")
                if peer.closed or watchdog.state() == "dead":
                    peer.close("watchdog")
                    peer = None
                    self.guest.focus_on = False
                    self.guest.focus_input.clear()
            if self.guest.map:
                with self.lock:
                    self.guest.step(dt)
                    events = len(self.guest.events)
                if now - last_state >= 1.0 / 30.0:
                    last_state = now
                    blob = B.pack_state(self.guest.state_header(), self.guest.entities(),
                                        self.guest.events)
                    seq = state_region.write(blob)
                    with self.lock:
                        self.guest.events = []
                    if peer is not None and not peer.closed:
                        peer.send("NOTIFY", kind="state", seq=seq, tick=self.guest.tick)
                    self.stats["state_commits"] = self.stats.get("state_commits", 0) + 1
                if now - last_frame >= 1.0 / 12.0:
                    last_frame = now
                    pixels = self.guest.render_frame()
                    frame_region.write(B.pack_frame(*self.guest.frame_size, pixels,
                                                    game_tick=self.guest.tick, scale_permille=300))
                    jpeg = self.encode_guest(pixels)
                    with self.lock:
                        self.guest_jpeg = jpeg
                        self.guest_frame_no += 1
            time.sleep(0.002)
        if peer is not None:
            peer.close("live shutdown")
        state_region.close()
        frame_region.close()
        server.close()

    def encode_guest(self, bgra: bytes) -> bytes:
        g = self.guest
        arr = np.frombuffer(bgra, dtype=np.uint8).reshape(g.frame_size[1], g.frame_size[0], 4)
        im = Image.fromarray(np.ascontiguousarray(arr[..., [2, 1, 0]]), "RGB")
        buf = io.BytesIO()
        im.save(buf, "JPEG", quality=72)
        return buf.getvalue()

    # ---------------- host ----------------
    def run_host(self) -> None:
        time.sleep(0.6)
        try:
            self.host.connect()
        except B.ControlError as e:
            self.errors.append(str(e))
            return
        self.host.export_terrain()
        self.host.send_cam()
        last_cam = last_terrain = last_render = last_ping = 0.0
        last_ent_snapshot = (0.0, {})
        dt = 1.0 / 20.0
        print("[live] host connected to the guest; rendering")
        while not self.stop.is_set():
            now = time.monotonic()
            if self.host.peer:
                for msg in self.host.peer.pump():
                    self.host.watchdog.beat()
                    if msg.type == "PONG":
                        self.stats["rtt_us"] = self.host.peer.rtt_us
            self.host.keys = dict(self.keys) if not self.focus else {}
            self.host.walk(dt)
            if self.host.peer and not self.host.peer.closed and self.focus:
                self.host.face_action()
                self.host.keys = dict(self.keys)
                self.host.walk(dt)
            if now - last_cam > 1.0 / 20.0:
                last_cam = now
                self.host.send_cam()
            if now - last_ping > 1.0 and self.host.peer and not self.host.peer.closed:
                last_ping = now
                self.host.peer.send("PING", t_us=B.monotonic_us())
            if now - last_terrain > self.terrain_period:
                last_terrain = now
                # same cadence as the fake host and the Rust host: re-centre on the player every period
                self.host.export_terrain()
            before = self.host.stats["state_reads"]
            self.host.read_guest()
            self.stats["state_reads"] = self.host.stats["state_reads"]
            self.stats["frame_reads"] = self.host.stats["frame_reads"]
            self.stats["events_applied"] = self.host.events_applied
            self.stats["map"] = self.host.stats["map"]
            self.stats["cam"] = self.host.stats["cam"]
            self.stats["torn"] = self.host.stats["torn"]
            self.host.update_entities(dt)
            if now - last_render > 1.0 / 12.0:
                last_render = now
                image = self.host.render()
                composed = self.host.compose(image)
                self.stats["sprites_max"] = max(self.stats.get("sprites_max", 0),
                                                getattr(self.host, "sprites_drawn", 0))
                im = Image.fromarray(composed, "RGB")
                buf = io.BytesIO()
                im.save(buf, "JPEG", quality=78)
                with self.lock:
                    self.host_jpeg = buf.getvalue()
                    self.host_frame_no += 1
            time.sleep(0.004)
        if self.host.peer:
            self.host.peer.close("live shutdown")
        if self.host.terrain_writer:
            self.host.terrain_writer.close()

    # ---------------- control ----------------
    def start(self) -> None:
        self.threads = [threading.Thread(target=self.run_guest, name="guest", daemon=True),
                        threading.Thread(target=self.run_host, name="host", daemon=True)]
        for t in self.threads:
            t.start()

    def set_key(self, k: str, down: bool) -> None:
        self.keys[k] = down
        if self.focus:
            self.host.forward_input("back" if k == "back" else k, down)

    def set_focus(self, on: bool) -> None:
        self.focus = on
        self.host.set_focus(on)
        if not on:
            o = self.keys
            for k in list(o):
                self.host.forward_input(k, False)

    def snapshot(self) -> dict:
        with self.lock:
            return {
                "focus": self.focus,
                "keys": {k: v for k, v in self.keys.items() if v},
                "guest": {"tick": self.guest.tick, "revision": self.guest.revision,
                          "vehicles": len(self.guest.vehicles),
                          "focus_on": self.guest.focus_on},
                "host": dict(self.stats),
                "frames": {"guest": self.guest_frame_no, "host": self.host_frame_no},
                "uptime_s": round(time.monotonic() - self.started, 1),
                "errors": self.errors[-5:],
            }


PAGE = """<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Veloren x RecWars - live merge</title>
<style>
  :root { color-scheme: dark; }
  body { margin: 0; font: 14px/1.4 ui-monospace, SFMono-Regular, Menlo, monospace;
         background: #0b0d10; color: #e8edf2; }
  header { padding: 10px 14px; border-bottom: 1px solid #23272e; display: flex; gap: 16px;
           align-items: center; flex-wrap: wrap; }
  h1 { font-size: 15px; margin: 0; font-weight: 600; letter-spacing: .2px; }
  .hint { color: #8b98a5; }
  main { display: grid; grid-template-columns: 1fr 1fr; gap: 1px; background: #23272e; }
  figure { margin: 0; background: #0b0d10; }
  figure img { display: block; width: 100%; height: auto; image-rendering: pixelated; }
  figcaption { padding: 6px 10px; color: #9fb0c0; font-size: 12px; border-top: 1px solid #23272e; }
  .pad { padding: 12px 14px; display: flex; gap: 8px; flex-wrap: wrap; align-items: center; }
  button { background: #171b21; color: #e8edf2; border: 1px solid #2c333c; border-radius: 6px;
           padding: 8px 12px; font: inherit; cursor: pointer; }
  button:active, button.on { background: #b6ff3b; color: #0b0d10; border-color: #b6ff3b; }
  #stats { padding: 8px 14px 24px; color: #8b98a5; white-space: pre-wrap; }
  @media (max-width: 900px) { main { grid-template-columns: 1fr; } }
</style></head>
<body>
<header>
  <h1>Veloren &times; RecWars &mdash; passthrough merge (live)</h1>
  <span class="hint">left: the tank game &middot; right: inside the voxel world &middot;
    the panel is the tank game's real framebuffer, the sprites are its vehicles</span>
</header>
<main>
  <figure><img src="/stream/guest" alt="guest"><figcaption>RecWars (guest stand-in)</figcaption></figure>
  <figure><img src="/stream/host" alt="host"><figcaption>Veloren (host stand-in) with the merge</figcaption></figure>
</main>
<div class="pad">
  <button id="focus">F6 &mdash; give input focus to the guest</button>
  <button data-k="forward">forward</button>
  <button data-k="back">back</button>
  <button data-k="left">left</button>
  <button data-k="right">right</button>
  <button data-k="fire">fire</button>
  <span class="hint">&larr;/&rarr; turn the player, up/down walk (when the host has focus)</span>
</div>
<div id="stats">loading…</div>
<script>
const keys = ['forward','back','left','right'];
const state = {};
function paint() {
  document.querySelectorAll('button[data-k]').forEach(b => b.classList.toggle('on', !!state[b.dataset.k]));
  document.getElementById('focus').classList.toggle('on', !!state.focus);
}
function send(url) { fetch(url).catch(() => {}); }
async function refresh() {
  try {
    const s = await (await fetch('/stats')).json();
    state.focus = s.focus;
    keys.forEach(k => state[k] = !!s.keys[k]);
    paint();
    document.getElementById('stats').textContent =
      `focus: ${s.focus ? 'guest' : 'host'}   uptime: ${s.uptime_s}s\\n` +
      `guest: tick ${s.guest.tick}, map revision ${s.guest.revision}, ${s.guest.vehicles} vehicles\\n` +
      `host: ${s.host.state_reads} state reads, ${s.host.frame_reads} frames, ` +
      `${s.host.events_applied} guest events applied, up to ${s.host.sprites_max} guest sprites per frame\\n` +
      `control: ${s.host.cam} CAM, ${s.host.map} terrain exports, rtt ${s.host.rtt_us ? s.host.rtt_us.toFixed(0) : '?'} us` +
      (s.errors.length ? `\\nerrors: ${s.errors.join(' | ')}` : '');
  } catch (e) { /* server busy: try again */ }
}
document.querySelectorAll('button[data-k]').forEach(b => {
  const k = b.dataset.k;
  const on = e => { e.preventDefault(); state[k] = true; paint(); send(`/cmd?k=${k}&d=1`); };
  const off = e => { e.preventDefault(); state[k] = false; paint(); send(`/cmd?k=${k}&d=0`); };
  b.addEventListener('mousedown', on); b.addEventListener('mouseup', off);
  b.addEventListener('mouseleave', off); b.addEventListener('touchstart', on, {passive:false});
  b.addEventListener('touchend', off);
});
document.getElementById('focus').addEventListener('click', () => {
  const on = !state.focus; state.focus = on; paint();
  send(`/focus?on=${on ? 1 : 0}`);
});
const KEYMAP = {ArrowUp:'forward', ArrowDown:'back', ArrowLeft:'left', ArrowRight:'right', ' ':'fire'};
addEventListener('keydown', e => {
  const k = KEYMAP[e.key]; if (!k || state[k]) return; e.preventDefault();
  state[k] = true; paint(); send(`/cmd?k=${k}&d=1`);
});
addEventListener('keyup', e => {
  const k = KEYMAP[e.key]; if (!k) return; state[k] = false; paint(); send(`/cmd?k=${k}&d=0`);
});
setInterval(refresh, 700); refresh();
</script>
</body></html>
"""


class Handler(BaseHTTPRequestHandler):
    merge: LiveMerge
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt, *args):    # keep the console for bridge events
        pass

    def _send(self, code: int, ctype: str, body: bytes, extra: dict | None = None) -> None:
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        for k, v in (extra or {}).items():
            self.send_header(k, v)
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)

    def do_HEAD(self):   # noqa: N802
        self.do_GET()

    def do_GET(self):    # noqa: N802
        u = urlparse(self.path)
        q = parse_qs(u.query)
        if u.path in ("/", "/index.html"):
            self._send(200, "text/html; charset=utf-8", PAGE.encode())
        elif u.path == "/stats":
            self._send(200, "application/json", (json.dumps(self.merge.snapshot()) + "\n").encode())
        elif u.path == "/cmd":
            k = (q.get("k") or [""])[0]
            d = (q.get("d") or ["1"])[0] not in ("0", "false")
            if k in ("forward", "back", "left", "right", "fire"):
                self.merge.set_key(k, d)
            self._send(200, "application/json", b'{"ok":true}\n')
        elif u.path == "/focus":
            on = (q.get("on") or ["1"])[0] not in ("0", "false")
            self.merge.set_focus(on)
            self._send(200, "application/json", b'{"ok":true}\n')
        elif u.path in ("/stream/guest", "/stream/host"):
            self.stream(u.path.endswith("guest"))
        else:
            self._send(404, "text/plain", b"not found\n")

    def stream(self, guest: bool) -> None:
        self.send_response(200)
        self.send_header("Content-Type", "multipart/x-mixed-replace; boundary=frame")
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        last_no = -1
        try:
            while True:
                with self.merge.lock:
                    jpeg = self.merge.guest_jpeg if guest else self.merge.host_jpeg
                    no = self.merge.guest_frame_no if guest else self.merge.host_frame_no
                if jpeg is not None and no != last_no:
                    last_no = no
                    self.wfile.write(b"--frame\r\nContent-Type: image/jpeg\r\nContent-Length: "
                                     + str(len(jpeg)).encode() + b"\r\n\r\n" + jpeg + b"\r\n")
                    self.wfile.flush()
                time.sleep(0.05)
        except (BrokenPipeError, ConnectionResetError, OSError):
            return


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__,
                                formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--port", type=int, default=8090, help="HTTP port for the preview")
    p.add_argument("--bridge-port", type=int, default=B.DEFAULT_PORT)
    p.add_argument("--run-dir", default=None)
    p.add_argument("--width", type=int, default=640)
    p.add_argument("--height", type=int, default=360)
    p.add_argument("--bots", type=int, default=5)
    p.add_argument("--host", default="0.0.0.0", help="bind address (0.0.0.0 so the preview can reach it)")
    p.add_argument("--terrain-period", type=float, default=2.0,
                   help="seconds between terrain exports (the Rust host uses the same 2 s cadence)")
    args = p.parse_args()
    if np is None or Image is None:
        print("numpy and pillow are required: pip install numpy pillow", file=sys.stderr)
        return 2
    run_dir = B.open_run_dir(args.run_dir or (HERE / "out" / "live_run"))
    merge = LiveMerge(args.bridge_port, run_dir, args.width, args.height, args.bots,
                      terrain_period=args.terrain_period)
    merge.start()
    Handler.merge = merge
    httpd = ThreadingHTTPServer((args.host, args.port), Handler)
    print(f"[live] http://{args.host}:{args.port}/  (both roles in one process)")
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        merge.stop.set()
        time.sleep(0.3)
        httpd.server_close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
