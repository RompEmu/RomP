<p align="center">
  <img src="docs/banner.png" alt="Romp" width="600">
</p>

<p align="center">
  <a href="https://github.com/RompEmu/RompEmu/releases/latest"><img src="https://img.shields.io/github/v/release/RompEmu/RompEmu" alt="Latest release"></a>
  <a href="https://github.com/RompEmu/RompEmu/releases/tag/nightly"><img src="https://github.com/RompEmu/RompEmu/actions/workflows/nightly.yml/badge.svg?branch=main" alt="Nightly build"></a>
  <a href="https://github.com/RompEmu/RompEmu/actions/workflows/ci.yml"><img src="https://github.com/RompEmu/RompEmu/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI"></a>
  <img src="https://img.shields.io/badge/platforms-macOS%20%7C%20Windows%20%7C%20Linux-blue" alt="Platforms: macOS, Windows and Linux">
  <a href="https://romm.app"><img src="https://img.shields.io/badge/RomM-5.0%2B-6f42c1" alt="Needs RomM 5.0 or newer"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0--or--later-green" alt="License: GPL-3.0-or-later"></a>
</p>

<p align="center">Play retro games from your <a href="https://romm.app">RomM</a> library, with each game running in its own sandboxed process.</p>

Download the [latest release](https://github.com/RompEmu/RompEmu/releases/latest) or the [nightly build](https://github.com/RompEmu/RompEmu/releases/tag/nightly) for macOS, Windows and Linux. On a Mac you can also install it with [Homebrew](https://brew.sh): `brew install --cask rompemu/tap/romp`. The macOS download of each release is signed and notarized by Apple. Nightly macOS builds aren't: the first time, right-click Romp and choose Open. Windows builds aren't signed yet: the first time, choose More info, then Run anyway.

<p align="center">
  <img src="docs/screenshots/library.png" alt="The Romp library, showing Amiga games" width="820">
</p>

| Game page | Playing | Nintendo DS |
|---|---|---|
| <img src="docs/screenshots/game-page.png" alt="A game's page with its details and cover"> | <img src="docs/screenshots/gameplay.png" alt="A game running in its own window"> | <img src="docs/screenshots/nintendo-ds.png" alt="A Nintendo DS game with its two screens in separate windows"> |

## Features

- **Your RomM library:** platforms, favorites, collections and recently played, with search, sorting and box art shaped like each console's.
- **One click to play:** Romp installs the right emulator and BIOS files. Downloaded games play offline.
- **Saves that follow you:** in-game saves and save states sync with RomM before and after you play. If both sides changed, you choose which to keep.
- **Sandboxed emulators:** emulators are native code downloaded from the internet, and games are files from anywhere. Each game runs in its own locked-down process that can only write to that game's saves and its own temporary files, so a buggy or malicious emulator or ROM can't touch your other files or take Romp down with it. xemu and RPCS3 run outside the sandbox.
- **Many systems:** from the Atari 2600 to the PS3 and original Xbox, plus arcade and home computers. See [supported systems](#supported-systems).
- **Controllers and multiplayer:** gamepads join as the next player, any key or button can be remapped, and the whole app works with a controller.
- **Light guns, mice and keyboards:** for the systems that use them.
- **In-game menu:** pause, restart, save slots, volume and full screen, from the Guide button or Esc.
- **Two-screen Nintendo DS:** a window per screen, with the mouse as the touch pen.
- **Picks up where you left off:** the last console, game and scroll position, and the size and position of every window.
- **Interface sizes:** 1x, 1.5x or 2x.

## Supported systems

Romp downloads the emulator for each system the first time you play one of its games.

| System | Emulator |
|---|---|
| Arcade, including Neo Geo and CPS 1–3 | FinalBurn Neo |
| Atari 2600 | Stella |
| Atari Jaguar | Virtual Jaguar |
| Atari Lynx | Beetle Lynx |
| Bandai WonderSwan and WonderSwan Color | Beetle WonderSwan |
| ColecoVision | Gearcoleco |
| Commodore 64 | VICE x64sc |
| Commodore Amiga | PUAE |
| DOS | DOSBox Pure |
| Intellivision | FreeIntv |
| Magnavox Odyssey 2 | O2EM |
| MSX, MSX2 and MSX2+ | blueMSX |
| NEC PC Engine / TurboGrafx-16 | Beetle PCE Fast |
| NEC PC Engine CD / TurboGrafx-CD | Beetle PCE |
| NEC SuperGrafx | Beetle SuperGrafx |
| Nintendo Entertainment System, Famicom and Famicom Disk System | Nestopia UE |
| Super Nintendo and Super Famicom | Snes9x |
| Nintendo 64 | Mupen64Plus-Next |
| Game Boy, Game Boy Color and Game Boy Advance | mGBA |
| Nintendo DS | DeSmuME |
| Nintendo GameCube and Wii | Dolphin |
| Nintendo Virtual Boy | Beetle VB |
| Philips CD-i | SAME CDi |
| 3DO | Opera |
| Sega Master System, Genesis / Mega Drive, Game Gear, Sega CD and SG-1000 | Genesis Plus GX |
| Sega 32X | PicoDrive |
| Sega Saturn | Beetle Saturn |
| Sega Dreamcast | Flycast |
| SNK Neo Geo Pocket and Pocket Color | Beetle NeoPop |
| Sony PlayStation | Beetle PSX HW |
| Sony PlayStation 2 | PCSX2 on Windows and Linux, [ARMSX2](https://github.com/RompEmu/ARMSX2) on macOS |
| Sony PlayStation 3 | RPCS3 |
| Sony PSP | PPSSPP |
| Vectrex | Vecx |
| Microsoft Xbox | xemu |
| ZX Spectrum | Fuse |

All emulators except xemu and RPCS3 are [libretro](https://www.libretro.com) cores. Dolphin, PPSSPP, Flycast, Beetle PSX HW, Mupen64Plus-Next and the PS2 emulators draw through Vulkan when the computer has a working Vulkan driver, and through OpenGL otherwise. On macOS, Vulkan comes from the bundled [MoltenVK](https://github.com/KhronosGroup/MoltenVK). Some systems need BIOS files, which Romp takes from your RomM server's firmware. PS3 games need the PS3 system software (`PS3UPDAT.PUP`) in RomM's PS3 firmware, and play from a game folder or an ISO.

## Running

Requires [rustup](https://rustup.rs) and CMake.

```sh
cargo build --release
./target/release/romp
```

`make dist` packages the app into `dist/`: `Romp.app` and a zip on macOS, a zip on Windows, an AppImage and a tarball on Linux (with [appimagetool](https://github.com/AppImage/appimagetool) installed). On Windows, run it from Git Bash with Make and 7-Zip installed.

`make check` runs the same checks as CI: formatting, Clippy, tests, [cargo-deny](https://github.com/EmbarkStudios/cargo-deny), [cargo-machete](https://github.com/bnjbvr/cargo-machete), [typos](https://github.com/crate-ci/typos), [actionlint](https://github.com/rhysd/actionlint) and [zizmor](https://github.com/zizmorcore/zizmor).

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
