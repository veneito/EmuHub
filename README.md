# EmuHub

A low-memory emulation hub for Windows, macOS and Linux/SteamOS (Rust + GPUI). Inspired by EmuDeck's setup and Playnite's library; styled like Project Launcher.

## Works now

- Scans an EmuDeck-style `Emulation/roms/<system>` folder, including subfolders (one entry per multi-disc `.m3u`).
- 14 systems, all run through RetroArch cores (see `systems.json`).
- Installs RetroArch (winget / Homebrew / Flatpak) and the needed cores from libretro's buildbot.
- Box art from libretro-thumbnails, with SteamGridDB as an optional fallback; cached on disk.
- Uses EmuDeck's `Emulation/bios` and `Emulation/saves/retroarch` folders when present.

## Build

Requires Rust (and the Visual Studio C++ Build Tools on Windows).

```
cargo run --release
```
