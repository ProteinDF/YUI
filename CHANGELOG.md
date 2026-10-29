# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Calendar Versioning](https://calver.org/) (`YYYY.MM.MICRO`).

> [!NOTE]
> [Japanese version (日本語版)](CHANGELOG.ja.md) is also available.

## [Unreleased]

### Changed
- `proteindf-bridge` is now pinned to the release tag `2026.10.20` (was `2026.10.7`). It adds the φ/ψ calculation that a coming Ramachandran plot needs; nothing visible changes yet.
- Python plugins now run as separate processes that YUI starts and stops, not inside the app, so a plugin that crashes or hangs cannot stop YUI. Builds no longer need `--features python` and no `libpython`; a plugin runs with the Python named by `plugins.python` in `settings.yaml`, or `python3` on the `PATH`, and the author API (`register(app)`, `app.log`, `app.add_command`, `app.add_menu_item`, `ctx.target`) is unchanged. What a plugin prints goes to `plugin.log`. A plugin with a `command` in its `plugin.yaml` is run as it is, in any language. See `architecture.md` §6.4. Checked by hand: on macOS, loading a plugin, the plugin list and a command from a right-click menu; on Windows, only building and starting the app. The `oneshot` mode has been run in the automated tests only, not yet from the app window.
- The app is named "YUI" in its titles and version information: the window title, the About dialog and its menu item, the web page title and badge, the backend console messages, the installer description and the README / ROADMAP / architecture / TESTING headings no longer use the 結 character. The name 結 (YUI) in the body text of the documents is unchanged.

### Added
- View → "Plot: Atom Positions" opens a scatter plot tab in the bottom panel (x and y of every atom, one series per chain). Clicking a point selects the atom, with the same Shift / Ctrl (Cmd) rules as the 3D view, and atoms selected in the 3D view are drawn highlighted in the plot. This is the generic plot that later graphs (Phase 6l-ii, 6l-iii) will use; this one is a development sample. Checked in a real window only up to opening the tab (no structure could be opened there); clicking points is covered by automated tests.
- A guide to the plugin protocol (`docs/plugins/plugin-protocol.md`) for writing a plugin in a language other than Python, with a complete example that speaks the protocol directly (`examples/protocol/raw-python`).
- Plugins can run in the `oneshot` mode (`mode: oneshot` in `plugin.yaml`): YUI starts a new process for each command call and does not keep one running. The menu items are written in `plugin.yaml` (`menu_items`, `commands`). A Python package keeps the same `register(app)` and `func(ctx, args)`; any other program gets `{input}` and `{output}` file paths in its `command`. One failed call does not disable the plugin. See the plugin-authoring guide, the example plugin `examples/plugins/hello-oneshot` and `architecture.md` §6.4.
- View → "Plugins..." lists the Python plugins found in the plugins folder, with their version, author, description and state (loaded, enabled, disabled, or failed with the reason). Enabling or disabling one writes `plugins: enabled:` in `settings.yaml`; the change applies at the next start. A plugin-author guide (`docs/plugins/plugin-authoring.md`) and an example plugin (`examples/plugins/hello-yui`) are included.
- Python plugins can add commands and menu items (`app.add_command`, `app.add_menu_item`). An item appears at the menu location it names (File, View, Help, or a right-click menu). A plugin whose command fails is disabled, and the reason is shown in the status bar and written to `plugin.log`.
- Python plugins named in `plugins: enabled:` in `settings.yaml` are loaded from `<config>/yui/plugins/` at startup. Failures appear in the status bar and are written to `plugin.log` in that folder.
- The app icon (the three linked atoms in `packaging/icons/`) is now the window icon of the desktop app and the favicon of the browser build.
- Tabs can be reordered by dragging them within their pane. The tabs of the editor area (3D Viewport and text files) no longer wrap to a second row: they scroll sideways (trackpad, Shift + wheel or the wheel), as in VSCode. The side and bottom panes keep wrapping. Checked on Linux under a virtual display; macOS and the browser (including touch) are not checked.
- While the editor is split, tabs can be dragged to the other side: onto its tabs (placed there) or onto its body (added at the end; the target is highlighted). This also works for the 3D Viewport tabs, and a moved viewport keeps its view. While split, each side always shows its tab bar.
- Dropping a tab on an edge of the editor area (the outer quarter of any side of a group) splits that group in that direction, left/right or top/bottom, as many times as needed; the half the tab will take is highlighted. A group whose last tab is moved away or closed disappears. The sizes you dragged (the docks and the other splits) are kept. View → "Split Editor Right" now merges every group into one when it is checked.
- Tabs can also be moved between the side and bottom panels and the editor area, in both directions (for example the Terminal next to the 3D Viewport, or the 3D Viewport in the bottom panel). A panel left without tabs stays and shows "Drag a tab here."; with `ui.hide_empty_docks: true` in the settings file it is hidden instead (View shows it again). Every pane now shows its tab bar, even with a single tab, so that every tab can be dragged.
- While a tab is dragged, a small box with its name follows the mouse, as in VSCode. The arrangement of the groups and their sizes are saved in the workspace (`editor_layout`) and restored when it is opened.

### Fixed
- The window icon of the desktop app is now 128 x 128: the 256 x 256 version was too large for the X11 request limit, so the window had no icon on X11.
- The macOS app bundle (the dmg) carries an `icon.icns` built from the app icon, so the Dock and Finder show the icon.
- The terminal takes the keyboard as soon as it connects, so typing works without clicking the screen first. The input line below the screen can be hidden with "Hide input line" (the quick keys Ctrl+C, Tab, Copy and Paste stay); it is shown by default.
- The terminal's scrollbar no longer covers the text: it sits at the right edge of the screen, and the text keeps the full width.
- The terminal accepts Japanese input (IME): the text being composed is shown underlined at the cursor, and the committed text is sent to the shell. Checked with the unit tests and a screenshot only; not yet tried with a real Japanese input method on macOS.
- The desktop app (standalone, without a backend) has a "Connect (Local shell)" button in the Terminal tab: it runs your shell on the same machine, so the terminal works without `backend`. The web build still needs `backend`. Checked on Linux under a virtual display only (typing a command ran in the local shell); Windows and macOS are not checked.
- The installers and the web build now ship the license texts: the app's `LICENSE` (GPL v3) and the SIL OFL 1.1 texts of the bundled fonts (JetBrains Mono, Noto Sans JP). Before, only the repository had them. Checked in the `.deb` (`usr/lib/yui-frontend/`) and the web build output; the macOS and Windows installers were not built.
- Full-width characters (Japanese) in the terminal are drawn with the bundled Noto Sans JP, centered in their two columns, instead of whatever font the OS offers; so they look the same on every system and line up with the columns. No new font was added. Checked on Linux under a virtual display; Mac not checked.

## [2026.10.10] - 2026-10-17

### Changed
- The choice buttons in the export dialogs (shape, size, framing, smoothing, background, and so on) now match the quiet buttons: the chosen one is filled with the accent color, the others have a faint fill and a thin border. A long structure description in the Explorer wraps instead of running into the "N models" label.
- Buttons that used the stock bright style (Connect / Clear in the Log Viewer, the playback bar, the model selector, the terminal and project-tree toolbars, Cancel / Close in dialogs) are now quiet: a faint fill with a thin border, and the accent color only for confirming actions (Export, Start, Create invitation, Save). The project tree rows are flat, with the selected file marked by a soft background. The Log Viewer toolbar has padding and muted labels, and "+ Add" in the Inspector no longer wraps onto two lines.
- Small touches to the flat look: the borders between panes are fainter (two neighbouring panes used to draw a doubled line), and a document tab has less space between its name and its close button. The UI screenshot tests now draw with the app's own theme, and a new one shows document tabs and the menu in both the dark and the light theme.
- More polish on the flat look: the pane titles ("Explorer / Playbook", "3D Viewport", "Inspector", "Bottom Panels") are small muted text on the menu-bar color, each pane has a thin border, and the standard widgets (buttons such as Connect / Refresh, the focus of text inputs, the pane-resize highlight) now use the UI accent color, which `ui.colors.accent` in the settings file changes. Checked with the UI screenshot tests and the real app under a virtual display (software renderer) only.
- Flatter, more modern menus and tabs: tabs have a muted label (a stronger one and a 2 px accent line under it for the selected tab), a soft hover background and no `[ ]` brackets; the menu bar and each tab bar have a thin line under them; menus and dialogs have rounder corners and a softer shadow.

### Added
- Help → About shows the build's git commit (short hash, with "(modified)" when the working tree has uncommitted changes; "unknown" outside a git checkout).
- Help → "Mouse & Keyboard..." lists the mouse, keyboard and terminal controls; the README has the same list in a new "Controls" section.
- Settings file section `ui:` to customize them (restart to apply): `tab_style` (`underline` / `filled`), `tab_font_size` (8 - 24), `corner_radius` (0 - 16) and color overrides by name (`colors` for both themes, `dark_colors` / `light_colors` for one; `#rrggbb` or `#rrggbbaa`; the names are listed in the settings template and in `architecture.md`). A wrong value keeps the default and shows a warning in the status bar. Checked with the UI screenshot tests and the real app under a virtual display (software renderer) only; the look on a real GPU and in the browser build is not checked.

### Known Limitations
- The terminal was checked only with a software renderer under a virtual display and a scripted keyboard / mouse: the feel of real typing, the mouse wheel and selection, `vim`, the browser build, macOS and Windows are not verified. Its row / column count and the selection use an estimated cell size (0.6 x the font size wide, 1.3 x + 1 high), so with other fonts the highlight can drift by a cell on long lines; the text in a full-screen program is not selectable beyond what is shown, and input-method (Japanese) text must go through the input line below the screen.
- The text editor's scrollbar follows the cursor with an estimate of the wrapped rows (about one line of drift on very long wrapped lines); dragging it was not tried with a real GPU or in the browser.
- The Python Console and AI Chat panel tabs in the Bottom pane are not yet implemented ("Coming soon" placeholders).
- The browser build compiles, is served (by `backend` or any static host) and has the same features as the desktop one except the MP4 (frames are downloaded as a zip), but it has not been run in a browser by the developers (no WebGPU adapter on the development machine): the browser exports, the animation download, the dialogs, the backend connection and SSAO are untested at run time. It needs a physical GPU with WebGPU. The compressed delivery (`.gz`) was checked with `curl` only: the `.br` files were never built (no `brotli` on the development machine) and a browser has not loaded them.
- `backend` is built for a few trusted people: sharing uses expiring Editor / Viewer invitations, failed authentications are rate limited, and the Origin / Host checks and the random Host secret are in place, but it speaks plain HTTP (expose it only behind a TLS-terminating reverse proxy with `YUI_BEHIND_TLS_PROXY=1`, and keep the terminal and file APIs off unless you need them). Checked only with a raw client, headless Chrome (the `#token=` flow, REST and WebSocket; the app itself cannot draw there) and the desktop app under a virtual display: the Share Session dialog with a Viewer / Editor secret and in the browser build, a remote backend with `--token`, the log files on macOS / Windows, and Windows were not tried.
- The installers are unsigned and not notarized (macOS and Windows warn on the first launch). Only the Linux `.deb` and `.AppImage` were built and started by the developers; the macOS `.dmg`, the Windows NSIS setup and the manual GitHub workflow that builds them have never been run. There is no Windows `.msi` (the CalVer version does not fit the MSI rules) and no `.rpm`.
- Molecular surface generation (and the CUBE isosurface) is computed on the CPU (several times faster since 2026.10.4, on several threads in the desktop build); the browser build hides the surface during playback but builds one per frame for an animation export (the page is busy meanwhile).
- The SVG export draws ribbons and surfaces as flat-shaded polygons with a painter's algorithm: interpenetrating triangles can overlap wrongly and translucent triangles can show faint seams; more than 500,000 triangles are refused. The selection highlight and SSAO are not in the SVG.
- SSAO is off by default; its blur has a fixed 2 px radius (some unevenness may remain at strength 1 or with a large radius) and was checked only with the render gallery (software renderer); translucent surfaces and the selection glow neither cast nor receive occlusion.
- The PNG, SVG and animation exports, the CUBE view, SSAO and the smoke / UI tests were checked with a software renderer (llvmpipe) only; the appearance on a real GPU, the selection glow size setting and the OS file dialogs are not automatically checked. Changes to the settings file need a restart. The desktop MP4 needs `ffmpeg` (otherwise the PNG frames and the command are given).
- `.mol2` files are loaded as a single residue, and partial (non-integer) atomic charges are not preserved on structure export (mmCIF refuses them; PDB rounds them).
- The look changes (flat buttons, the choice buttons in the export dialogs, the Help controls list, the Explorer wrapping) were checked with the UI screenshot tests and the software renderer on a virtual display only: the look on a real GPU and in the browser build is not checked.

## [2026.10.9] - 2026-10-15

### Changed
- SSAO is smoother: the occlusion is first drawn into its own texture and then blurred with a depth-aware 5 x 5 filter (neighbours across an outline are not mixed) before it darkens the picture, so the faint per-pixel grain is gone and atom outlines stay sharp. Checked with the render gallery (software renderer) only; the look on a real GPU is not checked.
- A connection opened with an invitation now closes at once when the invitation is revoked (measured: 0.001 s with a real `backend`; before, up to 5 seconds) and at the expiry time (before: checked every 5 seconds). Connections of the Host secret and of other invitations are untouched.
- The browser build is served compressed: `scripts/build-web.sh` writes a `.gz` (and a `.br` when `brotli` is installed) next to each file of `dist/`, and `backend` sends the variant the browser accepts. The `.wasm` goes from 13.7 MB to 6.3 MB (gzip). The zip still holds the plain files. Not measured in a browser (no WebGPU here): only the response headers and bytes with `curl`.

### Fixed
- An error from `backend` was shown with its reason twice ("403 Forbidden Forbidden: ..."); it now reads "403 Forbidden: this action requires the Host role".

### Known Limitations
- The terminal was checked only with a software renderer under a virtual display and a scripted keyboard / mouse: the feel of real typing, the mouse wheel and selection, `vim`, the browser build, macOS and Windows are not verified. Its row / column count and the selection use an estimated cell size (0.6 x the font size wide, 1.3 x + 1 high), so with other fonts the highlight can drift by a cell on long lines; the text in a full-screen program is not selectable beyond what is shown, and input-method (Japanese) text must go through the input line below the screen.
- The text editor's scrollbar follows the cursor with an estimate of the wrapped rows (about one line of drift on very long wrapped lines); dragging it was not tried with a real GPU or in the browser.
- The Python Console and AI Chat panel tabs in the Bottom pane are not yet implemented ("Coming soon" placeholders).
- The browser build compiles, is served (by `backend` or any static host) and has the same features as the desktop one except the MP4 (frames are downloaded as a zip), but it has not been run in a browser by the developers (no WebGPU adapter on the development machine): the browser exports, the animation download, the dialogs, the backend connection and SSAO are untested at run time. It needs a physical GPU with WebGPU. The compressed delivery (`.gz`) was checked with `curl` only: the `.br` files were never built (no `brotli` on the development machine) and a browser has not loaded them.
- `backend` is built for a few trusted people: sharing uses expiring Editor / Viewer invitations, failed authentications are rate limited, and the Origin / Host checks and the random Host secret are in place, but it speaks plain HTTP (expose it only behind a TLS-terminating reverse proxy with `YUI_BEHIND_TLS_PROXY=1`, and keep the terminal and file APIs off unless you need them). Checked only with a raw client, headless Chrome (the `#token=` flow, REST and WebSocket; the app itself cannot draw there) and the desktop app under a virtual display: the Share Session dialog with a Viewer / Editor secret and in the browser build, a remote backend with `--token`, the log files on macOS / Windows, and Windows were not tried.
- The installers are unsigned and not notarized (macOS and Windows warn on the first launch). Only the Linux `.deb` and `.AppImage` were built and started by the developers; the macOS `.dmg`, the Windows NSIS setup and the manual GitHub workflow that builds them have never been run. There is no Windows `.msi` (the CalVer version does not fit the MSI rules) and no `.rpm`.
- Molecular surface generation (and the CUBE isosurface) is computed on the CPU (several times faster since 2026.10.4, on several threads in the desktop build); the browser build hides the surface during playback but builds one per frame for an animation export (the page is busy meanwhile).
- The SVG export draws ribbons and surfaces as flat-shaded polygons with a painter's algorithm: interpenetrating triangles can overlap wrongly and translucent triangles can show faint seams; more than 500,000 triangles are refused. The selection highlight and SSAO are not in the SVG.
- SSAO is off by default; its blur has a fixed 2 px radius (some unevenness may remain at strength 1 or with a large radius) and was checked only with the render gallery (software renderer); translucent surfaces and the selection glow neither cast nor receive occlusion.
- The PNG, SVG and animation exports, the CUBE view, SSAO and the smoke / UI tests were checked with a software renderer (llvmpipe) only; the appearance on a real GPU, the selection glow size setting and the OS file dialogs are not automatically checked. Changes to the settings file need a restart. The desktop MP4 needs `ffmpeg` (otherwise the PNG frames and the command are given).
- `.mol2` files are loaded as a single residue, and partial (non-integer) atomic charges are not preserved on structure export (mmCIF refuses them; PDB rounds them).

## [2026.10.8] - 2026-10-15

### Added
- Invitations for shared sessions: the Host creates expiring, revocable Editor / Viewer secrets (`POST /api/invites`, `GET /api/invites`, `DELETE /api/invites/{id}`; `scripts/yui-invite.py create viewer --ttl 3600 | list | revoke <id>`). An invitation has its role only: it cannot create invitations, change the state or open the terminal. A connection opened with one closes within about 5 seconds after the invitation expires or is revoked. In the app: **File → Share Session...** creates (role, lifetime), copies the browser address / token, lists and revokes invitations.
- `backend` writes its own log to daily files too (`backend.<date>.log`, 7 days kept; `$XDG_STATE_HOME/yui/logs` on Linux, `~/Library/Logs/yui` on macOS, `%LOCALAPPDATA%\yui\logs` on Windows; `YUI_LOG_DIR` overrides) and only logs `info` and above by default (`RUST_LOG` changes it).

### Security
- Failed authentications are rate limited per peer address: 10 unknown tokens within 60 seconds lock that address out for 60 seconds (`429` with `Retry-After`, even for a valid token). Requests without any token are not counted.

### Known Limitations
- The terminal was checked only with a software renderer under a virtual display and a scripted keyboard / mouse: the feel of real typing, the mouse wheel and selection, `vim`, the browser build, macOS and Windows are not verified. Its row / column count and the selection use an estimated cell size (0.6 x the font size wide, 1.3 x + 1 high), so with other fonts the highlight can drift by a cell on long lines; the text in a full-screen program is not selectable beyond what is shown, and input-method (Japanese) text must go through the input line below the screen.
- The text editor's scrollbar follows the cursor with an estimate of the wrapped rows (about one line of drift on very long wrapped lines); dragging it was not tried with a real GPU or in the browser.
- The Python Console and AI Chat panel tabs in the Bottom pane are not yet implemented ("Coming soon" placeholders).
- The browser build compiles, is served (by `backend` or any static host) and has the same features as the desktop one except the MP4 (frames are downloaded as a zip), but it has not been run in a browser by the developers (no WebGPU adapter on the development machine): the browser exports, the animation download, the dialogs, the backend connection and SSAO are untested at run time. It needs a physical GPU with WebGPU.
- `backend` is built for a few trusted people: sharing uses expiring Editor / Viewer invitations, failed authentications are rate limited, and the Origin / Host checks and the random Host secret are in place, but it speaks plain HTTP (expose it only behind a TLS-terminating reverse proxy with `YUI_BEHIND_TLS_PROXY=1`, and keep the terminal and file APIs off unless you need them). Checked only with a raw client, headless Chrome (the `#token=` flow, REST and WebSocket; the app itself cannot draw there) and the desktop app under a virtual display: the Share Session dialog with a Viewer / Editor secret and in the browser build, a remote backend with `--token`, the log files on macOS / Windows, and Windows were not tried. A connection opened with an invitation closes up to 5 seconds after the invitation ends, not instantly.
- The installers are unsigned and not notarized (macOS and Windows warn on the first launch). Only the Linux `.deb` and `.AppImage` were built and started by the developers; the macOS `.dmg`, the Windows NSIS setup and the manual GitHub workflow that builds them have never been run. There is no Windows `.msi` (the CalVer version does not fit the MSI rules) and no `.rpm`.
- Molecular surface generation (and the CUBE isosurface) is computed on the CPU (several times faster since 2026.10.4, on several threads in the desktop build); the browser build hides the surface during playback but builds one per frame for an animation export (the page is busy meanwhile).
- The SVG export draws ribbons and surfaces as flat-shaded polygons with a painter's algorithm: interpenetrating triangles can overlap wrongly and translucent triangles can show faint seams; more than 500,000 triangles are refused. The selection highlight and SSAO are not in the SVG.
- SSAO is off by default and leaves a faint grain at high strength; translucent surfaces and the selection glow neither cast nor receive occlusion.
- The PNG, SVG and animation exports, the CUBE view, SSAO and the smoke / UI tests were checked with a software renderer (llvmpipe) only; the appearance on a real GPU, the selection glow size setting and the OS file dialogs are not automatically checked. Changes to the settings file need a restart. The desktop MP4 needs `ffmpeg` (otherwise the PNG frames and the command are given).
- `.mol2` files are loaded as a single residue, and partial (non-integer) atomic charges are not preserved on structure export (mmCIF refuses them; PDB rounds them).

## [2026.10.7] - 2026-10-14

### Changed
- The Terminal tab has no "Role Token" field any more (the token is one per app, see Security). The collab packet's `role_token` header field is ignored by `backend`.

### Security
- Every `backend` API now needs a secret. The fixed tokens 1 / 2 / 3 are gone: `backend` makes a random 256-bit secret at each start (`YUI_TOKEN` sets it instead) and writes it with its URL to `backend.json` in the config folder (mode 0600), which the desktop app reads (`--token` / `YUI_TOKEN` for a backend on another computer). The secret travels in `Authorization: Bearer`, or, for the browser's WebSocket, in the subprotocol `yui.token.<secret>`; it is never put in a URL (the old `?token=1` no longer works). Reading the state is open to every role; writing it and the terminal need the Host role. For the browser build, `backend` prints a `http://127.0.0.1:8080/#token=...` address (the fragment is not sent to the server), the page keeps the secret in `sessionStorage` and clears the address bar.
- New `YUI_BIND` (default `127.0.0.1:8080`): a non-loopback address is refused unless `YUI_BEHIND_TLS_PROXY=1` says a reverse proxy terminates TLS, and then the terminal and file APIs stay closed unless `YUI_ENABLE_TERMINAL=1` / `YUI_ENABLE_FS=1`.
- `backend` now refuses requests from other web sites: a WebSocket upgrade or REST call whose `Origin` is not the backend's own origin (or in `YUI_ALLOWED_ORIGINS`, comma-separated), or whose `Host` is not `127.0.0.1` / `localhost` / `[::1]` (or in `YUI_ALLOWED_HOSTS`), gets 403 before any token is checked, and the permissive CORS headers are gone. Before this, any web page the user visited could open `/api/terminal?token=1` on a locally running `backend` and run commands. Requests without an `Origin` (the desktop app, command-line tools) are unaffected. Failed authentications and terminal sessions are now logged (paths only, never tokens). The fixed tokens 1 / 2 / 3 are still there (Phase 6j-ii); do not expose `backend` to a network yet.

### Known Limitations
- The terminal was checked only with a software renderer under a virtual display and a scripted keyboard / mouse: the feel of real typing, the mouse wheel and selection, `vim`, the browser build, macOS and Windows are not verified. Its row / column count and the selection use an estimated cell size (0.6 x the font size wide, 1.3 x + 1 high), so with other fonts the highlight can drift by a cell on long lines; the text in a full-screen program is not selectable beyond what is shown, and input-method (Japanese) text must go through the input line below the screen.
- The text editor's scrollbar follows the cursor with an estimate of the wrapped rows (about one line of drift on very long wrapped lines); dragging it was not tried with a real GPU or in the browser.
- The Python Console and AI Chat panel tabs in the Bottom pane are not yet implemented ("Coming soon" placeholders).
- The browser build compiles, is served (by `backend` or any static host) and has the same features as the desktop one except the MP4 (frames are downloaded as a zip), but it has not been run in a browser by the developers (no WebGPU adapter on the development machine): the browser exports, the animation download, the dialogs, the backend connection and SSAO are untested at run time. It needs a physical GPU with WebGPU.
- `backend` is for one user: there is only the Host secret (no Editor / Viewer secrets yet, so sharing a session means sharing the Host), failed authentications are logged but not rate limited, and it speaks plain HTTP (expose it only behind a TLS-terminating reverse proxy with `YUI_BEHIND_TLS_PROXY=1`, and keep the terminal and file APIs off unless you need them). The new authentication was checked with a raw client and the desktop app only: the browser build's `#token=` flow, `trunk serve`, a remote backend with `--token`, and Windows were not tried.
- The installers are unsigned and not notarized (macOS and Windows warn on the first launch). Only the Linux `.deb` and `.AppImage` were built and started by the developers; the macOS `.dmg`, the Windows NSIS setup and the manual GitHub workflow that builds them have never been run. There is no Windows `.msi` (the CalVer version does not fit the MSI rules) and no `.rpm`.
- Molecular surface generation (and the CUBE isosurface) is computed on the CPU (several times faster since 2026.10.4, on several threads in the desktop build); the browser build hides the surface during playback but builds one per frame for an animation export (the page is busy meanwhile).
- The SVG export draws ribbons and surfaces as flat-shaded polygons with a painter's algorithm: interpenetrating triangles can overlap wrongly and translucent triangles can show faint seams; more than 500,000 triangles are refused. The selection highlight and SSAO are not in the SVG.
- SSAO is off by default and leaves a faint grain at high strength; translucent surfaces and the selection glow neither cast nor receive occlusion.
- The PNG, SVG and animation exports, the CUBE view, SSAO and the smoke / UI tests were checked with a software renderer (llvmpipe) only; the appearance on a real GPU, the selection glow size setting and the OS file dialogs are not automatically checked. Changes to the settings file need a restart. The desktop MP4 needs `ffmpeg` (otherwise the PNG frames and the command are given).
- `.mol2` files are loaded as a single residue, and partial (non-integer) atomic charges are not preserved on structure export (mmCIF refuses them; PDB rounds them).

## [2026.10.6] - 2026-10-14

### Added
- The terminal can be typed into directly: click the terminal screen (the border turns cyan) and the keys go to the shell, so Tab completion, the up-arrow history, Ctrl+C / Ctrl+D, Home / End, PageUp / PageDown and full-screen programs such as `vim` and `less` work. Clicking elsewhere gives the keyboard back (Esc, Space and the other app shortcuts are off only while the terminal has it). The input line below the screen stays for input-method text such as Japanese.
- The terminal now follows the size of its pane: the rows and columns are computed from the screen size and sent to the shell (it was fixed at 24 x 80), so `ls`, `vim` and `stty size` match the window.
- New setting `terminal.font_size` (6 - 72 px, default 12) in `settings.yaml` for the terminal text size; the rows/columns sent to the shell, mouse selection and wheel scrolling follow it. Needs a restart.
- The terminal keeps a 1000-line history: scroll it with the mouse wheel or Shift+PageUp / Shift+PageDown; the toolbar shows how far up you are, and typing returns to the live screen. Full-screen programs (`less`, `vim`) have no history.
- The terminal screen supports mouse selection (drag to highlight), copy (Ctrl+Shift+C, Cmd+C on macOS, or the "Copy" button) and paste (Ctrl+Shift+V, Cmd+V, Shift+Insert, or the "Paste" button; bracketed paste is used when the program asks for it). Ctrl+C still interrupts.
- The terminal screen now draws cell background colors, reverse video and underline (the status line of `less`, `ls --color` highlights, `vim` selections), not only foreground colors.
- Right-click a file in the Project tree to choose how to open it ("Open with"): a structure file (`.pdb`, `.cif`, `.mmcif`, `.ent`, `.mol2`) can now be opened in the 3D viewer or the text editor. A left click still opens structure files in the 3D viewer directly (and still asks for `.xyz` and `.yaml`/`.yml`); the prompt bar now has a Cancel button.
- The text editor has a scrollbar: drag the thumb, click the track, or use the wheel and the keys as before. It can be turned off with `editor.scrollbar: false` in the settings file (default `true`, read at startup). The editor is wrapped in a scroll area and follows the cursor after keyboard moves and edits; the row of a cursor in wrapped long lines is estimated, so it can be about a line off there.

### Changed
- The "Open with" choice of a file in the Project tree (right-click) now opens as a small menu next to the mouse pointer instead of the bar at the top of the window; the bar remains for the left click on `.xyz` and `.yaml`/`.yml` files.

### Known Limitations
- The terminal was checked only with a software renderer under a virtual display and a scripted keyboard / mouse: the feel of real typing, the mouse wheel and selection, `vim`, the browser build, macOS and Windows are not verified. Its row / column count and the selection use an estimated cell size (0.6 x the font size wide, 1.3 x + 1 high), so with other fonts the highlight can drift by a cell on long lines; the text in a full-screen program is not selectable beyond what is shown, and input-method (Japanese) text must go through the input line below the screen.
- The text editor's scrollbar follows the cursor with an estimate of the wrapped rows (about one line of drift on very long wrapped lines); dragging it was not tried with a real GPU or in the browser.
- The Python Console and AI Chat panel tabs in the Bottom pane are not yet implemented ("Coming soon" placeholders).
- The browser build compiles, is served (by `backend` or any static host) and has the same features as the desktop one except the MP4 (frames are downloaded as a zip), but it has not been run in a browser by the developers (no WebGPU adapter on the development machine): the browser exports, the animation download, the dialogs, the backend connection and SSAO are untested at run time. It needs a physical GPU with WebGPU.
- `backend` is local only: it binds `127.0.0.1` and its access tokens are fixed (1 is the Host), so anyone who can reach it can open the terminal and read files under the workspace root. Do not expose it to a network; real authentication has to be designed first.
- The installers are unsigned and not notarized (macOS and Windows warn on the first launch). Only the Linux `.deb` and `.AppImage` were built and started by the developers; the macOS `.dmg`, the Windows NSIS setup and the manual GitHub workflow that builds them have never been run. There is no Windows `.msi` (the CalVer version does not fit the MSI rules) and no `.rpm`.
- Molecular surface generation (and the CUBE isosurface) is computed on the CPU (several times faster since 2026.10.4, on several threads in the desktop build); the browser build hides the surface during playback but builds one per frame for an animation export (the page is busy meanwhile).
- The SVG export draws ribbons and surfaces as flat-shaded polygons with a painter's algorithm: interpenetrating triangles can overlap wrongly and translucent triangles can show faint seams; more than 500,000 triangles are refused. The selection highlight and SSAO are not in the SVG.
- SSAO is off by default and leaves a faint grain at high strength; translucent surfaces and the selection glow neither cast nor receive occlusion.
- The PNG, SVG and animation exports, the CUBE view, SSAO and the smoke / UI tests were checked with a software renderer (llvmpipe) only; the appearance on a real GPU, the selection glow size setting and the OS file dialogs are not automatically checked. Changes to the settings file need a restart. The desktop MP4 needs `ffmpeg` (otherwise the PNG frames and the command are given).
- `.mol2` files are loaded as a single residue, and partial (non-integer) atomic charges are not preserved on structure export (mmCIF refuses them; PDB rounds them).

## [2026.10.5] - 2026-10-14

### Added
- Animation export in the browser build: File → "Export Animation..." now works there. The browser has no folder and no `ffmpeg`, so the PNG frames are kept in memory and downloaded as one `yui-animation-<version>.zip` (limit: 400 MB of frames; a hint is shown if it is exceeded); unzip it and run the `ffmpeg` command shown in the status bar to make a video. The desktop export is unchanged. Not yet run in a real browser (the same code path was run on the desktop renderer and the zip checked with `unzip -t`).
- Native installers (unsigned): `scripts/build-installers.sh` builds a Linux `.deb` and `.AppImage` (macOS `.dmg` and a Windows NSIS setup `.exe` on those systems) with cargo-packager, and the manual GitHub workflow "Release installers" builds all three. The packages hold the app (`yui-frontend`, "YUI") and `yui-backend`, with an app icon. They are not signed or notarized, so macOS and Windows warn on the first launch (see the README, "Installing"). A Windows `.msi` is not built (the CalVer version does not fit the MSI rules). Only the Linux packages were built and started by the developers.
- The web (browser) build can be released and served: `scripts/build-web.sh` makes the release bundle (`target/web/dist` and `yui-web-<version>.zip`, about 14 MB / 6 MB), and `YUI_WEB_ROOT=target/web/dist cargo run --release -p yui-backend` serves it together with the API at `http://127.0.0.1:8080/` from one process. The bundle also runs standalone on any static host (use `--public-url` for a sub-path). The backend is still local only (it binds `127.0.0.1`, and its fixed tokens give the terminal and file reads to anyone who can reach it), so it must not be exposed to a network yet.

### Changed
- The browser build now connects, by default, to the `/api/collab` of the server that served the page (it was `ws://<host>:8080/api/collab`), which is what a served release bundle needs; `trunk serve` forwards `/api/` and the collaboration WebSocket to the backend (`frontend/Trunk.toml`), so `?collab=` is no longer needed in the development workflow either (it still selects another backend).

### Known Limitations
- The Python Console and AI Chat panel tabs in the Bottom pane are not yet implemented ("Coming soon" placeholders).
- The browser build compiles, is served (by `backend` or any static host) and has the same features as the desktop one except the MP4 (frames are downloaded as a zip), but it has not been run in a browser by the developers (no WebGPU adapter on the development machine): the browser exports, the animation download, the dialogs, the backend connection and SSAO are untested at run time. It needs a physical GPU with WebGPU.
- `backend` is local only: it binds `127.0.0.1` and its access tokens are fixed (1 is the Host), so anyone who can reach it can open the terminal and read files under the workspace root. Do not expose it to a network; real authentication has to be designed first.
- The installers are unsigned and not notarized (macOS and Windows warn on the first launch). Only the Linux `.deb` and `.AppImage` were built and started by the developers; the macOS `.dmg`, the Windows NSIS setup and the manual GitHub workflow that builds them have never been run. There is no Windows `.msi` (the CalVer version does not fit the MSI rules) and no `.rpm`.
- Molecular surface generation (and the CUBE isosurface) is computed on the CPU (several times faster since 2026.10.4, on several threads in the desktop build); the browser build hides the surface during playback but builds one per frame for an animation export (the page is busy meanwhile).
- The SVG export draws ribbons and surfaces as flat-shaded polygons with a painter's algorithm: interpenetrating triangles can overlap wrongly and translucent triangles can show faint seams; more than 500,000 triangles are refused. The selection highlight and SSAO are not in the SVG.
- SSAO is off by default and leaves a faint grain at high strength; translucent surfaces and the selection glow neither cast nor receive occlusion.
- The PNG, SVG and animation exports, the CUBE view, SSAO and the smoke / UI tests were checked with a software renderer (llvmpipe) only; the appearance on a real GPU, the selection glow size setting and the OS file dialogs are not automatically checked. Changes to the settings file need a restart. The desktop MP4 needs `ffmpeg` (otherwise the PNG frames and the command are given).
- `.mol2` files are loaded as a single residue, and partial (non-integer) atomic charges are not preserved on structure export (mmCIF refuses them; PDB rounds them).

## [2026.10.4] - 2026-10-14

### Added
- Ambient occlusion (SSAO), off by default: set `render: ssao_strength` (0 - 1; 0.6 - 1 looks good) in the settings file to darken crevices and the gaps between atoms, which gives the 3D view and the PNG export / animation frames more depth (`ssao_radius` sets how far in angstroms it looks, default 3). It only darkens, does not touch the background, and is not in the SVG export. Tested with the software renderer; not yet seen on a real GPU or in the browser build.
- Even more drawing settings in `render:`: `bond_color` (the Ball & Stick sticks; also in the SVG export), `ribbon_helix_width`, `ribbon_helix_thickness`, `ribbon_sheet_width`, `ribbon_sheet_thickness`, `ribbon_loop_radius` (the cartoon) and `selection_size` (the selection glow / halo). Same rules as the other `render:` items.
- The CUBE card has an Opacity slider (10–100 %). The orbital (data set), the isolevel, the opacity and the lobe colors are part of the workspace state (`cube_view` in the workspace YAML), so each CUBE keeps its own view: they are saved with the workspace and used again when the workspace reopens the same CUBE file (the same orbital at the same level), and reset when another CUBE is opened or the CUBE is closed.
- More drawing settings in `render:`: `interaction_dash` and `interaction_gap` (the dash pattern of the interaction lines; gap 0 gives a solid line), and `surface_grid_spacing` and `surface_probe_radius` (the molecular surface). Same rules as the other `render:` items (range-checked, read at startup).
- Drawing sizes can be set in the settings file (`render:` in `settings.yaml`, read at startup): `ball_scale`, `stick_radius`, `licorice_radius`, `line_radius` and `interaction_radius` (the last also applies to the SVG export). Out-of-range or wrongly typed values fall back to the default for that item with a warning. `File → Open Settings File` shows the keys in its template.

### Changed
- The SVG export now includes the ribbons (Cartoon), the molecular surfaces and the CUBE isosurfaces, as flat-shaded polygons that follow the same representations, colors and opacity as the screen (a back-face cut keeps opaque surfaces about half the size: a 1HLS surface is about 2 MB). It is a painter's algorithm, so interpenetrating triangles can overlap wrongly and translucent triangles can show faint seams; an export with more than 500,000 triangles is refused with a hint (hide the surface, `render: surface_grid_spacing`, or use PNG).
- The image and animation exports' default shape is now **3D pane** (it was "Window"): the canvas has the aspect ratio of the 3D viewport as it is drawn on screen, not of the whole window (which also holds the side panes and the bottom panel). Before the viewport has been drawn, the window's aspect is used. The fixed ratios and Custom are unchanged.
- Molecular surfaces (and CUBE isosurfaces) are built several times faster: about 2.6 s → 0.3 s for a 50,000-atom random cloud on the development machine (release build). The density is computed with per-axis tables instead of one `exp` per grid point and, in the desktop build, on several threads; the surface mesh is unchanged. The browser build gains from the first change only (one thread).

### Fixed
- The non-covalent interaction lines are thicker (radius 0.05 Å → 0.08 Å, still thinner than the 0.12 Å covalent sticks) so that they are easier to see at the default size; the SVG export uses the same width.
- "Hide" (tree and viewport right-click menus) now also hides the cartoon of the hidden residues. It used to hide only the balls and sticks and leave the ribbon. A residue's cartoon is hidden when its CA atom is hidden; the ribbon is cut there instead of bridging the gap. Hiding only a side-chain atom leaves the cartoon.
- The Project tab's file list no longer shows missing-glyph boxes: the 📁 / 📄 emoji (not in the bundled fonts) were removed (folders are shown as `name/`). Starting the desktop app without a backend no longer shows a "Backend unreachable" error in the Project tab (running standalone is normal; the browser build still shows it, because it needs the backend).

### Known Limitations
- The Python Console and AI Chat panel tabs in the Bottom pane are not yet implemented ("Coming soon" placeholders).
- Browser/WASM deployment requires a physical GPU with WebGPU support. The browser build compiles and is served, but it has not been run in a browser by the developers (no WebGPU adapter on the development machine): the browser exports, dialogs, backend connection and SSAO are untested at run time. Animation export is desktop only.
- Molecular surface generation (and the CUBE isosurface) is still computed on the CPU, several times faster than before and on several threads in the desktop build; a GPU version (Phase 5c-iii) was set aside because it would only pay off if the density grid moved to the GPU too. The browser build hides the surface during playback.
- The SVG export draws ribbons and surfaces as flat-shaded polygons with a painter's algorithm: interpenetrating triangles can overlap wrongly and translucent triangles can show faint seams; an export with more than 500,000 triangles is refused. The selection highlight and SSAO are not in the SVG.
- SSAO is off by default (`render: ssao_strength`) and leaves a faint grain at high strength; translucent surfaces and the selection glow neither cast nor receive occlusion.
- The PNG, SVG and animation exports, the CUBE view, SSAO and the smoke / UI tests were checked with a software renderer (llvmpipe) only; the appearance on a real GPU, the selection glow size setting and the OS file dialogs are not automatically checked. Changes to the settings file need a restart. The MP4 needs `ffmpeg` (otherwise the PNG frames and the command are given).
- `.mol2` files are loaded as a single residue, and partial (non-integer) atomic charges are not preserved on structure export (mmCIF refuses them; PDB rounds them).

## [2026.10.3] - 2026-10-13

### Fixed
- The tabs of a narrow pane now wrap onto the next line. With the default window the left pane is narrow and the "Project" tab (the way to open files from the backend or a folder) was clipped and could not be clicked.
- Cancelling the save dialog of an image export now shows "Export cancelled." instead of leaving a progress message, and the PNG export no longer says "Rendering the image..." before the file is chosen.
- Choosing a View-menu command such as "Spin", "Rock" or a playback command now closes the menu. It used to stay open (only the commands that go through the old menu actions closed it).

### Added
- Canvas size and framing for the image and animation exports. The SVG export now opens a dialog (like the PNG and animation ones) with the Shape (Window, 1:1, 4:3, 3:2, 16:9, or Custom with a typed width x height) and the Framing: "As on screen" or "Fit all" (the camera keeps its direction and moves so that the whole structure fits the canvas, whatever its shape).
- The SVG export (File → "Export Image (SVG)...") now includes the non-covalent interaction lines (hydrogen bonds, salt bridges, CH-π, disulfides) as dashed lines in their colors, following the Interactions cards. Ribbons and molecular surfaces are still not in the SVG.
- File → "Export Animation..." (desktop build): writes numbered PNG frames to a folder, and an MP4 with `ffmpeg` if it is installed (otherwise the command to run is shown). Motions: Spin (one loopable turn), Rock (one swing), or Frames (every model of a multi-model structure / every frame of an XYZ trajectory); choose the frame rate, length, width and smoothing. A bar above the panes shows the progress and a Cancel button. Molecular surfaces are included in every motion; with Frames each frame gets the surface of its own model or trajectory frame (slower, one surface build per frame).
- File → "Export Image (PNG)..." saves the current view as a PNG (a dialog sets the width 1000–4000 px, the smoothing Off / 2x / 3x, and a scene-color or transparent background; the height follows the window's aspect), drawn off-screen with the same renderer, so surfaces, CUBE isosurfaces, the cartoon and interaction lines are included (the SVG export has atoms, bonds and labels only). Drawn larger and averaged down in linear light for smooth edges; no selection highlight. Native build tested with the software renderer; the browser build is untested.
- File → "Open CUBE (Isosurface)..." / "Close CUBE": shows the positive (blue) and negative (red) isosurfaces of a Gaussian CUBE file (electron density, molecular orbital) as translucent surfaces. A card at the top of the Right pane shows the file, an orbital selector (CUBE files with several orbitals) and an isolevel slider (log scale; the default is 0.05 for a density and 0.02 for an orbital). Color swatches for the positive and negative lobes. Oblique grids are resampled. If no structure is open, the CUBE's atoms are shown. The file path is saved in the workspace YAML (`data_paths.cube_path`, native build) and reopened when the workspace is opened. `core/tests/data/sphere.cube` and `orbitals.cube` are small test files.

### Changed
- `proteindf-bridge` is now pinned to the release tag `2026.10.7` (was `2026.10.1`), which has the CUBE reader.
- While a multi-model structure or an XYZ trajectory plays (or the frame slider is dragged), the molecular surface now follows the shown frame instead of disappearing. It is built in the background and lags the atoms by about one build time; frames are skipped while a build is running. The browser build still hides the surface during playback.

### Added
- File → "Export Image (SVG)..." writes the current view as an SVG: the visible atoms (circles, sized by perspective), bonds (lines) and annotations, depth-sorted far to near, in the viewport's background color, following the visible representations and the displayed model. Ribbons (Cartoon), surfaces and interaction lines are not included. Works in the native and browser builds (the browser downloads the file). The old `export_to_svg` (all atoms, no bonds, fixed radius factor) was replaced by `render_svg`.
- An Opacity slider (0–100 %) on Surface and Interaction cards in the Representation Inspector. The surface or the interaction lines become see-through as you drag it; the surface mesh is not rebuilt. Other kinds do not use opacity, so they show no slider.
- Spacefill, Licorice and Line are now drawn differently from Ball & Stick (they only looked the same before, and Line drew nothing). Spacefill is a sphere of the full van der Waals radius with no sticks, Licorice is thick sticks colored by atom, and Line is thin lines colored by atom. Each representation is drawn on its own with its own selection and colors, and they are overlaid (a Spacefill and a Ball & Stick on the same atoms show the large spheres). Clicking and the selection highlight follow the new sizes.
- Playing a multi-frame XYZ trajectory for the structure that is shown: File → "Open Trajectory (XYZ)..." (or open an `.xyz` from the Project tab and choose "Trajectory (XYZ)"), File → "Close Trajectory". The frames play with the same control bar, commands and keys as the models of a multi-model structure; the atoms (ball-and-stick, cartoon, interaction lines, picking) follow each frame. Only the needed frame is read, so large files do not use memory per frame. The atom count and order must match the structure.
- Camera animation: View → "Spin" (continuous rotation; "Spin Speed" cycles 10 / 20 / 40 / 80 deg/s), "Rock" (swings ±30°), and "Zoom to Selection". Reset Camera and Zoom to Selection now move the camera smoothly (about 0.4 s) instead of jumping. A mouse drag or the wheel stops the animation. Animations are per user and not shared.
- Playback of multi-model structures (e.g. the 20 models of `1HLS.cif`): a control bar below the 3D viewport (first / previous / Play-Pause / next / last, a frame slider, speed 1–30 fps, loop mode Once / Repeat / Ping-pong), View → "Playback", and the keys Space (play / pause) and ← / → (one frame). Playback is per user and not shared. While playing, the molecular surface is not drawn and is rebuilt when playback pauses, and the selection is cleared whenever the displayed model changes.
- Pan and zoom by dragging with a modifier key, as in Chimera: Shift+left-drag pans the view, Ctrl+left-drag (Cmd on macOS) zooms (down = zoom out, up = zoom in). Dragging with no modifier still rotates, and the wheel still zooms.
- Selecting atoms in the 3D viewport. A left click (without dragging) selects the atom under the pointer; Shift+click adds, Ctrl+click (Cmd on macOS) toggles, a double click selects the whole residue, a click on empty space or the Esc key clears. Selected atoms are highlighted with a glow (default) or a halo, also when only the cartoon is shown and in both viewports; "View" -> "Selection Highlight:" switches Glow / Halo / Off and the choice is saved per user (`selection_highlight:` in `settings.yaml`, or `localStorage`). "View" -> "Save Selection as Named Selection" keeps a selection. The selection is per user: it is not saved in the workspace and not shared over collaboration.
- Dark and light UI themes. "View" -> "Theme:" switches between them at once and remembers the choice per user (`theme:` in `settings.yaml`, or `localStorage` in the browser; comments and other settings in the file are kept). The 3D viewport background color, the molecule colors and the terminal's ANSI colors are not affected by the theme, and the terminal stays dark. The dark theme is the default and looks as before, except that some similar gray texts were merged into five tones (a few percent apart). Standard widgets now follow the chosen theme instead of the operating system's color scheme.
- A second 3D viewport with its own camera. "View" -> "Split Editor Right" now also shows a "3D Viewport 2" tab on the right (before the file tabs); both viewports draw the same structure and differ only in their view. The second viewport starts with a copy of the first one's view. "View" -> "Link Viewport Cameras" (off by default) makes the two views move together. Reset Camera and opening a structure fit both viewports.
- Per-user settings file (`settings.yaml` in the OS config folder, or `localStorage` in the browser; read at startup). The first setting is the editor/terminal font: `editor.font_family` (the bundled JetBrains Mono, or any installed font) and `editor.font_size` (6 - 72). A broken or invalid setting never stops the app: only that item falls back to its default and a warning is shown in the status bar. File → "Open Settings File" creates and opens the file (native build).

### Fixed
- Balls and sticks were drawn inside out: the triangle winding of the sphere and cylinder meshes faced inward, so back-face culling removed the front surface and the far inner wall was shown. Sticks therefore looked as if they ran into the balls. The winding is fixed, so the lighting is now also on the visible (front) surface.
- Sticks no longer poke out of the balls toward the viewer: mid-distance sphere impostors now write the depth of the sphere surface instead of the depth of their center plane.
- The menus no longer look unstyled: they have a card background and border, items are transparent with a lighter background on hover (no purple fill), disabled items are dimmed, separators are thin lines, and the open menu button is shown by its background. The app name next to the menu bar was removed (it stays in the window title and the About dialog), and each dropdown opens directly under its button. The About dialog is shown on a dimmed background.
- Menus no longer run off the window: a right-click menu near the right or bottom edge opens to the left of or above the cursor, and a menu taller than the window is limited to the window height and scrolls.

### Changed
- Right-click "Select Atom" / "Select Residue" and clicking a node in the Structure tree now change the selection (highlighted in the viewport) instead of adding a yellow Representation, so the Representation list no longer grows with every selection. Workspaces that already contain such yellow Representations still load them as ordinary Representations.
- Internal: the File / View / Help menus and the three right-click menus are now built from a menu model and a command registry (`frontend/src/menu/`) instead of buttons written in code, with per-location contribution points that a future plugin mechanism can use. The look and behavior of the menus are unchanged.
- The camera is fitted to the structure (centered and fully visible, using the displayed model) when a structure or workspace is opened and on View → Reset Camera. Switching the displayed model does not move the camera. The zoom-out limit and the far clipping distance now scale so that large structures are not clipped.
- GitHub Actions CI no longer runs on every push and pull request; it runs only when started by hand. Routine verification is done locally with `scripts/check.sh --trunk`, which runs the same checks.
- Text files open as tabs next to the 3D viewport (VSCode style: `3D Viewport | a.yaml* | b.txt ×`) instead of in a single editor in the Bottom pane. Opening a file that is already open selects its tab; closing an unsaved file asks Save / Don't Save / Cancel; Ctrl+S (Cmd+S on macOS) saves. File → "New Text File" and "Open Text File..." were added. "View" -> "Split Editor Right" shows the file tabs to the right of the viewport, with a draggable border; the split is saved in the workspace (open files are per user and are not saved). Workspaces saved with the editor in the Bottom pane still open. The editor and the terminal now use a bundled monospace font (JetBrains Mono, SIL OFL), which also makes the terminal monospace in the browser. Editing results and errors are shown in the status bar. Quitting (File → Quit or closing the window) with unsaved files asks Save All / Don't Save / Cancel.
- The app now starts blank instead of loading a synthetic test structure. The Center viewport shows a hint on how to open a file until a structure is loaded.
- When a structure is opened and there are no representations yet, the default cartoon (secondary-structure coloring) and ball-and-stick (element coloring) representations are added. Previously, a structure opened after File → New was drawn as ball-and-stick only. Existing representations are left unchanged.

### Fixed
- The first viewport's camera was not connected to `AppState.camera`: moving the view did not change the saved camera, and a camera arriving from collaboration or the backend's initial state was not shown. They are now connected, so saving a workspace keeps the view (a workspace with a moved camera is no longer re-fitted when opened), and the view is no longer lost when the Center tab is switched to a text file and back.
- Status bar: operation result messages (structure loading, file save/open, export, selection, etc.) were stored but never rendered anywhere. A one-line status bar is now shown at the bottom of the window with the latest message (warnings in yellow, errors in red), the atom/bond counts of the loaded structure, the displayed model of a multi-model structure, a surface-generation indicator, and the collaboration connection state.
- A surface mesh still being generated could reappear after its Surface representation had been removed; such stale results are now discarded.

### Known Limitations
- The Python Console and AI Chat panel tabs in the Bottom pane are not yet implemented ("Coming soon" placeholders).
- Browser/WASM deployment requires a physical GPU with WebGPU support. The browser build compiles and is served, but it has not been run in a browser by the developers (no WebGPU adapter on the development machine); the browser exports, dialogs and backend connection are untested at run time.
- Molecular surface generation is computed on the CPU (GPU generation is planned for Phase 5c-iii); the CUBE isosurface is also CPU-built. The browser build hides the surface during playback.
- The SVG export has atoms, bonds, labels and interaction lines only; ribbons (Cartoon) and surfaces are not included. SVG and PNG use the window's aspect ratio by default (a custom canvas size is available).
- The PNG, SVG and animation exports, the CUBE view and the smoke / UI tests were checked with a software renderer (llvmpipe) only; the appearance on a real GPU and the OS file dialogs are not automatically checked. The MP4 needs `ffmpeg` (otherwise the PNG frames and the command are given).
- `.mol2` files are loaded as a single residue, and partial (non-integer) atomic charges are not preserved on structure export (mmCIF refuses them; PDB rounds them).

## [2026.10.2] - 2026-10-05

### Added
- **Tripos Mol2 Structure File Support (`.mol2`):**
  - Added support for loading `.mol2` files via `proteindf_bridge::format::SimpleMol2`.
  - Normalized flat MOL2 structures into YUI's standard 4-tier hierarchy (`model_1` → `A` → `1 <RES>` → atoms), preserving explicit bonds from `@<TRIPOS>BOND` sections and avoiding redundant heuristic bond overwrites.
  - Added `sample.mol2` test dataset to `core/tests/data/` with attribution.
- **Asynchronous File Selection Dialogs for File Menu:**
  - File menu Open, Save, and Save As now use cross-platform asynchronous file dialogs (`rfd::AsyncFileDialog`) on both native and WASM platforms.
  - Native platforms record the chosen workspace path on Open and Save As, allowing Save to directly overwrite the active file. On WASM, operations run asynchronously via browser upload/download dialogs without blocking the UI.
  - State changes upon loading run through full update pathways (`update_resolved_interactions`, `update_model_atom_ranges`, and `trigger_surface_rebuild`).
- **Structure File Export (PDB / mmCIF):**
  - Added "Export Structure..." to the File menu. The format is chosen from the file extension (`.pdb` / `.cif`) in the save dialog; works on native and WASM.
  - Export delegates to `proteindf-bridge` (`Pdb`, `SimpleMmcif::write_structure`). All models of a multi-model structure are exported, regardless of the displayed model.
  - Charges: mmCIF stores integer formal charges, so structures with non-integer partial charges (e.g. from `.mol2`) cannot be exported to mmCIF and a clear error is shown. PDB output also stores integer charges, so partial charges are rounded.

### Changed
- Switched `proteindf-bridge` dependency from untagged Git revision (`997ad24`) to release tag `2026.10.1` (`core/Cargo.toml` native/wasm32, `frontend/Cargo.toml` dev-dependencies).
- Removed fixed working-directory `state.yaml` read/write and sample peptide fallback from File > Open/Save/Save As.

### Known Limitations
- The Python Console and AI Chat panel tabs in the Bottom pane are not yet implemented ("Coming soon" placeholders).
- Browser/WASM deployment requires a physical GPU with WebGPU support.
- Molecular surface generation is computed on the CPU; very large structures or dense grids may take noticeable time before the mesh appears (GPU compute-shader generation is planned for Phase 5c-iii).
- The Representation Inspector has no control for opacity, so molecular surfaces are always drawn opaque (default opacity 1.0).
- `.mol2` files are loaded as a single residue, because `proteindf-bridge` does not yet keep MOL2 substructure information.
- Partial (non-integer) atomic charges are not preserved on structure export: mmCIF export refuses them with an error, and PDB export rounds them to integers.

## [2026.10.1] - 2026-10-04

### Added
- **Non-Covalent Interaction Detection & Rendering:**
  - Automated detection and storage of non-covalent interactions (hydrogen bonds, salt bridges, CH-π, and disulfide bonds) upon structure loading into `AppState.interactions`.
  - 3D viewport dashed-cylinder rendering pipeline with uniform dash intervals and semantic default coloring (hydrogen bonds: yellow, salt bridges: orange, CH-π: cyan, disulfide bonds: magenta).
  - Representation Inspector support for the `Interactions` kind, featuring per-interaction-type visibility toggles (H-bond, Salt bridge, CH-π, Disulfide).
  - Python bindings (`yui-py`) updated to support `RepresentationKind::Interaction`.
- **Molecular Surface Generation & Rendering:**
  - CPU reference implementation of molecular surface mesh generation based on Gaussian density fields (QuickSurf formulation) and Marching Cubes isosurface extraction, producing closed 2-manifold meshes.
  - Dedicated wgpu surface rendering pipeline supporting double-sided rendering (`cull_mode: None`), opaque and translucent alpha-blended passes, and vertex coloring via existing color schemes (Uniform, Element, Chain, Residue, SecondaryStructure).
  - Asynchronous background mesh generation on native platforms to prevent UI blocking, with generation tracking to safely handle stale computations.
  - Representation Inspector updated to fully enable `Surface` representations.
- **Multi-Model Structure Display & Model Switching:**
  - Support for multi-model structures (e.g. NMR ensembles) with model filtering controlled by `AppState.active_model`.
  - Automatic activation of the first model (e.g. `model_1`) upon loading multi-model structures.
  - Structure Tree navigation bar with previous/next model stepping and an "All" models display toggle.
  - Model-aware filtering integrated across all rendering pipelines (ball-and-stick, ribbon/cartoon, interaction lines, and molecular surface).
- **Continuous Integration (CI):**
  - GitHub Actions CI workflow executing `scripts/check.sh` (native and wasm32 compilation checks, unit/integration test suite, code formatting, clippy lints, documentation synchronization) and Trunk WASM bundle verification.

### Changed
- **AppState MessagePack Serialization Format:**
  - `AppState` MessagePack serialization now writes in named-field map format (`rmp_serde::to_vec_named`) for resilience against optional and future fields.
  - Backward compatibility is fully preserved: legacy array-format (tuple-format) data from version `2026.10.0` and earlier can still be read seamlessly.
- **Linting & Code Quality Enforcement:**
  - Integrated `cargo fmt` and `cargo clippy` (`-D warnings` across both native and wasm32 targets) into `scripts/check.sh` as default completion criteria.
- **Frontend Architecture Refactoring:**
  - Decomposed `viewport.rs` into dedicated submodules (`gpu_types`, `camera`, `mesh`, `scene`, `pipeline`, `primitive`, and WGSL shaders) and refactored `main.rs` into application and network stream modules without changing user-facing behavior.

### Known Limitations
- The Python Console and AI Chat panel tabs in the Bottom pane are not yet implemented ("Coming soon" placeholders).
- Browser/WASM deployment requires a physical GPU with WebGPU support.
- Molecular surface generation is currently computed on the CPU; generating surfaces for very large structures or dense grids may require noticeable computation time before the mesh appears (GPU compute-shader generation is planned for Phase 5c-iii).
- The `proteindf-bridge` dependency is pinned to an untagged Git revision (`997ad2478e8681ce2d55ff381feaa759b3395490`).

## [2026.10.0] - 2026-10-03

### Initial Release

Initial release of 結 (YUI), establishing the core architecture, 3D rendering pipeline, collaboration backend, WebAssembly build, and `proteindf-bridge` integration.

#### State Management & Serialization
- Core `AppState` supporting lossless round-trip conversions between Rust structs, MessagePack (uncompressed and zstd-compressed), and YAML.
- Hierarchical structure representation with `AtomGroup` and structured representations (`Representation`).
- Spatial partitioning via `AtomBvh` (Bounding Volume Hierarchy) for efficient spatial queries and bond calculations.

#### Rendering & Viewport
- Ball-and-stick and ribbon/cartoon rendering pipelines implemented with `wgpu` and `iced::widget::shader`.
- Catmull-Rom spline interpolation with secondary-structure cross-section profiles (helix, sheet, loop) and vertex coloring.
- GPU-driven compute-shader frustum culling (`draw_indexed_indirect`) and two-tier staged Level of Detail (LOD) with impostor billboard quads.
- Interactive camera controls (orbit, zoom) with HiDPI display support.

#### User Interface
- VSCode-style 4-pane docking layout (`iced::widget::pane_grid`) with resizable dividers and generalized multi-view tab switching.
- In-window menu bar (File, View, Help) with state load/save and About dialog.
- 3D viewport ray-cast picking with right-click context menu for atom and residue selection and styling.
- Interactive Structure Tree explorer and Representation Inspector widgets.
- Named Selections management to save and reuse named Selector definitions across representations and sessions.
- Configurable viewport background color selectable from View menu presets and preserved across workspace saves and collaboration sync.
- Built-in generic text editor (`iced::widget::text_editor`) for arbitrary file viewing and editing.
- Unified project directory tree browsing local files and backend workspaces.

#### Scripting & API
- Native Python bindings (`yui-py` / `import yui`) via PyO3 exposing `AppState`, `Selector`, `Representation`, and `AtomGroup`.
- Fluent method-chaining builder API (`app.select(...).show(...).color_by(...)`) and typed selector constructors.

#### Backend & Collaboration
- Axum-based server providing REST endpoints (`/api/state`, `/api/structure`, `/api/fs/list`, `/api/fs/read`).
- WebSocket delta-synchronization (`/api/collab`) for real-time collaboration with role-based access control (`RoleResolver`).
- Real-time log streaming (`/api/logs`) via `notify` with bounded buffer and filtering.
- Full-duplex terminal emulation (`/api/terminal`) via `portable-pty` and `vt100`, gated to the Host role.

#### Web & WebAssembly (WASM)
- Browser frontend build via Trunk with WebAssembly (`wasm32-unknown-unknown`) and WebGPU rendering.
- Cross-platform WebSocket transport (`gloo-net`) enabling collaboration, log tailing, and terminal access in the browser.

#### ProteinDF Bridge Integration
- Real `proteindf-bridge` integration replacing temporary local stand-ins.
- Native PDB and mmCIF structure parsing with DSSP secondary structure estimation and bond topology resolution via CCD templates and geometric heuristics (with double-bond detection via AtomGroup::setup()).
- Portable HTTPS Git dependency with target-gated decompression features (`zstd` for native, `ruzstd` for wasm32).

### Known Limitations
- The Python Console and AI Chat panel tabs in the Bottom pane are not yet implemented ("Coming soon" placeholders).
- Molecular surface rendering (`RepresentationKind::Surface`) is not yet rendered.
- Browser/WASM deployment requires a physical GPU with WebGPU support.
- Multi-model structures (e.g. NMR ensembles) currently load all models superimposed.
- The `proteindf-bridge` dependency is pinned to an untagged Git revision (`997ad2478e8681ce2d55ff381feaa759b3395490`).
