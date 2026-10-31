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
    pub core: String,
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

fn default_config(home: &Path) -> Config {
    let roms_dir = home.join("Emulation").join("roms");
    let steamgriddb_key = String::new();
    if cfg!(target_os = "windows") {
        let ra = PathBuf::from(r"C:\RetroArch-Win64");
        Config { roms_dir, retroarch_cmd: vec![ra.join("retroarch.exe").display().to_string()], cores_dir: ra.join("cores"), steamgriddb_key }
    } else if cfg!(target_os = "macos") {
        Config {
            roms_dir,
            retroarch_cmd: vec!["/Applications/RetroArch.app/Contents/MacOS/RetroArch".into()],
            cores_dir: home.join("Library/Application Support/RetroArch/cores"),
            steamgriddb_key,
        }
    } else {
        // SteamOS/EmuDeck installs RetroArch as a Flatpak.
        Config {
            roms_dir,
            retroarch_cmd: vec!["flatpak".into(), "run".into(), "org.libretro.RetroArch".into()],
            cores_dir: home.join(".var/app/org.libretro.RetroArch/config/retroarch/cores"),
            steamgriddb_key,
        }
    }
}

impl Config {
    pub fn load() -> Self {
        std::fs::read(config_dir().join("config.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_else(|| default_config(&home()))
    }
    pub fn save(&self) -> Result<(), String> {
        let dir = config_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::fs::write(dir.join("config.json"), serde_json::to_vec_pretty(self).unwrap()).map_err(|e| e.to_string())
    }
}

pub fn create_folders(cfg: &Config) -> Result<(), String> {
    for s in systems() {
        std::fs::create_dir_all(cfg.roms_dir.join(&s.id)).map_err(|e| e.to_string())?;
    }
    Ok(())
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
            scan_dir(&path, system, games);
            continue;
        }
        let ext = ext_of(&path);
        if path.is_file() && system.exts.contains(&ext) && (!has_playlist || ext == "m3u") {
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            games.push(Game { system: system.id.clone(), name, path });
        }
    }
}

fn core_file(core: &str) -> String {
    let suffix = if cfg!(target_os = "windows") { "dll" } else if cfg!(target_os = "macos") { "dylib" } else { "so" };
    format!("{core}_libretro.{suffix}")
}

pub fn launch(cfg: &Config, game: &Game) -> Result<(), String> {
    let sys = systems().into_iter().find(|s| s.id == game.system).ok_or("unknown system")?;
    let core = cfg.cores_dir.join(core_file(&sys.core));
    if !core.exists() {
        return Err(format!("Core not found: {}. Use Settings → Install cores.", core.display()));
    }
    let (exe, args) = cfg.retroarch_cmd.split_first().ok_or("RetroArch command is empty")?;
    let mut command = Command::new(exe);
    command.args(args);
    let overrides = retroarch_overrides(&cfg.roms_dir);
    if !overrides.is_empty() {
        let file = config_dir().join("retroarch-overrides.cfg");
        write_atomic(&file, overrides.as_bytes())?;
        command.arg("--appendconfig").arg(file);
    }
    command
        .arg("-L")
        .arg(core)
        .arg(&game.path)
        .spawn()
        .map_err(|e| format!("Could not start RetroArch: {e}"))?;
    Ok(())
}

/// RetroArch settings for this launch only: use EmuDeck's bios and save folders next to roms/ when they exist,
/// so BIOS files and saves copied from a Steam Deck work without moving anything.
fn retroarch_overrides(roms_dir: &Path) -> String {
    let Some(emulation) = roms_dir.parent() else { return String::new() };
    let mut text = String::new();
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

fn download(url: &str, auth: Option<&str>) -> Result<Vec<u8>, String> {
    let mut req = ureq::get(url);
    if let Some(auth) = auth {
        req = req.set("Authorization", auth);
    }
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut req.call().map_err(|e| e.to_string())?.into_reader(), &mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes)
}

/// Writes through a temp file so a failed download never leaves a half-written file behind.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("part");
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// Downloads every core systems.json needs that isn't already in cores_dir. Returns how many were installed.
pub fn install_cores(cfg: &Config) -> Result<usize, String> {
    let mut cores: Vec<String> = systems().into_iter().map(|s| s.core).collect();
    cores.sort();
    cores.dedup();
    let mut installed = 0;
    for core in cores {
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
        ("brew", &["install", "--cask", "retroarch"])
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

/// Where a game's cover is cached on disk.
pub fn cover_path(game: &Game) -> PathBuf {
    config_dir().join("covers").join(&game.system).join(format!("{}.png", thumb_name(&game.name)))
}

fn steamgrid_url(key: &str, name: &str) -> Result<Option<String>, String> {
    let auth = format!("Bearer {key}");
    let get = |url: &str| -> Result<serde_json::Value, String> {
        serde_json::from_slice(&download(url, Some(&auth))?).map_err(|e| e.to_string())
    };
    let found = get(&format!("https://www.steamgriddb.com/api/v2/search/autocomplete/{}", url_encode(search_title(name))))?;
    let Some(id) = found["data"][0]["id"].as_u64() else { return Ok(None) };
    let grids = get(&format!("https://www.steamgriddb.com/api/v2/grids/game/{id}?dimensions=600x900"))?;
    Ok(grids["data"][0]["url"].as_str().map(String::from))
}

/// Downloads a cover into the cache: libretro-thumbnails first, then SteamGridDB if a key is set.
/// Returns true if a cover now exists. A miss is remembered with a ".miss" file so it's tried once.
pub fn fetch_cover(game: &Game, steamgriddb_key: &str) -> bool {
    let path = cover_path(game);
    let miss = path.with_extension("miss");
    if path.exists() {
        return true;
    }
    if miss.exists() {
        return false;
    }
    let Some(sys) = systems().into_iter().find(|s| s.id == game.system) else { return false };
    let libretro = format!(
        "https://thumbnails.libretro.com/{}/Named_Boxarts/{}.png",
        url_encode(&sys.thumbs),
        url_encode(&thumb_name(&game.name))
    );
    let mut bytes = download(&libretro, None).ok();
    if bytes.is_none() && !steamgriddb_key.is_empty() {
        match steamgrid_url(steamgriddb_key, &game.name) {
            Ok(Some(url)) => bytes = download(&url, None).ok(),
            Ok(None) => {}
            Err(_) => return false, // network/API error: don't record a miss, retry next launch
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
    fn scan_matches_extensions_per_system() {
        let dir = std::env::temp_dir().join("emuhub_scan_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("snes")).unwrap();
        std::fs::write(dir.join("snes/Zelda.SFC"), b"").unwrap();
        std::fs::write(dir.join("snes/readme.txt"), b"").unwrap();
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
    fn retroarch_overrides_point_at_emudeck_folders() {
        let dir = std::env::temp_dir().join("emuhub_override_test/Emulation");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bios")).unwrap();
        let text = retroarch_overrides(&dir.join("roms"));
        assert!(text.contains("system_directory") && !text.contains("savefile_directory"));
    }

    #[test]
    fn names_are_cleaned_for_lookups() {
        assert_eq!(search_title("Super Mario World (USA) (Rev 1)"), "Super Mario World");
        assert_eq!(search_title("Sonic [!]"), "Sonic");
        assert_eq!(thumb_name("Q*bert: Arcade"), "Q_bert_ Arcade");
        assert_eq!(url_encode("A B(1)"), "A%20B%281%29");
    }
}
