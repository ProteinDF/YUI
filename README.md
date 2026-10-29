# YUI — Next-Generation Large-Scale Molecular Orbital & Structure Visualization Platform

[![CI](https://github.com/ProteinDF/YUI/actions/workflows/ci.yml/badge.svg)](https://github.com/ProteinDF/YUI/actions/workflows/ci.yml)

*[日本語版](README.ja.md)*

"結 (YUI)" is a data-driven, multi-modal platform for high-speed visualization, control, and sharing — in both the web browser and native environments — of the computational results and molecular structures produced by ProteinDF/QCLObot, massively parallel large-scale quantum chemistry computation software for proteins. See [`architecture.md`](./architecture.md) for the detailed design, [`ROADMAP.md`](./ROADMAP.md) for the phased implementation plan, and [`TESTING.md`](./TESTING.md) for a hands-on guide to trying what's built so far.

---

## 🎯 Core Philosophy & Targets
- **Data-Oriented MVI (Model-View-Intent):** All state is consolidated into `AppState` and driven by a unidirectional data flow. `iced`'s Elm Architecture maps directly onto this.
- **Single Source of Truth (SSOT):** Intermediate storage and communication use fast **MessagePack**; human-facing import/export uses **YAML** (with relative path resolution).
- **Fearless Concurrency:** Massively parallel parsing across CPU cores (Rayon/Tokio) combined with GPU-driven rendering (WebGPU/Compute Shader + Indirect Draw) renders complexes of up to roughly **1 million atoms** at practical frame rates. While the QM computation target itself is on the order of thousands to tens of thousands of atoms, the primary use case is to visualize it positioned within a much larger structural context, such as a cryo-EM complex.
- **Visualization research support for canonical molecular orbital calculations:** Features an MO browser, per-orbital population/contribution mapping, convergence/fidelity comparison across QCLObot growth steps, and comparison (diff view) across multiple calculations — supporting research into canonical MO calculations by ProteinDF/QCLObot themselves.
- **Collaboration:** Natively supports a "presentation mode" in which the presenter (Host) holds primary control and can delegate operating privileges to viewers.
- **Modeling integration:** Force-field optimization, docking, etc. are not implemented in-house; instead the platform integrates with configurable external tools (OpenBabel/RDKit, etc.), run either locally or via HTTP API.
- **Publication-quality output:** In addition to vector (SVG) export, supports offline high-resolution rendering via SSAO, with path tracing planned for the future.
- **License:** GPLv3.

---

## 🛠 Technology Stack
- **Core Language:** Rust (Latest Stable, Wasm & Native Core)
- **Graphics API:** `wgpu` (WebGPU / Vulkan / Metal / DX12 backend)
- **GUI Engine:** `iced` (Elm Architecture, VSCode-like docking UI via `pane_grid`)
- **Structure Data I/O:** `proteindf-bridge` (external crate; a 1:1 Rust port of `ProteinDF_bridge` (Python/GPLv3), also providing C/C++/Python bindings)
- **Serialization:** `serde`, `rmp-serde` (MessagePack), `serde_yaml_ng`
- **Concurrency:** `tokio` (Async I/O / Streaming), `rayon` (Parallel Parsing)
- **Scripting & AI:** `PyO3` (native Python) / `Pyodide` (Wasm Python), Web Speech API + AI Chat Assistant (pluggable LLM providers: Claude / Gemini / local via Ollama)
- **Transport:** WebSocket (initial implementation) → transport abstraction allowing future migration to WebTransport (QUIC)

---

## 🔢 Versioning

Following the sister projects ProteinDF/QCLObot/ProteinDF_bridge, this project uses **CalVer (`YYYY.MM.RELEASE`)**. For example, `2026.9.0` means "the first release of September 2026". Additional releases within the same month increment the release number: `2026.9.1`, `2026.9.2`, and so on. This is not SemVer (`MAJOR.MINOR.PATCH`), so check the release notes for breaking changes.

Each crate's version is inherited from `[workspace.package] version` in the root `Cargo.toml` via `version.workspace = true`.

---

## 🖱 Controls

The same list is in the app under **Help → Mouse & Keyboard...**.

**3D Viewport (mouse)**
- Left drag: orbit around the center
- Shift + left drag: pan
- Ctrl (Cmd on macOS) + left drag: zoom (drag down to zoom out); mouse wheel also zooms
- Left click on an atom: select only that atom
- Shift + click: add the atom to the selection
- Ctrl (Cmd on macOS) + click: toggle the atom in the selection
- Double-click an atom: select its whole residue
- Click empty space: clear the selection (with Shift or Ctrl, the selection is kept)
- Right click: context menu

**Keyboard**
- Space: play / pause
- Left / Right arrow: previous / next frame
- Esc: clear the selection
- Ctrl+S (Cmd+S on macOS): save the open text file

**Terminal**
- Click the screen: type into the terminal (cyan border); click elsewhere to stop
- Ctrl+C: interrupt the running program
- Ctrl+Shift+C / V (Cmd+C / V on macOS): copy / paste; Shift+Insert also pastes
- Mouse wheel, Shift+PageUp / PageDown: scroll the history

---

## 📂 Project Layout
```text
yui/
├── Cargo.toml              # Workspace configuration (centrally manages [workspace.package] version = "2026.10.10", etc.)
├── core/                   # [Shared] AppState definitions, serialization, proteindf-bridge integration
│   └── src/
├── frontend/               # [Wasm/Browser] iced + wgpu frontend
│   └── src/
├── backend/                # [Server] Axum-based REST/WebSocket API server
│   └── src/
└── renderer-native/        # [Native/CLI] Headless wgpu for high-resolution/vector/offline output
    └── src/

# External dependency (managed in a separate project/repository)
proteindf-bridge/                 # Rust-based structure I/O & modeling preprocessing foundation (with C/C++/Python bindings)
```

---

## 🏃 Build & Run

All checks and tests are run locally with `scripts/check.sh --trunk` (install trunk once with `cargo install trunk --locked`); this runs the same checks as the GitHub Actions CI, which runs on every pull request and on pushes to `main` / `develop`. To contribute, see [CONTRIBUTING.md](CONTRIBUTING.md).

Per-user settings (e.g. the editor font) are in `settings.yaml` in your OS config folder (`~/.config/yui/` on Linux); use **File → Open Settings File** in the app to create and edit it. Settings are read at startup, so restart YUI after editing. See `architecture.md` §8.6.

```bash
# Run the entire workspace test suite
cargo test

# Run as a standalone desktop app (no backend required)
cargo run --bin yui-frontend

# Headless renderer (verify loading a YAML state file)
cargo run --bin yui-renderer-native -- path/to/state.yaml
```

### Running `backend` together with `frontend` (collaboration mode)

`yui-backend` is a REST/WebSocket API server only — it has no route at `/`, so opening
`http://127.0.0.1:8080/` directly in a browser returns 404 by design. It must be paired with
`frontend` (native or web) to be useful:

```bash
# 1. Start the API server (listens on http://127.0.0.1:8080)
cargo run --bin yui-backend

# 2a. Connect the native desktop app to it
cargo run --bin yui-frontend -- --collab ws://127.0.0.1:8080/api/collab
```

### Web build (browser, via Trunk)

```bash
# One-time setup
rustup target add wasm32-unknown-unknown
cargo install trunk

cd frontend
trunk serve --port 8085 index.html
```

Open `http://127.0.0.1:8085/` in a browser. The page connects to the `/api/collab` of the server that
served it, and `trunk serve` forwards `/api/` to `backend` (`frontend/Trunk.toml`), so with `backend`
running (port 8080) it is connected, and without it the page works standalone. A different backend can still be
given with `?collab=ws://host:port` in the URL. **Do not** open `frontend/dist/index.html` directly via `file://` — the ES module
and `.wasm` fetch are blocked by CORS and the page stays blank.

Rendering requires a browser with WebGPU **backed by real GPU hardware acceleration**; on a
software/fallback WebGPU adapter (e.g. a VM, remote desktop, or headless environment) the app
now shows a diagnostic error overlay instead of a silent blank page.

#### Serving the web build (release)

```bash
scripts/build-web.sh                 # release build -> target/web/dist and target/web/yui-web-<version>.zip
YUI_WEB_ROOT=target/web/dist cargo run --release -p yui-backend
```

`backend` then serves the page and the API from one process: open the address it prints, `http://127.0.0.1:8080/#token=...`
(the token is a random secret made at each start; the page keeps it in `sessionStorage` and clears it from the address
bar; no `?collab=` needed, no second server). The zip holds the same files (`index.html`, `.js`, `.wasm`; about 14 MB, 6 MB zipped) for any static
host. `dist/` also holds a `.gz` (and, if `brotli` is installed when you build, a `.br`) next to each file: `backend` sends the one the browser accepts (the `.wasm` is 13.7 MB, 6.3 MB as `.gz`; the zip has the plain files). For a static host (GitHub Pages and the like; build with `--public-url /sub/path/` if it is not at the root). Without `backend`
the page is standalone (open a workspace with File → Open). A browser with WebGPU on real GPU hardware and a secure
context (`https://` or `localhost`) is required.

**Protect `backend`'s secrets.** Every API needs the random per-start secret (written to `backend.json` in
your config folder and printed with the web address), requests from other web sites are refused, and it binds to
`127.0.0.1` unless `YUI_BIND` says otherwise (which also needs `YUI_BEHIND_TLS_PROXY=1`, and keeps the terminal and
file APIs closed unless `YUI_ENABLE_TERMINAL=1` / `YUI_ENABLE_FS=1`). Share a session with `scripts/yui-invite.py create viewer`
(an expiring, revocable Viewer or Editor secret that cannot open the terminal or change the state); anyone you give the
*Host* secret to can open the terminal (arbitrary commands) and read files under the workspace root. See
`architecture.md` §7.4.

### Building on Windows (build and launch checked)

The native app builds with the MSVC toolchain. Building the app and starting it were checked on Windows. The other functions (plugins, the installer, the terminal) have not been checked there yet, so please report any problem you meet.

1. Install [Rust](https://rustup.rs) with the default MSVC toolchain, and the "Desktop development with C++" workload of the Visual Studio Build Tools.
2. Install Git (the `proteindf-bridge` dependency is fetched from GitHub).
3. In PowerShell:
   ```powershell
   git clone https://github.com/ProteinDF/YUI.git
   cd YUI
   git checkout develop
   cargo run --bin yui-frontend
   ```
4. The GPU needs Vulkan or DirectX 12; a current graphics driver is enough.
5. The per-user settings and the plugins are in `%APPDATA%\yui\` (`settings.yaml`, `plugins\`).
6. For the setup `.exe`: `cargo install cargo-packager --locked`, then `cargo packager --release -f nsis`. The result is in `target\packager\`. `scripts/build-installers.sh` is a bash script, so from PowerShell run the `cargo packager` command directly.

### Installers (native)

```bash
scripts/build-installers.sh          # this machine's OS: Linux .deb + .AppImage; macOS .dmg; Windows NSIS setup .exe
```

Built with [cargo-packager](https://github.com/crabnebula-dev/cargo-packager) (`cargo install cargo-packager --locked`; the
settings are `[package.metadata.packager]` in `frontend/Cargo.toml`); the results are in `target/packager/`. An
installer can only be built on its own OS, so for all three use the GitHub workflow **Release installers**
(`.github/workflows/release-installers.yml`; manual only, because building all of them takes long): "Run workflow" in the Actions tab, or
`gh workflow run release-installers.yml --ref <tag>`. The installers come out as workflow artifacts.

**Building locally (macOS).** Install cargo-packager once, then build the dmg on the Mac:
1. `cargo install cargo-packager --locked` (the first install takes a while). `cargo packager` is not a cargo built-in: without this step the command reports "no such command: `packager`".
2. In the repository root, on `develop` and up to date: `git checkout develop && git pull`.
3. `scripts/build-installers.sh dmg` (or `cargo packager --release -f dmg`). The `.dmg` is in `target/packager/`.
4. To check the app icon, open the `.dmg`, drag YUI to Applications, and look at the Dock and Finder. If the icon is missing, `Contents/Resources/` of `YUI.app` should contain `icon.icns`.

The packages contain `yui-frontend` (the app, "YUI" in the menus) and the optional `yui-backend`.

**Installing.** The installers are **not signed or notarized**, so the first launch is warned about:
- *macOS:* open the `.dmg`, drag YUI to Applications, then right-click the app → Open (or
  `xattr -dr com.apple.quarantine /Applications/YUI.app`).
- *Windows:* SmartScreen says "Windows protected your PC" → More info → Run anyway.
- *Linux:* `sudo apt install ./yui-frontend_<version>_amd64.deb` (it needs a Vulkan or OpenGL driver), or
  `chmod +x yui-frontend_<version>_x86_64.AppImage && ./yui-frontend_<version>_x86_64.AppImage`.

### Overriding `proteindf-bridge` with a local checkout

By default, this repository fetches `proteindf-bridge` via Git release tag (`tag = "2026.10.20"`) from GitHub. If you are developing `proteindf-bridge` concurrently on your local machine, you can override the dependency using Cargo's `[patch]` feature.

Add the following configuration to your user-wide Cargo config (e.g., `~/.cargo/config.toml`), or pass it via `--config`:

```toml
[patch."https://github.com/ProteinDF/ProteinDF_bridge.git"]
proteindf-bridge = { path = "/absolute/path/to/ProteinDF_bridge/rust/crates/proteindf-bridge" }
```

> **Note:** To maintain repository portability, do not commit local filesystem paths to the repository's `.cargo/config.toml` or `Cargo.toml`. Always use external user configurations or temporary `--config` flags.

See [`TESTING.md`](./TESTING.md) for the full hands-on walkthrough (collaboration delta-sync,
Log Viewer, Terminal, etc.).
