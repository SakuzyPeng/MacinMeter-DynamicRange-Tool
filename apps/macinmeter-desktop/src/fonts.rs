//! Where the UI's faces come from.
//!
//! The desktop GUI builds two ways. With `bundled-fonts` (the default) it
//! embeds Noto Sans CJK SC and Source Serif 4 Semibold, so the window and an
//! exported PNG look the same on every host. Without it the binary embeds
//! neither (egui's own small default faces remain in both builds): at start-up
//! it looks up the Tauri stylesheet's own stacks
//! (`--font-ui` and `--font-serif`) among the host's fonts, as the WebView did,
//! so CJK titles get the system's Songti or SimSun and the binary stays small.
//!
//! Either way the result is one pair of fallback chains. The window
//! ([`crate::theme`]), card layout and PNG rasterisation
//! ([`crate::report_image`]) all read the same chains, so the window and an
//! export still draw one card.

use std::sync::OnceLock;

/// One face: a font file's bytes and the face's index inside it.
#[derive(Debug, Clone)]
pub struct Face {
    /// A unique name for egui's font table.
    pub key: String,
    /// The family the face reports, for the rasteriser's generic families.
    pub family: String,
    pub data: &'static [u8],
    pub index: u32,
}

/// Fallback chains, first face first. Serif text falls back through `serif`
/// and then `sans`; everything else uses `sans`.
#[derive(Debug)]
pub struct UiFonts {
    pub sans: Vec<Face>,
    pub serif: Vec<Face>,
}

impl UiFonts {
    /// The chain serif text is drawn with: serif faces, then sans faces for
    /// what they lack (CJK in a bundled build).
    pub fn serif_chain(&self) -> impl Iterator<Item = &Face> {
        self.serif.iter().chain(&self.sans)
    }
}

/// The process's fonts, resolved once. A system build scans the host's fonts
/// on first use, so callers should touch this early (the window does it while
/// installing the theme).
pub fn ui_fonts() -> &'static UiFonts {
    static FONTS: OnceLock<UiFonts> = OnceLock::new();
    FONTS.get_or_init(resolve)
}

#[cfg(feature = "bundled-fonts")]
fn resolve() -> UiFonts {
    UiFonts {
        sans: vec![Face {
            key: "noto-sans-cjk-sc".to_owned(),
            family: "Noto Sans CJK SC".to_owned(),
            data: bundled::SANS,
            index: 0,
        }],
        serif: vec![Face {
            key: "source-serif-4-semibold".to_owned(),
            family: "Source Serif 4".to_owned(),
            data: bundled::SERIF,
            index: 0,
        }],
    }
}

#[cfg(feature = "bundled-fonts")]
mod bundled {
    /// Noto Sans CJK SC Regular, fetched and checksum-verified by `build.rs`.
    pub const SANS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/NotoSansCJKsc-Regular.otf"));
    /// Source Serif 4 Semibold, the face the app icon's "DR" is drawn from.
    pub const SERIF: &[u8] = include_bytes!("../fonts/SourceSerif4-Semibold.otf");
}

#[cfg(not(feature = "bundled-fonts"))]
fn resolve() -> UiFonts {
    system::resolve()
}

/// The Tauri stylesheet's `--font-ui`, in its order (Chinese before Japanese,
/// see `tauri-app/src/styles.css`), with what `system-ui` and `sans-serif`
/// resolve to on each platform written out, since fontdb takes names.
#[cfg_attr(
    feature = "bundled-fonts",
    allow(dead_code, reason = "only a system-font build searches these")
)]
const SANS_STACK: &[&str] = &[
    "Inter",
    "SF Pro Text",
    ".SF NS",
    "Segoe UI",
    "PingFang SC",
    "Microsoft YaHei",
    "Hiragino Sans",
    "Hiragino Kaku Gothic Pro",
    "Yu Gothic",
    "Meiryo",
    "Helvetica Neue",
    "Noto Sans CJK SC",
    "Noto Sans",
    "DejaVu Sans",
];

/// The stylesheet's `--font-serif`.
#[cfg_attr(
    feature = "bundled-fonts",
    allow(dead_code, reason = "only a system-font build searches these")
)]
const SERIF_STACK: &[&str] = &[
    "Iowan Old Style",
    "Palatino Linotype",
    "Palatino",
    "Songti SC",
    "Noto Serif CJK SC",
    "Source Han Serif SC",
    "SimSun",
];

/// Characters a chain should cover: Latin, digits, the Chinese that once fell
/// between two Windows faces (原顺序), Japanese kana and Korean. A face that
/// adds none of these to the faces before it is not loaded, which keeps a
/// system build from holding every CJK collection on the host in memory.
#[cfg_attr(
    feature = "bundled-fonts",
    allow(dead_code, reason = "only a system-font build searches these")
)]
const COVERAGE_PROBE: &str = "Aa1 原顺序欅坂あセゾン한국";

#[cfg(not(feature = "bundled-fonts"))]
mod system {
    use super::{COVERAGE_PROBE, Face, SANS_STACK, SERIF_STACK, UiFonts};
    use fontdb::{Database, Family, ID, Query, Stretch, Style, Weight};
    use skrifa::{FontRef, MetadataProvider};

    pub fn resolve() -> UiFonts {
        let mut database = Database::new();
        database.load_system_fonts();
        let mut sans = chain(
            &database,
            SANS_STACK,
            Family::SansSerif,
            Weight::NORMAL,
            "sans",
        );
        fill_coverage(&database, &mut sans, Weight::NORMAL, "sans");
        if sans.is_empty() {
            // A host with none of the stack (a bare Linux container) still
            // gets a Latin face, so layout and rasterisation always have one.
            sans.push(Face {
                key: "ubuntu-light".to_owned(),
                family: "Ubuntu".to_owned(),
                data: epaint_default_fonts::UBUNTU_LIGHT,
                index: 0,
            });
        }
        // The stylesheet sets serif text at weight 600.
        let serif = chain(
            &database,
            SERIF_STACK,
            Family::Serif,
            Weight::SEMIBOLD,
            "serif",
        );
        UiFonts { sans, serif }
    }

    fn chain(
        database: &Database,
        stack: &[&str],
        generic: Family<'_>,
        weight: Weight,
        prefix: &str,
    ) -> Vec<Face> {
        let mut faces: Vec<Face> = Vec::new();
        let mut seen = Vec::new();
        let names = stack.iter().map(|name| Family::Name(name)).chain([generic]);
        for family in names {
            let query = Query {
                families: &[family],
                weight,
                stretch: Stretch::Normal,
                style: Style::Normal,
            };
            let Some(id) = database.query(&query) else {
                continue;
            };
            if seen.contains(&id) {
                continue;
            }
            seen.push(id);
            if gain(database, id, &faces) > 0 {
                push(database, id, prefix, &mut faces);
            }
        }
        faces
    }

    /// The browser's last resort, after its named stack: any host face that
    /// covers a probe character the chain still lacks. On macOS and Windows
    /// the stack's own faces normally cover everything and this finds nothing
    /// to do; on Linux the stack's names are mostly absent, and this is what
    /// finds the host's CJK face. Each round takes the face covering the most
    /// missing characters, so a broad CJK face wins over a bitmap fallback;
    /// ties go to the closest weight and then the family name, so the choice
    /// does not depend on scan order.
    fn fill_coverage(database: &Database, faces: &mut Vec<Face>, weight: Weight, prefix: &str) {
        let mut candidates: Vec<_> = database
            .faces()
            .filter(|info| info.style == Style::Normal && !info.monospaced)
            .map(|info| {
                let name = info
                    .families
                    .first()
                    .map_or_else(String::new, |(name, _)| name.clone());
                (info.weight.0.abs_diff(weight.0), name, info.index, info.id)
            })
            .collect();
        candidates
            .sort_by(|left, right| (left.0, &left.1, left.2).cmp(&(right.0, &right.1, right.2)));
        loop {
            let best = candidates
                .iter()
                .map(|candidate| (gain(database, candidate.3, faces), candidate.3))
                .filter(|(gain, _)| *gain > 0)
                // `max_by_key` keeps the last maximum; reverse so the first
                // (closest weight, then name) wins a tie.
                .rev()
                .max_by_key(|(gain, _)| *gain);
            let Some((_, id)) = best else {
                break;
            };
            push(database, id, prefix, faces);
            candidates.retain(|candidate| candidate.3 != id);
        }
    }

    /// How many probe characters the face maps that none of `faces` maps. A
    /// first face counts its whole coverage: it is the chain's primary face.
    fn gain(database: &Database, id: ID, faces: &[Face]) -> usize {
        database
            .with_face_data(id, |data, index| {
                let Ok(candidate) = FontRef::from_index(data, index) else {
                    return 0;
                };
                let covered = |character: char| {
                    faces.iter().any(|face| {
                        FontRef::from_index(face.data, face.index)
                            .is_ok_and(|font| font.charmap().map(character).is_some())
                    })
                };
                COVERAGE_PROBE
                    .chars()
                    .filter(|character| !character.is_whitespace())
                    .filter(|character| {
                        candidate.charmap().map(*character).is_some() && !covered(*character)
                    })
                    .count()
            })
            .unwrap_or(0)
    }

    fn push(database: &Database, id: ID, prefix: &str, faces: &mut Vec<Face>) {
        let Some(info) = database.face(id) else {
            return;
        };
        let Some(data) = database.with_face_data(id, |data, _| data.to_vec()) else {
            return;
        };
        let name = info
            .families
            .first()
            .map_or_else(|| "unknown".to_owned(), |(name, _)| name.clone());
        faces.push(Face {
            key: format!("{prefix}-{}-{}", faces.len(), name),
            family: name,
            // Held for the life of the process, like an embedded font.
            data: Box::leak(data.into_boxed_slice()),
            index: info.index,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skrifa::{FontRef, MetadataProvider};

    #[test]
    fn every_face_parses_and_the_chains_are_never_empty() {
        let fonts = ui_fonts();
        assert!(!fonts.sans.is_empty());
        for face in fonts.serif_chain() {
            FontRef::from_index(face.data, face.index)
                .unwrap_or_else(|error| panic!("{} does not parse: {error}", face.key));
        }
        // The primary sans face draws Latin; later faces exist for what it lacks.
        let primary = &fonts.sans[0];
        let primary = FontRef::from_index(primary.data, primary.index).unwrap();
        assert!(primary.charmap().map('A').is_some());
        let mut keys: Vec<_> = fonts.serif_chain().map(|face| &face.key).collect();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), fonts.serif.len() + fonts.sans.len());
    }

    #[cfg(feature = "bundled-fonts")]
    #[test]
    fn the_bundled_build_needs_no_host_font() {
        let fonts = ui_fonts();
        let keys: Vec<_> = fonts.serif_chain().map(|face| face.key.as_str()).collect();
        assert_eq!(keys, ["source-serif-4-semibold", "noto-sans-cjk-sc"]);
    }

    #[cfg(not(feature = "bundled-fonts"))]
    #[test]
    fn the_system_build_searches_the_stylesheet_stacks() {
        let fonts = ui_fonts();
        // `--nocapture` shows what this host resolved to.
        let keys = |faces: &[Face]| {
            faces
                .iter()
                .map(|face| face.key.clone())
                .collect::<Vec<_>>()
        };
        eprintln!(
            "sans: {:?}\nserif: {:?}",
            keys(&fonts.sans),
            keys(&fonts.serif)
        );
        // Whatever the host is, the sans chain covers every probe character
        // some host face covers: what the browser's fallback would draw.
        let mut database = fontdb::Database::new();
        database.load_system_fonts();
        let maps = |face: &Face, character: char| {
            FontRef::from_index(face.data, face.index)
                .is_ok_and(|font| font.charmap().map(character).is_some())
        };
        for character in COVERAGE_PROBE.chars().filter(|c| !c.is_whitespace()) {
            let on_host = database.faces().any(|info| {
                database
                    .with_face_data(info.id, |data, index| {
                        FontRef::from_index(data, index)
                            .is_ok_and(|font| font.charmap().map(character).is_some())
                    })
                    .unwrap_or(false)
            });
            if on_host {
                assert!(
                    fonts.sans.iter().any(|face| maps(face, character)),
                    "the host has {character} but the sans chain does not"
                );
            }
        }
        // Whatever the host has, a face only joins a chain if it covers
        // something the faces before it do not.
        for chain in [&fonts.sans, &fonts.serif] {
            for (position, face) in chain.iter().enumerate().skip(1) {
                assert!(
                    system_probe_gain(&chain[..position], face),
                    "{} adds nothing to its chain",
                    face.key
                );
            }
        }
    }

    #[cfg(not(feature = "bundled-fonts"))]
    fn system_probe_gain(before: &[Face], face: &Face) -> bool {
        let font = FontRef::from_index(face.data, face.index).unwrap();
        COVERAGE_PROBE.chars().any(|character| {
            !character.is_whitespace()
                && font.charmap().map(character).is_some()
                && !before.iter().any(|earlier| {
                    FontRef::from_index(earlier.data, earlier.index)
                        .is_ok_and(|font| font.charmap().map(character).is_some())
                })
        })
    }
}
