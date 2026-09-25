# Cartridge

A cross-platform app for playing retro games. Each game runs a libretro core in its own sandboxed process, and a [RomM](https://romm.app) server provides the library, downloads and save sync.

**Status:** early development. Today you can play a local ROM with a libretro core you already have. RomM support is next.

## Requirements

- macOS (Apple Silicon) or Linux (x86_64)
- [rustup](https://rustup.rs). The toolchain version is pinned in `rust-toolchain.toml` and installs automatically.
- CMake, which the audio library needs (`brew install cmake` or your distro's package)
- macOS: Xcode or the Command Line Tools

If macOS linking fails with `ld: tapi error ... unknown architecture`, your Command Line Tools SDK is newer than your Xcode linker. Point the build at Xcode's SDK:

```sh
export SDKROOT=/Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk
```

## Build and run

```sh
cargo build --workspace
./target/debug/cartridge --core <path to a libretro core> --rom <path to a ROM>
```

Add `--jit` for cores that need a dynamic recompiler (for example Flycast or Mupen64Plus-Next).

Cores come from the [libretro buildbot](https://buildbot.libretro.com/nightly/). For example, on Apple Silicon: `apple/osx/arm64/latest/snes9x_libretro.dylib.zip`.

### Keys

| Key | Action |
|---|---|
| Arrow keys | D-pad |
| X / Z | A / B |
| S / A | X / Y |
| Q / W | L / R |
| D / F | L2 / R2 |
| Enter / Backspace | Start / Select |
| F5 / F7 | Save / load slot 1 |
| Esc | Quit |

### Where things go

Files live in the data directory: `~/Library/Application Support/Cartridge` on macOS and `~/.local/share/Cartridge` on Linux. Set `CARTRIDGE_DATA_DIR` to use a different one.

- `saves/<game>/`: `game.srm` (in-game save), `slot-N.state` and `auto.state` (written on quit)
- `system/`: BIOS files

### Troubleshooting

- `CARTRIDGE_NO_SANDBOX=1` runs the game without the sandbox. If a game only works with it, please report it.
- `CARTRIDGE_SANDBOX_PERMISSIVE_READ=1` / `CARTRIDGE_SANDBOX_PERMISSIVE_MACH=1` (macOS) loosen single parts of the sandbox.
- `RUST_LOG=debug` gives more detailed logs. Lines from the game process are prefixed with `[runner]`.

## Development

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

| Crate | Purpose |
|---|---|
| `cartridge-app` | The app: Slint UI; starts and supervises game processes |
| `cartridge-runner` | Per-game process: runs the core, audio, sandbox, saves |
| `cartridge-proto` | Messages and the shared-memory frame buffer between app and runner |
| `cartridge-libretro` | libretro FFI bindings |
