# Merge demo — results

Ran `fakes/run_demo.py` for 23.02 s in this sandbox (no GPU, no Rust toolchain: both games are stand-ins, the bridge is real).

## Oracles

| Check | Result | Detail |
|---|---|---|
| guest committed state | PASS | state_reads=158 |
| guest committed frames | PASS | frame_reads=158 |
| no torn region reads | PASS | torn=0 |
| guest events reached the host | PASS | events_applied=496 |
| guest entities drawn in the host world | PASS | sprites_drawn_max=11 |
| entities visible on most frames | PASS | frames_with_sprites=138 |
| guest sprites land where the docs say | PASS | worst=1.349 px off by, 0 drawn-but-offscreen, over 81 sprites in 24 frames (limit 3.0 px) |
| terrain flowed host -> guest | PASS | terrain_revision=12, guest_revision=12 |
| guest rebuilt its map from host terrain | PASS | guest rev=12 |
| guest frames are fresh enough to look live | PASS | frame_age_avg=32.92400632911391 ms |
| rtt measured | PASS | rtt_us=63910.0 |

## Bridge numbers

- guest state commits read by the host: **158**
- guest frames read: **158**, average age **32.9 ms** (max 68.331 ms)
- torn/corrupt region reads: **0**
- guest events applied to the host world: **496**
- guest entities drawn in the host's world: up to **11** per frame
- sprite positions re-derived from the docs: worst **1.349 px** over 81 sprites
- terrain exports (region writes): **12**; guest map rebuilds: **12**
- round-trip control latency: **63910.0** µs
- host frames rendered: **158**

## Artifacts

- `merge_frame.png` — guest view (left) beside the host's merged view (right)
- `host_view.png` — the host's view alone
- `frames/merge.gif` — the merge running
- `host.log`, `guest.log`, `host_stats.json`, `guest_stats.json` — raw runs

**All oracles passed.**
