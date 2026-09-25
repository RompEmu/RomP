# Cartridge

Play retro games using libretro cores, with each game running in its own sandboxed process.

## Running

Requires [rustup](https://rustup.rs) and CMake.

```sh
cargo build --release
./target/release/cartridge --core <libretro core> --rom <game>
```

Add `--jit` for cores that use a dynamic recompiler. Cores are available from the [libretro buildbot](https://buildbot.libretro.com/nightly/).

## Controls

| Key | Action |
|---|---|
| Arrow keys | D-pad |
| X, Z, S, A | A, B, X, Y |
| Q, W, D, F | L, R, L2, R2 |
| Enter, Backspace | Start, Select |
| F5, F7 | Save, load state |
| Esc | Quit |

Saves are stored in `~/Library/Application Support/Cartridge` on macOS and `~/.local/share/Cartridge` on Linux.
