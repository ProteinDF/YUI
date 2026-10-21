# YUI — Testing Guide

*[日本語版](TESTING.ja.md)*

This is a practical, hands-on guide to trying out "結 (YUI)" as of release `2026.10.1` (`develop` branch) — not just running the automated test suite (`cargo test --workspace`), but launching the application and verifying each feature interactively. See [`ROADMAP.md`](./ROADMAP.md) for overall architectural progress and [`CHANGELOG.md`](./CHANGELOG.md) for release notes. This document is about *how to manually test what is currently implemented*.

---

## Prerequisites

- The Rust toolchain (this repository targets edition 2024; use a recent stable toolchain).
- To try the web build: the `wasm32-unknown-unknown` target (`rustup target add wasm32-unknown-unknown`), [Trunk](https://trunkrs.dev/) (`cargo install trunk`), and a browser with WebGPU support enabled on physical GPU hardware.
- To try the Terminal feature: nothing extra — it spawns your operating system's normal shell via a PTY session on `backend`.
- Test data: realistic structure fixtures `core/tests/data/1hls.pdb` (single model, 782 atoms) and `core/tests/data/1HLS.cif` (20-model NMR ensemble, 15,640 atoms) are already included in this repository.

---

## 1. Native build (standalone, no backend)

```bash
cargo run --bin yui-frontend
```

**What you can do:**
- **4-Pane Docking Layout:** resizable layout with Left, Center, Right, and Bottom panes by dragging pane borders.
- **Blank startup (Center pane):** nothing is loaded at startup. The Center viewport shows `No structure loaded. Open a file from the Project tab or File > Open.`, and the Representation Inspector has no cards. Open a structure (e.g. `core/tests/data/1hls.pdb`, section 5.1) to try the features below.
- **Default Representations:** when a structure is opened and there are no representations yet, two are added and rendered together: ball-and-stick (`ball_and_stick_default`, Element coloring) and cartoon ribbon (`cartoon_default`, SecondaryStructure coloring). The hint text disappears.
- **Left pane:** tabs for "Explorer / Playbook" (Structure Tree), "Named Selections", and "Project" (Project Tree).
- **Right pane:** Inspector displaying connection status, atom/bond counts, and active Representation cards.
- **Center pane:** a "3D Viewport" tab, plus one tab per open text file (with a `×` button); "View" -> "Split Editor Right" shows the file tabs to the right of the viewport instead.
- **Bottom pane:** tabs for "Log Viewer", "Python Console", "Terminal", and "AI Chat".
- **Camera controls:** rotate camera by left-dragging mouse, zoom with scroll wheel.
- **Level of Detail (LOD):** zooming out far enough switches mid-range atoms from ball-and-stick geometry to billboard impostor spheres, printing `[Culling & LOD] Spheres visible: ...` to the console.
- **Not yet functional:** the Bottom pane's "Python Console" and "AI Chat" tabs are placeholders only.

---

## 2. Native build + backend (collaboration, log viewer, terminal)

Start `backend` first (in a separate terminal):
```bash
cargo run --bin yui-backend
# -> listens on http://127.0.0.1:8080
```
- Environment variable `YUI_LOG_ROOT`: allowed directory for log file streaming (default: `./logs`).
- **Log files:** `backend` also writes its own log (not the streamed logs above) to a daily file `backend.<date>.log`, keeping 7 days: `$XDG_STATE_HOME/yui/logs` (`~/.local/state/yui/logs`) on Linux, `~/Library/Logs/yui` on macOS, `%LOCALAPPDATA%\yui\logs` on Windows; `YUI_LOG_DIR` sets another folder. Level `info`, or `RUST_LOG`. Tokens and URL queries are never written (paths only). If the folder cannot be made, `backend` keeps running with standard output only.
- Environment variable `YUI_WORKSPACE_ROOT`: root directory for workspace file operations (default: `.`).
- **Token:** every API needs a secret. `backend` makes a random one at each start and writes it, with the URL, to `backend.json` in your config folder (`~/.config/yui/backend.json`, readable only by you); the desktop `frontend` reads that file, so the two programs need no setup on one computer. `YUI_TOKEN=<16+ characters>` sets it instead; for a `backend` on another computer start the frontend with `--token <token>` (or `YUI_TOKEN`). A wrong or old token shows as a failed terminal connection and an empty Project tab.
- `YUI_BIND` (default `127.0.0.1:8080`): another address is refused unless `YUI_BEHIND_TLS_PROXY=1` (a reverse proxy terminates TLS); off loopback the terminal and file APIs are closed unless `YUI_ENABLE_TERMINAL=1` / `YUI_ENABLE_FS=1`. `YUI_ALLOWED_ORIGINS` / `YUI_ALLOWED_HOSTS`: extra web origins / host names (comma-separated).

Start `frontend` pointing to it:
```bash
cargo run --bin yui-frontend -- --collab ws://127.0.0.1:8080/api/collab
# or: YUI_COLLAB_URL=ws://127.0.0.1:8080/api/collab cargo run --bin yui-frontend
```

On startup, `frontend` fetches `backend`'s initial state over REST (`GET /api/state`). By default, `backend` holds the 1AON demo structure (21 subunits, 57,960 atoms, ~58,000 atoms) in its flat molecule cache. Connection status is reflected in the title bar (`[Connected: ...]`) and Inspector pane.

**Trying the Log Viewer:**
```bash
mkdir -p logs && echo "hello world" >> logs/job.log
```
1. In the Bottom pane, select the "Log Viewer" tab.
2. Leave the "Path:" field at its default (`job.log`) and click the "Connect" button.
3. The log streams in real-time (`tail -f` style).
4. Run `echo "second log line" >> logs/job.log` from another terminal and verify that the line appears immediately.
5. Test the "Filter:" field (`Search/filter lines...`) to filter log lines.
6. Toggle "Auto Scroll: ON/OFF", test the "Clear" button, and click "Disconnect" to stop.

**Trying the Terminal:**
1. In the Bottom pane, select the "Terminal" tab.
2. (There is no token field: the desktop app reads the token from `backend.json`, see section 2.)
3. Click "Connect (Host)" — this spawns a real PTY shell on the machine running `backend`.
4. Enter commands in the input field (e.g. `ls --color`, `uname -a`, or `echo -e "\e[31mred\e[0m"`) and press Enter to see ANSI colors rendered.
5. Click the terminal screen (its border turns cyan) and type directly: `ls`, Tab for completion, the up arrow for history, Ctrl+C, and a full-screen program such as `vim` or `less README.md` (arrows, `q`, `:q`). Resize the window or the pane and run `stty size`: it shows the new rows and columns, and `ls` or `vim` fills the new width. Click elsewhere (the border turns back) and press Space or Esc: they work as app shortcuts again. Drag over some output with the mouse (the text is highlighted), press Ctrl+Shift+C (Cmd+C on macOS) or the "Copy" button, and paste it into another application; copy text elsewhere and press Ctrl+Shift+V (Cmd+V, Shift+Insert) or the "Paste" button: it is typed at the prompt (Ctrl+C alone still interrupts).  A click without dragging clears the selection. Run `seq 1 200`, then turn the mouse wheel over the screen or press Shift+PageUp: the older lines show and "History: N lines up" appears in the toolbar (the cursor is hidden); Shift+PageDown moves back, and typing anything returns to the live screen. Inside `less` or `vim` the wheel and Shift+PageUp do nothing (those programs have no history).
6. Click "Disconnect" to close the session.
7. (Cross-site check) With the backend running, `curl -s -o /dev/null -w '%{http_code}\n' -H 'Origin: https://evil.example' http://127.0.0.1:8080/api/state` prints `403` (and the backend log shows `Rejected /api/state: OriginNotAllowed`), while the same request without the `Origin` header prints `200`; a `Host: evil.example` request is refused too.
8. (Invitations in the app) With the backend running, open **File → Share Session...**: the list says "none". Choose Editor and "1 day", click "Create invitation": the invitation id, a `http://127.0.0.1:8080/#token=...` address and the buttons "Copy link" / "Copy token" appear, and the invitation is listed with its remaining time. "Copy link" puts the address on the clipboard (paste it into a browser's address bar). "Revoke" removes it from the list. Started with a Viewer's `--token`, "Create invitation" shows the backend's `403 Forbidden: this action requires the Host role`.
9. (Invitations on the command line) `scripts/yui-invite.py create viewer --ttl 120` prints a token, a browser address and a desktop command; `scripts/yui-invite.py list` shows it (no secret); start the frontend with `--token <that token>`: the app works read-only (it cannot open the Terminal, `403`); `scripts/yui-invite.py revoke <id>` and the open connection closes at once (within a second), a new one is refused (`403`). `create editor` works the same; `create host` is refused.
10. (Rate limit) Ten requests with a wrong token (`curl -H 'Authorization: Bearer wrong-token-0123456789' http://127.0.0.1:8080/api/state`) in a minute answer `403`; then every request, even with the right token, answers `429` with `Retry-After` for 60 seconds, and the backend log says `Too many failed authentications`. Requests with no token at all (`401`) are not counted.
11. (Token check) `curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:8080/api/state` prints `401`; with `-H 'Authorization: Bearer wrong-token-0123456789'` it prints `403`; with the token from `backend.json` it prints `200`; the old `?token=1` in a URL no longer works. Starting the frontend with `--token wrong-token-0123456789` makes "Connect" fail and the Project tab stay empty.

---

## 3. Web build (browser)

```bash
cd frontend
trunk serve --port 8085 index.html
```
*(Trunk's default port is 8080, which conflicts with `backend`, so `--port 8085` is required.)*

**Verification status:** the web build compiles with `trunk` (about 1 minute for the first debug build), and the page is served. It has **not** been run in a browser by the developers: on the development machine (Linux, no usable GPU), Chrome reports that no WebGPU hardware adapter was found, and the application stops at its start-up message. The browser exports, dialogs and the backend connection (`?collab=`) are therefore unverified at run time. Check them on a machine with a WebGPU-capable GPU (see section 6).

Open `http://127.0.0.1:8085/#token=<token from backend.json>` in a WebGPU-compatible browser (the page keeps the token in `sessionStorage` and clears it from the address bar) (`trunk serve` forwards `/api/` to the backend on port 8080, see `frontend/Trunk.toml`):
- **Standalone:** starts blank with the same hint text as the native build. Without a backend, the Project tab is unavailable in the browser, so structures can be opened with "File" -> "Open" (workspace YAML) only.
- **Connected to backend:** start the backend (`cargo run -p yui-backend`, port 8080) before or after opening the page; the page connects to the `/api/collab` of the server that served it (through the `trunk serve` proxy), so no URL parameter is needed (`?collab=ws://host:port` still selects another backend). Collaboration delta-sync, Log Viewer, and Terminal all function in the browser using the same code paths.
- **Release bundle served by the backend:** `scripts/build-web.sh`, then `YUI_WEB_ROOT=target/web/dist cargo run --release -p yui-backend`, and open `http://127.0.0.1:8080/`. Expected: the same page as above, already connected to the backend (the Project tab lists the workspace files, no `?collab=`). `curl -sI http://127.0.0.1:8080/<name>_bg.wasm` says `content-type: application/wasm`; an unknown path (`/nothing`) and `/api/nothing` give 404; a wrong `YUI_WEB_ROOT` makes the backend exit with a message. Not checked by the developers in a browser (see the status above); the `curl` checks, the proxy (HTTP and the WebSocket handshake, 101) and the tests were run.
- **Hardware requirement:** physical GPU hardware supporting WebGPU is required (software adapters or headless environments will fail to initialize the wgpu surface).

---

### 3.1 Installers

- [ ] **Linux packages:**
  - **Action:** `scripts/build-installers.sh` (the first release build takes a few minutes), then on a clean Debian / Ubuntu system `sudo apt install ./target/packager/yui-frontend_<version>_amd64.deb`; also `chmod +x` and run the `.AppImage`.
  - **Expected result:** "YUI" appears in the application menu with the icon; it starts (`yui-frontend`), and `yui-backend` is also installed in `/usr/bin`. `sudo apt remove yui` removes it. Verified by the developers only up to: the packages build, `dpkg-deb -I` shows the metadata, and the binary from the deb and the AppImage start on a virtual screen.
- [ ] **macOS and Windows packages (nothing here was verified by the developers):**
  - **Action:** run the GitHub workflow "Release installers" (Actions tab, "Run workflow", or `gh workflow run release-installers.yml --ref <tag>`; it costs Actions minutes), download the artifacts, and install the `.dmg` (macOS) and the NSIS `.exe` (Windows).
  - **Expected result:** the workflow's three jobs succeed; each installer installs YUI with the icon; the first launch shows the unsigned-app warning described in the README, and YUI starts after it is accepted; the Windows installer can uninstall it. Report what fails, with the job log.

---

## 4. Python API (core-py)

The GUI's "Python Console" tab is not yet implemented. To exercise the Python API:

- **Via the test suite (verified):**
  ```bash
  cargo test -p yui-py -- --nocapture
  ```
  `core-py/tests/test_python_binding.rs` contains working examples of the builder API, such as `app.select(...).show(...).color_by(...)`.
- **Using an external Python interpreter (`import yui`):** requires `maturin` (`pip install maturin`), but this repository does not yet provide a `pyproject.toml` and this installation path has not been verified in this project.

---

## 5. Feature verification checklist

Use this checklist to interactively verify the features added up to version `2026.10.1`.

### 5.1 Structure loading via Project Tree (1HLS)

- [ ] **Standalone / Local folder loading:**
  - **Action:** In the Left pane, switch to the "Project" tab. Click the "Open Folder" button on the toolbar (or "フォルダを開く..." when empty). In the file dialog, choose `core/tests/data`. When the tree displays files, click `1hls.pdb`.
  - **Expected result:** Human insulin structure (1 model, 782 atoms) loads into the 3D viewport, centered and fully visible (the camera is fitted to the structure). The status bar at the bottom of the window shows `Loaded structure '1hls.pdb': 782 atoms, 793 bonds` on the left and `782 atoms, 793 bonds` on the right. Secondary structures are automatically calculated (DSSP equivalent) and displayed as cartoons. The "Explorer / Playbook" tab in the Left pane shows the hierarchy (Model → Chain → Residue → Atom).
- [ ] **Loading via backend workspace:**
  - **Action:** Start backend with `YUI_WORKSPACE_ROOT=core/tests/data cargo run --bin yui-backend` and start frontend with `--collab ws://127.0.0.1:8080/api/collab`. In the "Project" tab (`[Backend FS]`), click `1HLS.cif`.
  - **Expected result:** The 20-model NMR ensemble structure `1HLS.cif` (15,640 atoms) loads successfully.
- [ ] **MOL2 structure loading (`sample.mol2`):**
  - **Action:** In the Left pane "Project" tab, click `sample.mol2` inside `core/tests/data`.
  - **Expected result:** Ethanol molecule (9 atoms, 8 explicit bonds) loads into the 3D viewport. The bottom status bar shows `Loaded structure 'sample.mol2': 9 atoms, 8 bonds`. The "Explorer / Playbook" tab displays the normalized 4-tier hierarchy (`model_1` → `A` → `1 ETH` → 9 atoms) and bonds connect atoms according to `@<TRIPOS>BOND`.

### 5.2 Non-covalent interactions & disulfide bonds

- [ ] **Displaying interactions and toggling types:**
  - **Action:** With `1hls.pdb` loaded, in the Right pane (Inspector), click "+ Add" to create a new representation. On the new card, click the "Interactions" button under Kind. Click the "H-bond", "Salt bridge", and "CH-π" buttons to toggle them off, leaving only "Disulfide" active.
  - **Expected result:** Dashed magenta cylinders appear in the 3D viewport. Exactly 3 disulfide bonds (the 3 covalent/disulfide bridges of insulin: A6–A11, A7–B7, A20–B19) are rendered. The summary shows `1 active`.

### 5.3 Multi-model switching (1HLS.cif)

- [ ] **Single model filtering and All models display:**
  - **Action:** Load `core/tests/data/1HLS.cif` (via Project tab or backend). Switch to the "Explorer / Playbook" (Structure Tree) tab in the Left pane. Observe the model selector bar at the top of the tree (`Model: ◀ model_1 (1/20) ▶ All`). Click "▶" to step forward to model 2 (`model_2 (2/20)`), then click "All".
  - **Expected result:** On initial load, only the first model (`model_1 (1/20)`, 782 atoms) is displayed without superimposition clutter. Clicking "▶" smoothly switches the displayed model in 3D (e.g. `model_2 (2/20)`); the camera does not move when the model is switched. Clicking "All" changes the label to `All (20)` and renders all 20 NMR ensemble models superimposed (15,640 atoms, 60 total disulfide bonds).

### 5.4 Molecular surface (Surface)

- [ ] **Surface generation and color scheme changes:**
  - **Action:** With a structure loaded (e.g. `1hls.pdb`), in the Right pane (Inspector), on any representation card (or after clicking "+ Add"), click "Surface" under Kind. Then click each color button under Color: "Element", "Chain", "Residue", "SS", and "Uniform".
  - **Expected result:** A molecular surface mesh is generated in the background using Gaussian density fields and Marching Cubes, and rendered in the 3D viewport. Selecting different color schemes instantly updates the surface vertex coloring. Drag the **Opacity** slider on the Surface card (0–100 %): the surface becomes see-through at once, without being rebuilt (no "generating" indicator), and the atoms behind it show. The slider appears only on Surface and Interaction cards; on an Interaction card it fades the lines.

### 5.5 Structure Tree & context menus

- [ ] **Tree navigation and right-click styling:**
  - **Action:** In the "Explorer / Playbook" tab, click the "▶" expand buttons next to models, chains, and residues. Left-click a residue node. Then right-click a chain or residue node to open the context menu (`Tree Node Menu`). Try clicking "Select / Highlight", "Show As:" ("Spacefill" or "Cartoon"), and "Color By:" ("Chain" or "Residue").
  - **Expected result:** Nodes expand and collapse smoothly. Left-clicking highlights the node. The right-click menu opens at the cursor; selecting representation and color options creates or updates representation cards targeting the selected node's path.

### 5.6 Representation Inspector & context menus

- [ ] **Representation card controls:**
  - **Action:** In the Right pane (Inspector), click "+ Add" to create a representation. Click the eye icon (`👁` / `🚫`) to toggle visibility. Switch between kinds (`Ball&Stick`, `Cartoon`, `Spacefill`, `Licorice`, `Line`, `Surface`, `Interactions`). Right-click the card to test the context menu. Click the "✕" button to delete the representation.
  - **Expected result:** Visibility toggling immediately hides or shows the representation. Switching kinds re-renders the geometry in the 3D viewport. Deleting removes the card and its visual elements.

### 5.7 Named selections

- [ ] **Creating, renaming, and applying selections:**
  - **Action:** Right-click a node in the Structure Tree (or an atom in the 3D viewport) and select "Save as Named Selection". Switch to the "Named Selections" tab in the Left pane. Edit the name in the text field (e.g. rename `selection_1` to `my_helix`) and press Enter. Click the "Apply" button. Finally click "×" to delete it.
  - **Expected result:** The new named selection card appears with a selector summary. Pressing Enter commits the rename. "Apply" highlights the selected atoms. "×" removes the card.

### 5.8 Viewport background color

- [ ] **Background color presets:**
  - **Action:** Click "View" on the top menu bar, look under "Background Color:", and select each preset:
    - `  Dark (Default)`
    - `  Black`
    - `  White`
    - `  Light Gray`
  - **Expected result:** The 3D viewport clear color updates immediately to the selected preset. Structures remain clearly visible against both dark and light backgrounds.

### 5.9 Text editor tabs & file open handler choice

- [ ] **Opening several text files as tabs:**
  - **Action:** In the Left pane "Project" tab, click a plain text file such as `core/tests/data/README.md`, then click another one (or the same one again).
  - **Expected result:** The Center pane shows a tab row `3D Viewport | README.md ×`; the file opens in the selected tab with a monospace font (JetBrains Mono) and the pane title shows the file name. A second file adds a second tab. Clicking the same file again does not add a tab; it selects the existing one. Clicking the "3D Viewport" tab switches back to the molecule (the "3D Viewport" tab has no `×`). The Bottom pane has no "Text Editor" tab. The status bar shows `Opened 'README.md'`.
- [ ] **Editing, unsaved mark, and saving:**
  - **Action:** Type some text in a file's tab. Move the cursor with the arrow keys only in another tab. Press Ctrl+S (Cmd+S on macOS) in the edited tab; also try the toolbar "Save" and "Save As" buttons.
  - **Expected result:** Typing adds `*` before the file name in the tab and the toolbar; only moving the cursor or selecting text does not. Saving removes the `*` and the status bar shows `Saved '<name>'`. Ctrl+S works both while the editor has focus and after clicking elsewhere (e.g. the Left pane) with the file's tab selected. Save As lets you pick a new name, and the tab is renamed.
- [ ] **New and Open Text File from the File menu:**
  - **Action:** Click "File" -> "New Text File" twice, type in one of them, then click "File" -> "Open Text File..." and choose a file.
  - **Expected result:** Tabs `Untitled-1` and `Untitled-2` appear (numbers are not reused after closing); the chosen file opens in its own tab. Saving an untitled file asks for a name.
- [ ] **Closing tabs and the unsaved-changes confirmation:**
  - **Action:** Click `×` on a saved file's tab. Then edit another file and click its `×`; try each of "Cancel", "Don't Save", and (on another edited file) "Save". With "Save" on an untitled file, cancel the save dialog.
  - **Expected result:** A saved file closes immediately and the neighbouring tab is selected (the 3D Viewport after the last file). An edited file shows the dialog `Do you want to save the changes to '<name>'?` with Save / Don't Save / Cancel, and the rest of the window cannot be clicked. Cancel keeps the tab; Don't Save closes it without saving; Save saves, then closes. If the save dialog is cancelled, the tab stays open and unsaved.
- [ ] **Quitting with unsaved files:**
  - **Action:** Edit two text files without saving, then click "File" -> "Quit" (and, separately, the window's close button). Try "Cancel", then quit again and choose "Save All". With one of them an `Untitled-N` file, cancel its save dialog. Finally, with nothing unsaved, quit.
  - **Expected result:** A dialog `2 files have unsaved changes. Save before quitting?` lists the file names (`1 file ...` with a "Save" button for one file) with Save All / Don't Save / Cancel, and the rest of the window cannot be clicked. Cancel keeps everything. Save All saves the files in the order they were opened (an untitled file asks for a name) and quits only after all are saved; cancelling a save dialog cancels quitting and the file stays open and unsaved. Don't Save quits at once. With nothing unsaved, both ways quit without a dialog.
- [ ] **Splitting the editor to the right of the viewport:**
  - **Action:** With a structure (e.g. `1hls.pdb`) and two text files open, one of them edited but not saved, click "View" -> "Split Editor Right". Drag the border between the two halves. Then click "View" -> "Split Editor Right" again. Also try the split with no text file open.
  - **Expected result:** While the item is checked (`✓ Split Editor Right`), the 3D viewport is on the left and the file tabs are on the right, and the border can be dragged to resize them; the Center-left pane has no file tabs. Unchecking it returns the file tabs to the Center tab row. In both directions the open files, their content, the unsaved mark (`*`), and the selected tab are kept. Opening another text file while split shows it in the right half. While split, the right half has a "3D Viewport 2" tab before the file tabs (with no file open, it shows the second viewport). Unchecking the split removes that tab.
- [ ] **Workspaces and open files:**
  - **Action:** With the split on and files open, click "File" -> "Save As" and save `split_test.yaml`. Click "File" -> "New", then "File" -> "Open" and choose `split_test.yaml`. Open the saved `split_test.yaml` in the text editor from the Project tab.
  - **Expected result:** "File" -> "New" turns the split off, but the open files stay open as Center tabs. Opening `split_test.yaml` turns the split on again, with the open files on the right. The YAML contains `center_right` but no file tabs, and open files are not restored after restarting the app. A workspace saved by `2026.10.2` or earlier (with the text editor in the Bottom pane) opens without a "Text Editor" tab.
- [ ] **Monospace font in the editor and terminal:**
  - **Action:** In a text file, type `iiiiiiiiii` and `WWWWWWWWWW` on two lines; open the Terminal tab (with the backend) and run `ls`.
  - **Expected result:** The two lines have the same width in the editor; the terminal also uses the same monospace font, and bold text (e.g. `ls --color` directory names) is bold. In the browser build the terminal is also monospace. Japanese characters are shown (in a different, non-monospace font).
- [ ] **Choosing the open handler for multi-purpose files:**
  - **Action:** In the "Project" tab, click a `.yaml` or `.yml` file.
  - **Expected result:** A top prompt bar appears with `Open '<filename>' with:` offering `[Workspace State]` and `[Text Editor]` buttons. Clicking `[Text Editor]` opens the file in a new tab, while clicking `[Workspace State]` imports the workspace state.
- [ ] **"Open with" by right-click:**
  - **Action:** In the "Project" tab, left-click a `.pdb` file; then right-click the same file; click `Text Editor` in the menu; right-click again and click somewhere outside the menu.
  - **Expected result:** The left click opens the 3D view directly (no prompt). The right click opens a small menu "Open with" right next to the mouse pointer (kept inside the window near its edges) listing `Structure Viewer` and `Text Editor`; `Text Editor` opens the file as text in a tab and closes the menu; a click outside closes the menu and opens nothing. Right-clicking a `.txt` file lists only `Text Editor`. Left-clicking a `.xyz` or `.yaml` file still shows the prompt bar with a Cancel button.

### 5.10 Camera controls & LOD

- [ ] **Interactive orbit, zoom, and reset:**
  - **Action:** Open `core/tests/data/1hls.pdb` (section 5.1). In the Center pane 3D viewport, drag with the left mouse button to orbit the camera. Use the scroll wheel to zoom in and out. Zoom far out to trigger LOD impostor switching. Then click "View" -> "Reset Camera" in the top menu bar.
  - **Expected result:** Smooth camera rotation and zooming. At large distances, mid-range atoms switch to billboard impostors and `[Culling & LOD] Spheres visible: ...` is logged to the console. Reset Camera fits the camera to the structure again (the whole structure centered in the viewport) and the status bar shows `Camera reset to fit the structure.`. Make the Center pane tall and narrow (e.g. hide the Right pane and drag the borders) and click Reset Camera again: the structure still fits horizontally. With nothing loaded (after "File" -> "New"), Reset Camera restores the default camera and shows `Camera position reset to default.`.
- [ ] **Pan and zoom with modifier keys:**
  - **Action:** Open a structure. Drag with the left button while holding Shift. Then drag while holding Ctrl (Cmd on macOS): down, then up. Then drag with no modifier. Zoom in a lot and Shift+drag again. Click an atom while holding Shift (no drag).
  - **Expected result:** Shift+drag moves the molecule in the screen plane and it follows the pointer, also when zoomed in (it does not rotate). Ctrl/Cmd+drag down zooms out and up zooms in, with no rotation. No modifier still rotates. A modifier+click still adds to / toggles the selection and does not move the camera. With two viewports, each one moves independently (or together when linked).

### 5.11 File menu workspace Open / Save / Save As

- [ ] **Exporting, saving, and re-opening workspace with file dialogs:**
  - **Action (Save As):** With any structure loaded and customized (e.g. representations added or background changed), click "File" -> "Save As". In the asynchronous file dialog, filter for YAML (`.yaml` / `.yml`), choose a destination directory, enter `workspace_test.yaml`, and confirm.
  - **Expected result:** The workspace state and pane layout are exported. The status bar shows `Saved workspace to 'workspace_test.yaml'`. On WASM, a browser file download is triggered.
  - **Action (Save overwrite):** Make an additional modification (e.g. toggle representation visibility), then click "File" -> "Save".
  - **Expected result:** On native platforms, the file is overwritten directly at `workspace_test.yaml` without opening another dialog, and the status bar reports `Saved workspace to 'workspace_test.yaml'`. On WASM, it triggers another download.
  - **Action (Open):** Click "File" -> "New" to reset the workspace to an empty state, then click "File" -> "Open". In the file dialog, select `workspace_test.yaml`.
  - **Expected result:** The saved state is fully restored: structure atoms/bonds, active representations, and pane layout are reinstated, interactions and surfaces update accordingly, and status shows `Loaded workspace from 'workspace_test.yaml'`. Cancelling any dialog leaves the current state untouched.

### 5.12 Structure file export (PDB / mmCIF)

- [ ] **Exporting PDB and mmCIF structures and re-opening to verify:**
  - **Action (Exporting PDB):** Load `core/tests/data/1hls.pdb`. In the top menu bar, click "File" -> "Export Structure...". In the save dialog, select the PDB filter (`*.pdb`), specify `exported_1hls.pdb`, and save.
  - **Expected result:** The structure is written to disk. The status bar displays `Exported structure to '.../exported_1hls.pdb'` (or file name on WASM).
  - **Action (Verifying PDB):** In the Left pane "Project" tab, click the newly saved `exported_1hls.pdb` to open it.
  - **Expected result:** The structure loads with 782 atoms and identical chain/residue structure as the original `1hls.pdb`.
  - **Action (Exporting mmCIF):** Load `core/tests/data/1HLS.cif` (all 20 models). Click "File" -> "Export Structure...". Select the mmCIF filter (`*.cif`), specify `exported_1hls.cif`, and save.
  - **Expected result:** All 20 models are exported. The status bar displays `Exported structure to '.../exported_1hls.cif'`.
  - **Action (Verifying mmCIF):** Open `exported_1hls.cif`.
  - **Expected result:** The structure loads with all 20 models (15,640 atoms) intact.
  - **Action (Non-integer charge handling):** Load `core/tests/data/sample.mol2` (which has non-integer partial charges). Click "File" -> "Export Structure..." and choose mmCIF (`.cif`).
  - **Expected result:** An informative error message appears in red in the status bar (prefixed with `Error: `), explaining that structures with partial charges cannot be written as mmCIF formal charges and advising export to PDB instead. (PDB also stores integer charges, so partial charges are rounded.)

### 5.13 Status bar

- [ ] **Messages, structure counts, model, surface generation, and connection:**
  - **Action (Startup):** Start the native frontend without a backend (`cargo run --bin yui-frontend`).
  - **Expected result:** A one-line bar is shown along the bottom of the window, below all panes. The right end shows `Local` (gray). Because nothing is loaded at startup, no atom/bond counts are shown. After opening `1hls.pdb`, `782 atoms, 793 bonds` appears to the left of `Local`.
  - **Action (Info message):** Click "View" -> "Reset Camera".
  - **Expected result:** `Camera position reset to default.` appears on the left in the normal (light gray) text color.
  - **Action (Warning message):** Click "File" -> "New", then "File" -> "Export Structure...".
  - **Expected result:** `No structure loaded to export.` appears in yellow. After "File" -> "New", the atom/bond counts are no longer shown.
  - **Action (Error message):** Load `core/tests/data/sample.mol2`, click "File" -> "Export Structure...", and save as `sample.cif` (mmCIF).
  - **Expected result:** `Error: Structures with non-integer partial charges cannot be exported to mmCIF, ...` appears in red. The error stays until another message replaces it (it is not cleared automatically).
  - **Action (Multi-model):** Load `core/tests/data/1HLS.cif`, then use the model switcher at the top of the "Explorer / Playbook" tab (`◀` / `▶` / `All`).
  - **Expected result:** The right side shows `model_1 (1/20)`, and it follows the switcher (e.g. `model_2 (2/20)`). With `All`, it shows `All models (20)`. For a single-model structure such as `1hls.pdb`, no model label is shown.
  - **Action (Surface generation):** With `1HLS.cif` loaded, add a representation with "+ Add" in the Representation Inspector and set its Kind to `Surface`.
  - **Expected result:** `Generating surface...` appears on the right while the mesh is being generated and disappears when the surface is drawn. (It may only be visible briefly for small structures.)
  - **Action (Connection):** Start the backend and run the frontend with `--collab ws://127.0.0.1:8080/api/collab` (section 2). Then repeat with the backend stopped.
  - **Expected result:** With the backend running, the right end shows `Online` (green). While connecting, it shows `Connecting...` (yellow). If fetching the initial state fails, it shows `Offline: <error>` (red).

### 5.14 User settings file (editor font)

- [ ] **Creating and editing the settings file:**
  - **Action:** Click "File" -> "Open Settings File" (native build only; the item is not shown in the browser build).
  - **Expected result:** `settings.yaml` opens as a text tab with an all-comment template (the file and its folder are created in your OS config folder, e.g. `~/.config/yui/settings.yaml`); the status bar says `Opened 'settings.yaml'. Settings are read at startup: restart YUI after saving.` Clicking the menu item again selects the same tab. An existing file is never overwritten.
- [ ] **Changing the editor font:**
  - **Action:** In that file, enable `editor:` with `font_family: "DejaVu Sans Mono"` (or any monospace font installed on your computer) and `font_size: 20`, save with Ctrl+S, quit and start YUI again, then open a text file and the Terminal tab.
  - **Expected result:** No warning at startup. The text editor shows the new font at 20 px; the terminal uses the same font family at its usual size (it has its own `terminal:` / `font_size:` setting, 6 - 72, default 12; with `font_size: 18` the terminal text is larger and `stty size` shows fewer columns). Removing the keys (or the file) and restarting gives JetBrains Mono at 14 px again.
- [ ] **Changing the drawing sizes:**
  - **Action:** In the settings file enable `render:` with `interaction_radius: 0.2`, `ball_scale: 0.6` and `line_radius: 0.1`, save, quit and start YUI again, open `1hls.pdb` and add an Interaction card; also try a Line card and the PNG / SVG exports.
  - **Expected result:** The balls of Ball & Stick are larger, the interaction lines are clearly thicker (also in the SVG), and the Line card is thicker. Removing the keys and restarting gives the default sizes. `ball_scale: 5` or `stick_radius: thick` gives a status-bar warning for that key only, and the default size for it.
- [ ] **Changing the dashes, the surface and the CUBE opacity:**
  - **Action:** In `render:` set `interaction_dash: 0.1`, `interaction_gap: 0.4`, `surface_grid_spacing: 1.0`, `surface_probe_radius: 1.4`; restart; open `1hls.pdb`, add an Interaction card and a Surface card.
  - **Expected result:** The interaction lines are short dashes with long gaps (`interaction_gap: 0` gives solid lines). The surface is coarser and smoother-looking (1.0 Å grid) and larger (1.4 Å probe), and builds faster. Removing the keys gives the defaults again.
- [ ] **Changing the bond color, the cartoon and the selection glow:**
  - **Action:** In `render:` set `bond_color: [0.9, 0.2, 0.2]`, `ribbon_helix_width: 4.5`, `ribbon_loop_radius: 0.6` and `selection_size: 2.0`; restart; open `1hls.pdb` (cartoon + Ball & Stick) and click an atom.
  - **Expected result:** The sticks are red, the helices are wide flat ribbons and the loops are thick tubes, and the glow around the selected atom is twice as large (with `selection_highlight: halo` the ring is twice as wide). Removing the keys gives the defaults again. `bond_color: [1, 2, 0]` or `bond_color: red` gives a warning for that key only.
- [ ] **Ambient occlusion (SSAO):**
  - **Action:** In `render:` set `ssao_strength: 0.8` (and try `ssao_radius: 6`); restart; open `1hls.pdb` and show Spacefill or Surface; rotate the view; then File → Export Image (PNG)...
  - **Expected result:** The gaps between atoms and the recesses between the surface lobes are darker, which gives more depth; flat or convex areas and the background stay as they are; the darkening is smooth (the per-pixel grain is blurred away, with no halo around atoms or at the silhouette). The darkening follows the rotation without flicker or a halo around the structure, the 3D pane's edges and the menus above it are not darkened, and the PNG has the same effect. Removing the key (or `0`) gives the previous look exactly. Check it in the browser build too (WebGL2 / WebGPU).
- [ ] **Flat menus and tabs, and their settings:**
  - **Action:** Start YUI with no `ui:` in the settings file; look at the menu bar, open File, hover the items, and click the tabs of the left and the bottom panes (narrow the window so that the tabs wrap). Then add `ui:` with `tab_style: filled`, `tab_font_size: 14`, `corner_radius: 14` and `colors: {accent: "#ff8800"}` (plus a wrong name such as `colors: {nope: "#ffffff"}`), restart, and look again.
  - **Expected result:** By default the selected tab has a stronger label and a thin blue line under it only below that tab (the unselected tabs are muted and light up when hovered; wrapped tabs stay side by side, not one per row); there is a thin line under the menu bar and under each tab bar; the pane titles are small muted text on the menu-bar color, each pane has a thin border, and the buttons (Connect, Refresh, ...) use the accent color (so `colors: {accent: ...}` recolors them too); the menu card has rounded corners and soft shadow. With the settings the selected tab is filled instead of underlined, the labels are larger, the menu corners rounder and the accent orange; the wrong color name shows a warning in the status bar and is ignored. Removing the section gives the default look.
- [ ] **Editor scrollbar:**
  - **Action:** Open a text file with a few hundred lines (some very long lines too). Drag the scrollbar thumb on the right of the editor, click the empty part of the bar, use the mouse wheel, press Ctrl+End / Ctrl+Home, move with the arrow keys past the bottom edge, and press Enter at the end of the file. Then put `scrollbar: false` under `editor:` in the settings file and restart.
  - **Expected result:** The thumb follows the wheel and the keys, dragging it scrolls the text, and the cursor stays in view after keyboard moves and edits (at most a line off when long lines wrap). With `scrollbar: false` the editor has no bar and scrolls as before.
- [ ] **Broken settings never stop the app:**
  - **Action:** Restart with each of these in the file: (a) `font_family: "No Such Font"`; (b) `font_size: 100`; (c) `font_size: big` together with `font_family: "DejaVu Sans Mono"`; (d) a line of garbage such as `editor: [` ; (e) an unknown key such as `tab_width: 4`.
  - **Expected result:** The app always starts. (a) A yellow status-bar warning `... font 'No Such Font' was not found; using JetBrains Mono` and the editor uses JetBrains Mono. (b) A warning that the size is out of range (6 - 72) and the editor uses 14 px. (c) A warning about `font_size`, but the DejaVu font is still used. (d) A warning `invalid YAML ... using default settings` with the file path, and all defaults. (e) No warning.

### 5.15 Second 3D viewport and camera link

- [ ] **Two viewports with separate cameras:**
  - **Action:** Open `core/tests/data/1hls.pdb`. Click "View" -> "Split Editor Right", and select the "3D Viewport 2" tab on the right if it is not selected. Drag in the left viewport to orbit and scroll to zoom, then do the same in the right one.
  - **Expected result:** Both viewports draw the same structure (same representations, same displayed model, same background). On the first split, the right viewport starts with the same view as the left one (a copy); after that each viewport turns and zooms independently. Right-clicking an atom in either viewport opens the atom context menu for the atom under the cursor in that viewport.
- [ ] **Link Viewport Cameras:**
  - **Action:** Click "View" -> "Link Viewport Cameras" (it shows `✓` and the status bar says `Viewport cameras are linked.`). Orbit and zoom in the left viewport, then in the right one. Click the item again.
  - **Expected result:** Turning the link on makes the right view take the left view; while linked, turning either viewport moves the other the same way. After unlinking, they move independently again. The link is off at startup.
- [ ] **Reset Camera and opening files with two viewports:**
  - **Action:** With the split on and the cameras turned differently, click "View" -> "Reset Camera". Then open another structure.
  - **Expected result:** Both viewports are fitted to the structure (centered, fully visible, each for its own pane shape). Opening a structure fits both again. Switching to a text file tab and back to "3D Viewport 2" does not change its view.
- [ ] **Split off and on again:**
  - **Action:** Turn the split off and on again. Open a text file while split and close it.
  - **Expected result:** The "3D Viewport 2" tab disappears with the split and comes back first in the right tab row; the second viewport keeps its previous view. Rendering stays correct (no flicker or black pane) after the toggles.
- [ ] **Saving the workspace restores the first viewport's camera:**
  - **Action:** Turn the left viewport to a distinctive angle and zoom level (and the right one differently), save the workspace with "File" -> "Save As", then "File" -> "New" and open the saved file.
  - **Expected result:** The left viewport shows the saved angle and zoom exactly (it is not re-fitted). The right viewport does not restore its view (it is a copy of the left one). A workspace saved before this version (camera never moved) is fitted to the structure when opened.

---

## Not testable yet (known limitations)

The following items are not yet available or are known limitations in version `2026.10.1`:
- **Python Console & AI Chat:** the Bottom pane's "Python Console" and "AI Chat" tabs are non-functional placeholders ("Coming soon").
- **Terminal limitations:** supports a single session only (no tabs) and draws foreground and background colors, bold, reverse video and underline (italic, dim, strikethrough and blink are not drawn). Direct typing does not take input-method (e.g. Japanese) text; use the input line below the screen. The history keeps the last 1000 lines. Mouse selection works on whatever is shown (also in the history) and assumes an estimated cell size, so with other fonts the highlight may drift by a cell on long lines.
- **Surface generation compute:** molecular surface mesh generation is currently computed on the CPU; generating surfaces for very large structures or dense grids may require noticeable computation time before the mesh appears (GPU compute-shader generation is planned for Phase 5c-iii).
- **WebGPU hardware requirement:** browser/WASM execution requires a physical GPU with WebGPU support.
- **Dependency pin:** the `proteindf-bridge` dependency is pinned to release tag `2026.10.1`.

---

## Reporting issues

If you encounter an unexpected crash, panic, or rendering glitch:
1. Capture the terminal outputs from both `frontend` and `backend` (including full backtraces if available).
2. Note the structure file format and size, the active representations, and the exact sequence of UI actions.
3. Report the issue on GitHub: https://github.com/ProteinDF/YUI/issues

### 5.16 UI theme (dark / light)

- [ ] **Switching the theme:**
  - **Action:** Open a structure (e.g. `core/tests/data/1hls.pdb`) and a text file, then click "View" -> "Theme:" -> "Light", and then "Dark" again.
  - **Expected result:** The status bar says `Theme: Light`. The menus, the status bar, the Project / Structure / Representation panes, Named Selections, the tabs and the text editor all switch between a light and a dark look at once, and the current theme is checked in the View menu. In the light theme all texts (including gray, small and disabled ones) are readable on their backgrounds. The 3D viewport background color and the molecule colors do not change. The terminal stays dark in both themes. The dark theme looks like it did before (some gray texts may differ slightly).
- [ ] **The theme is remembered:**
  - **Action:** Select "Light", quit, and start YUI again. Then open "File" -> "Open Settings File" (native build).
  - **Expected result:** YUI starts in the light theme. The settings file contains a line `theme: light`, and the comments and other settings you wrote before are still there. In the browser build, a reload also starts in the light theme (stored in `localStorage`).
- [ ] **A broken theme setting never stops the app:**
  - **Action:** Put `theme: blue` in the settings file and start YUI.
  - **Expected result:** YUI starts in the dark theme with a status-bar warning that the theme is not `dark` or `light`. Selecting a theme from the View menu fixes the line.

### 5.17 Selecting atoms and highlighting the selection

- [ ] **Selecting with the mouse:**
  - **Action:** Open `core/tests/data/1hls.pdb`. Click an atom (without moving the mouse). Then Shift+click another atom, Ctrl+click (Cmd+click on macOS) one of the selected atoms, double-click an atom, and finally click on empty space.
  - **Expected result:** The clicked atom glows (cyan-ish in the dark theme, blue in the light theme) and the status bar says `1 atom(s) selected`. Shift+click adds the atom; Ctrl/Cmd+click removes an already selected atom (or adds a new one); the double click selects all atoms of that residue; a click on empty space clears the selection. Dragging with the left button rotates the view and does not change the selection, and a click with a tiny movement (a few pixels) does not rotate the view. The Representation list does not get new rows.
- [ ] **Clearing and keeping a selection:**
  - **Action:** Select some atoms, press Esc. Select again and click "View" -> "Save Selection as Named Selection", then "View" -> "Clear Selection".
  - **Expected result:** Esc clears the glow. The named selection appears in the Named Selections list; "Clear Selection" and "Save ..." are grayed out while nothing is selected.
- [ ] **Other ways to select:**
  - **Action:** Right-click an atom and choose "Select Atom", then "Select Residue"; click a residue node in the Structure tree.
  - **Expected result:** Each replaces the selection and is highlighted the same way, without adding a yellow Representation.
- [ ] **Halo / Glow / Off:**
  - **Action:** Select a few atoms, then choose "View" -> "Selection Highlight:" -> "Halo", "Off" and "Glow". Show only the Cartoon representation and select an atom. Split the editor ("Split Editor Right") and look at both viewports. Switch between the dark and the light theme and between a dark and a white viewport background.
  - **Expected result:** Halo is a ring around each selected atom; Glow is a soft light; Off shows nothing (the selection itself is kept). The highlight is visible with the cartoon only, is hidden by atoms in front, appears in both viewports, and is visible on both dark and light backgrounds (the glow looks like a tint on white).
- [ ] **The highlight choice is remembered:**
  - **Action:** Choose "Halo", quit, start YUI again, and select an atom.
  - **Expected result:** The halo is used. The settings file has the line `selection_highlight: halo`; comments and other settings are kept. A line `selection_highlight: blur` starts YUI with the glow and a status-bar warning.

### 5.18 Playback of a multi-model structure

- [ ] **Play, pause and step (1HLS.cif, 20 models):**
  - **Action:** Open `core/tests/data/1HLS.cif` (section 5.1). Look below the 3D viewport: a control bar appears. Click "Play"; watch the model number in the status bar (`Playing model_N (N/20)`). Click "Pause". Use ◀ / ▶, |◀ / ▶| and drag the slider. Change the speed and click "Loop:" to cycle the loop mode (Once / Repeat / Ping-pong) and play to the end each time. Open a single-model structure (`1hls.pdb`).
  - **Expected result:** The molecule steps through the 20 models smoothly at the chosen speed (1–30 fps); the bar and View → Playback are present only for a multi-model structure. Repeat returns to the first model, Ping-pong reverses, Once stops at the last model. The application does not use CPU while paused. With `1hls.pdb` there is no bar and the View → Playback items are disabled.
- [ ] **Keys and the Structure tree:**
  - **Action:** With nothing being typed in, press Space, then ← and →. Click in a text field (e.g. the terminal's input) and press Space. While playing, click a model in the Structure tree's model bar.
  - **Expected result:** Space plays / pauses and the arrows step by one model, but typing in a text field is not affected. Choosing a model in the tree while playing stops playback and shows that model.
- [ ] **Surface and selection during playback:**
  - **Action:** Add a Surface representation, then play, pause and drag the slider. Select an atom, then step to another model. Play with two viewports (View → Split Editor Right).
  - **Expected result:** With a Surface card, the surface keeps up with the playing frames, a little behind the atoms (native build; frames are skipped when a build is still running); when paused or the slider is released it matches the shown model. In the browser build the surface disappears while playing or dragging and comes back when paused. The selection is cleared whenever the model changes. Both viewports follow playback. Check the smoothness on `1HLS.cif` with the Interactions representation too.

### 5.19 Camera animation (Spin / Rock / smooth moves)

- [ ] **Spin and Rock:**
  - **Action:** Open `core/tests/data/1hls.pdb` (section 5.1). Click View → "Spin"; then "Spin Speed" a few times; then "Rock" (it replaces Spin); then turn it off. While it moves, drag with the left button, then scroll the wheel.
  - **Expected result:** Spin turns the molecule around the vertical axis at the chosen speed (the label shows the speed; the menu stays checked). Rock swings about ±30° with a 4 s period around the view at the moment it was switched on. Dragging or scrolling stops the animation at once and the view continues from where it is, without a jump. The CPU is idle when nothing moves.
- [ ] **Smooth Reset Camera and Zoom to Selection:**
  - **Action:** Move and zoom the view away. Click View → "Reset Camera". Select a few atoms (click, Shift+click) and click View → "Zoom to Selection". With nothing selected, look at the item. Then open another structure.
  - **Expected result:** Reset Camera and Zoom to Selection move the camera over about 0.4 s (slow at both ends), without a jump or a pass through the molecule, and end with the same view as before this change (Reset) or with the selected atoms filling the view (Zoom). "Zoom to Selection" is disabled without a selection. Opening a structure still fits at once.
- [ ] **Two viewports:**
  - **Action:** View → "Split Editor Right". Rotate the right viewport a little, then View → "Spin". Then turn on "Link Viewport Cameras" and Spin again.
  - **Expected result:** Spin acts on the viewport moved last. With the link on, both viewports show the same rotating view; dragging in either stops both.

### 5.20 Trajectory playback (multi-frame XYZ)

- [ ] **Open and play a trajectory:**
  - **Action:** Open `core/tests/data/1hls.pdb`. Create a multi-frame XYZ for it (782 atoms, in the same order as the PDB, a few frames with slightly moved coordinates; for a quick check a script can write the PDB's coordinates plus a small shift per frame). Use File → "Open Trajectory (XYZ)..." and choose it. For the 3-atom `core/tests/data/multiframe.xyz`, open a 3-atom structure (e.g. a PDB with C, O, N) first. Click "Play", "Pause", ◀ / ▶, drag the slider, and change the loop mode.
  - **Expected result:** The molecule's atoms move frame by frame (ball-and-stick and the cartoon both follow; non-covalent interaction lines follow too). The control bar and the status bar show `step N (i/total)`. A trajectory with a different number of atoms is refused with an error in the status bar and nothing changes. Atom picking (right click, click) and the selection still work on the shown frame, and the selection is kept across frames.
- [ ] **Close it, and open an `.xyz` from the Project tab:**
  - **Action:** File → "Close Trajectory". Then open the `.xyz` from the Project tab (with a structure loaded): choose "Trajectory (XYZ)" in the dialog; open it once more with "Text Editor". Open a trajectory, then open another structure.
  - **Expected result:** Closing restores the original coordinates and the control bar disappears (for a single-model structure). The Project tab opens the file as a trajectory or as text as chosen. Opening another structure closes the trajectory.
- [ ] **Surface and large trajectories:**
  - **Action:** Add a Surface representation and play; pause. Open a large trajectory (many thousands of frames) and play; watch memory.
  - **Expected result:** The surface follows the playing frames with a delay (native) and matches the shown frame when paused. Memory does not grow with the number of frames (only the needed frame is read). Playback of a big structure may be slower than the chosen speed; frames are skipped to keep up.

### 5.21 Spacefill, Licorice and Line

- [ ] **The four kinds look different:**
  - **Action:** Open `core/tests/data/1hls.pdb`. Delete the second card (`ball_and_stick_default`) with its ✕ so that only the first card is left, then switch the first card's kind between Ball&Stick, Spacefill, Licorice and Line.
  - **Expected result:** Ball&Stick shows small balls and gray sticks. Spacefill shows large balls at the van der Waals radius, with no sticks. Licorice shows thick sticks colored by atom with smooth joints. Line shows thin colored lines and no balls. The kind of a Show As menu item (right-click a tree node) does the same.
- [ ] **Mixed kinds, picking and selection:**
  - **Action:** With the two default cards (Cartoon and Ball&Stick), change the first card to Spacefill, then to Licorice. Then bring back a single Ball&Stick card and add a second card with a selection of one chain (or use Show As on a residue) and give it Spacefill. Left-click atoms of each kind, then select one.
  - **Expected result:** Each card is drawn on its own and they overlap. With Spacefill on the first card, the whole structure is drawn as large balls (the Ball&Stick inside is hidden); with Licorice, thick sticks appear around the balls. The one chain is drawn as Spacefill and the rest as before; where they meet the sticks end at the big balls. A card that is switched off (🚫) stops drawing only itself: the other cards stay as they are, even if they select the same atoms. Hiding atoms (the tree's or the viewport menu's Hide) creates a card marked "hides these atoms"; while its eye is on (👁) those atoms are gone from every card, and switching it off (🚫) brings them back. Clicking an atom picks it in every kind (a Line atom within a few pixels of the line), and the selection highlight is as large as the drawn atom.

### 5.22 Exporting the view as SVG

- [ ] **The SVG matches the screen:**
  - **Action:** Open `core/tests/data/1hls.pdb`, turn the view to some angle, then click "File" -> "Export Image (SVG)..." and save `view.svg`. Open `view.svg` in a web browser.
  - **Expected result:** The status bar shows `Exported image to '.../view.svg'`. The browser shows the atoms as circles and the bonds as lines, in the same orientation as the screen, with the background color of the viewport. Near atoms hide far ones. The interaction lines (add an Interactions card first) appear as dashed lines in their colors; the ribbon (Cartoon) appears as flat-shaded polygons, and so do molecular surfaces and CUBE isosurfaces (a see-through surface lets the atoms and ribbon show through). Try a Surface card too (opaque, then with an opacity of 50 %): the file gets larger (a few MB) but opens in a browser. Interpenetrating triangles can overlap wrongly, and faint seams can show between translucent triangles (known limits of the painter's algorithm); with a very large surface the status bar says that there are too many triangles. The picture has the aspect of the 3D pane.
- [ ] **Follows the representations:**
  - **Action:** Switch the Ball&Stick card to Spacefill (or hide a card with 🚫), then export again.
  - **Expected result:** The SVG shows what the screen shows (large spheres, no sticks for Spacefill; a hidden card is absent). After "File" -> "New", "Export Image (SVG)..." is disabled.

### 5.23 Gaussian CUBE isosurface

- [ ] **Open a CUBE file and see its isosurface:**
  - **Action:** Click "File" -> "Open CUBE (Isosurface)..." and choose `core/tests/data/sphere.cube` (a test sphere, exp(-r²), in Bohr). Optionally open `core/tests/data/1hls.pdb` first.
  - **Expected result:** The status bar shows `Opened CUBE 'sphere.cube' (isosurface at ±0.050)`. A blue translucent sphere of radius about 0.92 Å (sqrt(ln 20) Bohr) appears around the origin. "File" -> "Close CUBE" removes it. Choosing a file that is not a CUBE shows an error and changes nothing. For an MO CUBE the positive lobe is blue and the negative lobe red. A CUBE card appears at the top of the Right pane (file name, isolevel slider, ✕ to close).
- [ ] **Isolevel slider and orbital selection:**
  - **Action:** Open `core/tests/data/orbitals.cube` (two datasets: s-like and p-like). Drag the Isolevel slider; use ◀ / ▶ to switch between orbital 1 and orbital 2.
  - **Expected result:** Orbital 1 is a blue sphere; orbital 2 is a pair of lobes, blue on the +x side and red on the -x side. Lowering the level makes the surface bigger, raising it makes it smaller and finally empty (the card says "No surface at this level"). The UI stays responsive while dragging (the surface may lag a little). Switching orbital resets the level to that orbital's default. The two lobes have color swatches (+ color / - color); clicking one recolors the surface at once.
- [ ] **The CUBE view is saved per file:**
  - **Action:** Open `core/tests/data/orbitals.cube`; go to orbital 2 (▶), set the Isolevel to about 0.3, drag the card's Opacity slider to about 25 % and pick another + color; File → Save As a workspace file; File → New; open that workspace file.
  - **Expected result:** The lobes become more see-through as you drag (no recomputation, the isolevel is kept). After reopening the workspace the same CUBE comes back showing orbital 2 at the level you set, with the 25 % opacity and the chosen color. Opening a different CUBE (File → Open CUBE...) or Close CUBE and open again starts at orbital 1, the default level, 60 % and the default colors. The saved YAML has a `cube_view:` block.
- [ ] **Atoms, workspace, and reopening:**
  - **Action:** Start with "File" -> "New" and open `core/tests/data/sphere.cube`. Then save the workspace ("File" -> "Save As YAML..."), choose "File" -> "New", and open that YAML again (native build). Also open `core/tests/data/1hls.pdb` first and then a CUBE.
  - **Expected result:** With no structure open, the CUBE's atom appears (one carbon) and the camera fits it. After reopening the YAML, the CUBE and its card come back (the path is stored in the YAML as `cube_path`); "File" -> "New" closes the card. With 1HLS open, the CUBE does not replace the structure. If the CUBE file was moved, an error is shown. Real ProteinDF density / MO CUBE files and oblique CUBE grids from real programs have not been tried (oblique grids are covered by unit tests only).

### 5.24 Exporting the view as PNG

- [ ] **The PNG matches the screen:**
  - **Action:** Open `core/tests/data/1hls.pdb`, turn the view, add a Surface card (and open `core/tests/data/sphere.cube` if you like), then click "File" -> "Export Image (PNG)...". A dialog offers the width (1000 / 2000 / 3000 / 4000 px), smoothing (Off / 2x / 3x) and the background (Scene color / Transparent); keep the defaults and click "Export...", then save `view.png`. Open it in an image viewer.
  - **Expected result:** The status bar first shows `Rendering the image...`, then `Exported image to '.../view.png'`. The image is 2000 px wide with the aspect ratio of the 3D pane (not of the whole window). It shows what the screen shows (balls, sticks, cartoon, the surface, the CUBE isosurface, interaction lines) in the same orientation and background color, without the selection highlight, with smooth edges. The framing can differ from the screen because the aspect of the whole window is used. Cancelling the dialog does nothing. After "File" -> "New", "Export Image (PNG)..." is disabled. A big surface or a software adapter can take several seconds; the window stays responsive. The browser build has not been tried.
- [ ] **Options:**
  - **Action:** Export again with width 1000 px, then 4000 px, with smoothing Off and 3x, and with the background set to Transparent (open the PNGs in a viewer that shows transparency, or place them on a colored page). Cancel the dialog once.
  - **Expected result:** The widths match; Off shows jagged edges and 2x / 3x smooth ones. With Transparent, the background is see-through, and the edges of the molecule blend with whatever is behind them without a dark outline; translucent surfaces keep their color. The dialog remembers the last choice, and Cancel exports nothing. At 4000 px with 3x the smoothing is lowered automatically (the file is still written). Not verified: how the edges of translucent surfaces on a transparent background look next to the exact result (premultiplied alpha is divided out).

### 5.25 Exporting an animation

- [ ] **Spin and Rock to numbered PNGs:**
  - **Action (desktop build):** Open `core/tests/data/1hls.pdb`, fit and turn the view. Click "File" -> "Export Animation...", keep Spin, 24 fps, 8 s, 1280 px, and click "Choose folder..." and pick an empty folder. Wait. Then do it again with Rock and a different frame rate.
  - **Expected result:** A bar above the panes shows `Exporting the animation: n/192 frames` with a Cancel button, and the window stays responsive (but do not touch the view). At the end the status bar shows `Exported 192 frames ...`. The folder holds `frame_0001.png` ... `frame_0192.png` (1280 x height, even sizes). Played as a slideshow, the molecule makes one full turn about the vertical axis and the last frame is just before the first (it loops). If `ffmpeg` is installed, `movie.mp4` is also there and plays; if not, the status bar shows the `ffmpeg ...` command to run. Cancel stops after the current frame, keeps the written frames and says how many. A Surface card and a CUBE isosurface are included in Spin / Rock.
- [ ] **Animation export in the browser (a zip; not run by the developers):**
  - **Action (browser build, WebGPU):** Open a structure, File → Export Animation... (the dialog has no MP4 row and the button says "Start"), keep Spin, 24 fps, 4 s, 960 px, and click Start. Wait (do not use the page). Also try Frames with `1HLS.cif`, and a large width with Smoothing 2x to reach the memory limit.
  - **Expected result:** The bar shows `n/96 frames`, then the browser downloads `yui-animation-<version>.zip` (the status bar names it and shows the ffmpeg command). `unzip -t` is clean; it holds `frame_0001.png` ... `frame_0096.png`, a full turn that loops. If the memory limit is exceeded the export stops with the "more than the browser can hold" message and nothing is downloaded. Cancel stops it without a download. Report the browser and GPU if it fails; the desktop build's memory path and the zip were checked by the developers.
- [ ] **Frames of a multi-model structure or a trajectory:**
  - **Action:** Open `core/tests/data/1HLS.cif` (20 models) and export with "Frames (20)"; also open an XYZ trajectory and do the same.
  - **Expected result:** 20 frames, one per model (the first model first), then the app returns to the model it showed before. With a Surface card, each frame shows the surface of that model / frame (the export is slower by one surface build per frame). The "Frames" button is disabled for a structure with one model. The browser build shows "Desktop only" for the menu item. Not tried: long exports of large structures, `ffmpeg` on this development machine (it was not installed; only the argument list is tested), and Windows / macOS.

### 5.26 Canvas size and framing of the exports

- [ ] **Shape, custom size and framing:**
  - **Action:** Open `core/tests/data/1hls.pdb`. Click "File" -> "Export Image (SVG)...": a dialog offers Shape (3D pane / 1:1 / 4:3 / 3:2 / 16:9 / Custom), the width, and Framing (As on screen / Fit all). Export with 16:9 and open the SVG in a browser; export again with Custom 600 x 1000 and "As on screen", then with "Fit all". Do the same from "Export Image (PNG)..." and "Export Animation...".
  - **Expected result:** The picture has the chosen shape (the SVG's `viewBox` and the PNG's pixel size match; 1:1 gives a square, Custom gives exactly the typed size). With "As on screen" the molecule keeps the screen's scale, so a narrow canvas can cut it at the sides; with "Fit all" the whole structure is inside the canvas whatever the shape, in the same viewing direction. Custom with an empty field, or a side outside 16 to 16384 px, shows an error in the status bar and exports nothing. A very large PNG that the graphics adapter cannot draw shows an error. The animation's size is made even (for the encoder). Not tried: very large canvases (memory and time) and what a real printed figure needs (DPI and units are not set).

---

## 6. Trying a pull request on another machine

Use this to check a pull request (or any pushed branch) on a computer other than the development one — especially to see whether the renderer starts and draws correctly on a different GPU, OS or driver.

1. **Get the code** (HTTPS only). First time: `git clone https://github.com/ProteinDF/YUI.git && cd YUI`. Then fetch the branch: `gh pr checkout <number>`, or `git fetch origin <branch> && git checkout <branch>`. To update an existing clone, run `git fetch origin <branch>` and `git checkout <branch>` again.
2. **Prerequisites:** see "Prerequisites" above. On Linux the GPU driver must provide Vulkan. macOS (Metal) and Windows (Vulkan or DX12) need nothing more.
3. **Start the native build:** `cargo run --release --bin yui-frontend` (the first build takes a while). Then follow the checklist in section 5 for the feature in the pull request.
4. **Start the web build (optional):** `trunk serve --port 8085 index.html` and open it in a WebGPU-capable browser. It uses a different graphics path from the native build, so it is worth checking too.
5. **Check first:** the application starts without an error on the terminal and the viewport draws. A failure at start-up usually means a render pipeline could not be created on that GPU.
6. **Compare when needed:** to compare with the state before the change, check out `develop` in another clone or worktree and start it the same way.
7. **Report:** the result for each item, together with the OS, the GPU and driver, and the terminal output if something failed (see "Reporting issues").

---

## 7. Checking the rendering without a screen (headless)

The renderer can draw into an off-screen texture and be read back as an image, so a change in how things look can be checked on a machine with no display or no GPU (a CI box, an SSH session, a container). It was used to check the representation kinds (Spacefill / Licorice / Line) in a PR. This section lists what the machine needs. It is now a permanent test, `frontend/src/viewport/headless.rs` (it draws with the same `viewport/offscreen.rs` that File → Export Image (PNG) uses). Run it with `cargo test -p yui-frontend headless -- --nocapture`. It prints the adapter it found and the pixel counts, and it is skipped (prints `skipped`, and passes) when no adapter is found, so `scripts/check.sh` still works on a machine without one.

**What the machine needs**

- A wgpu adapter. A real GPU is not required: a software one is enough. On Linux (Debian / Ubuntu) install the Mesa software rasteriser:
  - `libgl1-mesa-dri` (provides `swrast_dri.so` / llvmpipe) and `libegl1` with `libegl-mesa0` and `libgbm1`: wgpu then finds an adapter through its OpenGL (EGL) backend. This is what was used (reported as "llvmpipe, Mesa GL 4.5").
  - Alternatively `mesa-vulkan-drivers` (lavapipe) with `libvulkan1`: wgpu finds a software Vulkan adapter. Not tried here.
- No display server is needed for the off-screen path, but a build that opens a window (`cargo run --bin yui-frontend`) still needs one.
- The Rust toolchain only. No extra Cargo dependency was needed: `wgpu` and `iced` are already in the workspace.
- To turn the pixels into a PNG a small script was enough (Python 3 with `zlib` and `struct` from the standard library).

**How to check the machine**

- List the adapters: `vulkaninfo --summary` (package `vulkan-tools`) for Vulkan, `eglinfo` (package `mesa-utils-bin`) for EGL. A line with `llvmpipe` or `lavapipe` means a software adapter is present.
- If the program finds no adapter, wgpu reports it at start-up. Install the packages above and run again.

**How it was done** (for whoever turns it into a test)

1. Build the `ViewportProgram` of `frontend/src/viewport/mod.rs` with the structure, representations and a camera. Set `fit_pending: true` to get a camera fitted to the structure.
2. Create a wgpu `Instance`, request any adapter (a software one is fine), then a `Device` and a `Queue`.
3. Create `ViewportPipeline::new(&device, &queue, format)` (`viewport/pipeline.rs`) and call `prepare` with a `Viewport::with_physical_size`. Then `render` into a texture (`Rgba8UnormSrgb`, `RENDER_ATTACHMENT | COPY_SRC`).
4. Copy the texture to a buffer and map it. `bytes_per_row` must be a multiple of 256, so use a width that is a multiple of 64 pixels (for example 256 × 256).
5. Compare the images, for example by counting the pixels that differ from the background colour, or write a PNG and look at it.

**Limits**

- A software adapter draws slowly and may differ slightly from a real GPU (anti-aliasing, precision), so compare counts and shapes, not exact pixels, and do not use it to judge frame rate or smoothness.
- It checks that something is drawn and how it differs between settings. It does not replace the manual checks in section 5 on a real GPU.
- The web build (WebGPU) is not covered.

**UI operations by simulated clicks:** `frontend/src/app/ui_tests.rs` uses `iced_test` (a dev-dependency) to build the real window (`Yui::view`), find widgets by their exact text, click them without a screen, and feed the resulting messages back to the app. It covers the File menu items (and that they are disabled without a structure), the PNG / SVG / animation dialogs (choices, Custom size, Cancel, Export, remembered options), the Representation cards (kind, the Opacity slider appearing only for Surface / Interaction, the eye icon, delete), the playback bar buttons, the CUBE card, and the View menu (a chosen item closes the menu). They run with `cargo test -p yui-frontend ui_tests` and are part of `scripts/check.sh`. With `YUI_UI_SHOTS_DIR=<dir>` they also save a PNG of the window at key points (software renderer; the 3D viewport is not drawn there) so the layout can be looked at. Limits: the file / folder dialogs (system dialogs) and the real GPU drawing are not exercised; widgets are found by text, so same-text buttons (for example the `▶` in several panes) need `click_nth` / `click_last`.

**Real-window smoke test:** `python3 scripts/smoke-native.py [output dir]` (default `target/smoke`) builds and starts the real backend and frontend on a virtual screen (Xvfb), sends clicks with `xdotool`, and saves a screenshot after every step (`01_start.png` ... `08_multi_model_loaded.png`) plus the logs. It checks that the window opens, the screen is not blank, each action changes the screen (in the expected region), the app keeps running and there is no panic in the logs. The steps: start; Project tab (backend file list); load `1hls.pdb` (the Inspector cards appear); File menu; the PNG dialog and Cancel; View menu and closing it; load `1HLS.cif` (20 models: the playback bar appears). It uses iced's software renderer (`ICED_BACKEND=tiny-skia`), because Xvfb has no GPU surface; so the **3D viewport is not drawn** (use the render gallery for that) and the OS file / folder dialogs cannot appear (files are opened from the backend list). It only points at the virtual display (it unsets `WAYLAND_DISPLAY`) so no window appears on your own desktop. Needs `Xvfb`, `xdotool` and `python3`, port 8080 free; it is skipped (exit 0) when something is missing. The click coordinates assume the default 1024 x 768 window; update the script if the layout changes. It is not part of `scripts/check.sh`.

**Render gallery (visual check without a screen):** `scripts/render-gallery.sh [output dir]` (default `target/gallery`) draws about 30 scenes off-screen with the same renderer, writes one PNG per scene, a contact sheet per group (`sheet_*.png`) and `manifest.txt` (which tile is which). The groups are: the representation kinds and color schemes; overlapping cards, an eye-off card and a hide card; molecular surfaces (opaque, translucent, with the cartoon); interaction lines (with and without the card); CUBE isosurfaces (density, p-type orbital, isolevel, colors); export options (transparent background on a checkerboard, portrait / 16:9 canvases with "As on screen" and "Fit all"); animation frames (Spin, and the Frames of 1HLS.cif each with its own surface). Each scene is checked for "something is drawn" (and a few relative checks: a higher CUBE isolevel gives a smaller surface, spin frames differ, each model has its own surface, the interaction card adds pixels). The test is `#[ignore]`d, so `scripts/check.sh` does not run it; the script runs it and is skipped without an adapter. The images are meant to be looked at (by a person or an AI) to find visual problems; they are not compared pixel by pixel because software renderers differ slightly.

**What the test checks:** it draws 1HLS at 256 × 256 with several representation sets and counts the pixels that differ from an image with every representation hidden. On llvmpipe (Mesa GL 4.5) it saw: Ball & Stick 8084, Cartoon + Ball & Stick 8377, Spacefill + Ball & Stick 14205, Licorice 3369, Line 481. It asserts only the order (Ball & Stick > 1000, adding Cartoon or Spacefill adds pixels, Licorice < Ball & Stick, Line < Licorice), not the exact counts. Other adapters and operating systems have not been tried.
