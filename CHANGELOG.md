# Changelog

All notable changes to Romp are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and Romp uses [semantic versioning](https://semver.org/).

## [Unreleased]

### Added

- A Licenses tab in the About window, with the license of Romp and of every emulator it downloads.
- Resolution settings on Macs for N64, GameCube and Wii, PlayStation, PSP and Dreamcast, and a 4x choice for PS2. They default to 2x, or 3x for PSP.

### Changed

- The About window is roomier and easier to read, and says who makes Romp.

## [0.5.0] - 2026-09-29

### Changed

- PS2 games play with ARMSX2 on Macs, in place of Play!. It needs a PS2 BIOS in the platform's firmware on your RomM server.
- Romp updates ARMSX2 when a newer build is published.
- PS2 games on Macs render at three times their native resolution, with 16x anisotropic filtering.
- Save states are stored at their real size. ARMSX2's shrink from 68 MB to a few MB.
- On Macs, GameCube, Wii, PSP, Dreamcast and PlayStation games draw through Vulkan instead of Apple's outdated OpenGL.
- N64 games on Macs use the accurate paraLLEl-RDP renderer through Vulkan, in place of the slow software renderer.

### Added

- A Consoles tab in Settings, starting with PS2 resolution, anisotropic filtering and widescreen patches.

### Fixed

- PS2 games on Macs can save to their memory card, and pick up where you left off.

## [0.4.0] - 2026-09-28

### Added

- Linux releases include an AppImage alongside the tarball.
- Games are sandboxed on Windows: the emulator can't start other programs or use the clipboard, and can only write to the game's save folder.
- Game windows open where you last left them, with their size, for each console. The Nintendo DS touch screen window is remembered separately.

### Changed

- Esc closes the About and remap windows. While a key is being remapped, Esc cancels just that key.

## [0.3.0] - 2026-09-28

Tagged but not published as a download. Its changes ship in 0.4.0.

### Added

- Windows builds, published with every release and nightly.
- PS2 games play with PCSX2 on Windows and Linux. It needs a PS2 BIOS in the platform's firmware on your RomM server. Macs keep using Play!.

### Changed

- On Windows, games and saves are stored in `%LOCALAPPDATA%\Romp`.

### Fixed

- Games no longer close as soon as they start on Windows.

## [0.2.0] - 2026-09-28

### Added

- A short welcome guide on first launch that helps you connect to your RomM server.

## [0.1.0] - 2026-09-28

First release, for macOS and Linux.

### Added

- Connect to a RomM 5.0 or newer server and approve Romp by scanning a code or opening a link.
- Browse your library by platform, favorites, collections and recently played, with search, sorting and covers in each console's box shape.
- Game pages with details, screenshots and similar games, and editing of favorites and collections.
- Download games to play them, including offline, with resumable, verified downloads and multi-disc sets.
- Emulator cores and BIOS files installed automatically, with each game running in its own sandboxed process.
- Original Xbox games through xemu.
- In-game saves and save states synced with RomM before and after play, with a choice when both sides changed.
- An in-game menu with pause, restart, save slots, volume and full screen.
- Multiple players with gamepads, remappable keys and buttons, and controller navigation of the whole app.
- Keyboard and mouse for computer systems, and light guns and other accessories per port.
- Two windows for Nintendo DS screens, and rotated screens for vertical arcade games.
- Interface sizes of 1x, 1.5x and 2x, and the window, last view and scroll position remembered between runs.

[Unreleased]: https://github.com/RompEmu/RompEmu/compare/v0.5.0...HEAD
[0.5.0]: https://github.com/RompEmu/RompEmu/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/RompEmu/RompEmu/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/RompEmu/RompEmu/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/RompEmu/RompEmu/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/RompEmu/RompEmu/releases/tag/v0.1.0
