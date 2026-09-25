# Cartridge

Play retro games from your [RomM](https://romm.app) library, with each game running in its own sandboxed process.

## Running

Requires [rustup](https://rustup.rs) and CMake.

```sh
cargo build --release
./target/release/cartridge
```

Enter your RomM server address (RomM 5.0 or newer). Approve Cartridge in RomM by scanning the code or opening the link.

Open a game from your library to download and play it. Cartridge installs the emulator core it needs, and fetches BIOS files from your server's firmware. Downloaded games stay playable when the server is offline.

To play a game file directly with a libretro core:

```sh
./target/release/cartridge --core <libretro core> --rom <game>
```

Add `--jit` for cores that use a dynamic recompiler. Cores are available from the [libretro buildbot](https://buildbot.libretro.com/nightly/).

## Controls

A connected gamepad controls player 1. On the keyboard:

| Key | Action |
|---|---|
| Arrow keys | D-pad |
| X, Z, S, A | A, B, X, Y |
| Q, W, D, F | L, R, L2, R2 |
| Enter, Backspace | Start, Select |
| F5, F7 | Save, load state |
| Esc | Quit |

Your games, saves and library cache are stored in `~/Library/Application Support/Cartridge` on macOS and `~/.local/share/Cartridge` on Linux.
