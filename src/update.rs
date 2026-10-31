//! Update check and self-update, modeled on Sonora's: ask GitHub for the latest release, and when the
//! user presses Update, download this install's asset, check it against the release's SHA256SUMS,
//! then install it (Windows: the setup, silently; macOS: swap the .app; Linux: swap the AppImage).
use crate::emu;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const RELEASES_URL: &str = "https://github.com/veneito/EmuHub/releases/latest";
const LATEST_API: &str = "https://api.github.com/repos/veneito/EmuHub/releases/latest";
/// Only files from this repo's releases are ever downloaded.
const DOWNLOAD_PREFIX: &str = "https://github.com/veneito/EmuHub/releases/download/";
const SUMS: &str = "SHA256SUMS";

/// How this copy was installed, which decides whether it can replace itself.
#[derive(Clone, Debug, PartialEq)]
pub enum Install {
    /// Installed by EmuHub-Setup.exe (its uninstaller sits beside the exe).
    Setup,
    /// EmuHub.app in a folder EmuHub can write to.
    Bundle(PathBuf),
    /// An AppImage in a folder EmuHub can write to.
    AppImage(PathBuf),
    /// Portable exe, bare Linux binary, a read-only place: updates open the Releases page.
    Manual,
}

impl Install {
    pub fn detect() -> Self {
        let Ok(exe) = std::env::current_exe() else { return Install::Manual };
        if let Some(image) = std::env::var_os("APPIMAGE").map(PathBuf::from) {
            return detect_appimage(image);
        }
        detect_from(&exe)
    }

    /// The release file this install updates from.
    fn asset(&self) -> Option<&'static str> {
        match self {
            Install::Setup => Some("EmuHub-Setup.exe"),
            Install::Bundle(_) => Some("EmuHub.dmg"),
            Install::AppImage(_) => Some("EmuHub-x86_64.AppImage"),
            Install::Manual => None,
        }
    }
}

fn detect_appimage(image: PathBuf) -> Install {
    if image.parent().is_some_and(writable) { Install::AppImage(image) } else { Install::Manual }
}

fn detect_from(exe: &Path) -> Install {
    if cfg!(target_os = "windows") {
        return if exe.with_file_name("unins000.exe").is_file() { Install::Setup } else { Install::Manual };
    }
    // .../EmuHub.app/Contents/MacOS/emuhub; skip the mounted dmg and macOS's read-only "translocated" copy.
    let bundle = exe.ancestors().nth(3).filter(|b| b.extension().is_some_and(|e| e == "app"));
    match bundle {
        Some(b) if cfg!(target_os = "macos") && !b.starts_with("/Volumes") && !b.to_string_lossy().contains("/AppTranslocation/") && b.parent().is_some_and(writable) => {
            Install::Bundle(b.to_path_buf())
        }
        _ => Install::Manual,
    }
}

fn writable(dir: &Path) -> bool {
    let probe = dir.join(".emuhub-write-test");
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(probe);
    ok
}

/// A newer release, and what this install needs to update to it.
#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    /// (file name, URL) of this install's asset, when it can update itself.
    asset: Option<(String, String)>,
    sums: Option<String>,
}

impl Release {
    /// True when Update installs in place; false means it opens the Releases page.
    pub fn installs_itself(&self) -> bool {
        self.asset.is_some() && self.sums.is_some()
    }
}

/// The latest release if it's newer than this build and not the version the user skipped.
pub fn check(skipped: Option<&str>) -> Option<Release> {
    let json: serde_json::Value = serde_json::from_slice(&emu::download(LATEST_API, None).ok()?).ok()?;
    let release = parse_release(&json, env!("CARGO_PKG_VERSION"), &Install::detect())?;
    (skipped != Some(release.version.as_str())).then_some(release)
}

fn parse_release(json: &serde_json::Value, current: &str, install: &Install) -> Option<Release> {
    let version = json["tag_name"].as_str()?.trim_start_matches('v').to_owned();
    if !is_newer(&version, current) {
        return None;
    }
    let url_of = |name: &str| {
        json["assets"].as_array()?.iter().find(|a| a["name"] == name)?["browser_download_url"]
            .as_str()
            .filter(|u| u.starts_with(DOWNLOAD_PREFIX))
            .map(str::to_owned)
    };
    let asset = install.asset().and_then(|name| Some((name.to_owned(), url_of(name)?)));
    Some(Release { version, asset, sums: url_of(SUMS) })
}

/// Compares "1.10.0" > "1.9.2" by number, part by part.
fn is_newer(latest: &str, current: &str) -> bool {
    let parts = |v: &str| v.split('.').map(|p| p.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
    parts(latest) > parts(current)
}

/// Downloads this install's file and keeps it only if its SHA-256 matches the release's SHA256SUMS.
pub fn download_verified(release: &Release) -> Result<PathBuf, String> {
    let ((name, url), sums) = release.asset.as_ref().zip(release.sums.as_ref()).ok_or("This copy can't update itself.")?;
    let sums = String::from_utf8_lossy(&emu::download(sums, None)?).into_owned();
    let expected = expected_hash(&sums, name).ok_or("The release has no checksum for this download.")?;
    let bytes = emu::download(url, None)?;
    if hex(&Sha256::digest(&bytes)) != expected {
        return Err("The download didn't match its checksum, so it wasn't installed.".into());
    }
    let path = std::env::temp_dir().join("emuhub-update").join(name);
    emu::write_atomic(&path, &bytes)?;
    Ok(path)
}

/// The hash listed for `name` in a `sha256sum` file ("<hex>  <name>" per line).
fn expected_hash(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, file) = line.split_once(char::is_whitespace)?;
        (file.trim().trim_start_matches('*') == name).then(|| hash.to_lowercase())
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Puts the verified download in place and starts the new version once this process exits.
/// The caller quits EmuHub right after.
pub fn install(download: &Path, install: &Install) -> Result<(), String> {
    match install {
        // The setup closes EmuHub if needed, upgrades in place and relaunches it (see emuhub.iss).
        Install::Setup => Command::new(download)
            .args(["/SILENT", "/SUPPRESSMSGBOXES", "/NORESTART"])
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Could not start the installer: {e}")),
        Install::Bundle(bundle) => {
            swap_bundle(download, bundle)?;
            relaunch(&format!("open {}", quote(bundle)))
        }
        Install::AppImage(image) => {
            let staged = image.with_file_name(".emuhub-update.AppImage");
            std::fs::copy(download, &staged).map_err(|e| format!("Could not copy the update: {e}"))?;
            make_executable(&staged)?;
            // The running AppImage keeps its open file, so renaming over it is safe.
            std::fs::rename(&staged, image).map_err(|e| format!("Could not replace the AppImage: {e}"))?;
            relaunch(&quote(image))
        }
        Install::Manual => Err("This copy can't update itself.".into()),
    }
}

/// Copies EmuHub.app off the dmg beside the running one, then swaps them by renaming, putting the
/// old one back if the swap fails.
fn swap_bundle(dmg: &Path, bundle: &Path) -> Result<(), String> {
    let mount = std::env::temp_dir().join("emuhub-update-mount");
    run(Command::new("/usr/bin/hdiutil").args(["attach", "-nobrowse", "-readonly", "-mountpoint"]).arg(&mount).arg(dmg))?;
    let staged = bundle.with_file_name(".EmuHub-update.app");
    let _ = std::fs::remove_dir_all(&staged);
    let copied = run(Command::new("/usr/bin/ditto").arg(mount.join("EmuHub.app")).arg(&staged));
    let _ = run(Command::new("/usr/bin/hdiutil").args(["detach", "-quiet"]).arg(&mount));
    copied?;
    let retired = bundle.with_file_name(".EmuHub-old.app");
    let _ = std::fs::remove_dir_all(&retired);
    std::fs::rename(bundle, &retired).map_err(|e| format!("Could not move the old version aside: {e}"))?;
    if let Err(e) = std::fs::rename(&staged, bundle) {
        let _ = std::fs::rename(&retired, bundle);
        return Err(format!("Could not put the new version in place: {e}"));
    }
    let _ = std::fs::remove_dir_all(&retired);
    Ok(())
}

/// Runs `command` from a shell that waits for this process to exit first, so two copies never run at once.
fn relaunch(command: &str) -> Result<(), String> {
    let script = format!("while kill -0 {} 2>/dev/null; do sleep 0.2; done; {command}", std::process::id());
    Command::new("/bin/sh").args(["-c", &script]).spawn().map(|_| ()).map_err(|e| format!("Could not restart EmuHub: {e}"))
}

fn quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', r"'\''"))
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())
}

#[cfg(not(unix))]
fn make_executable(_: &Path) -> Result<(), String> {
    Ok(())
}

fn run(command: &mut Command) -> Result<(), String> {
    let out = command.output().map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_owned()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_number() {
        assert!(is_newer("0.10.0", "0.9.1"));
        assert!(is_newer("0.2.1", "0.2.0"));
        assert!(!is_newer("0.2.0", "0.2.0"));
        assert!(!is_newer("garbage", "0.1.0"));
    }

    #[test]
    fn checksums_are_matched_by_file_name() {
        let sums = "ABC123  EmuHub-Setup.exe\ndef456 *EmuHub.dmg\n";
        assert_eq!(expected_hash(sums, "EmuHub-Setup.exe").as_deref(), Some("abc123"));
        assert_eq!(expected_hash(sums, "EmuHub.dmg").as_deref(), Some("def456"));
        assert_eq!(expected_hash(sums, "emuhub-linux"), None);
        assert_eq!(hex(&Sha256::digest(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn only_this_repos_downloads_are_used() {
        let release = |url: &str| {
            serde_json::json!({ "tag_name": "v9.0.0", "assets": [
                { "name": "EmuHub-Setup.exe", "browser_download_url": url },
                { "name": "SHA256SUMS", "browser_download_url": "https://github.com/veneito/EmuHub/releases/download/v9.0.0/SHA256SUMS" },
            ]})
        };
        let good = release("https://github.com/veneito/EmuHub/releases/download/v9.0.0/EmuHub-Setup.exe");
        assert!(parse_release(&good, "0.2.0", &Install::Setup).unwrap().installs_itself());
        let evil = release("https://example.com/EmuHub-Setup.exe");
        assert!(!parse_release(&evil, "0.2.0", &Install::Setup).unwrap().installs_itself());
        assert!(!parse_release(&good, "0.2.0", &Install::Manual).unwrap().installs_itself());
        assert!(parse_release(&good, "9.0.0", &Install::Setup).is_none()); // not newer
    }

    #[test]
    fn setup_installs_are_recognized() {
        let dir = std::env::temp_dir().join("emuhub_detect_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("emuhub.exe");
        if cfg!(target_os = "windows") {
            assert_eq!(detect_from(&exe), Install::Manual); // portable
            std::fs::write(dir.join("unins000.exe"), b"").unwrap();
            assert_eq!(detect_from(&exe), Install::Setup);
        } else {
            assert_eq!(detect_from(&exe), Install::Manual);
        }
    }
}
