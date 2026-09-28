<p align="center">
  <img src="docs/banner.png" alt="Romp" width="600">
</p>

<p align="center">
  <a href="https://github.com/RompEmu/RompEmu/releases/latest"><img src="https://img.shields.io/github/v/release/RompEmu/RompEmu" alt="Latest release"></a>
  <a href="https://github.com/RompEmu/RompEmu/releases/tag/nightly"><img src="https://github.com/RompEmu/RompEmu/actions/workflows/nightly.yml/badge.svg?branch=main" alt="Nightly build"></a>
  <a href="https://github.com/RompEmu/RompEmu/actions/workflows/ci.yml"><img src="https://github.com/RompEmu/RompEmu/actions/workflows/ci.yml/badge.svg?event=pull_request" alt="CI"></a>
  <img src="https://img.shields.io/badge/platforms-macOS%20%7C%20Windows%20%7C%20Linux-blue" alt="Platforms: macOS, Windows and Linux">
  <a href="https://romm.app"><img src="https://img.shields.io/badge/RomM-5.0%2B-6f42c1" alt="Needs RomM 5.0 or newer"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0--or--later-green" alt="License: GPL-3.0-or-later"></a>
</p>

<p align="center">Play retro games from your <a href="https://romm.app">RomM</a> library, with each game running in its own sandboxed process.</p>

Download the [latest release](https://github.com/RompEmu/RompEmu/releases/latest) or the [nightly build](https://github.com/RompEmu/RompEmu/releases/tag/nightly) for macOS, Windows and Linux. Nightly builds aren't signed: the first time, on macOS right-click Romp and choose Open, and on Windows choose More info, then Run anyway. Windows support is new and games aren't sandboxed there yet.

<p align="center">
  <img src="docs/screenshots/library.png" alt="The Romp library, showing Amiga games" width="820">
</p>

| Game page | Playing | Nintendo DS |
|---|---|---|
| <img src="docs/screenshots/game-page.png" alt="A game's page with its details and cover"> | <img src="docs/screenshots/gameplay.png" alt="A game running in its own window"> | <img src="docs/screenshots/nintendo-ds.png" alt="A Nintendo DS game with its two screens in separate windows"> |

## Running

Requires [rustup](https://rustup.rs) and CMake.

```sh
cargo build --release
./target/release/romp
```

`make dist` packages the app into `dist/`: `Romp.app` and a zip on macOS, a zip on Windows, a tarball on Linux. On Windows, run it from Git Bash with Make and 7-Zip installed.

Enter your RomM server address (RomM 5.0 or newer). Approve Romp in RomM by scanning the code or opening the link.

Open a game from your library to download and play it. Romp installs the emulator core it needs, and fetches BIOS files from your server's firmware. Downloaded games stay playable when the server is offline.

In-game saves and save states sync with RomM before and after you play, so you can continue on another device. If a save changed in both places, Romp asks which to keep and backs up the other.

## Controls

- Gamepads are picked up automatically: the first one joins the keyboard as player 1, and each new one becomes the next player.
- Change players or remap any key or button in **Settings → Players**.
- The Guide button, or Select and Start together, opens the game menu.
- In Amiga, DOS, C64 and ZX Spectrum games the keyboard types into the game and F12 opens the menu. Click in the game to use your mouse; F12 releases it.
- Accessories such as the SNES Mouse, Super Scope, Zapper or GunCon are chosen per port in the game menu. A light gun aims where you point and fires with the left button; the right button reloads.

Default keyboard layout:

| Key | Action |
|---|---|
| Arrow keys | D-pad |
| X, Z, S, A | A, B, X, Y |
| Q, W, D, F | L, R, L2, R2 |
| Enter, Backspace | Start, Select |
| F5, F7 | Save, load state |
| F6 | Next save slot |
| P | Pause |
| F11 | Full screen |
| Esc | Game menu |

Your games, saves and library cache are stored in `~/Library/Application Support/Romp` on macOS, `%LOCALAPPDATA%\Romp` on Windows and `~/.local/share/Romp` on Linux.

## License

Romp is free software: you can redistribute it and/or modify it under the terms of the [GNU General Public License](LICENSE), version 3 or (at your option) any later version.
