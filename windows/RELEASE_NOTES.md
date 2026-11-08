A first preview of TinyHand for Windows: a tiny hand on a stick that points at things on your screen. It uses the same hands, sounds and logo as the Mac app.

## Try it

1. Download `TinyHand-Windows-0.1.0.zip` and unzip it anywhere.
2. Run `TinyHand.exe`. A tray icon appears.
3. Press **Ctrl+Alt+H**, or click the tray icon, to turn the pointer on.

Click to tap, hold and drag to draw, **Esc** to exit. The toolbar at the bottom of the screen picks the hand. Right-click the tray icon for Settings.

Windows 10 or 11, 64-bit. Settings needs the WebView2 runtime, which comes with Windows 11 and current Windows 10.

## What works

- The 36 hands and the laser, with shadow, tap sounds and drawing that fades.
- A toolbar you can customize, and per-monitor placement and scaling.
- **Click through**: plain clicks reach the app under the pointer while drags still draw. Toggle with **Ctrl+Alt+C**.
- Hold **Alt** to click for real with your normal cursor.
- A settings window for all of the above.

## Known limits

- **This build is not code-signed.** Windows SmartScreen may warn the first time you run it. Choose "More info", then "Run anyway".
- Apps running as administrator cannot be drawn on or tapped, because Windows blocks input hooks across that boundary.
- Not done yet: an installer, auto-update, a shortcut recorder, "make your own hand", the toolbar ink color button and opening at login.
- Mixed display scaling has only been checked with a simulated second monitor.

If something looks wrong, a log is in `%APPDATA%\TinyHand\log.txt`.

Based on TinyHand for Mac by Kamran Ahmed. MIT license.
