<img src="./.github/logo.png" width="80" />

# TinyHand

[![Latest release](https://img.shields.io/github/v/release/nilbuild/tinyhand?display_name=tag&sort=semver&cacheSeconds=3600)](https://github.com/nilbuild/tinyhand/releases/latest)
[![Platform: macOS and Windows](https://img.shields.io/badge/platform-macOS%20%7C%20Windows-lightgrey)](https://tinyhand.app)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](./LICENSE)

A tiny hand on a stick that points at things on your screen.

<img src="./.github/banner.jpg" alt="TinyHand: a tiny hand for your Mac" />

[![Download for macOS](https://img.shields.io/badge/Download_for_macOS-000000?style=for-the-badge&logo=apple&logoColor=white)](https://github.com/nilbuild/tinyhand/releases/latest/download/TinyHand.dmg)
[![Visit website](https://img.shields.io/badge/Visit_website-ffffff?style=for-the-badge&logo=safari&logoColor=black)](https://tinyhand.app)

**macOS** 13 or later, Apple Silicon and Intel. **Windows** 10 or 11, 64-bit (build from source for now, see [windows/](windows/README.md)).

## Install

### macOS

Open `TinyHand.dmg`, drag TinyHand to Applications and allow **Accessibility** when asked. It needs that to turn your clicks into taps. TinyHand updates itself.

### Windows

No permission prompt and no installer yet. Build it with Rust and run it, as described in [windows/README.md](windows/README.md).

## Use

| macOS | Windows | |
|---|---|---|
| Click the menu bar icon, or **⌃⌥H** | Click the tray icon, or **Ctrl+Alt+H** | Turn the pointer on or off |
| Right-click the menu bar icon | Right-click the tray icon | Menu and Settings |
| Click | Click | Tap (nothing underneath gets clicked) |
| Hold and drag | Hold and drag | Draw on the screen |
| **⌃⌥X** | **Ctrl+Alt+X** | Clear the drawing |
| **Esc** | **Esc** | Clear everything and turn the pointer off |
| Hold **⌥** and click | Hold **Alt** and click | Click for real, with your normal cursor |
| | **Ctrl+Alt+C** | Let plain clicks through to apps while drags still draw (Windows) |

While the pointer is on, a small toolbar at the bottom of the screen lets you pick one of
36 hands or a laser dot, mute the tap sound, or exit.

## License

MIT. See [LICENSE](LICENSE).
