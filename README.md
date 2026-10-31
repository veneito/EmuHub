# EmuHub

A low-memory emulation hub for Windows, macOS and Linux/SteamOS (Rust + GPUI). Inspired by EmuDeck's setup and Playnite's library; styled like Project Launcher.

## Works now

- Scans an EmuDeck-style `Emulation/roms/<system>` folder, including subfolders (one entry per multi-disc `.m3u`).
- 14 systems, all run through RetroArch cores (see `systems.json`).
- Installs RetroArch (winget / Homebrew / Flatpak) and the needed cores from libretro's buildbot.
- Box art from libretro-thumbnails, with SteamGridDB as an optional fallback; cached on disk.
- Uses EmuDeck's `Emulation/bios` and `Emulation/saves/retroarch` folders when present.
- RetroArch-style controller navigation (ignored while another window, like a running game, has focus):
  - Side menu: up/down moves and previews each page; A (✕ on PlayStation) or Right goes into it.
  - Game grid: D-pad/stick moves, A plays; B (◯) or Left from the first column returns to the menu.
  - Settings: up/down through the buttons, A presses, B or Left returns to the menu.
  - LB/RB (L1/R1) switch systems from anywhere.

## Build

Requires Rust (and the Visual Studio C++ Build Tools on Windows).

```
cargo run --release
```
