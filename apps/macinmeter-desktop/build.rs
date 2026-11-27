#![forbid(unsafe_code)]

//! Supplies the desktop GUI's external Noto Sans CJK font for the default
//! `bundled-fonts` build, and on Windows links the executable's icon, version
//! resource and application manifest. A build without `bundled-fonts` embeds
//! no font and fetches nothing.
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

// What the installers, the version resource and the window all call the
// application: the names the Tauri GUI was installed under, so an install of
// this one replaces a 0.3.x install in place.
const PRODUCT_NAME: &str = "MacinMeter DR GUI";
const INSTALLED_EXECUTABLE: &str = "macinmeter-gui.exe";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && let Err(error) = embed_windows_resources()
    {
        panic!("failed to embed the Windows resources: {error}");
    }
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

/// The resource script is generated so the version resource always carries
/// the crate's own version, and its `FontBuild` value says which font build
/// the binary is. It names no header: `rc` then needs no Windows SDK include
/// path, so the constants below are winver.h's values written out.
fn embed_windows_resources() -> Result<(), String> {
    let manifest_dir = env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| "Cargo did not provide CARGO_MANIFEST_DIR".to_owned())?;
    let out_dir = env::var_os("OUT_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| "Cargo did not provide OUT_DIR".to_owned())?;
    let icon = manifest_dir.join("../../tauri-app/src-tauri/icons/icon.ico");
    let manifest = manifest_dir.join("packaging/windows/macinmeter-gui.manifest");
    for input in [&icon, &manifest] {
        println!("cargo:rerun-if-changed={}", input.display());
        if !input.is_file() {
            return Err(format!("{} is missing", input.display()));
        }
    }
    let number = |name: &str| -> Result<u16, String> {
        env::var(name)
            .map_err(|error| format!("{name}: {error}"))?
            .parse()
            .map_err(|error| format!("{name}: {error}"))
    };
    let (major, minor, patch) = (
        number("CARGO_PKG_VERSION_MAJOR")?,
        number("CARGO_PKG_VERSION_MINOR")?,
        number("CARGO_PKG_VERSION_PATCH")?,
    );
    let version = env::var("CARGO_PKG_VERSION").map_err(|error| error.to_string())?;
    let font_build = if env::var_os("CARGO_FEATURE_BUNDLED_FONTS").is_some() {
        "bundled-fonts"
    } else {
        "system-fonts"
    };
    let licence = manifest_dir.join("../../LICENSE");
    println!("cargo:rerun-if-changed={}", licence.display());
    let copyright = fs::read_to_string(&licence)
        .map_err(|error| format!("could not read {}: {error}", licence.display()))?
        .lines()
        .find(|line| line.starts_with("Copyright"))
        .map(|line| line.replace('"', "'"))
        .ok_or_else(|| format!("{} has no copyright line", licence.display()))?;
    let literal = |path: &Path| path.display().to_string().replace('\\', "\\\\");
    let script = format!(
        r#"1 ICON "{icon}"
1 24 "{manifest}"
1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEFLAGSMASK 0x3F
FILEFLAGS 0x0
FILEOS 0x40004
FILETYPE 0x1
FILESUBTYPE 0x0
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "MacinMeter"
      VALUE "FileDescription", "{PRODUCT_NAME}"
      VALUE "FileVersion", "{version}"
      VALUE "FontBuild", "{font_build}"
      VALUE "InternalName", "macinmeter-gui"
      VALUE "LegalCopyright", "{copyright}"
      VALUE "OriginalFilename", "{INSTALLED_EXECUTABLE}"
      VALUE "ProductName", "{PRODUCT_NAME}"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        icon = literal(&icon),
        manifest = literal(&manifest),
    );
    let script_path = out_dir.join("macinmeter-gui.rc");
    fs::write(&script_path, script)
        .map_err(|error| format!("could not write {}: {error}", script_path.display()))?;
    embed_resource::compile_for(&script_path, ["macinmeter-desktop"], embed_resource::NONE)
        .manifest_required()
        .map_err(|error| error.to_string())
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
