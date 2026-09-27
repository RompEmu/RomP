# Romp

Play retro games from your [RomM](https://romm.app) library, with each game running in its own sandboxed process.

Source and releases: [github.com/RompEmu/RompEmu](https://github.com/RompEmu/RompEmu)

## Running

Requires [rustup](https://rustup.rs) and CMake.

```sh
cargo build --release
./target/release/romp
```

Enter your RomM server address (RomM 5.0 or newer). Approve Romp in RomM by scanning the code or opening the link.

Open a game from your library to download and play it. Romp installs the emulator core it needs, and fetches BIOS files from your server's firmware. Downloaded games stay playable when the server is offline.

Original Xbox games play in [xemu](https://xemu.app), which Romp installs and sets up with your controls, volume and display settings. Add the Xbox boot ROM (`mcpx_1.0.bin`) and an Xbox BIOS to the Xbox platform's firmware in RomM. Games need to be in XISO format. In xemu, press Esc or your controller's Guide button for its menu.

In the library, ⌘F searches, ⌘R refreshes and ⌘, opens Settings. Esc or your mouse's back button leaves a game's page.

In-game saves and save states sync with RomM before and after you play, so you can continue on another device. If a save changed in both places, Romp asks which to keep and backs up the other.

Favorites and collections come from RomM, including its automatic series, franchise and genre collections. Favorite a game or add it to your collections from its page; create a collection with **+** in the sidebar, and right-click one to rename or delete it. **Recently played** lists the games you last played, here or in RomM, and the library can be sorted by name, date added or last played.

When a new version of Romp needs more access to your server, the library shows a **Pair again** button.

To play a game file directly with a libretro core:

```sh
./target/release/romp --core <libretro core> --rom <game>
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

Your games, saves and library cache are stored in `~/Library/Application Support/Romp` on macOS and `~/.local/share/Romp` on Linux. An existing Cartridge folder from before the rename is moved there on first start.
