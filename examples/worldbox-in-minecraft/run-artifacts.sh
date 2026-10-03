#!/usr/bin/env bash
# Rebuild the bridge artifacts: start a bridge, let a client render it, stop.
#
#   bash examples/worldbox-in-minecraft/run-artifacts.sh [seconds]
#
# Produces:
#   artifacts/world-y*.png     the whole map, every 700 frames
#   artifacts/zoom-y*-t*.png   one kingdom, every frame
#   artifacts/showcase-world.png, artifacts/showcase-zoom.png
#                              the newest of each, for the README
#   artifacts/mcview-run.txt   the client's log
#
# No Minecraft is needed: the client is `worldforge mcview`, which builds the same
# block world the Fabric mod builds and draws it isometrically, from the same wire
# messages the mod receives.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
WORLDFORGE="${WORLDFORGE:-$HERE/../worldbox-rust-rewrite/worldforge}"
SECONDS_TO_RUN="${1:-45}"
PORT="${PORT:-25631}"
SIZE="${SIZE:-large}"
SEED="${SEED:-20241003}"

if [ ! -x "$WORLDFORGE/target/debug/worldforge" ]; then
	echo "building worldforge (debug)..."
	(cd "$WORLDFORGE" && cargo build --offline --bins)
fi
BIN="$WORLDFORGE/target/debug/worldforge"
mkdir -p "$HERE/artifacts"
rm -f "$HERE"/artifacts/world-y*.png "$HERE"/artifacts/zoom-y*.png

echo "bridge:  $SIZE seed $SEED on 127.0.0.1:$PORT"
"$BIN" serve --size "$SIZE" --seed "$SEED" --civs 4 --animals 40 --tps 60 --port "$PORT" \
	>"$HERE/artifacts/serve.log" 2>&1 &
BRIDGE=$!
trap 'kill "$BRIDGE" 2>/dev/null || true' EXIT
sleep 2

echo "client:  the whole world, with the simulation running"
"$BIN" mcview --connect "127.0.0.1:$PORT" --png "$HERE/artifacts/world" --every 700 --scale 7 \
	--timeout "$SECONDS_TO_RUN" | tee "$HERE/artifacts/mcview-run.txt"

# Find a kingdom to zoom into: the busiest village becomes the centre of the crop.
CROP="$(python3 - "$HERE/artifacts/mcview-run.txt" <<'PY'
import re, sys
best = (0, 58, 40)
for line in open(sys.argv[1]):
    match = re.search(r"village .*pop\s+(\d+) at tile\s+(\d+),(\d+)", line)
    if match:
        pop, col, row = int(match.group(1)), int(match.group(2)), int(match.group(3))
        if pop > best[0]:
            best = (pop, col, row)
print(f"{max(0, best[1] - 13)},{max(0, best[2] - 9)},26,18")
PY
)"
echo "client:  one kingdom, close up (region $CROP)"
"$BIN" mcview --connect "127.0.0.1:$PORT" --png "$HERE/artifacts/zoom" --every 20 --scale 16 \
	--region "$CROP" --timeout 4 | tee -a "$HERE/artifacts/mcview-run.txt"

kill "$BRIDGE" 2>/dev/null || true
wait "$BRIDGE" 2>/dev/null || true
trap - EXIT
tail -2 "$HERE/artifacts/serve.log"

# Stable names for the README, and strip the PNGs: worldforge's own writer stores
# uncompressed data (zero dependencies), which is 20x bigger than it needs to be.
LATEST_WORLD="$(ls -1 "$HERE"/artifacts/world-y*.png | tail -1)"
LATEST_ZOOM="$(ls -1 "$HERE"/artifacts/zoom-y*.png | tail -1)"
cp "$LATEST_WORLD" "$HERE/artifacts/showcase-world.png"
cp "$LATEST_ZOOM" "$HERE/artifacts/showcase-zoom.png"
python3 - "$HERE/artifacts/showcase-world.png" "$HERE/artifacts/showcase-zoom.png" <<'PY' || \
	echo "note: python3 with zlib not available, the showcase PNGs are left as written"
import os, struct, sys, zlib

def read_png(path):
    data = open(path, "rb").read()
    assert data[:8] == b"\x89PNG\r\n\x1a\n"
    i, chunks = 8, []
    while i < len(data):
        length = struct.unpack(">I", data[i:i + 4])[0]
        kind = data[i + 4:i + 8]
        chunks.append((kind, data[i + 8:i + 8 + length]))
        i += 12 + length
    width, height, depth, color, _, _, interlace = struct.unpack(
        ">IIBBBBB", next(d for k, d in chunks if k == b"IHDR"))
    assert depth == 8 and color == 2 and interlace == 0, (depth, color, interlace)
    raw = zlib.decompress(b"".join(d for k, d in chunks if k == b"IDAT"))
    stride, out, prev, pos = width * 3, bytearray(), bytearray(width * 3), 0
    for _ in range(height):
        filter_kind = raw[pos]
        pos += 1
        line = bytearray(raw[pos:pos + stride])
        pos += stride
        if filter_kind == 1:
            for x in range(3, stride):
                line[x] = (line[x] + line[x - 3]) & 255
        elif filter_kind == 2:
            for x in range(stride):
                line[x] = (line[x] + prev[x]) & 255
        elif filter_kind == 3:
            for x in range(stride):
                a = line[x - 3] if x >= 3 else 0
                line[x] = (line[x] + ((a + prev[x]) >> 1)) & 255
        elif filter_kind == 4:
            for x in range(stride):
                a = line[x - 3] if x >= 3 else 0
                b = prev[x]
                c = prev[x - 3] if x >= 3 else 0
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pr = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                line[x] = (line[x] + pr) & 255
        elif filter_kind != 0:
            raise SystemExit(f"unexpected filter {filter_kind}")
        out += b"\x00" + bytes(line)
        prev = line
    return width, height, bytes(out)

def write_png(path, width, height, raw):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)
    ihdr = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    body = zlib.compress(raw, 9)
    open(path, "wb").write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr)
                           + chunk(b"IDAT", body) + chunk(b"IEND", b""))

for path in sys.argv[1:]:
    width, height, raw = read_png(path)
    before = os.path.getsize(path)
    write_png(path, width, height, raw)
    print(f"{os.path.basename(path)}: {width}x{height}, {before // 1024} -> {os.path.getsize(path) // 1024} KiB")
PY
ls -la "$HERE"/artifacts/showcase-*.png
