# Security

If you find a security problem in EmuHub, please don't open a public issue. Report it privately through GitHub instead: **Security → Report a vulnerability** on this repo. I'll reply as soon as I can.

## What EmuHub touches

- **Network:** box art and screenshots from thumbnails.libretro.com, optional covers from SteamGridDB (only if you add your own API key), RetroArch cores from buildbot.libretro.com, and the latest version number from GitHub's API. Everything goes over HTTPS.
- **Updates:** EmuHub checks GitHub for a newer version at startup and asks first. Only when you press Update does it download the new version, and only from this repo's releases. The file's SHA-256 must match the release's `SHA256SUMS` before it's installed. Releases aren't code-signed yet, so Windows and macOS warn on the first manual install.
- **Your files:** it reads your Emulation folder and writes its settings, covers and play history to `%APPDATA%\EmuHub` (Windows), `~/Library/Application Support/EmuHub` (macOS) or `~/.config/emuhub` (Linux).
- **SteamGridDB key:** stored in plain text in that settings folder, like most desktop apps. Don't share your `config.json`.
- **Games and BIOS:** EmuHub doesn't include or download games, BIOS files, keys or firmware. Use your own legally obtained copies.
