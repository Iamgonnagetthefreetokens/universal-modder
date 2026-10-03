#!/usr/bin/env python3
"""Watch a worldforge bridge without Minecraft, and poke it.

This is the same client the mod is, in miniature: it speaks the JSON-lines
protocol on 127.0.0.1, keeps the tile field and the units in memory, and prints a
one-line dashboard per frame. It exists so the bridge can be tested -- and driven
-- before Minecraft is involved at all.

    python3 host/mcview.py [--port 25607] [--every 10]
    # then type commands at it:
    #   power nuke            cast at a random land tile
    #   power lightning 40 30 cast at tile 40,30
    #   spawn Orc soldier 40 30
    #   pause / resume / step 20 / speed 30 / tiles / quit

Everything it receives is checked against the protocol's own invariants (tiles
match the map size, unit ids are unique, frames move forwards), so a bug in the
bridge shows up here as a complaint rather than a mystery in-game.
"""

from __future__ import annotations

import argparse
import json
import random
import socket
import sys
import threading
import time

PROTOCOL = 1


class Bridge:
    def __init__(self, port: int) -> None:
        self.sock = socket.create_connection(("127.0.0.1", port), timeout=10)
        self.sock.settimeout(None)
        self.rfile = self.sock.makefile("rb")
        self.write_lock = threading.Lock()
        self.size = (0, 0)
        self.palette: list[str] = []
        self.tiles: list[list] = []
        self.units: dict[int, list] = {}
        self.frames = 0
        self.blocks = 0
        self.edits = 0
        self.problems: list[str] = []

    def send(self, message: dict) -> None:
        line = (json.dumps(message) + "\n").encode()
        with self.write_lock:
            self.sock.sendall(line)

    def problem(self, text: str) -> None:
        self.problems.append(text)
        print(f"  !! {text}", file=sys.stderr)

    def read(self) -> dict:
        line = self.rfile.readline()
        if not line:
            raise EOFError("the bridge closed the connection")
        return json.loads(line)

    def apply(self, message: dict) -> None:
        kind = message.get("t")
        if kind == "hello":
            if message.get("protocol") != PROTOCOL:
                self.problem(f"protocol {message.get('protocol')} (this client speaks {PROTOCOL})")
            self.size = tuple(message.get("size", (0, 0)))
            self.palette = message.get("palette", [])
            print(
                f"hello: {self.size[0]}x{self.size[1]} `{message.get('world')}` "
                f"seed {message.get('seed')}, {len(self.palette)} names, tag `{message.get('tag')}`"
            )
        elif kind == "tiles":
            surface = message["surface"]
            heights = message["h"]
            tops = message["top"]
            water = message["water"]
            extra = message["extra"]
            if len(surface) != self.size[0] * self.size[1]:
                self.problem(
                    f"tile field has {len(surface)} columns, the map has {self.size[0] * self.size[1]}"
                )
            if not (len(surface) == len(heights) == len(tops) == len(water) == len(extra)):
                self.problem("the tile arrays are different lengths")
            self.tiles = [
                [surface[i], heights[i], tops[i], water[i], extra[i]] for i in range(len(surface))
            ]
            print(f"tiles: {len(self.tiles)} columns")
        elif kind == "edits":
            flat = message["tiles"]
            if len(flat) % 2:
                self.problem("an edits message has an odd number of values")
            for i in range(0, len(flat) - 1, 2):
                index, tile = flat[i], flat[i + 1]
                if not 0 <= index < len(self.tiles):
                    self.problem(f"edit for tile {index}, outside the map")
                    continue
                self.tiles[index] = tile
                self.edits += 1
        elif kind == "structures":
            self.blocks += len(message["ops"])
        elif kind == "frame":
            units = {u[0]: u for u in message.get("units", [])}
            if len(units) != len(message.get("units", [])):
                self.problem("two units share an id")
            self.units = units
            self.frames += 1
            self.report(message)
        elif kind == "pong":
            print(f"  pong (tick {message.get('tick')})")
        else:
            self.problem(f"unknown message kind `{kind}`")

    def report(self, message: dict) -> None:
        villages = message.get("villages", [])
        kingdoms = message.get("kingdoms", [])
        if len(villages) != message.get("village_count", -1):
            self.problem(
                f"frame says {message.get('village_count')} villages, carries {len(villages)}"
            )
        if len(kingdoms) != message.get("kingdom_count", -1):
            self.problem(
                f"frame says {message.get('kingdom_count')} kingdoms, carries {len(kingdoms)}"
            )
        print(
            f"year {message['year']:>4} tick {message['tick']:>6} | pop {message['pop']:>3} | "
            f"villages {len(villages):>2} | kingdoms {len(kingdoms):>2} | "
            f"animals {message['animals']:>2} monsters {message['monsters']:>2} | "
            f"{message['age']} | {message['hash']}",
            flush=True,
        )
        for year, text in message.get("news", [])[:2]:
            print(f"        {year}: {text}")
        for village in villages[:3]:
            print(
                f"        {village[1]:<12} {village[2]:<6} pop {village[3]:>3} "
                f"tile {village[4]},{village[5]} kingdom {village[6]}"
            )

    def random_tile(self) -> tuple[int, int]:
        land = [
            i
            for i, tile in enumerate(self.tiles)
            if tile and tile[0] != 0 and self.palette[tile[0]] != "water"
        ]
        index = random.choice(land) if land else random.randrange(max(1, len(self.tiles)))
        return index % self.size[0], index // self.size[0]


def repl(bridge: Bridge, every: int, frames: int) -> None:
    if frames:
        for i in range(frames):
            bridge.apply(bridge.read())
            if every and not i % every:
                pass
        return
    stop = threading.Event()

    def reader() -> None:
        try:
            while not stop.is_set():
                bridge.apply(bridge.read())
        except (EOFError, OSError) as error:
            print(f"\n{error}")
            stop.set()

    thread = threading.Thread(target=reader, daemon=True)
    thread.start()
    print("type `help` for commands")
    while not stop.is_set():
        try:
            line = input()
        except (EOFError, KeyboardInterrupt):
            break
        parts = line.split()
        if not parts:
            continue
        verb, rest = parts[0], parts[1:]
        try:
            if verb == "quit":
                break
            elif verb == "help":
                print(__doc__.strip().splitlines()[-6:])
            elif verb == "power":
                name = rest[0] if rest else "nuke"
                col, row = (int(rest[1]), int(rest[2])) if len(rest) > 2 else bridge.random_tile()
                bridge.send({"t": "power", "name": name, "col": col, "row": row})
                print(f"  asked for {name} at {col},{row}")
            elif verb == "spawn":
                race = rest[0] if rest else "Orc"
                soldier = len(rest) > 1 and rest[1].lower() in ("soldier", "1", "true")
                col, row = (int(rest[2]), int(rest[3])) if len(rest) > 3 else bridge.random_tile()
                bridge.send({"t": "spawn", "race": race, "soldier": soldier, "col": col, "row": row})
                print(f"  asked for a {race} at {col},{row}")
            elif verb == "pause":
                bridge.send({"t": "pause", "on": True})
            elif verb == "resume":
                bridge.send({"t": "pause", "on": False})
            elif verb == "step":
                bridge.send({"t": "step", "ticks": int(rest[0]) if rest else 1})
            elif verb == "speed":
                bridge.send({"t": "speed", "tps": float(rest[0]) if rest else 10})
            elif verb == "tiles":
                bridge.send({"t": "tiles"})
            else:
                print(f"  ? {verb}")
        except (ValueError, IndexError):
            print(f"  ! bad arguments for `{verb}`")
        if every and bridge.frames and bridge.frames % every == 0:
            pass
    stop.set()
    print(f"\n{bridge.frames} frames, {bridge.edits} tile edits, {bridge.blocks} structure blocks")
    if bridge.problems:
        print(f"{len(bridge.problems)} protocol complaints:")
        for problem in bridge.problems[:10]:
            print(f"  {problem}")
        sys.exit(1)
    print("no protocol complaints")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", type=int, default=25607)
    parser.add_argument("--frames", type=int, default=0, help="read N frames and exit")
    parser.add_argument("--every", type=int, default=0, help="reserved: report every N frames")
    args = parser.parse_args()
    try:
        bridge = Bridge(args.port)
    except OSError as error:
        print(f"cannot reach the bridge on 127.0.0.1:{args.port}: {error}")
        print("start one with:  worldforge serve --size large --seed 20241003")
        return 1
    started = time.time()
    repl(bridge, args.every, args.frames)
    print(f"{time.time() - started:.1f}s")
    return 0


if __name__ == "__main__":
    sys.exit(main())
