# EmuHub

A low-memory emulation hub for Windows, macOS and Linux/SteamOS (Rust + GPUI). Inspired by EmuDeck's setup, Playnite's library and the old EmuHub's games-first layout; styled like Project Launcher (glass or solid, five color themes).

## Works now

**Library**
- Scans an EmuDeck-style `Emulation/roms/<system>` folder, including subfolders (one entry per multi-disc `.m3u`; `dlc/` and `updates/` skipped, `[BIOS]` files hidden). Accepts `.zip` and `.7z` for cartridge systems.
- 15 systems (see `systems.json`): 14 through RetroArch cores, plus Nintendo Switch through Eden.
- Home page: the last game played with its screenshot, a Continue Playing row and a Systems row.
- Library grid that stays smooth with thousands of games (only the rows on screen are drawn).
- Box art from libretro-thumbnails (exact name, then the closest region match), with SteamGridDB as an optional fallback; cached on disk.
- Game details (right-click or Options): Play, and Change cover with every libretro region and SteamGridDB choice.

**Setup** (Settings)
- Choose the Emulation folder: an existing EmuDeck copy is used as is, or a new one is created with `roms/`, `bios/` and `tools/`.
- Installs RetroArch (winget / Homebrew / Flatpak) and the needed cores from libretro's buildbot, and shows what's already installed.
- Standalone emulators: Eden for Switch is found in `Emulation/tools/eden`, the usual install places, or picked by hand. EmuHub links to its official site and doesn't download it. Keys and firmware are copied from `Emulation/bios/eden` on first launch.
- Uses EmuDeck's `Emulation/bios` and `Emulation/saves/retroarch` folders when present.

**Playing**
- RetroArch opens fullscreen (Metal driver on Intel Macs); one game runs at a time.
- Share + Options (Select + Start) closes the game: RetroArch quits after saving, and Eden is asked to close.

**Controller** (PlayStation or Xbox; the hints bar shows the right buttons)
- Side menu: up/down moves and previews each page; ✕/A or Right goes into it.
- Grid and Home rows: move with the D-pad or stick; ✕/A plays, Options opens details, ◯/B or Left from the edge returns to the menu.
- Settings and the details window: up/down through the buttons, ✕/A presses, ◯/B goes back.
- L1/R1 (LB/RB) switch systems from anywhere. Input is ignored while a game has focus.

## Download

Every push to `main` builds on GitHub Actions; open the latest run under **Actions** and download from **Artifacts**:

- **Windows:** `emuhub.exe`.
- **macOS:** `EmuHub.dmg` (universal: Apple Silicon and Intel). Drag EmuHub to Applications. It isn't notarized, so approve it once: right-click → Open (macOS 14 and older) or System Settings → Privacy & Security → Open Anyway (macOS 15+).
- **Linux / SteamOS:** `emuhub`. Run `chmod +x emuhub` first. On Arch, install `libxkbcommon-x11 wayland vulkan-icd-loader fontconfig freetype2 alsa-lib` and your GPU's Vulkan driver. Add it to Steam as a non-Steam game to use it in Game Mode.

## Build

Requires Rust (and the Visual Studio C++ Build Tools on Windows).

```
cargo run --release
```
