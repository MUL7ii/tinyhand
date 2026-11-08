# TinyHand for Windows

The Windows version of [TinyHand](../README.md): a tiny hand on a stick that points at things on your screen. It uses the same hands, sounds and logo as the Mac app, and behaves the same way: click to tap, drag to draw, a toolbar to pick a hand.

Written in Rust. It draws with a layered window and a low-level mouse hook, so there is no runtime to install apart from WebView2 for the settings window (already part of Windows 11).

## Requirements

- Windows 10 or 11, 64-bit. Developed and tested on Windows 11.
- To build: [Rust](https://rustup.rs) (stable) and the MSVC build tools with the Windows SDK. The "Desktop development with C++" workload of Visual Studio Build Tools has both.
- The WebView2 runtime for the settings window. It ships with Windows 11 and current Windows 10.

## Build and run

```powershell
cd windows
cargo run --release
```

The app looks for `assets/` (hand images and tap sounds) next to the exe, then in this repository. To use the exe on its own, copy it together with `assets/hand_*.png` and `assets/sounds/`.

Turn the pointer on with **Ctrl+Alt+H**, or click the tray icon.

## Use

| | |
|---|---|
| Click the tray icon, or **Ctrl+Alt+H** | Turn the pointer on or off |
| Right-click the tray icon | Menu and Settings |
| Click | Tap (nothing underneath gets clicked) |
| Hold and drag | Draw on the screen |
| **Ctrl+Alt+X** | Clear the drawing |
| **Ctrl+Alt+C** | Click through on or off |
| **Esc** | Clear everything and turn the pointer off |
| Hold **Alt** and click | Click for real, with your normal cursor |

**Click through** is Windows only. With it on, plain clicks go to the app under the pointer and the hand still taps. A drag draws and never reaches the app, so it cannot select text underneath. Hold **Alt** when you need a real drag.

The toolbar at the bottom of the screen appears when you turn the pointer on, and again when you move the mouse near it. It picks the hand or laser, toggles click through and the tap sound, and exits. The hold-to-click key can be changed to Ctrl or Shift, or turned off, in Settings.

## Settings

Right-click the tray icon and choose **Settings**. Everything applies as you change it.

- **General**: click through, the hold-to-click key, the shortcuts, reset.
- **Pointers**: which hands show on the toolbar (up to 8), their order, and the one in use.
- **Appearance**: pointer size and shadow.
- **Drawing**: how long strokes last, and the ink color.
- **Sound**: tap sound for hands and for the laser, and which of the six sounds.
- **About**.

Settings are stored in `%APPDATA%\TinyHand\settings.txt`. A log with any crash details is in `%APPDATA%\TinyHand\log.txt`.

## How it works

| File | Role |
|---|---|
| `src/main.rs` | The message loop, tray icon, hotkeys, and the pieces below wired together |
| `src/hook.rs` | Low-level mouse and keyboard hooks. They only set flags and post messages, so the system never waits on TinyHand |
| `src/renderer.rs` | A thread that draws at the display's refresh rate and sleeps when nothing moves |
| `src/arm.rs`, `src/hand.rs` | The hand and stick: measuring each image, aiming, the shadow |
| `src/scene.rs` | Ink strokes and the laser dot |
| `src/toolbar.rs` | The toolbar, drawn by hand and scaled per monitor |
| `src/sound.rs` | Tap sounds through WASAPI, on their own thread |
| `src/settings.rs`, `settings.html` | The settings window, a WebView2 page created on demand and destroyed on close |
| `src/monitor.rs` | Which monitor the cursor is on, and its display scaling |

Things worth knowing if you change the code:

- Input is never injected from inside a hook callback. Replayed clicks and keys are posted to the main window and sent from there, because injecting inside the hook can stall all input.
- Every window the app owns refuses focus, and hands it straight back if Windows gives it away.
- The real cursors are blanked while the pointer is on and always restored on exit, on a crash and at startup.

## Differences from the Mac app

- Click through mode, and Ctrl+Alt+C for it.
- No Accessibility permission to grant.
- Windows menus, flyouts and the taskbar are detected and let clicks through, in place of the Mac menu bar handling.
- The pointer pauses while Snipping Tool or the Win+Shift+S overlay has focus.
- Not yet ported: a recorder to change the shortcuts, "make your own hand", the toolbar ink color button, opening at login, the first-run hint, auto-update and an installer.

## Known limits

- Windows does not let a program see or block input going to an app running as administrator, so TinyHand cannot draw over or tap such windows.
- The overlay covers the monitor the cursor is on, and drawings clear when you move to another monitor, as on the Mac.
- The toolbar and hands scale per monitor, but mixed display scaling has only been checked with a simulated second monitor.

## Troubleshooting

- **The cursor looks missing after a crash**: start TinyHand again. It restores the normal cursors on launch.
- **Nothing happens when you click**: press **Esc** or **Ctrl+Alt+H** to turn the pointer off. Both work even if drawing is stuck.
- **A hotkey does nothing**: another program may already use that combination.

## License

MIT, the same as the rest of the repository.
