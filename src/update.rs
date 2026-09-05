use std::{fs, path::PathBuf};

use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{error::SearchError, fsutil::{atomic_write, set_mode}, paths::AppPaths, process::run_inherit};

const RELEASE_API: &str = "https://api.github.com/repos/ShehabYahya/Hermes-Search-Agent/releases/latest";
const ASSET: &str = "hermes-search-agent-linux-x86_64";

#[derive(Deserialize)]
struct Release { tag_name: String, assets: Vec<Asset> }
#[derive(Deserialize)]
struct Asset { name: String, browser_download_url: String }

pub async fn run(check_only: bool) -> Result<(), SearchError> {
    let paths = AppPaths::discover()?;
    let client = reqwest::Client::builder().user_agent("hermes-search-agent-updater").build()?;
    let release: Release = client.get(RELEASE_API).send().await?.error_for_status()?.json().await?;
    let remote = Version::parse(release.tag_name.trim_start_matches('v')).map_err(|e| SearchError::Install(format!("release tag is not semver: {e}")))?;
    let current = Version::parse(env!("CARGO_PKG_VERSION")).map_err(|e| SearchError::Install(format!("current version is not semver: {e}")))?;
    if remote <= current {
        println!("Already current: {current}");
        return Ok(());
    }
    println!("Update available: {current} -> {remote}");
    if check_only { return Ok(()); }

    let binary = release.assets.iter().find(|a| a.name == ASSET).ok_or_else(|| SearchError::Install(format!("release {} has no {ASSET} asset", release.tag_name)))?;
    let checksum = release.assets.iter().find(|a| a.name == format!("{ASSET}.sha256")).ok_or_else(|| SearchError::Install("release checksum asset is missing".into()))?;
    let bytes = client.get(&binary.browser_download_url).send().await?.error_for_status()?.bytes().await?;
    let checksum_text = client.get(&checksum.browser_download_url).send().await?.error_for_status()?.text().await?;
    let expected = checksum_text.split_whitespace().next().ok_or_else(|| SearchError::Install("invalid checksum asset".into()))?;
    let actual = hex::encode(Sha256::digest(&bytes));
    if !expected.eq_ignore_ascii_case(&actual) {
        return Err(SearchError::Install(format!("checksum mismatch: expected {expected}, got {actual}")));
    }
    let staged: PathBuf = paths.state_dir.join(format!("update-{}", Uuid::new_v4().simple()));
    atomic_write(&staged, &bytes, Some(0o755))?;
    set_mode(&staged, 0o755)?;
    if let Some(parent) = paths.binary_path.parent() { fs::create_dir_all(parent)?; }
    fs::rename(&staged, &paths.binary_path)?;
    println!("Updated binary to {remote}; running repair.");
    run_inherit(&paths.binary_path, ["repair", "--non-interactive"])?;
    Ok(())
}
