# FocusLock

A macOS menu bar app (Rust, no Xcode) that runs focus sessions. During a
session you can switch freely between *allowed* apps. Anything else gets hidden,
and Chrome tabs on non-allowed sites get redirected to a "blocked" page.

## Features

- **Sessions:** "Start 25 min" / "Start 50 min". The menu bar shows `🔒 24:59`, and you
  get a notification when the session ends.
- **App allowlist:** a non-allowed app that comes to the front is hidden, and the
  last allowed app comes back. FocusLock itself, Finder, Spotlight, System Settings,
  and login/security/permission dialogs are always allowed.
- **Chrome URL checks:** while Chrome is frontmost, the active tab is checked every
  1.5 s. A non-allowed site is replaced with a local page showing the time left.
- **Escape hatch:** "I need something else…" asks why. A reason of at least 10
  characters grants a 2-minute pass (`🎟 01:45 · 🔒 20:10`), up to 3 per session.
- **Quit during a session** asks for a reason too. "Stop session" is the
  no-questions emergency exit (it's logged).
- **The FocusLock window** (🎯 → "Today's stats", "Settings…" or "Welcome & help"):
  - **Today:** focus time, sessions, blocked attempts, passes, a 7-day chart, top
    distractions and today's activity.
  - **Settings:** tick allowed apps from your installed apps (with icons and search),
    edit allowed websites, pass rules, Chrome permission status, and a Dock icon toggle.
    Saved to `config.toml`. Changes made during a session apply to the next one.
  - **Welcome:** opens on first launch and explains that FocusLock lives in the menu bar.

## Build & run

Requirements: macOS 14+, Rust (`rustup`), and Xcode Command Line Tools
(`xcode-select --install`). Full Xcode is not needed.

```sh
cargo run                       # dev run (permissions get attributed to your terminal)
FOCUSLOCK_FAST=1 cargo run      # test mode: 1 "minute" = 5 s (25 min ≈ 2 min, pass = 10 s)
cargo test                      # unit tests (timer, domain matching, stats, config)
```

### Building the .app

```sh
scripts/bundle.sh               # -> target/FocusLock.app (release build, ad-hoc signed)
open target/FocusLock.app
```

The script writes `Contents/Info.plist` (with `LSUIElement=true` and
`NSAppleEventsUsageDescription`), copies the binary to `Contents/MacOS/FocusLock`,
and signs it with `codesign -s -`. Copy it to `/Applications` and add it under
System Settings → General → Login Items to start it at login.

> Every rebuild changes the ad-hoc signature, so macOS may ask for Automation
> permission again after rebuilding.

### Packaging for distribution

```sh
scripts/package.sh
```

Produces (filenames are the same for every release; the version is inside):

| File | What |
|---|---|
| `dist/FocusLock.dmg` | drag-to-install disk image (volume name "FocusLock <version>") |
| `dist/FocusLock.zip` | the same app, zipped |
| `dist/checksums.txt` | SHA-256; verify with `cd dist && shasum -a 256 -c checksums.txt` |
| `dist/latest.json` | version, checksums, sizes (for the website) |
| `dist/install.sh` | one-line installer: `curl -fsSL <url>/install.sh \| bash` |

Bake your download location into the installer (where `FocusLock.zip` and
`checksums.txt` will be hosted):

```sh
FOCUSLOCK_BASE_URL=https://<your-host>/download scripts/package.sh
```

The version comes from `Cargo.toml`. The app icon is generated from
`assets/icon.svg` (`scripts/make-icon.sh`, re-run automatically when the SVG changes).

**The app is ad-hoc signed, not notarized.** After downloading, macOS will refuse
the first launch ("Apple could not verify…"). This is expected.

### Installing with the one-line installer (no warning)

```sh
curl -fsSL https://<your-site>/install.sh | bash
```

It downloads `FocusLock.zip`, checks its SHA-256 against `checksums.txt` and its
code signature, installs to `/Applications` (or `~/Applications` if that isn't
writable), and launches it. Files downloaded with curl don't get macOS's
quarantine flag, so there's no "could not verify" dialog.
For testing: `FOCUSLOCK_INSTALL_DIR=/some/dir FOCUSLOCK_NO_LAUNCH=1`.

### Installing from the DMG

1. Open `FocusLock.dmg` and drag **FocusLock** onto **Applications**.
2. Open FocusLock. macOS blocks it ("FocusLock" Not Opened). Click **Done**, which
   only closes the dialog. Then go to **System Settings →
   Privacy & Security**, scroll down and click **Open Anyway** next to FocusLock.
   (On macOS 15 and later, right-click → Open no longer bypasses this.)
   Or, in Terminal:
   ```sh
   xattr -dr com.apple.quarantine /Applications/FocusLock.app
   ```
3. On the first session that checks Chrome, allow Automation when asked.

## Permissions

- **Automation (Chrome):** the first time a session checks Chrome, macOS asks
  *"FocusLock wants to control Google Chrome"*. Click **OK**. If you clicked "Don't
  Allow", turn it back on in **System Settings → Privacy & Security → Automation →
  FocusLock → Google Chrome**. (With `cargo run`, the entry is under your terminal
  app instead.) To reset the decision: `tccutil reset AppleEvents dev.focuslock.FocusLock`.
- **Notifications:** sent via `osascript`, so they show up as **Script Editor**.
  If you don't see any, enable them under System Settings → Notifications → Script Editor.
- **Hiding other apps** needs no permission.

## Configuration

`~/.config/focuslock/config.toml` is created on first run. Edit it in
🎯 → Settings…, or by hand. It's re-read whenever a session starts.

```toml
allowed_apps = ["com.microsoft.VSCode", "net.sourceforge.skim-app.skim",
                "com.apple.Terminal", "com.google.Chrome"]
allowed_domains = ["docs.rs", "doc.rust-lang.org", "github.com",
                   "stackoverflow.com", "localhost"]   # subdomains included
max_passes = 3
pass_minutes = 2
min_reason_chars = 10
chrome_poll_ms = 1500
show_dock_icon = false
```

Only `http(s)` pages are checked. `chrome://` pages and `file://` URLs (local PDFs,
the blocked page) are always allowed.

### Finding an app's bundle ID

```sh
osascript -e 'id of app "Visual Studio Code"'   # com.microsoft.VSCode
```

## Files

| Path | What |
|---|---|
| `~/.config/focuslock/config.toml` | config |
| `~/.local/share/focuslock/events.jsonl` | event log, one JSON object per line |
| `~/.local/share/focuslock/errors.log` | errors (e.g. Chrome permission denied) |
| `~/.local/share/focuslock/blocked.html` | page shown for blocked sites |
| `~/.local/share/focuslock/.welcome-shown` | delete it to see the Welcome window again |

Event log example:
```json
{"ts":"2026-11-19T14:02:11+01:00","event":"blocked_app","bundle_id":"com.apple.Music"}
```
Events: `session_start {minutes}`, `session_end {planned_minutes, focused_minutes, how}`
(`how` = completed / stopped / quit), `blocked_app {bundle_id}`, `blocked_url {domain}`,
`pass_granted {reason}`, `quit_with_reason {reason}`.

## Architecture

```
main thread: tao event loop (= AppKit run loop)
  └─ App  ← the only owner of state
       ▲  AppEvent via EventLoopProxy
       ├─ menu clicks                    (muda MenuEvent handler)
       ├─ NSWorkspace "app activated"    (block on the main queue, forwards only)
       ├─ FocusLock window (web view)    (ipc.postMessage → AppEvent::Ui)
       └─ Chrome poller thread           (osascript; only polls while told to)
              ▲ chrome::Cmd via mpsc (SetActive / SetInterval / Redirect)
```

- During a session the loop wakes about once a second to redraw the countdown. When
  idle it sleeps with `ControlFlow::Wait`.
- **Bounce-back:** `hide()` the offending app, then `activateWithOptions` the last
  allowed app. macOS 14's cooperative activation can ignore that, so 600 ms later we
  check, and if needed fall back to `open -b <bundle id>`.
- **Chrome is never launched by us:** polling is only on while blocking is active
  and Chrome is frontmost, and every script also checks `is running` and
  `frontmost` itself before touching Chrome.
- **Dialogs** (`NSAlert`) run modally on the main thread. The menu bar countdown
  pauses while one is open and catches up immediately after.
- `session.rs` contains no macOS code and takes a `Clock`, so tests are deterministic.
  It uses wall-clock time, so a session keeps counting while the Mac sleeps.

| File | Purpose |
|---|---|
| `src/main.rs` | event loop, `AppEvent`, `App` state and all decisions |
| `src/tray.rs` | menu bar title and menu |
| `src/session.rs` | session + pass timer, `Clock` trait |
| `src/watcher.rs` | NSWorkspace observer, hide/activate apps, always-allowed list |
| `src/chrome.rs` | poller thread, AppleScript, domain matching, blocked page |
| `src/dialog.rs` | NSAlert reason prompt |
| `src/ui.rs` | the FocusLock window (tao + wry web view), its messages and `focuslock://` protocol |
| `assets/ui/index.html` | the window's page (Welcome / Today / Settings), compiled into the binary |
| `src/apps.rs` | installed-app list + app icons for Settings |
| `src/permission.rs` | Chrome Automation permission check (no AppleScript) |
| `src/config.rs` | config file + paths |
| `src/log.rs` | JSON-lines event log, error log, today's stats |
| `src/notify.rs` | notifications via `osascript` |
| `scripts/bundle.sh` | builds and signs `FocusLock.app` |

## License

[FSL-1.1-MIT](LICENSE.md) (Functional Source License, MIT future license).
You may use, modify and share FocusLock for any purpose except building a
competing commercial product. Two years after each version is released, that
version also becomes available under the MIT license.
