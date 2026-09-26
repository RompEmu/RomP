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

In-game saves and save states sync with RomM before and after you play, so you can continue on another device. If a save changed in both places, Cartridge asks which to keep and backs up the other.

Favorites and collections come from RomM, including its automatic series, franchise and genre collections. Favorite a game or add it to your collections from its page; create a collection with **+** in the sidebar, and right-click one to rename or delete it.

When a new version of Cartridge needs more access to your server, the library shows a **Pair again** button.

To play a game file directly with a libretro core:

```sh
./target/release/cartridge --core <libretro core> --rom <game>
```

Add `--jit` for cores that use a dynamic recompiler. Cores are available from the [libretro buildbot](https://buildbot.libretro.com/nightly/).

## Controls

Gamepads are picked up automatically: the first one joins the keyboard as player 1, and each new one becomes the next player. Change who plays as which player, or remap any keyboard key or controller button, in **Settings → Players**. The Guide button, or Select and Start together, opens the game menu. In Amiga, DOS, C64 and ZX Spectrum games your keyboard types into the game and F12 opens the game menu; click in the game to use your mouse, and F12 releases it. For accessories such as the SNES Mouse, Super Scope, Zapper or GunCon, choose them per port in the game menu: a mouse is captured the same way, and a light gun aims where you point and fires with the left button (right button reloads). The default keyboard layout:

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

Your games, saves and library cache are stored in `~/Library/Application Support/Cartridge` on macOS and `~/.local/share/Cartridge` on Linux.
