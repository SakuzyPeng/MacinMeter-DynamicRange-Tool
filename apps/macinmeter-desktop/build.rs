#![forbid(unsafe_code)]

//! Supplies the UI fonts the desktop GUI embeds.
//!
//! egui draws only the fonts it is given and has no system fallback, so a CJK
//! file name needs a CJK face compiled in, and the result card's serif title and
//! DR number need a serif face. Each face is fetched once and pinned by SHA-256.
//! Together they are this repository's single network exception: build inputs
//! of this crate only. No test fetches anything, and no other member builds
//! this script. Each font's own `*_PATH` variable supplies a local copy for an
//! offline build; `MACINMETER_UI_FONT_CACHE_DIR` shares one download across
//! profiles and checkouts. Either way the checksum is enforced.

use sha2::{Digest, Sha256};
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const FONT_CACHE_ENV: &str = "MACINMETER_UI_FONT_CACHE_DIR";

struct PinnedFont {
    file: &'static str,
    label: &'static str,
    override_env: &'static str,
    sha256: &'static str,
    /// Tried in order; the checksum, not the host, identifies the file.
    urls: &'static [&'static str],
}

const FONTS: [PinnedFont; 2] = [
    // The same pinned file the MacinDecode Spatial Player embeds.
    PinnedFont {
        file: "NotoSansCJKsc-Regular.otf",
        label: "Noto Sans CJK SC",
        override_env: "MACINMETER_UI_FONT_PATH",
        sha256: "2c76254f6fc379fddfce0a7e84fb5385bb135d3e399294f6eeb6680d0365b74b",
        urls: &[
            "https://cdn.jsdelivr.net/gh/notofonts/noto-cjk@165c01b46ea533872e002e0785ff17e44f6d97d8c/Sans/OTF/SimplifiedChinese/NotoSansCJKsc-Regular.otf",
        ],
    },
    // Source Serif 4 Semibold 4.005, adobe-fonts/source-serif tag 4.005R: the
    // face the app icon's "DR" is drawn from, for the card's serif text.
    PinnedFont {
        file: "SourceSerif4-Semibold.otf",
        label: "Source Serif 4 Semibold",
        override_env: "MACINMETER_SERIF_FONT_PATH",
        sha256: "25e034392847d9965c92f98971b3646436b7ab919ece9118b0c0e5ec94c02efc",
        urls: &[
            "https://raw.githubusercontent.com/adobe-fonts/source-serif/2823e993c53fca27c5c8749f529b56a5a7c77b6b/OTF/SourceSerif4-Semibold.otf",
            "https://cdn.jsdelivr.net/gh/adobe-fonts/source-serif@2823e993c53fca27c5c8749f529b56a5a7c77b6b/OTF/SourceSerif4-Semibold.otf",
        ],
    },
];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed={FONT_CACHE_ENV}");
    for font in &FONTS {
        println!("cargo:rerun-if-env-changed={}", font.override_env);
        if let Err(error) = prepare(font) {
            panic!("failed to prepare {}: {error}", font.label);
        }
    }
}

fn prepare(font: &PinnedFont) -> Result<(), String> {
    let out_dir = env::var_os("OUT_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| "Cargo did not provide OUT_DIR".to_owned())?;
    let destination = out_dir.join(font.file);

    if let Some(source) = env::var_os(font.override_env).map(PathBuf::from) {
        println!("cargo:rerun-if-changed={}", source.display());
        let bytes = read_verified(font, &source)?;
        return fs::write(&destination, bytes).map_err(|error| {
            format!(
                "could not copy {} to {}: {error}",
                source.display(),
                destination.display()
            )
        });
    }

    if read_verified(font, &destination).is_ok() {
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
        let bytes = ensure(font, &cache.join(font.file))?;
        fs::write(&destination, bytes)
            .map_err(|error| format!("could not write {}: {error}", destination.display()))?;
    } else {
        ensure(font, &destination)?;
    }
    Ok(())
}

fn ensure(font: &PinnedFont, destination: &Path) -> Result<Vec<u8>, String> {
    if let Ok(bytes) = read_verified(font, destination) {
        return Ok(bytes);
    }
    let partial = destination.with_extension("otf.part");
    println!(
        "cargo:warning=Downloading checksum-pinned {} for the UI",
        font.label
    );
    let mut failures = Vec::new();
    for url in font.urls {
        if partial.exists() {
            fs::remove_file(&partial).map_err(|error| {
                format!("could not remove stale {}: {error}", partial.display())
            })?;
        }
        match download(url, &partial).and_then(|()| read_verified(font, &partial)) {
            Ok(bytes) => {
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
                return Ok(bytes);
            }
            Err(error) => failures.push(format!("{url}: {error}")),
        }
    }
    Err(format!(
        "{}; set {} for an offline build",
        failures.join("; "),
        font.override_env
    ))
}

fn download(url: &str, partial: &Path) -> Result<(), String> {
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
            // jsDelivr intermittently answers these files with a 403 that it
            // caches for 60 s (`s-maxage=60`); a retry inside that window gets
            // the same cached refusal, so each retry waits it out.
            "--retry",
            "3",
            "--retry-delay",
            "61",
            "--retry-all-errors",
        ])
        .arg("--output")
        .arg(partial)
        .arg(url)
        .status()
        .map_err(|error| format!("could not start curl ({error}); install curl"))?;
    if status.success() {
        Ok(())
    } else {
        Err("curl could not download it".to_owned())
    }
}

fn read_verified(font: &PinnedFont, path: &Path) -> Result<Vec<u8>, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let digest = Sha256::digest(&bytes);
    let mut actual = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut actual, "{byte:02x}")
            .map_err(|error| format!("could not format the font checksum: {error}"))?;
    }
    if actual != font.sha256 {
        return Err(format!(
            "{} has SHA-256 {actual}, expected {}",
            path.display(),
            font.sha256
        ));
    }
    Ok(bytes)
}
