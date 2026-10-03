"""Generate the reference patches for the two games, then verify they apply.

    python3 patches/make_patches.py

Reads the pinned upstream sources from the recon clones, applies the changes described in
`patches/files/*.rs` and in the change lists below with `difflib`, writes
`patches/veloren-host.patch` and `patches/recwars-guest.patch`, and prints the `git apply --check`
result for each so a failure is visible immediately.

Nothing here touches the upstream clones: it reads them, builds the unified diffs, and (for the check)
uses a temporary copy.
"""

from __future__ import annotations

import difflib
import pathlib
import shutil
import subprocess
import sys

HERE = pathlib.Path(__file__).resolve().parent
FILES = HERE / "files"
RECON = pathlib.Path("/home/user/recon")
VELOREN = RECON / "veloren"
RECWARS = RECON / "rec-wars"
OUT = HERE

# ------------------------------------------------------------------ helpers
def unified(rel_path: str, old: str, new: str, context: int = 3) -> str:
    a = old.splitlines(keepends=True)
    b = new.splitlines(keepends=True)
    diff = difflib.unified_diff(a, b, fromfile=f"a/{rel_path}", tofile=f"b/{rel_path}", n=context)
    body = "".join(diff)
    if not body:
        raise SystemExit(f"no changes for {rel_path}")
    return f"diff --git a/{rel_path} b/{rel_path}\n" + body


def new_file(rel_path: str, content: str) -> str:
    lines = content.splitlines(keepends=True)
    header = (
        f"diff --git a/{rel_path} b/{rel_path}\n"
        "new file mode 100644\n"
        f"index 0000000..0000000\n"
        f"--- /dev/null\n"
        f"+++ b/{rel_path}\n"
        f"@@ -0,0 +1,{len(lines)} @@\n"
    )
    return header + "".join("+" + line for line in lines)


def read(path: pathlib.Path) -> str:
    return path.read_text()


def replace_once(text: str, old: str, new: str, what: str) -> str:
    if text.count(old) != 1:
        raise SystemExit(f"anchor for {what} matched {text.count(old)} times, expected exactly 1")
    return text.replace(old, new)


def git(cwd: pathlib.Path, *args: str) -> subprocess.CompletedProcess:
    return subprocess.run(["git", *args], cwd=str(cwd), capture_output=True, text=True)


# ------------------------------------------------------------------ Veloren (host)
def veloren_patch() -> tuple[str, list[str]]:
    files: list[str] = []

    # 1. voxygen's dependency on the bridge crate
    rel = "voxygen/Cargo.toml"
    src = read(VELOREN / rel)
    patched = replace_once(
        src,
        '[dependencies]\nclient = { package = "veloren-client", path = "../client" }',
        '[dependencies]\n'
        "# The Veloren x RecWars passthrough bridge. Copy `um-bridge/` from the universal-modder example\n"
        "# (examples/veloren-recwars-passthrough/bridge) into this repo's root; it has no dependencies of\n"
        "# its own, so it links into the client without touching the dependency graph.\n"
        'um-bridge = { path = "../um-bridge" }\n'
        'client = { package = "veloren-client", path = "../client" }',
        "voxygen/Cargo.toml dependencies",
    )
    files.append(unified(rel, src, patched))

    # 2. the module
    rel = "voxygen/src/lib.rs"
    src = read(VELOREN / rel)
    patched = replace_once(
        src,
        "pub mod audio;\npub mod cli;",
        "pub mod audio;\npub mod bridge;\npub mod cli;",
        "voxygen/src/lib.rs module list",
    )
    files.append(unified(rel, src, patched))

    # 3. the module itself
    files.append(new_file("voxygen/src/bridge/mod.rs", read(FILES / "veloren_bridge.rs")))

    # 4. one hook in the session tick, right after the camera dependents are computed
    rel = "voxygen/src/session/mod.rs"
    src = read(VELOREN / rel)
    patched = replace_once(
        src,
        """            let camera::Dependents {
                cam_pos, cam_dir, ..
            } = self.scene.camera().dependents();
            let focus_pos = self.scene.camera().get_focus_pos();
            let focus_off = focus_pos.map(|e| e.trunc());
            let cam_pos = cam_pos + focus_off;
""",
        """            let camera::Dependents {
                cam_pos, cam_dir, ..
            } = self.scene.camera().dependents();
            let focus_pos = self.scene.camera().get_focus_pos();
            let focus_off = focus_pos.map(|e| e.trunc());
            let cam_pos = cam_pos + focus_off;

            // universal-modder bridge (Veloren x RecWars passthrough). Publishes this camera and the
            // terrain around the player to the guest, reads the guest's simulation and frames back.
            // A no-op unless UM_BRIDGE=1: see voxygen/src/bridge/mod.rs. The drawing half of the
            // merge goes in `Scene::render`'s first pass (voxygen/src/scene/mod.rs:1552) and is
            // documented in the example's docs/ARCHITECTURE.md.
            crate::bridge::maintain(
                &client,
                self.scene.camera(),
                global_state.window.renderer_mut().resolution(),
            );
""",
        "voxygen/src/session/mod.rs camera hook",
    )
    files.append(unified(rel, src, patched))

    return header_veloren() + "".join(files), [
        "voxygen/Cargo.toml",
        "voxygen/src/lib.rs",
        "voxygen/src/bridge/mod.rs (new)",
        "voxygen/src/session/mod.rs",
    ]


# ------------------------------------------------------------------ RecWars (guest)
def recwars_patch() -> tuple[str, list[str]]:
    files: list[str] = []

    # 1. the dependency, compiled out on wasm (the browser build must not open sockets or files)
    rel = "Cargo.toml"
    src = read(RECWARS / rel)
    patched = replace_once(
        src,
        "[dev-dependencies]\nwalkdir = \"2.5.0\"",
        "# The universal-modder bridge (Veloren x RecWars passthrough). Copy `um-bridge/` from the\n"
        "# example (examples/veloren-recwars-passthrough/bridge) next to this Cargo.toml. It is only\n"
        "# built for native targets: the wasm build has no filesystem, no sockets and no business\n"
        "# opening either.\n"
        "[target.'cfg(not(target_arch = \"wasm32\"))'.dependencies]\n"
        'um-bridge = { path = "um-bridge" }\n'
        "\n"
        "[dev-dependencies]\nwalkdir = \"2.5.0\"",
        "Cargo.toml dev-dependencies",
    )
    files.append(unified(rel, src, patched))

    # 2. the module declaration
    rel = "src/main.rs"
    src = read(RECWARS / rel)
    step1 = replace_once(
        src,
        "pub mod assets;\npub mod client;",
        "pub mod assets;\n#[cfg(not(target_arch = \"wasm32\"))]\npub mod bridge;\npub mod client;",
        "src/main.rs module list",
    )
    # 3. `--bridge` on the command line (in addition to RCW_BRIDGE=1)
    step2 = replace_once(
        step1,
        """    match endpoint {
        // LATER None should launch client and offer choice in menu
        None => {""",
        """    // universal-modder bridge: enable the guest side (see src/bridge.rs). Also settable with
    // RCW_BRIDGE=1, which is how the host's wrapper script usually does it.
    #[cfg(not(target_arch = "wasm32"))]
    {
        let bridge_flag = env::args().any(|a| a == "--bridge");
        bridge::ENABLED.store(
            bridge_flag || env::var("RCW_BRIDGE").is_ok_and(|v| v != "0" && !v.is_empty()),
            std::sync::atomic::Ordering::Relaxed,
        );
    }

    match endpoint {
        // LATER None should launch client and offer choice in menu
        None => {""",
        "src/main.rs flag handling",
    )
    # 4. start the bridge next to the client, and tick it in the loop
    step3 = replace_once(
        step2,
        """    let mut client = Client::new(&cvars, assets, map, gs, conn, player1_handle, None);

    loop {""",
        """    let mut client = Client::new(&cvars, assets, map, gs, conn, player1_handle, None);

    // universal-modder bridge: only in the local/client endpoint (a dedicated server has no window
    // and nothing to publish). `None` when disabled or when the bridge could not start, in which
    // case the match below is skipped and the game behaves exactly as before.
    #[cfg(not(target_arch = "wasm32"))]
    let mut bridge = if bridge::enabled() {
        bridge::GuestBridge::start()
    } else {
        None
    };

    loop {""",
        "src/main.rs bridge creation",
    )
    step4 = replace_once(
        step3,
        """        client.post_render(&cvars);

        let before = get_time();""",
        """        client.post_render(&cvars);

        // universal-modder bridge: publish this frame's simulation and framebuffer, read the host's
        // terrain and input. Does nothing when the bridge is off. See src/bridge.rs.
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(bridge) = bridge.as_mut() {
            bridge.tick(&mut client);
        }

        let before = get_time();""",
        "src/main.rs loop hook",
    )
    files.append(unified(rel, src, step4))

    # 5. a public constructor so the bridge can build a Map from the host's heightfield
    rel = "src/map.rs"
    src = read(RECWARS / rel)
    patched = replace_once(
        src,
        """impl Map {
    /// The path is only used as an identifier.
    fn new(tiles: Vec<Vec<Tile>>, surfaces: Vec<Surface>, path: &str) -> Self {""",
        """impl Map {
    /// Build a map from another game's terrain (the universal-modder bridge, src/bridge.rs).
    ///
    /// Spawn points and bases come from `SurfaceKind::Spawn` / `SurfaceKind::Base` tiles, exactly like
    /// the game's own maps, so the caller marks the host's spawnable cells with those surfaces.
    pub fn from_bridge(tiles: Vec<Vec<Tile>>, surfaces: Vec<Surface>, path: String) -> Self {
        Self::new(tiles, surfaces, &path)
    }

    /// The path is only used as an identifier.
    fn new(tiles: Vec<Vec<Tile>>, surfaces: Vec<Surface>, path: &str) -> Self {""",
        "src/map.rs constructor",
    )
    files.append(unified(rel, src, patched))

    # 6. the module itself
    files.append(new_file("src/bridge.rs", read(FILES / "recwars_bridge.rs")))

    return header_recwars() + "".join(files), [
        "Cargo.toml",
        "src/main.rs",
        "src/map.rs",
        "src/bridge.rs (new)",
    ]


def header_veloren() -> str:
    return """# Veloren (host side) — the RecWars passthrough bridge
#
# Reference implementation for examples/veloren-recwars-passthrough in universal-modder.
# Pinned upstream: veloren/veloren @ 585a91b4a76fcf5df7a4851127cf5907a3ce34df (2026-09-30, GPL-3.0)
#
#   git clone https://github.com/veloren/veloren.git
#   cd veloren && git checkout 585a91b4a76fcf5df7a4851127cf5907a3ce34df
#   cp -r <universal-modder>/examples/veloren-recwars-passthrough/bridge ./um-bridge
#   git apply <this patch>
#   UM_BRIDGE=1 cargo run --release -p veloren-voxygen
#
# Verified with `git apply --check` against the pinned commit (see patches/README.md). NOT compiled:
# the machine that wrote this had no Rust toolchain and no GPU. Expect to fix small compile errors and
# to add the drawing half (see the comment in voxygen/src/session/mod.rs and docs/ARCHITECTURE.md).
"""


def header_recwars() -> str:
    return """# RecWars (guest side) — the Veloren passthrough bridge
#
# Reference implementation for examples/veloren-recwars-passthrough in universal-modder.
# Pinned upstream: martin-t/rec-wars @ 201690250a9a04ea9b9eb36dbec3bd84c211b0cd (2024-12-28, AGPL-3.0)
#
#   git clone https://github.com/martin-t/rec-wars.git
#   cd rec-wars && git checkout 201690250a9a04ea9b9eb36dbec3bd84c211b0cd
#   cp -r <universal-modder>/examples/veloren-recwars-passthrough/bridge ./um-bridge
#   git apply <this patch>
#   ./target/release/rec-wars local --bridge     (or RCW_BRIDGE=1 cargo run --release -- local)
#
# Verified with `git apply --check` against the pinned commit (see patches/README.md). NOT compiled:
# the machine that wrote this had no Rust toolchain. Expect to fix small compile errors.
"""


def check(patch_path: pathlib.Path, repo: pathlib.Path) -> str:
    result = git(repo, "apply", "--check", str(patch_path))
    if result.returncode == 0:
        return "applies cleanly"
    return f"FAILED: {result.stderr.strip()}"


def main() -> int:
    if not VELOREN.exists() or not RECWARS.exists():
        print("recon clones are missing:", VELOREN, RECWARS, file=sys.stderr)
        return 2
    veloren, veloren_files = veloren_patch()
    recwars, recwars_files = recwars_patch()
    vpath = OUT / "veloren-host.patch"
    rpath = OUT / "recwars-guest.patch"
    vpath.write_text(veloren)
    rpath.write_text(recwars)
    for path, files in ((vpath, veloren_files), (rpath, recwars_files)):
        print(f"wrote {path} ({len(path.read_text().splitlines())} lines, touching {', '.join(files)})")
    print(f"veloren: {check(vpath, VELOREN)}")
    print(f"recwars: {check(rpath, RECWARS)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
