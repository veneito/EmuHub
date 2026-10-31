![EmuHub](assets/banner.png)

# EmuHub

A low-memory emulation hub for Windows, macOS and Linux/SteamOS, written in Rust with GPUI.

I wanted one app that sets up emulators like EmuDeck, shows my library like Playnite, and is easy to get around with a controller like RetroArch, without eating memory. EmuHub is that app. It works with an existing EmuDeck `Emulation` folder, so a SteamOS backup just works on a PC or Mac.

## Features

**Library**
- Scans an EmuDeck-style `Emulation/roms/<system>` folder, including subfolders (one entry per multi-disc `.m3u`; `dlc/` and `updates/` skipped, `[BIOS]` files hidden). Accepts `.zip` and `.7z` for cartridge systems.
- Finds an existing `Emulation` folder by itself: in your home folder, on any drive, or on a Steam Deck SD card.
- 15 systems (see `systems.json`): 14 through RetroArch cores, plus Nintendo Switch through Eden.
- Home page: the last game played with its screenshot, a Continue Playing row and a Systems row.
- Library grid that stays smooth with thousands of games.
- Box art from libretro-thumbnails, with SteamGridDB as an optional fallback. Covers are cached small, so they load fast and take little space.
- Full-screen game page (Options or right-click): Play, and Change cover with every libretro region and SteamGridDB choice.

**Setup** (Settings)
- Choose the Emulation folder: an existing EmuDeck copy is used as is, or a new one is created with `roms/`, `bios/` and `tools/`.
- Installs RetroArch (winget / Homebrew / Flatpak) and the needed cores from libretro's buildbot, and shows what's already installed.
- Standalone emulators: Eden for Switch is found in `Emulation/tools/eden`, the usual install places, or picked by hand. EmuHub links to its official site and doesn't download it. Keys and firmware are copied from `Emulation/bios/eden` on first launch.
- Uses EmuDeck's `Emulation/bios` and `Emulation/saves/retroarch` folders when present.
- Updates itself: a popup offers each new version, and Update downloads it, checks it and restarts EmuHub on it (installer, Mac app and AppImage; the portable copies open the Releases page).

**Playing**
- RetroArch opens fullscreen; one game runs at a time. Intel Macs use the OpenGL driver, which runs smoother there.
- Share + Options (Select + Start) closes the game: RetroArch quits after saving, and Eden is asked to close.

**Controller** (PlayStation or Xbox; the hints bar shows the right buttons)
- Side menu: up/down moves and previews each page; ✕/A or Right goes into it.
- Grid and Home rows: move with the D-pad or stick; ✕/A plays, Options opens the game page, ◯/B or Left from the edge returns to the menu.
- Settings and the game page: move through the buttons, ✕/A presses, ◯/B goes back.
- L1/R1 (LB/RB) switch systems from anywhere. Input is ignored while a game has focus.

## Download

Get the latest version from [Releases](https://github.com/veneito/EmuHub/releases/latest):

- **Windows:** `EmuHub-Setup.exe`. Installs for your user, no admin needed, with optional desktop and Start menu shortcuts. Windows SmartScreen may warn the first time because it isn't code-signed: More info → Run anyway. `emuhub-windows.exe` is a portable copy.
- **macOS:** `EmuHub.dmg` (universal: Apple Silicon and Intel). Drag EmuHub to Applications. It isn't notarized, so approve it once: right-click → Open (macOS 14 and older) or System Settings → Privacy & Security → Open Anyway (macOS 15+).
- **Linux / SteamOS:** `EmuHub-x86_64.AppImage` (one file with an icon; Gear Lever or AppImageLauncher adds it to the app menu), or the bare `emuhub-linux`. Make it executable first (`chmod +x`). On Arch, install `libxkbcommon-x11 wayland vulkan-icd-loader fontconfig freetype2 alsa-lib` and your GPU's Vulkan driver. Add it to Steam as a non-Steam game to use it in Game Mode.

EmuHub doesn't include games, BIOS files, keys or firmware. Use your own.

## Build

Requires Rust (and the Visual Studio C++ Build Tools on Windows).

```
cargo run --release
```

Tests: `cargo test`. To publish a release, bump `version` in `Cargo.toml` and push a matching tag (`git tag v0.2.0 && git push origin v0.2.0`); CI builds all three systems and attaches them to the release.

## License

MIT, see [LICENSE](LICENSE). Security issues: see [SECURITY.md](SECURITY.md).
