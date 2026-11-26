#![forbid(unsafe_code)]

//! Supplies the desktop GUI's external Noto Sans CJK font for the default
//! `bundled-fonts` build. A build without that feature embeds no font and
//! fetches nothing.
//!
//! egui draws only the fonts it is given and has no system fallback, so a CJK
//! file name needs a CJK face compiled in. The face is fetched once, pinned by
//! SHA-256, and is this repository's single network exception: it is a build
//! input of this crate only. No test fetches anything, and no other member
//! builds this script. `MACINMETER_UI_FONT_PATH` supplies a local copy for an
//! offline build; `MACINMETER_UI_FONT_CACHE_DIR` shares one download across
//! profiles and checkouts. Either way the checksum is enforced.

use sha2::{Digest, Sha256};
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const FONT_FILE_NAME: &str = "NotoSansCJKsc-Regular.otf";
const FONT_OVERRIDE_ENV: &str = "MACINMETER_UI_FONT_PATH";
const FONT_CACHE_ENV: &str = "MACINMETER_UI_FONT_CACHE_DIR";
// The same pinned file the MacinDecode Spatial Player embeds.
const FONT_SHA256: &str = "2c76254f6fc379fddfce0a7e84fb5385bb135d3e399294f6eeb6680d0365b74b";
const FONT_URL: &str = "https://cdn.jsdelivr.net/gh/notofonts/noto-cjk@165c01b46ea533872e002e0785ff17e44f6d97d8c/Sans/OTF/SimplifiedChinese/NotoSansCJKsc-Regular.otf";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // A system-font build embeds no font, so it fetches nothing.
    if env::var_os("CARGO_FEATURE_BUNDLED_FONTS").is_none() {
        return;
    }
    println!("cargo:rerun-if-env-changed={FONT_OVERRIDE_ENV}");
    println!("cargo:rerun-if-env-changed={FONT_CACHE_ENV}");

    if let Err(error) = prepare_ui_font() {
        panic!("failed to prepare the UI font: {error}");
    }
}

fn prepare_ui_font() -> Result<(), String> {
    let out_dir = env::var_os("OUT_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| "Cargo did not provide OUT_DIR".to_owned())?;
    let destination = out_dir.join(FONT_FILE_NAME);

    if let Some(source) = env::var_os(FONT_OVERRIDE_ENV).map(PathBuf::from) {
        println!("cargo:rerun-if-changed={}", source.display());
        let bytes = read_verified_font(&source)?;
        return fs::write(&destination, bytes).map_err(|error| {
            format!(
                "could not copy {} to {}: {error}",
                source.display(),
                destination.display()
            )
        });
    }

    if read_verified_font(&destination).is_ok() {
        return Ok(());
    }

    if let Some(cache) = env::var_os(FONT_CACHE_ENV).map(PathBuf::from) {
        if !cache.is_absolute() {
            return Err(format!("{FONT_CACHE_ENV} must be absolute"));
        }
        fs::create_dir_all(&cache)
            .map_err(|error| format!("could not create {}: {error}", cache.display()))?;
        // Parallel builds (test, clippy, release) may share one cache.
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(cache.join(".font.lock"))
            .map_err(|error| format!("could not open the font cache lock: {error}"))?;
        lock.lock()
            .map_err(|error| format!("could not lock the font cache: {error}"))?;
        let bytes = ensure_ui_font(&cache.join(FONT_FILE_NAME))?;
        fs::write(&destination, bytes)
            .map_err(|error| format!("could not write {}: {error}", destination.display()))?;
    } else {
        ensure_ui_font(&destination)?;
    }
    Ok(())
}

fn ensure_ui_font(destination: &Path) -> Result<Vec<u8>, String> {
    if let Ok(bytes) = read_verified_font(destination) {
        return Ok(bytes);
    }
    let partial = destination.with_extension("otf.part");
    if partial.exists() {
        fs::remove_file(&partial)
            .map_err(|error| format!("could not remove stale {}: {error}", partial.display()))?;
    }
    println!("cargo:warning=Downloading checksum-pinned Noto Sans CJK SC for the UI");
    let status = Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--connect-timeout",
            "20",
            "--max-time",
            "300",
            // jsDelivr intermittently answers this file with a 403 that it
            // caches for 60 s (`s-maxage=60`); a retry inside that window gets
            // the same cached refusal, so each retry waits it out.
            "--retry",
            "3",
            "--retry-delay",
            "61",
            "--retry-all-errors",
        ])
        .arg("--output")
        .arg(&partial)
        .arg(FONT_URL)
        .status()
        .map_err(|error| {
            format!("could not start curl ({error}); install curl or set {FONT_OVERRIDE_ENV}")
        })?;
    if !status.success() {
        return Err(format!(
            "curl could not download {FONT_URL}; set {FONT_OVERRIDE_ENV} for an offline build"
        ));
    }

    let bytes = read_verified_font(&partial)?;
    if destination.exists() {
        fs::remove_file(destination).map_err(|error| {
            format!(
                "could not replace invalid {}: {error}",
                destination.display()
            )
        })?;
    }
    fs::rename(&partial, destination).map_err(|error| {
        format!(
            "could not move {} to {}: {error}",
            partial.display(),
            destination.display()
        )
    })?;
    Ok(bytes)
}

fn read_verified_font(path: &Path) -> Result<Vec<u8>, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let digest = Sha256::digest(&bytes);
    let mut actual = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut actual, "{byte:02x}")
            .map_err(|error| format!("could not format the font checksum: {error}"))?;
    }
    if actual != FONT_SHA256 {
        return Err(format!(
            "{} has SHA-256 {actual}, expected {FONT_SHA256}",
            path.display()
        ));
    }
    Ok(bytes)
}
