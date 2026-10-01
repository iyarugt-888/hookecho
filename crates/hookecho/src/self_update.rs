//! Self-update from the CI builds of this branch.
//!
//! The branch workflow (`.github/workflows/wsv3-redesign-build.yml`) builds the Windows installer,
//! the portable EXE and the Android APK, then republishes them on a rolling pre-release
//! (`TAG`) with a `build.json` naming the CI run number, the commit and each file's SHA-256.
//! Release assets download without a token and outside the API's hourly limit, which Actions
//! artifacts do not: an artifact needs an authenticated request even on a public repository.
//!
//! The running build knows its own run number (`HOOKECHO_BUILD`, set by the same workflow at
//! compile time). A build without one, a local `cargo build`, never offers an update: there is
//! nothing to compare it with, and replacing someone's own build with CI's would be wrong.
//!
//! Installing:
//! - Windows, installed (Inno's uninstaller beside the EXE): run the new installer silently and
//!   quit; the installer closes the app if needed and relaunches it (`/RELAUNCH=1`).
//! - Windows, portable: rename the running EXE aside (Windows allows renaming a running EXE),
//!   put the new one in its place, start it and quit. The old one is removed at the next launch.
//! - Android: hand the APK to the system installer, which always asks the user to confirm and
//!   only accepts an APK signed with the same key.
//!
//! Every download is checked against the SHA-256 in `build.json` before it is run.

use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Mutex;

/// The repository and rolling pre-release the CI builds are published to.
pub const REPO: &str = "iyarugt-888/hookecho";
pub const TAG: &str = "wsv3-latest";

/// One published CI build, from `build.json`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BuildInfo {
    /// The workflow run number: monotonic per workflow, so a plain comparison orders builds.
    pub build: u64,
    /// The commit it was built from.
    pub sha: String,
    #[serde(default)]
    pub version: String,
    /// File name to lowercase hex SHA-256.
    #[serde(default)]
    pub assets: std::collections::BTreeMap<String, String>,
}

impl BuildInfo {
    pub fn short_sha(&self) -> &str {
        &self.sha[..self.sha.len().min(7)]
    }
}

/// Where the updater is. Shared with its background task, so it is one value behind a lock.
#[derive(Debug, Clone, PartialEq)]
pub enum State {
    Idle,
    Checking,
    /// A local build with no CI run number: updates are never offered.
    DevBuild,
    UpToDate,
    /// No file for this platform in the build (or a platform that cannot self-update).
    Unsupported,
    Available(BuildInfo),
    Downloading {
        info: BuildInfo,
        done: u64,
        total: Option<u64>,
    },
    Ready {
        info: BuildInfo,
        path: PathBuf,
    },
    Failed(String),
    /// The user closed the card for this build.
    Dismissed(u64),
}

static STATE: Mutex<State> = Mutex::new(State::Idle);

pub fn state() -> State {
    STATE.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn set_state(s: State) {
    *STATE.lock().unwrap_or_else(|e| e.into_inner()) = s;
}

/// This build's CI run number, when CI built it.
pub fn local_build() -> Option<u64> {
    option_env!("HOOKECHO_BUILD")?.trim().parse().ok()
}

/// Whether `remote` should replace a build numbered `local`. Never for a local build.
pub fn is_newer(local: Option<u64>, remote: &BuildInfo) -> bool {
    local.is_some_and(|l| remote.build > l)
}

pub fn asset_url(name: &str) -> String {
    format!("https://github.com/{REPO}/releases/download/{TAG}/{name}")
}

/// Which published file replaces this build, or `None` where self-update does not apply.
pub fn asset_name() -> Option<&'static str> {
    if cfg!(target_os = "android") {
        Some("HookEcho-wsv3-arm64-v8a.apk")
    } else if cfg!(target_os = "windows") {
        Some(if installed_windows() {
            "HookEcho-setup-x86_64.exe"
        } else {
            "HookEcho-portable-x86_64.exe"
        })
    } else {
        None
    }
}

/// Installed by the Inno Setup installer: its uninstaller sits beside the EXE.
fn installed_windows() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("unins000.exe").exists()))
        .unwrap_or(false)
}

/// Lowercase hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Whether a download matches the build's record of it. A build that lists no hash for the
/// file is refused: an unchecked executable is not run.
pub fn verify(info: &BuildInfo, name: &str, bytes: &[u8]) -> Result<(), String> {
    match info.assets.get(name) {
        None => Err(format!(
            "build #{} lists no checksum for {name}",
            info.build
        )),
        Some(want) if want.eq_ignore_ascii_case(&sha256_hex(bytes)) => Ok(()),
        Some(_) => Err(format!(
            "{name} does not match build #{}'s checksum",
            info.build
        )),
    }
}

/// Fetch `build.json` from the rolling pre-release.
#[cfg(not(target_arch = "wasm32"))]
pub async fn fetch_info(http: &reqwest::Client) -> anyhow::Result<BuildInfo> {
    let info = http
        .get(asset_url("build.json"))
        .header("User-Agent", "hookecho")
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    Ok(serde_json::from_str(&info)?)
}

/// Download `name` from `info`'s build into `dir`, reporting progress in the shared state, and
/// verify it. Returns the file's path.
#[cfg(not(target_arch = "wasm32"))]
pub async fn download(
    http: &reqwest::Client,
    info: &BuildInfo,
    name: &str,
    dir: PathBuf,
) -> anyhow::Result<PathBuf> {
    let mut resp = http
        .get(asset_url(name))
        .header("User-Agent", "hookecho")
        .send()
        .await?
        .error_for_status()?;
    let total = resp.content_length();
    let mut bytes = Vec::with_capacity(total.unwrap_or(0) as usize);
    while let Some(chunk) = resp.chunk().await? {
        bytes.extend_from_slice(&chunk);
        set_state(State::Downloading {
            info: info.clone(),
            done: bytes.len() as u64,
            total,
        });
    }
    verify(info, name, &bytes).map_err(|e| anyhow::anyhow!(e))?;
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(name);
    let partial = dir.join(format!("{name}.part"));
    std::fs::write(&partial, &bytes)?;
    std::fs::rename(&partial, &path)?;
    Ok(path)
}

/// Where downloads go: the app's cache on Android (shared with the installer through its
/// FileProvider), the temp directory elsewhere.
pub fn download_dir() -> Option<PathBuf> {
    if cfg!(target_os = "android") {
        crate::platform::update_dir().map(PathBuf::from)
    } else {
        Some(std::env::temp_dir().join("hookecho-update"))
    }
}

/// Install a downloaded build. On Windows this does not return: the app quits for the new one.
pub fn install(path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.contains("setup") {
            std::process::Command::new(path)
                .args([
                    "/SILENT",
                    "/SUPPRESSMSGBOXES",
                    "/NORESTART",
                    "/CLOSEAPPLICATIONS",
                    "/RELAUNCH=1",
                ])
                .spawn()
                .map_err(|e| format!("could not start the installer: {e}"))?;
        } else {
            let current =
                std::env::current_exe().map_err(|e| format!("where am I running from: {e}"))?;
            let aside = current.with_extension("exe.old");
            let _ = std::fs::remove_file(&aside);
            std::fs::rename(&current, &aside)
                .map_err(|e| format!("could not move the running build aside: {e}"))?;
            if let Err(e) = std::fs::copy(path, &current) {
                // Put the running build back rather than leave no EXE behind.
                let _ = std::fs::rename(&aside, &current);
                return Err(format!("could not put the new build in place: {e}"));
            }
            std::process::Command::new(&current)
                .spawn()
                .map_err(|e| format!("could not start the new build: {e}"))?;
        }
        std::process::exit(0);
    }
    #[cfg(target_os = "android")]
    {
        return crate::platform::install_apk(&path.to_string_lossy());
    }
    #[allow(unreachable_code)]
    {
        let _ = path;
        Err("this platform does not update itself".into())
    }
}

/// At launch: remove the build a portable self-update moved aside.
pub fn cleanup() {
    if let Ok(current) = std::env::current_exe() {
        let _ = std::fs::remove_file(current.with_extension("exe.old"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(build: u64) -> BuildInfo {
        BuildInfo {
            build,
            sha: "0123456789abcdef".into(),
            version: "0.12.0".into(),
            assets: [("a.apk".to_string(), sha256_hex(b"apk bytes"))].into(),
        }
    }

    #[test]
    fn only_a_ci_build_is_offered_a_newer_one() {
        assert!(is_newer(Some(41), &info(42)));
        assert!(!is_newer(Some(42), &info(42)));
        assert!(!is_newer(Some(43), &info(42)));
        // A local build has no run number to compare: it is never replaced.
        assert!(!is_newer(None, &info(42)));
    }

    #[test]
    fn a_download_is_run_only_when_it_matches_its_checksum() {
        let i = info(42);
        assert!(verify(&i, "a.apk", b"apk bytes").is_ok());
        assert!(verify(&i, "a.apk", b"tampered").is_err());
        assert!(verify(&i, "missing.exe", b"anything").is_err());
    }

    #[test]
    fn build_json_parses_as_ci_writes_it() {
        let json = r#"{"build": 57, "sha": "a1b2c3d4e5f6", "version": "0.12.0-beta.2",
            "assets": {"HookEcho-wsv3-arm64-v8a.apk": "ab12"}}"#;
        let i: BuildInfo = serde_json::from_str(json).unwrap();
        assert_eq!((i.build, i.short_sha()), (57, "a1b2c3d"));
        assert_eq!(i.assets["HookEcho-wsv3-arm64-v8a.apk"], "ab12");
        assert!(asset_url("build.json").ends_with("/releases/download/wsv3-latest/build.json"));
    }
}
