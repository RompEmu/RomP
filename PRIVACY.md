# Privacy

Romp is a desktop app. It has no servers of its own, no accounts of its own, no analytics, no crash reporting and no ads. Nothing about you or how you use Romp is sent to its developers.

## What Romp sends, and where

Romp only talks to these services, and only to do what you ask of it:

| Service | When | What it receives |
|---|---|---|
| Your RomM server | While you use your library | Your sign-in, the requests for your library, covers and games, your saves, save states and screenshots, play sessions and play time, and a device name made of "RomP on" and your computer's name |
| RetroAchievements (retroachievements.org) | Only after you sign in to it in Settings | Your username, a sign-in token, which game you're playing (by the game file's hash), the achievements you unlock, leaderboard scores and what you're doing in the game ("rich presence") |
| libretro buildbot (buildbot.libretro.com) | When Romp installs or updates an emulator, or system files one needs | A request for that file |
| GitHub (github.com) | When Romp installs or updates the PS2 emulator on macOS, xemu or RPCS3 | A request for that release |

Every request also carries your IP address and a short description of the app, such as `Romp/0.9.0 (macOS)`, as any internet request does. Each of these services handles that under its own privacy policy: [RetroAchievements](https://retroachievements.org/privacy), [GitHub](https://docs.github.com/site-policy/privacy-policies/github-general-privacy-statement), and for your RomM server, whoever runs it.

Emulators run in a sandbox. On macOS it also stops them reaching the internet. On Windows and Linux it limits which files they can change, but not their network access. Achievement requests from a game go through the app, which only sends them to RetroAchievements. xemu and RPCS3 run as their own programs, outside Romp's sandbox.

## What Romp keeps on your computer

- **Sign-in tokens** for your RomM server and RetroAchievements, in your system's keychain or credential store. Romp never stores your passwords.
- **Your library list, downloaded games, saves, save states, screenshots and settings**, in `~/Library/Application Support/Romp` on macOS, `%LOCALAPPDATA%\Romp` on Windows and `~/.local/share/Romp` on Linux.

They stay until you remove them. Signing out of RomM in Settings removes its token and the library list. Signing out of RetroAchievements removes its token. Deleting the folder above removes everything else.

## Your rights

Romp's developers hold no data about you, so there's nothing for them to show, correct or delete under the GDPR or similar laws. For data held by your RomM server, ask whoever runs it. For RetroAchievements, see its privacy policy.

## Changes

Changes to this policy are part of Romp's source history, and each release's notes mention them.

## Contact

Questions about privacy can go to [Romp's issues on GitHub](https://github.com/RompEmu/RompEmu/issues).
