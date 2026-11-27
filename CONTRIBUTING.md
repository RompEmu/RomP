# Contributing to RomP

Thanks for wanting to help. This page covers reporting problems, building RomP, and sending a change. To help translate RomP you don't need any of it: see [Translations](README.md#translations).

## Reporting a bug or suggesting something

[Open an issue](https://github.com/RompEmu/RomP/issues/new/choose) with the form that fits: a bug, an idea, or a game that doesn't play. For a bug, the log helps most: in About RomP, Copy the last game's log puts it on the clipboard.

Security problems go through a [private report](https://github.com/RompEmu/RomP/security/advisories/new), so they can be fixed before they're public. Problems that also happen in RomM's own web pages belong with [RomM](https://github.com/rommapp/romm/issues).

## Building

Requires [rustup](https://rustup.rs) and CMake.

```sh
cargo build --release
./target/release/romp
```

`make dist` packages the app into `dist/`: `RomP.app` and a zip on macOS, a zip on Windows, an AppImage, a .deb, an .rpm and a tarball on Linux (Make downloads a pinned [appimagetool](https://github.com/AppImage/appimagetool) and [nfpm](https://nfpm.goreleaser.com) and checks them). On Windows, run it from Git Bash with Make and 7-Zip installed.

## How the code is laid out

RomP is a Rust workspace under `crates/`:

- `romp-app` is the app: the library, settings and game windows, written with [Slint](https://slint.dev), and everything that talks to RomM.
- `romp-runner` is the sandboxed process each game runs in. It loads the emulator, and hands frames and sound to the app.
- `romp-proto` holds the messages the app and the runner send each other.
- `romp-libretro` wraps the libretro API that the emulator cores speak.
- `romp-cheevos` wraps [rcheevos](https://github.com/RetroAchievements/rcheevos) for RetroAchievements.

## Checks

`make check` runs the same checks as CI: formatting, Clippy, tests, [cargo-deny](https://github.com/EmbarkStudios/cargo-deny), [typos](https://github.com/crate-ci/typos), [actionlint](https://github.com/rhysd/actionlint) and [zizmor](https://github.com/zizmorcore/zizmor).

Text the app shows goes through the translation templates. When you add or change any, run `make translations` and include the updated templates, or `make check` fails.

## Trying a change

`romp --core <core library> --rom <file>` plays a file without a RomM server, which helps when testing. `ROMP_PLATFORM=snes` (or any RomM platform name) gives it that console's look. With `--smoke <folder>` it plays briefly as a check, saving a frame there, and exits with an error if the game doesn't play; the Smoke test workflow runs this on each packaged build before a release.

`make update-rcheevos` replaces the bundled [rcheevos](https://github.com/RetroAchievements/rcheevos) with its latest release, or a given one with `RCHEEVOS=v12.5.0`.

## Sending a change

- Pull requests are welcome. For anything larger than a small fix, open an issue first, so the idea can be talked through before you spend time on it.
- Run `make check` before sending.
- Keep commits small, each with a title that says what changes for the person using RomP, such as "Scroll the version menu when it has many versions".
- Add a line to the Unreleased part of [CHANGELOG.md](CHANGELOG.md) when the change is something a user would notice.
- AI models and coding agents are part of how software gets written now, and that's fine for changes to RomP. There has to be a person behind every change, though: one who decided to send it, not an agent opening pull requests by itself on someone's behalf. The change is still yours. Don't add a co-author line, or any other credit in the commit, for a tool, agent or model.
- Translations are done on [Crowdin](https://crowdin.com/project/romp), not through pull requests.

## License

RomP is free software under the [GNU General Public License](LICENSE), version 3 or later. By sending a change you agree to license it the same way.
