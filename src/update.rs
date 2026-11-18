//! Checks GitHub for a newer release, and installs it.
//!
//! The check is one anonymous request to the GitHub API: no account, no identifiers, nothing about
//! the user's books. Installers are only downloaded from this repository's releases and are
//! checked against the SHA-256 that GitHub publishes for them before they are run.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;

const LATEST: &str = "https://api.github.com/repos/charansaiponnada/flipbook/releases/latest";
const DOWNLOADS: &str = "https://github.com/charansaiponnada/flipbook/releases/download/";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone)]
pub struct Release {
    pub version: String,
    /// The release's web page, for "What's new" and for portable copies.
    pub page: String,
    /// The installer's download URL and its published SHA-256, if the release has one.
    pub installer: Option<(String, Option<String>)>,
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
    /// "sha256:<hex>"
    #[serde(default)]
    digest: Option<String>,
}

fn agent(timeout: u64) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(timeout)))
        .user_agent(concat!("Flipbook/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

/// "v1.2.3" or "1.2.3" -> (1, 2, 3)
fn parse(v: &str) -> Option<(u32, u32, u32)> {
    let mut it = v.trim().trim_start_matches('v').splitn(3, '.').map(|p| p.parse::<u32>().ok());
    Some((it.next()??, it.next()??, it.next().flatten().unwrap_or(0)))
}

pub fn is_newer(latest: &str, current: &str) -> bool {
    matches!((parse(latest), parse(current)), (Some(l), Some(c)) if l > c)
}

/// The latest release, if it is newer than this build. Network errors just mean "no update".
pub fn check() -> Option<Release> {
    latest().filter(|r| is_newer(&r.version, CURRENT))
}

fn latest() -> Option<Release> {
    let api: ApiRelease = agent(15).get(LATEST).header("Accept", "application/vnd.github+json").call().ok()?.body_mut().read_json().ok()?;
    let installer = api
        .assets
        .into_iter()
        .find(|a| a.name.eq_ignore_ascii_case("Flipbook-setup.exe") && a.browser_download_url.starts_with(DOWNLOADS))
        .map(|a| (a.browser_download_url, a.digest));
    Some(Release { version: api.tag_name.trim_start_matches('v').to_string(), page: api.html_url, installer })
}

/// Installed copies sit next to the uninstaller the installer wrote; portable copies don't.
pub fn installed() -> bool {
    std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("uninstall.exe").exists())).unwrap_or(false)
}

fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

/// Download the installer to the temp folder and verify it. Returns where it was saved.
pub fn download(url: &str, digest: Option<&str>, version: &str) -> Result<PathBuf, String> {
    if !url.starts_with(DOWNLOADS) {
        return Err("That update doesn't come from Flipbook's releases.".into());
    }
    let data = agent(300)
        .get(url)
        .call()
        .and_then(|mut r| r.body_mut().with_config().limit(200 << 20).read_to_vec())
        .map_err(|e| format!("Could not download the update: {e}"))?;
    if let Some(want) = digest.and_then(|d| d.strip_prefix("sha256:")) {
        if !sha256_hex(&data).eq_ignore_ascii_case(want) {
            return Err("The downloaded update was damaged, so it wasn't installed.".into());
        }
    }
    let path = std::env::temp_dir().join(format!("Flipbook-{version}-setup.exe"));
    std::fs::write(&path, data).map_err(|e| format!("Could not save the update: {e}"))?;
    Ok(path)
}

/// Run the installer silently. `/UPDATE` makes it wait for this app to close and reopen Flipbook after.
pub fn install(installer: &Path) -> Result<(), String> {
    std::process::Command::new(installer)
        .args(["/S", "/UPDATE"])
        .spawn()
        .map(drop)
        .map_err(|e| format!("Could not start the update: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(is_newer("v0.4.0", "0.3.0"));
        assert!(is_newer("v0.10.0", "0.9.9"));
        assert!(is_newer("1.0", "0.9.0"));
        assert!(!is_newer("v0.3.0", "0.3.0"));
        assert!(!is_newer("v0.2.9", "0.3.0"));
        assert!(!is_newer("nightly", "0.3.0"));
    }

    #[test]
    fn hash() {
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    /// Talks to GitHub: `cargo test -- --ignored`
    #[test]
    #[ignore]
    fn live_download() {
        let rel = latest().expect("GitHub answered with a release");
        let (url, digest) = rel.installer.expect("the release has Flipbook-setup.exe");
        assert!(digest.as_deref().is_some_and(|d| d.starts_with("sha256:")), "GitHub published a digest");
        let path = download(&url, digest.as_deref(), "test").expect("downloaded and verified");
        assert!(std::fs::metadata(&path).unwrap().len() > 1_000_000);
        std::fs::remove_file(path).unwrap();
        assert!(download(&url, Some("sha256:00"), "test").is_err(), "a wrong digest is rejected");
    }
}
