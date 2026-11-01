//! Library scanning, settings, RetroArch launching, installs and cover art. No UI code here.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

// Folder ids follow EmuDeck's Emulation/roms/<id> layout so existing setups just work.
const SYSTEMS_JSON: &str = include_str!("../systems.json");

#[derive(Serialize, Deserialize, Clone)]
pub struct System {
    pub id: String,
    pub name: String,
    pub exts: Vec<String>,
    /// RetroArch core; empty for systems that use a standalone emulator.
    #[serde(default)]
    pub core: String,
    /// Standalone emulator id (see EMULATORS); empty means RetroArch.
    #[serde(default)]
    pub standalone: String,
    /// Folder name on thumbnails.libretro.com.
    pub thumbs: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Config {
    pub roms_dir: PathBuf,
    /// Command that starts RetroArch, e.g. ["retroarch"] or ["flatpak", "run", "org.libretro.RetroArch"].
    pub retroarch_cmd: Vec<String>,
    pub cores_dir: PathBuf,
    /// Optional SteamGridDB API key, used for covers libretro-thumbnails doesn't have.
    #[serde(default)]
    pub steamgriddb_key: String,
    /// Standalone emulator programs picked in Settings, by emulator id.
    #[serde(default)]
    pub emulators: std::collections::HashMap<String, PathBuf>,
    /// The update version the user chose to skip; it isn't offered again.
    #[serde(default)]
    pub skipped_update: Option<String>,
}

#[derive(Clone)]
pub struct Game {
    pub system: String,
    pub name: String,
    pub path: PathBuf,
}

pub fn systems() -> Vec<System> {
    serde_json::from_str(SYSTEMS_JSON).expect("systems.json is valid")
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_default()
}

/// Per-user settings folder: %APPDATA%\EmuHub, ~/Library/Application Support/EmuHub, or ~/.config/emuhub.
pub fn config_dir() -> PathBuf {
    if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(home).join("EmuHub")
    } else if cfg!(target_os = "macos") {
        home().join("Library/Application Support/EmuHub")
    } else {
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".config")).join("emuhub")
    }
}

/// Places an existing EmuDeck "Emulation" folder usually lives: home, then every drive (D:\, a Mac's
/// /Volumes/*, the Steam Deck's SD card under /run/media). Home first so a fresh setup stays there.
fn emulation_candidates(home: &Path) -> Vec<PathBuf> {
    let mut places = vec![home.to_path_buf()];
    if cfg!(target_os = "windows") {
        places.extend((b'C'..=b'Z').map(|d| PathBuf::from(format!("{}:\\", d as char))));
    } else {
        let subdirs = |dir: &Path| std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).collect::<Vec<_>>();
        places.extend(subdirs(Path::new("/Volumes")));
        // SteamOS mounts the SD card at /run/media/<card> or /run/media/<user>/<card>.
        for dir in subdirs(Path::new("/run/media")) {
            places.extend(subdirs(&dir));
            places.push(dir);
        }
    }
    places
}

/// The roms folder of the first existing Emulation folder, or a new one in home.
fn find_roms_dir(home: &Path, places: &[PathBuf]) -> PathBuf {
    places.iter().map(|p| p.join("Emulation").join("roms")).find(|r| r.is_dir()).unwrap_or_else(|| home.join("Emulation").join("roms"))
}

fn default_config(home: &Path) -> Config {
    let roms_dir = find_roms_dir(home, &emulation_candidates(home));
    let steamgriddb_key = String::new();
    if cfg!(target_os = "windows") {
        let ra = PathBuf::from(r"C:\RetroArch-Win64");
        Config { roms_dir, retroarch_cmd: vec![ra.join("retroarch.exe").display().to_string()], cores_dir: ra.join("cores"), steamgriddb_key, emulators: Default::default(), skipped_update: None }
    } else if cfg!(target_os = "macos") {
        Config {
            roms_dir,
            retroarch_cmd: vec!["/Applications/RetroArch.app/Contents/MacOS/RetroArch".into()],
            cores_dir: home.join("Library/Application Support/RetroArch/cores"),
            steamgriddb_key,
            emulators: Default::default(),
            skipped_update: None,
        }
    } else {
        // SteamOS/EmuDeck installs RetroArch as a Flatpak.
        Config {
            roms_dir,
            retroarch_cmd: vec!["flatpak".into(), "run".into(), "org.libretro.RetroArch".into()],
            cores_dir: home.join(".var/app/org.libretro.RetroArch/config/retroarch/cores"),
            steamgriddb_key,
            emulators: Default::default(),
            skipped_update: None,
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let mut cfg: Self = std::fs::read(config_dir().join("config.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_else(|| default_config(&home()));
        // A saved folder that's gone (an unplugged drive, the old default) gives way to one that exists.
        if !cfg.roms_dir.is_dir() {
            let found = find_roms_dir(&home(), &emulation_candidates(&home()));
            if found.is_dir() {
                cfg.roms_dir = found;
            }
        }
        cfg
    }
    pub fn save(&self) -> Result<(), String> {
        let dir = config_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::fs::write(dir.join("config.json"), serde_json::to_vec_pretty(self).unwrap()).map_err(|e| e.to_string())
    }
}

/// Creates EmuDeck's layout: roms/<system>, bios/, plus tools/<emulator> and bios/<emulator> for each
/// standalone emulator. Safe to run again: existing folders and files are left alone.
pub fn create_folders(cfg: &Config) -> Result<(), String> {
    let emulation = emulation_dir(cfg);
    let mut dirs: Vec<PathBuf> = systems().into_iter().map(|s| cfg.roms_dir.join(&s.id)).collect();
    dirs.push(emulation.join("bios"));
    for emu in &EMULATORS {
        dirs.extend([emulator_dir(cfg, emu), emulation.join("bios").join(emu.id)]);
    }
    for dir in dirs {
        std::fs::create_dir_all(&dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    }
    Ok(())
}

/// The roms folder for a folder picked as the Emulation folder. A folder named "Emulation" or one that
/// already has roms/ is used as is (an existing EmuDeck copy); anything else gets an Emulation folder inside.
pub fn roms_dir_for(picked: &Path) -> PathBuf {
    let is_emulation = picked.file_name().is_some_and(|n| n.eq_ignore_ascii_case("emulation")) || picked.join("roms").is_dir();
    if is_emulation { picked.join("roms") } else { picked.join("Emulation").join("roms") }
}

/// The Emulation folder (parent of roms/), shown in Settings.
pub fn emulation_folder(cfg: &Config) -> PathBuf {
    emulation_dir(cfg)
}

/// Where EmuHub looks for (and suggests unzipping) a standalone emulator: Emulation/tools/<id>.
pub fn emulator_dir(cfg: &Config, emu: &Emulator) -> PathBuf {
    emulation_dir(cfg).join("tools").join(emu.id)
}

/// Shows a folder in Explorer, Finder or the Linux file manager.
/// The .app bundle a macOS program lives in, e.g. /Applications/RetroArch.app for .../Contents/MacOS/RetroArch.
fn app_bundle(exe: &Path) -> Option<&Path> {
    exe.ancestors().find(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("app")))
}

/// A command that starts a program. On macOS a program inside a .app is opened through `open`, like the
/// Finder does, so it comes to the front; started directly it opens behind EmuHub. Add arguments as usual.
fn start(exe: &Path) -> Command {
    match app_bundle(exe).filter(|_| cfg!(target_os = "macos")) {
        Some(bundle) => {
            // -n: a fresh copy, so the game arguments are used even if the app is already open.
            // -W: `open` waits until the app quits, so EmuHub can tell a game is still running.
            let mut command = Command::new("open");
            command.arg("-n").arg("-W").arg("-a").arg(bundle).arg("--args");
            command
        }
        None => Command::new(exe),
    }
}

/// Opens a web page in the default browser.
/// Linux: adds EmuHub to the app menu (Hyprland/wofi/rofi, KDE, GNOME all read ~/.local/share/applications).
/// Points at the AppImage when run from one, so moving the file and relaunching fixes the entry.
pub fn add_to_app_menu() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let Some(exe) = std::env::var_os("APPIMAGE").map(PathBuf::from).or_else(|| std::env::current_exe().ok()) else { return };
    let data = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".local/share"));
    let icon = data.join("icons/hicolor/512x512/apps/emuhub.png");
    let desktop = data.join("applications/emuhub.desktop");
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=EmuHub\nComment=Emulation hub: library, setup and controller navigation\nExec=\"{}\"\nIcon={}\nCategories=Game;Emulator;\nTerminal=false\nStartupWMClass=emuhub\n",
        exe.display(),
        icon.display()
    );
    if !icon.exists() {
        let _ = write_atomic(&icon, include_bytes!("../assets/emuhub.png"));
    }
    if std::fs::read_to_string(&desktop).ok().as_deref() != Some(entry.as_str()) {
        let _ = write_atomic(&desktop, entry.as_bytes());
    }
}

pub fn open_url(url: &str) -> Result<(), String> {
    open_folder(Path::new(url))
}

pub fn open_folder(dir: &Path) -> Result<(), String> {
    let opener = if cfg!(target_os = "windows") { "explorer" } else if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    Command::new(opener).arg(dir).spawn().map(|_| ()).map_err(|e| format!("Could not open {}: {e}", dir.display()))
}

pub fn scan(roms_dir: &Path, systems: &[System]) -> Vec<Game> {
    let mut games = Vec::new();
    for s in systems {
        scan_dir(&roms_dir.join(&s.id), s, &mut games);
    }
    games.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    games
}

/// Walks a system folder and its subfolders. A folder with an .m3u playlist is one multi-disc
/// game, so its .cue/.chd discs aren't listed separately.
fn scan_dir(dir: &Path, system: &System, games: &mut Vec<Game>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    let ext_of = |p: &Path| p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let has_playlist = paths.iter().any(|p| ext_of(p) == "m3u");
    for path in paths {
        if path.is_dir() {
            let name = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
            if !["dlc", "updates", "update"].contains(&name.as_str()) {
                scan_dir(&path, system, games);
            }
            continue;
        }
        let ext = ext_of(&path);
        if path.is_file() && system.exts.contains(&ext) && (!has_playlist || ext == "m3u") {
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            // No-Intro sets include BIOS dumps named "[BIOS] ..."; they aren't games.
            if name.to_lowercase().starts_with("[bios]") {
                continue;
            }
            games.push(Game { system: system.id.clone(), name, path });
        }
    }
}

fn core_file(core: &str) -> String {
    let suffix = if cfg!(target_os = "windows") { "dll" } else if cfg!(target_os = "macos") { "dylib" } else { "so" };
    format!("{core}_libretro.{suffix}")
}

/// A running emulator, so EmuHub can tell when it's still open and close it on Share+Options.
pub struct Running {
    child: std::process::Child,
    /// Standalone emulators are closed by EmuHub; RetroArch handles the combo itself (and saves first).
    close_on_combo: bool,
    /// The game being played, for its play time; None when RetroArch was opened on its own.
    game: Option<PathBuf>,
    started: std::time::Instant,
}

impl Running {
    pub fn is_open(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Adds this session to the game's play time. Call once, after the emulator closed.
    pub fn record_playtime(&self) {
        if let Some(game) = &self.game {
            let mut times = load_playtime();
            *times.entry(game.clone()).or_default() += self.started.elapsed().as_secs();
            // Best effort, like the history: a failed write only loses this session.
            let _ = write_atomic(&config_dir().join("playtime.json"), &serde_json::to_vec(&times).unwrap());
        }
    }

    /// Asks the emulator to close, like clicking its window's X (not a force-quit, so it can save).
    pub fn close(&self) {
        if !self.close_on_combo {
            return;
        }
        let pid = self.child.id().to_string();
        let _ = if cfg!(target_os = "windows") {
            Command::new("taskkill").args(["/PID", &pid]).output()
        } else {
            Command::new("kill").args(["-TERM", &pid]).output()
        };
    }
}

/// NES light-gun games. They run in FCEUmm instead of the system's core: it draws a crosshair
/// for the Zapper (Mesen shows nothing to aim with), and port 2 is set to the Zapper.
const ZAPPER_GAMES: &[&str] = &[
    "duck hunt", "hogan's alley", "wild gunman", "gumshoe", "freedom force", "operation wolf",
    "the adventures of bayou billy", "baby boomer", "barker bill's trick shooting", "chiller",
    "mechanized attack", "shooting range", "to the earth", "lone ranger", "laser invasion", "track & field ii",
];
const ZAPPER_CORE: &str = "fceumm";
/// FCEUmm's Zapper device: RETRO_DEVICE_SUBCLASS(RETRO_DEVICE_MOUSE, 0).
const ZAPPER_DEVICE: &str = "258";

fn zapper_game(game: &Game) -> bool {
    let name = game.name.to_lowercase();
    game.system == "nes" && ZAPPER_GAMES.iter().any(|z| name.starts_with(z))
}

/// Starts a game in RetroArch or its standalone emulator.
pub fn launch(cfg: &Config, game: &Game) -> Result<Running, String> {
    let sys = systems().into_iter().find(|s| s.id == game.system).ok_or("unknown system")?;
    if let Some(emu) = emulator(&sys.standalone) {
        let mut running = launch_standalone(cfg, emu, game)?;
        running.game = Some(game.path.clone());
        record_play(game);
        return Ok(running);
    }
    let zapper = zapper_game(game);
    let core = cfg.cores_dir.join(core_file(if zapper { ZAPPER_CORE } else { &sys.core }));
    if !core.exists() {
        return Err(format!("Core not found: {}. Use Settings → Install cores.", core.display()));
    }
    let child = retroarch(cfg, zapper)?.arg("-L").arg(core).arg(&game.path).spawn().map_err(|e| format!("Could not start RetroArch: {e}"))?;
    record_play(game);
    Ok(Running { child, close_on_combo: false, game: Some(game.path.clone()), started: std::time::Instant::now() })
}

/// RetroArch, fullscreen, with EmuHub's settings for this run (see retroarch_overrides).
fn retroarch(cfg: &Config, zapper: bool) -> Result<Command, String> {
    let (exe, args) = cfg.retroarch_cmd.split_first().ok_or("RetroArch command is empty")?;
    let file = config_dir().join("retroarch-overrides.cfg");
    // Always set port 2: RetroArch saves it on exit, and the Zapper's id means something else to other cores.
    let mut port2 = format!("input_libretro_device_p2 = \"{}\"
", if zapper { ZAPPER_DEVICE } else { "1" });
    if zapper {
        // Player 2 reads the trigger from mouse 1 by default, which doesn't exist with one mouse.
        port2 += "input_player2_mouse_index = \"0\"
input_player2_gun_trigger_mbtn = \"1\"
";
    }
    write_atomic(&file, (retroarch_overrides(&cfg.roms_dir) + &port2).as_bytes())?;
    let mut command = start(Path::new(exe));
    command.args(args).arg("--fullscreen").arg("--appendconfig").arg(file);
    Ok(command)
}

// ---- standalone emulators --------------------------------------------------

/// An emulator that runs on its own instead of as a RetroArch core (systems.json "standalone").
pub struct Emulator {
    pub id: &'static str,
    pub name: &'static str,
    /// Official site, opened from Settings. EmuHub doesn't download these: some face takedowns,
    /// and unofficial mirrors aren't safe to fetch automatically.
    pub site: &'static str,
    exe: &'static str,
    /// Arguments before the game path.
    args: &'static [&'static str],
}

pub const EMULATORS: [Emulator; 6] = [
    Emulator { id: "eden", name: "Eden", site: "https://eden-emu.dev", exe: "eden", args: &["-f", "-g"] },
    Emulator { id: "pcsx2", name: "PCSX2", site: "https://pcsx2.net", exe: "pcsx2-qt", args: &["-batch", "-fullscreen", "--"] },
    Emulator { id: "dolphin", name: "Dolphin", site: "https://dolphin-emu.org", exe: "Dolphin", args: &["-b", "-e"] },
    Emulator { id: "ppsspp", name: "PPSSPP", site: "https://www.ppsspp.org", exe: if cfg!(target_os = "windows") { "PPSSPPWindows64" } else { "PPSSPPSDL" }, args: &["--fullscreen"] },
    Emulator { id: "cemu", name: "Cemu", site: "https://cemu.info", exe: "Cemu", args: &["-f", "-g"] },
    Emulator { id: "azahar", name: "Azahar", site: "https://azahar-emu.org", exe: "azahar", args: &["-f"] },
];

pub fn emulator(id: &str) -> Option<&'static Emulator> {
    EMULATORS.iter().find(|e| e.id == id)
}

/// The folder holding roms/, bios/, tools/ (EmuDeck's "Emulation" folder).
fn emulation_dir(cfg: &Config) -> PathBuf {
    cfg.roms_dir.parent().map(Path::to_path_buf).unwrap_or_default()
}

/// The emulator's program: the one picked in Settings, else the usual install places, else PATH.
pub fn find_emulator(cfg: &Config, emu: &Emulator) -> Option<PathBuf> {
    if let Some(picked) = cfg.emulators.get(emu.id).filter(|p| p.is_file()) {
        return Some(picked.clone());
    }
    let exe = if cfg!(target_os = "windows") { format!("{}.exe", emu.exe) } else { emu.exe.to_owned() };
    let mut places = vec![emulator_dir(cfg, emu).join(&exe)];
    if cfg!(target_os = "windows") {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            places.push(PathBuf::from(local).join("Programs").join(emu.id).join(&exe));
        }
    } else if cfg!(target_os = "macos") {
        // The program inside the bundle isn't always named like the Windows exe (PCSX2.app has "PCSX2").
        let macos = PathBuf::from(format!("/Applications/{}.app/Contents/MacOS", emu.name));
        places.extend(std::fs::read_dir(macos).into_iter().flatten().flatten().map(|e| e.path()));
    } else if let Ok(entries) = std::fs::read_dir(home().join("Applications")) {
        // EmuDeck keeps AppImages like ~/Applications/Eden-x86_64.AppImage.
        places.extend(entries.flatten().map(|e| e.path()).filter(|p| {
            let name = p.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
            name.starts_with(emu.id) && name.ends_with(".appimage")
        }));
    }
    if let Some(path) = std::env::var_os("PATH") {
        places.extend(std::env::split_paths(&path).map(|dir| dir.join(&exe)));
    }
    places.into_iter().find(|p| p.is_file())
}

/// Eden's data folder: "user" next to the program in portable mode, else the per-user folder.
fn eden_user_dir(exe: &Path) -> PathBuf {
    let portable = exe.parent().unwrap_or(Path::new("")).join("user");
    if portable.is_dir() {
        portable
    } else if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(home).join("eden")
    } else if cfg!(target_os = "macos") {
        home().join("Library/Application Support/eden")
    } else {
        home().join(".local/share/eden")
    }
}

/// Like EmuDeck: copies the user's own keys and firmware from Emulation/bios/eden into Eden if it has none yet.
fn prepare_eden(cfg: &Config, exe: &Path) -> Result<(), String> {
    let bios = emulation_dir(cfg).join("bios").join("eden");
    let user = eden_user_dir(exe);
    for keys in ["prod.keys", "title.keys"] {
        let (from, to) = (bios.join(keys), user.join("keys").join(keys));
        if from.is_file() && !to.exists() {
            std::fs::create_dir_all(to.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::copy(&from, &to).map_err(|e| format!("Could not copy {keys}: {e}"))?;
        }
    }
    let registered = user.join("nand/system/Contents/registered");
    let has_firmware = std::fs::read_dir(&registered).is_ok_and(|mut d| d.next().is_some());
    let mut zips: Vec<PathBuf> = std::fs::read_dir(&bios)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().unwrap_or_default().to_string_lossy().to_lowercase().starts_with("firmware") && p.extension().is_some_and(|e| e == "zip"))
        .collect();
    zips.sort(); // shortcut: picks the last by name, which is the newest for EmuDeck's "Firmware.X.Y.Z.zip" names
    if let (false, Some(zip)) = (has_firmware, zips.last()) {
        let file = std::fs::File::open(zip).map_err(|e| e.to_string())?;
        let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&registered).map_err(|e| e.to_string())?;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
            // Keep only the file name so nothing in the archive can write outside the firmware folder.
            let Some(name) = Path::new(entry.name()).file_name().map(|n| n.to_owned()) else { continue };
            if entry.is_dir() {
                continue;
            }
            let mut out = std::fs::File::create(registered.join(name)).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn launch_standalone(cfg: &Config, emu: &Emulator, game: &Game) -> Result<Running, String> {
    let exe = find_emulator(cfg, emu).ok_or_else(|| format!("{} isn't installed. Get it in Settings → Emulators, then choose its program.", emu.name))?;
    if emu.id == "eden" {
        prepare_eden(cfg, &exe)?;
    }
    let child = start(&exe).args(emu.args).arg(&game.path).spawn().map_err(|e| format!("Could not start {}: {e}", emu.name))?;
    // On macOS the child is `open -W` (it waits for the app), so closing it wouldn't close the emulator.
    // shortcut: the combo doesn't close macOS standalone emulators; quit the app by bundle id if needed.
    let mac_app = cfg!(target_os = "macos") && app_bundle(&exe).is_some();
    Ok(Running { child, close_on_combo: !mac_app, game: None, started: std::time::Instant::now() })
}

// ---- play history ----------------------------------------------------------

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// When each game was last played (unix seconds), keyed by its file path.
pub fn load_history() -> std::collections::HashMap<PathBuf, u64> {
    std::fs::read(config_dir().join("history.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn record_play(game: &Game) {
    let mut history = load_history();
    history.insert(game.path.clone(), now());
    // Best effort: a failed write only loses the "Continue Playing" order.
    let _ = write_atomic(&config_dir().join("history.json"), &serde_json::to_vec(&history).unwrap());
}

/// Seconds played per game, keyed by its file path.
pub fn load_playtime() -> std::collections::HashMap<PathBuf, u64> {
    std::fs::read(config_dir().join("playtime.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// "2h 15m played", "40m played", "Under 1m played", or None if never played.
pub fn playtime_text(secs: u64) -> Option<String> {
    match (secs / 3600, secs / 60 % 60) {
        _ if secs == 0 => None,
        (0, 0) => Some("Under 1m played".into()),
        (0, m) => Some(format!("{m}m played")),
        (h, m) => Some(format!("{h}h {m}m played")),
    }
}

/// Favorite games, by file path.
pub fn load_favorites() -> std::collections::HashSet<PathBuf> {
    std::fs::read(config_dir().join("favorites.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn save_favorites(favorites: &std::collections::HashSet<PathBuf>) -> Result<(), String> {
    write_atomic(&config_dir().join("favorites.json"), &serde_json::to_vec(favorites).unwrap())
}

/// "35 seconds ago", "5 days ago".
pub fn ago(when: u64) -> String {
    let secs = now().saturating_sub(when);
    let (n, unit) = match secs {
        0..60 => (secs, "second"),
        60..3600 => (secs / 60, "minute"),
        3600..86400 => (secs / 3600, "hour"),
        86400..2_592_000 => (secs / 86400, "day"),
        2_592_000..31_536_000 => (secs / 2_592_000, "month"),
        _ => (secs / 31_536_000, "year"),
    };
    format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" })
}

/// RetroArch settings for this launch only: use EmuDeck's bios and save folders next to roms/ when they exist,
/// so BIOS files and saves copied from a Steam Deck work without moving anything.
fn retroarch_overrides(roms_dir: &Path) -> String {
    // Share+Options (Select+Start) quits right away; RetroArch then writes saves before closing.
    // 4 is RetroArch's "Start + Select" combo.
    let mut text = String::from("input_quit_gamepad_combo = \"4\"\nquit_press_twice = \"false\"\n");
    // Intel Macs stutter on Vulkan (MoltenVK) and on RetroArch's Metal driver; glcore ran smoothly on a 2017 MacBook.
    // Threaded video stays off: macOS OpenGL must draw on the main thread, and RetroArch crashed with it on.
    if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        text += "video_driver = \"glcore\"\nvideo_threaded = \"false\"\n";
    }
    let Some(emulation) = roms_dir.parent() else { return text };
    for (key, rel) in [
        ("system_directory", "bios"),
        ("savefile_directory", "saves/retroarch/saves"),
        ("savestate_directory", "saves/retroarch/states"),
    ] {
        let dir = emulation.join(rel);
        if dir.is_dir() {
            text += &format!("{key} = \"{}\"\n", dir.display());
        }
    }
    text
}

/// Path on buildbot.libretro.com for this OS and CPU.
fn buildbot_platform() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", _) => "windows/x86_64",
        ("macos", "aarch64") => "apple/osx/arm64",
        ("macos", _) => "apple/osx/x86_64",
        // shortcut: ARM Linux (e.g. Raspberry Pi) not handled; add its buildbot path if needed.
        _ => "linux/x86_64",
    }
}

pub(crate) fn download(url: &str, auth: Option<&str>) -> Result<Vec<u8>, String> {
    let mut req = ureq::get(url);
    if let Some(auth) = auth {
        req = req.set("Authorization", auth);
    }
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut req.call().map_err(|e| e.to_string())?.into_reader(), &mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes)
}

/// Writes through a temp file so a failed download never leaves a half-written file behind.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("part");
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// Every core systems.json needs, without duplicates.
fn needed_cores() -> Vec<String> {
    let mut cores: Vec<String> = systems().into_iter().map(|s| s.core).filter(|c| !c.is_empty()).collect();
    cores.push(ZAPPER_CORE.to_owned());
    cores.sort();
    cores.dedup();
    cores
}

/// Returns (missing, total) core counts for the Settings page.
pub fn core_status(cfg: &Config) -> (usize, usize) {
    let cores = needed_cores();
    let missing = cores.iter().filter(|c| !cfg.cores_dir.join(core_file(c)).exists()).count();
    (missing, cores.len())
}

/// Whether the configured RetroArch command points at something installed.
pub fn retroarch_installed(cfg: &Config) -> bool {
    match cfg.retroarch_cmd.first().map(String::as_str) {
        Some("flatpak") => [home().join(".local/share/flatpak/app/org.libretro.RetroArch"), PathBuf::from("/var/lib/flatpak/app/org.libretro.RetroArch")]
            .iter()
            .any(|p| p.is_dir()),
        Some(exe) if Path::new(exe).is_absolute() => Path::new(exe).exists(),
        // shortcut: a bare command like "retroarch" is assumed to be on PATH; check PATH if that misleads people.
        Some(_) => true,
        None => false,
    }
}

/// RetroArch 1.21.0 for Macs: 1.22.x lags everywhere (menus too) on Intel Macs; 1.21.0 runs smoothly.
pub const RETROARCH_INTEL_MAC_URL: &str = "https://buildbot.libretro.com/stable/1.21.0/apple/osx/universal/";

/// Intel Mac only: a warning when the installed RetroArch is a 1.22 release (read from its Info.plist).
pub fn retroarch_version_warning(cfg: &Config) -> Option<String> {
    if !cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        return None;
    }
    // .../RetroArch.app/Contents/MacOS/RetroArch → .../RetroArch.app/Contents/Info.plist
    let plist = Path::new(cfg.retroarch_cmd.first()?).parent()?.parent()?.join("Info.plist");
    let version = plist_version(&std::fs::read_to_string(plist).ok()?)?;
    version.starts_with("1.22").then(|| format!("RetroArch {version} lags on Intel Macs. Install 1.21.0 over it (Download)."))
}

/// CFBundleShortVersionString from an XML Info.plist.
fn plist_version(plist: &str) -> Option<String> {
    let after = plist.split("<key>CFBundleShortVersionString</key>").nth(1)?;
    Some(after.split("<string>").nth(1)?.split("</string>").next()?.trim().to_owned())
}

/// Starts RetroArch on its own, e.g. to change its settings.
pub fn open_retroarch(cfg: &Config) -> Result<Running, String> {
    let child = retroarch(cfg, false)?.spawn().map_err(|e| format!("Could not start RetroArch: {e}"))?;
    Ok(Running { child, close_on_combo: false, game: None, started: std::time::Instant::now() })
}

/// Downloads every core systems.json needs that isn't already in cores_dir. Returns how many were installed.
pub fn install_cores(cfg: &Config) -> Result<usize, String> {
    let mut installed = 0;
    for core in needed_cores() {
        let file = core_file(&core);
        if cfg.cores_dir.join(&file).exists() {
            continue;
        }
        let url = format!("https://buildbot.libretro.com/nightly/{}/latest/{file}.zip", buildbot_platform());
        let bytes = download(&url, None).map_err(|e| format!("{core}: {e}"))?;
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| format!("{core}: {e}"))?;
        // Only extract the expected core file, never paths from inside the archive.
        let mut entry = zip.by_name(&file).map_err(|e| format!("{core}: {e}"))?;
        let mut data = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut data).map_err(|e| e.to_string())?;
        write_atomic(&cfg.cores_dir.join(&file), &data)?;
        installed += 1;
    }
    Ok(installed)
}

/// Installs RetroArch with the OS package manager; each lands where default_config expects it.
pub fn install_retroarch() -> Result<(), String> {
    let (exe, args): (&str, &[&str]) = if cfg!(target_os = "windows") {
        ("winget", &["install", "--exact", "--id", "Libretro.RetroArch", "--silent", "--accept-package-agreements", "--accept-source-agreements"])
    } else if cfg!(target_os = "macos") {
        // Homebrew's own folders (Intel, Apple Silicon) often aren't on the PATH an app starts with.
        // Homebrew no longer supports Intel Macs, so only suggest it on Apple Silicon.
        let missing = if cfg!(target_arch = "aarch64") {
            "Homebrew isn't installed. Get RetroArch from retroarch.com (drag it into Applications), or install Homebrew from brew.sh."
        } else {
            "Get RetroArch 1.21.0 from buildbot.libretro.com/stable/1.21.0/apple/osx/universal and drag it into Applications. Avoid 1.22, which lags on Intel Macs."
        };
        let brew = ["/usr/local/bin/brew", "/opt/homebrew/bin/brew"].into_iter().find(|p| Path::new(p).is_file()).ok_or(missing)?;
        (brew, &["install", "--cask", "retroarch"])
    } else {
        ("flatpak", &["install", "--user", "-y", "flathub", "org.libretro.RetroArch"])
    };
    let out = Command::new(exe).args(args).output().map_err(|e| format!("Could not run {exe}: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("{exe} failed: {}", String::from_utf8_lossy(&out.stderr).trim()))
    }
}

// ---- cover art ------------------------------------------------------------

/// libretro-thumbnails replaces these characters with "_" in its file names.
fn thumb_name(name: &str) -> String {
    name.chars().map(|c| if "&*/:`<>?\\|\"".contains(c) { '_' } else { c }).collect()
}

fn url_encode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// "Super Mario World (USA) (Rev 1)" -> "Super Mario World", so the search matches SteamGridDB titles.
fn search_title(name: &str) -> &str {
    name.split(" (").next().unwrap_or(name).split(" [").next().unwrap_or(name).trim()
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(b)) => {
                out.push(b);
                i += 3;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn cover_dir(game: &Game) -> PathBuf {
    config_dir().join("covers").join(&game.system)
}

/// The automatically downloaded cover.
fn default_cover(game: &Game) -> PathBuf {
    cover_dir(game).join(format!("{}.png", thumb_name(&game.name)))
}

/// Where a game's cover is on disk. A cover picked in the details window is saved under a new file name
/// (GPUI caches images by path, so overwriting wouldn't show) and recorded in a ".cover" file.
pub fn cover_path(game: &Game) -> PathBuf {
    let default = default_cover(game);
    match std::fs::read_to_string(default.with_extension("cover")) {
        Ok(file) if !file.trim().is_empty() => cover_dir(game).join(file.trim()),
        _ => default,
    }
}

fn libretro_url(sys: &System, name: &str) -> String {
    libretro_media(sys, "Named_Boxarts", name)
}

/// A libretro-thumbnails image: "Named_Boxarts" (covers) or "Named_Snaps" (in-game screenshots).
fn libretro_media(sys: &System, folder: &str, name: &str) -> String {
    format!("https://thumbnails.libretro.com/{}/{folder}/{}.png", url_encode(&sys.thumbs), url_encode(name))
}

/// Every box-art name libretro has for a system, cached on disk because the listing is about 2 MB.
fn libretro_names(sys: &System) -> Vec<String> {
    let cache = config_dir().join("listings").join(format!("{}.txt", sys.id));
    // shortcut: the listing is cached forever; delete the listings folder to pick up newly added libretro art.
    let text = match std::fs::read_to_string(&cache) {
        Ok(text) => text,
        Err(_) => {
            let Ok(html) = download(&format!("https://thumbnails.libretro.com/{}/Named_Boxarts/", url_encode(&sys.thumbs)), None) else {
                return vec![];
            };
            let text = parse_listing(&String::from_utf8_lossy(&html)).join("\n");
            let _ = write_atomic(&cache, text.as_bytes());
            text
        }
    };
    text.lines().map(String::from).collect()
}

/// Pulls the .png names out of libretro's directory listing page.
fn parse_listing(html: &str) -> Vec<String> {
    html.split("href=\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .filter_map(|href| href.strip_suffix(".png"))
        .map(percent_decode)
        .collect()
}

/// libretro names for the same title, best first: USA, then World/Europe, then other regions; demos and betas last.
fn libretro_matches(game: &Game, names: &[String]) -> Vec<String> {
    let title = search_title(&thumb_name(&game.name)).to_lowercase();
    let mut found: Vec<&String> = names.iter().filter(|n| search_title(n).to_lowercase() == title).collect();
    found.sort_by_key(|n| {
        let n = n.to_lowercase();
        let junk = ["demo", "beta", "proto", "kiosk", "sample"].iter().any(|w| n.contains(w));
        (junk, !n.contains("usa"), !(n.contains("world") || n.contains("europe")))
    });
    found.into_iter().cloned().collect()
}

/// (preview, full image) URL pairs from SteamGridDB for the game's title, portrait grids only.
fn steamgrid_grids(key: &str, name: &str) -> Result<Vec<(String, String)>, String> {
    let auth = format!("Bearer {key}");
    let get = |url: &str| -> Result<serde_json::Value, String> {
        serde_json::from_slice(&download(url, Some(&auth))?).map_err(|e| e.to_string())
    };
    let found = get(&format!("https://www.steamgriddb.com/api/v2/search/autocomplete/{}", url_encode(search_title(name))))?;
    let Some(id) = found["data"][0]["id"].as_u64() else { return Ok(vec![]) };
    let grids = get(&format!("https://www.steamgriddb.com/api/v2/grids/game/{id}?dimensions=600x900"))?;
    Ok(grids["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|g| Some((g["thumb"].as_str()?.to_owned(), g["url"].as_str()?.to_owned())))
        .collect())
}

/// Asks SteamGridDB whether the key works, with a message fit to show the user if it doesn't.
pub fn check_steamgriddb_key(key: &str) -> Result<(), String> {
    match ureq::get("https://www.steamgriddb.com/api/v2/search/autocomplete/mario").set("Authorization", &format!("Bearer {key}")).call() {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(401, _)) => Err("SteamGridDB says this API key is invalid. Copy it again from steamgriddb.com → Preferences → API.".into()),
        Err(error) => Err(format!("Could not reach SteamGridDB: {error}")),
    }
}

/// Forgets which games had no cover, so they're looked up again (e.g. with a new SteamGridDB key).
pub fn forget_cover_misses() {
    for system in std::fs::read_dir(config_dir().join("covers")).into_iter().flatten().flatten() {
        for file in std::fs::read_dir(system.path()).into_iter().flatten().flatten() {
            if file.path().extension().is_some_and(|e| e == "nocover") {
                let _ = std::fs::remove_file(file.path());
            }
        }
    }
}

/// Downloads a cover into the cache: libretro by exact name, then libretro's closest name, then SteamGridDB
/// if a key is set. Returns true if a cover now exists. A miss is remembered so it's only tried once.
pub fn fetch_cover(game: &Game, steamgriddb_key: &str) -> bool {
    if cover_path(game).exists() {
        return true;
    }
    // Earlier versions wrote ".miss" before fuzzy matching existed; those are ignored so the game gets another try.
    let miss = default_cover(game).with_extension("nocover");
    if miss.exists() {
        return false;
    }
    let Some(sys) = systems().into_iter().find(|s| s.id == game.system) else { return false };
    let mut bytes = if sys.thumbs.is_empty() { None } else { download(&libretro_url(&sys, &thumb_name(&game.name)), None).ok() };
    if bytes.is_none() && !sys.thumbs.is_empty() {
        if let Some(best) = libretro_matches(game, &libretro_names(&sys)).first() {
            bytes = download(&libretro_url(&sys, best), None).ok();
        }
    }
    if bytes.is_none() && !steamgriddb_key.is_empty() {
        match steamgrid_grids(steamgriddb_key, &game.name) {
            Ok(grids) => bytes = grids.first().and_then(|(_, url)| download(url, None).ok()),
            Err(_) => return false, // network/API error: don't record a miss, retry next launch
        }
    }
    match bytes {
        Some(bytes) => write_atomic(&default_cover(game), &shrink_cover(&bytes)).is_ok(),
        None => {
            let _ = write_atomic(&miss, b"");
            false
        }
    }
}

/// A cover the user can pick in the details window.
#[derive(Clone)]
pub struct Candidate {
    pub preview: PathBuf,
    pub url: String,
    /// Region for libretro choices ("USA, Australia"), or "SteamGridDB".
    pub label: String,
}

/// Downloads previews of every cover option: libretro's matching regions, then SteamGridDB's grids.
pub fn cover_candidates(game: &Game, steamgriddb_key: &str) -> Vec<Candidate> {
    use std::hash::{Hash, Hasher};
    let Some(sys) = systems().into_iter().find(|s| s.id == game.system) else { return vec![] };
    let mut options: Vec<(String, String, String)> = libretro_matches(game, &libretro_names(&sys))
        .into_iter()
        .take(8)
        .map(|name| {
            let url = libretro_url(&sys, &name);
            let region = name.get(search_title(&name).len()..).unwrap_or("").trim().replace(") (", ", ");
            (url.clone(), url, region.trim_matches(['(', ')']).to_owned())
        })
        .collect();
    if !steamgriddb_key.is_empty() {
        let grids = steamgrid_grids(steamgriddb_key, &game.name).unwrap_or_default();
        options.extend(grids.into_iter().take(12).map(|(thumb, url)| (thumb, url, "SteamGridDB".to_owned())));
    }
    let dir = config_dir().join("previews");
    options
        .into_iter()
        .filter_map(|(preview_url, url, label)| {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            preview_url.hash(&mut hasher);
            let preview = dir.join(format!("{:016x}.img", hasher.finish()));
            if !preview.exists() {
                write_atomic(&preview, &download(&preview_url, None).ok()?).ok()?;
            }
            Some(Candidate { preview, url, label })
        })
        .collect()
}

/// Downloads the chosen cover and makes it the game's cover.
/// Covers are drawn at most ~240px wide, so 360px stays sharp on high-DPI screens. libretro's
/// full-size box art is 5-10x bigger, which made every scroll decode megabytes per card.
const COVER_WIDTH: u32 = 360;

/// Whether a cover of this size isn't 2:3 (within 1%), the shape of every cover box.
fn off_shape(w: u32, h: u32) -> bool {
    (w as f32 * 1.5 - h as f32).abs() > h as f32 * 0.01
}

/// The image center-cropped to 2:3 and scaled down to COVER_WIDTH as PNG, or the original bytes if it
/// already fits or is not an image. Cropped here because GPUI rounds an image's corners on its fitted
/// size, so art that overflows its box would show square corners.
fn shrink_cover(bytes: &[u8]) -> Vec<u8> {
    let Ok(mut img) = image::load_from_memory(bytes) else { return bytes.to_vec() };
    let (w, h) = (img.width(), img.height());
    if w <= COVER_WIDTH && !off_shape(w, h) {
        return bytes.to_vec();
    }
    if off_shape(w, h) {
        let (cw, ch) = if h * 2 > w * 3 { (w, w * 3 / 2) } else { (h * 2 / 3, h) };
        img = img.crop_imm((w - cw) / 2, (h - ch) / 2, cw, ch);
    }
    let small = if img.width() > COVER_WIDTH { img.resize(COVER_WIDTH, u32::MAX, image::imageops::FilterType::Triangle) } else { img };
    let mut out = std::io::Cursor::new(Vec::new());
    match small.write_to(&mut out, image::ImageFormat::Png) {
        Ok(()) => out.into_inner(),
        Err(_) => bytes.to_vec(),
    }
}

/// The cover's most colorful shade as 0xRRGGBB, for the ambient background: saturated pixels count more
/// so a red logo beats a grey sky.
pub fn cover_color(path: &Path) -> Option<u32> {
    let small = image::open(path).ok()?.thumbnail(24, 36).to_rgb8();
    let (mut sum, mut weight) = ([0f32; 3], 0f32);
    for p in small.pixels() {
        let [r, g, b] = p.0.map(f32::from);
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let w = (max - min) / 255. * (max / 255.) + 0.01;
        sum = [sum[0] + r * w, sum[1] + g * w, sum[2] + b * w];
        weight += w;
    }
    let [r, g, b] = sum.map(|c| (c / weight) as u32);
    Some(r << 16 | g << 8 | b)
}

/// Shrinks a cover saved by an older EmuHub in place. Cheap when it's already small: only the file size
/// and image header are read, so this doesn't decode every big-but-narrow cover on each launch.
pub fn shrink_cached_cover(path: &Path) {
    if image::image_dimensions(path).is_ok_and(|(w, h)| w > COVER_WIDTH || off_shape(w, h)) {
        if let Ok(bytes) = std::fs::read(path) {
            let fixed = shrink_cover(&bytes);
            if fixed != bytes {
                let _ = write_atomic(path, &fixed);
            }
        }
    }
}

pub fn set_cover(game: &Game, url: &str) -> Result<(), String> {
    let bytes = download(url, None)?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
    let file = format!("{}.{stamp}.png", thumb_name(&game.name));
    write_atomic(&cover_dir(game).join(&file), &shrink_cover(&bytes))?;
    write_atomic(&default_cover(game).with_extension("cover"), file.as_bytes())
}

/// Where a game's in-game screenshot (used for the Home hero image) is cached.
pub fn snap_path(game: &Game) -> PathBuf {
    cover_dir(game).join(format!("{}.snap.png", thumb_name(&game.name)))
}

/// Downloads a game's screenshot from libretro, by exact name then by closest name. Returns true if one exists.
pub fn fetch_snap(game: &Game) -> bool {
    let path = snap_path(game);
    let miss = path.with_extension("nosnap");
    if path.exists() {
        return true;
    }
    if miss.exists() {
        return false;
    }
    let Some(sys) = systems().into_iter().find(|s| s.id == game.system).filter(|s| !s.thumbs.is_empty()) else { return false };
    let mut bytes = download(&libretro_media(&sys, "Named_Snaps", &thumb_name(&game.name)), None).ok();
    if bytes.is_none() {
        if let Some(best) = libretro_matches(game, &libretro_names(&sys)).first() {
            bytes = download(&libretro_media(&sys, "Named_Snaps", best), None).ok();
        }
    }
    match bytes {
        Some(bytes) => write_atomic(&path, &bytes).is_ok(),
        None => {
            let _ = write_atomic(&miss, b"");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_color_prefers_the_colorful_part() {
        let path = std::env::temp_dir().join("emuhub-color-test.png");
        let img = image::RgbImage::from_fn(10, 10, |x, _| if x < 7 { image::Rgb([128, 128, 128]) } else { image::Rgb([230, 20, 20]) });
        img.save(&path).unwrap();
        let color = cover_color(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(color >> 16 > (color >> 8 & 0xff) + 60, "{color:06x} should be reddish");
    }

    #[test]
    fn playtime_reads_in_hours_and_minutes() {
        assert_eq!(playtime_text(0), None);
        assert_eq!(playtime_text(19), Some("Under 1m played".into()));
        assert_eq!(playtime_text(40 * 60), Some("40m played".into()));
        assert_eq!(playtime_text(2 * 3600 + 15 * 60 + 30), Some("2h 15m played".into()));
    }

    #[test]
    fn scan_matches_extensions_per_system() {
        let dir = std::env::temp_dir().join("emuhub_scan_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("snes")).unwrap();
        std::fs::write(dir.join("snes/Zelda.SFC"), b"").unwrap();
        std::fs::write(dir.join("snes/readme.txt"), b"").unwrap();
        std::fs::write(dir.join("snes/[BIOS] Super Game Boy (USA).sfc"), b"").unwrap();
        let games = scan(&dir, &systems());
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].name, "Zelda");
        assert_eq!(games[0].system, "snes");
    }

    #[test]
    fn scan_finds_subfolders_and_prefers_playlists() {
        let dir = std::env::temp_dir().join("emuhub_scan_sub_test");
        let _ = std::fs::remove_dir_all(&dir);
        let ff7 = dir.join("psx/Final Fantasy VII");
        std::fs::create_dir_all(&ff7).unwrap();
        for f in ["Final Fantasy VII.m3u", "Disc 1.chd", "Disc 2.chd"] {
            std::fs::write(ff7.join(f), b"").unwrap();
        }
        std::fs::write(dir.join("psx/Crash.chd"), b"").unwrap();
        let names: Vec<String> = scan(&dir, &systems()).into_iter().map(|g| g.name).collect();
        assert_eq!(names, ["Crash", "Final Fantasy VII"]);
    }

    #[test]
    fn zapper_games_are_nes_light_gun_titles() {
        let game = |name: &str, system: &str| Game { name: name.into(), system: system.into(), path: PathBuf::new() };
        assert!(zapper_game(&game("Duck Hunt (World)", "nes")));
        assert!(!zapper_game(&game("Duck Hunt (World)", "snes")));
        assert!(!zapper_game(&game("Super Mario Bros. (World)", "nes")));
        assert!(needed_cores().contains(&ZAPPER_CORE.to_owned()));
    }

    #[test]
    fn retroarch_overrides_point_at_emudeck_folders() {
        let dir = std::env::temp_dir().join("emuhub_override_test/Emulation");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bios")).unwrap();
        let text = retroarch_overrides(&dir.join("roms"));
        assert!(text.contains("system_directory") && !text.contains("savefile_directory"));
        assert!(text.contains("input_quit_gamepad_combo = \"4\""));
    }

    #[test]
    fn plist_version_is_read() {
        let plist = "<dict>
	<key>CFBundleShortVersionString</key>
	<string>1.22.2</string>
</dict>";
        assert_eq!(plist_version(plist).as_deref(), Some("1.22.2"));
        assert_eq!(plist_version("<dict></dict>"), None);
    }

    #[test]
    fn big_covers_are_shrunk() {
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(800, 1200).write_to(&mut png, image::ImageFormat::Png).unwrap();
        let small = image::load_from_memory(&shrink_cover(png.get_ref())).unwrap();
        assert_eq!((small.width(), small.height()), (360, 540));
        assert_eq!(shrink_cover(b"not an image"), b"not an image");
        // Square art is cropped to the 2:3 cover shape.
        let mut square = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(300, 300).write_to(&mut square, image::ImageFormat::Png).unwrap();
        let cropped = image::load_from_memory(&shrink_cover(square.get_ref())).unwrap();
        assert_eq!((cropped.width(), cropped.height()), (200, 300));
    }

    #[test]
    fn existing_emulation_folder_is_found() {
        let root = std::env::temp_dir().join("emuhub_find_test");
        let _ = std::fs::remove_dir_all(&root);
        let (home, drive) = (root.join("home"), root.join("D"));
        std::fs::create_dir_all(drive.join("Emulation/roms")).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        assert_eq!(find_roms_dir(&home, &[home.clone(), drive.clone()]), drive.join("Emulation/roms"));
        assert_eq!(find_roms_dir(&home, &[home.clone()]), home.join("Emulation/roms"));
    }

    #[test]
    fn names_are_cleaned_for_lookups() {
        assert_eq!(search_title("Super Mario World (USA) (Rev 1)"), "Super Mario World");
        assert_eq!(search_title("Sonic [!]"), "Sonic");
        assert_eq!(thumb_name("Q*bert: Arcade"), "Q_bert_ Arcade");
        assert_eq!(url_encode("A B(1)"), "A%20B%281%29");
        assert_eq!(percent_decode("Mario%20Kart%20DS%20(USA,%20Australia)"), "Mario Kart DS (USA, Australia)");
    }

    #[test]
    fn eden_gets_keys_and_firmware_from_the_backup() {
        let root = std::env::temp_dir().join("emuhub_eden_test");
        let _ = std::fs::remove_dir_all(&root);
        let bios = root.join("Emulation/bios/eden");
        std::fs::create_dir_all(&bios).unwrap();
        std::fs::write(bios.join("prod.keys"), b"keys").unwrap();
        let mut zip = zip::ZipWriter::new(std::fs::File::create(bios.join("Firmware.21.1.0.zip")).unwrap());
        // "../evil.nca" checks that archive paths can't escape the firmware folder.
        for name in ["a.nca", "sub/b.nca", "../evil.nca"] {
            zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut zip, b"x").unwrap();
        }
        zip.finish().unwrap();
        let exe = root.join("eden/eden.exe");
        std::fs::create_dir_all(root.join("eden/user")).unwrap(); // portable mode
        let cfg = Config { roms_dir: root.join("Emulation/roms"), ..default_config(&root) };
        prepare_eden(&cfg, &exe).unwrap();
        let user = root.join("eden/user");
        assert_eq!(std::fs::read(user.join("keys/prod.keys")).unwrap(), b"keys");
        let registered = user.join("nand/system/Contents/registered");
        let mut files: Vec<String> = std::fs::read_dir(&registered).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into()).collect();
        files.sort();
        assert_eq!(files, ["a.nca", "b.nca", "evil.nca"]);
        assert!(!root.join("eden/user/nand/system/Contents/evil.nca").exists());
    }

    #[test]
    fn create_folders_builds_the_emudeck_layout_and_keeps_files() {
        let root = std::env::temp_dir().join("emuhub_folders_test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Emulation/bios/eden")).unwrap();
        std::fs::write(root.join("Emulation/bios/eden/prod.keys"), b"keys").unwrap();
        let cfg = Config { roms_dir: root.join("Emulation/roms"), ..default_config(&root) };
        create_folders(&cfg).unwrap();
        create_folders(&cfg).unwrap(); // running twice is fine
        assert!(root.join("Emulation/roms/switch").is_dir());
        assert!(root.join("Emulation/tools/eden").is_dir());
        assert_eq!(std::fs::read(root.join("Emulation/bios/eden/prod.keys")).unwrap(), b"keys");
    }

    #[test]
    fn picked_folders_become_an_emulation_folder() {
        let root = std::env::temp_dir().join("emuhub_pick_test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Backup/roms")).unwrap();
        assert_eq!(roms_dir_for(&root.join("Backup")), root.join("Backup/roms")); // existing copy
        assert_eq!(roms_dir_for(&root.join("Emulation")), root.join("Emulation/roms"));
        assert_eq!(roms_dir_for(&root.join("Games")), root.join("Games/Emulation/roms")); // new install spot
    }

    #[test]
    fn switch_dlc_and_updates_are_not_games() {
        let dir = std::env::temp_dir().join("emuhub_switch_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("switch/dlc")).unwrap();
        std::fs::create_dir_all(dir.join("switch/updates")).unwrap();
        std::fs::write(dir.join("switch/Kirby.xci"), b"").unwrap();
        std::fs::write(dir.join("switch/dlc/Kirby DLC.nsp"), b"").unwrap();
        std::fs::write(dir.join("switch/updates/Kirby v1.nsp"), b"").unwrap();
        let names: Vec<String> = scan(&dir, &systems()).into_iter().map(|g| g.name).collect();
        assert_eq!(names, ["Kirby"]);
    }

    #[test]
    fn mac_programs_are_found_inside_their_app_bundle() {
        let exe = Path::new("/Applications/RetroArch.app/Contents/MacOS/RetroArch");
        assert_eq!(app_bundle(exe), Some(Path::new("/Applications/RetroArch.app")));
        assert_eq!(app_bundle(Path::new("/usr/bin/retroarch")), None);
    }

    #[test]
    fn ago_reads_naturally() {
        assert_eq!(ago(now() - 35), "35 seconds ago");
        assert_eq!(ago(now() - 3600), "1 hour ago");
        assert_eq!(ago(now() - 5 * 86400), "5 days ago");
    }

    #[test]
    fn short_names_find_the_best_libretro_region() {
        let html = r#"<a href="Mario%20Kart%20DS%20(Japan).png">x</a><a href="Mario%20Kart%20DS%20(Europe)%20(Demo).png">x</a>
            <a href="Mario%20Kart%20DS%20(USA,%20Australia).png">x</a><a href="Mario%20Party%20DS%20(USA).png">x</a>"#;
        let names = parse_listing(html);
        let game = Game { system: "nds".into(), name: "Mario Kart DS".into(), path: PathBuf::new() };
        assert_eq!(libretro_matches(&game, &names), ["Mario Kart DS (USA, Australia)", "Mario Kart DS (Japan)", "Mario Kart DS (Europe) (Demo)"]);
    }
}
