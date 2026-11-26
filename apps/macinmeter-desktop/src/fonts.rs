//! The font definitions shared by the window, card measurement and PNG export.

use eframe::egui::{FontData, FontDefinitions, FontFamily};
use std::sync::{Arc, OnceLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Family {
    Sans,
    Serif,
    Mono,
}

impl Family {
    pub fn egui(self) -> FontFamily {
        match self {
            Self::Sans => FontFamily::Proportional,
            Self::Serif => FontFamily::Name("serif".into()),
            Self::Mono => FontFamily::Monospace,
        }
    }
}

type FontBytes = Arc<dyn AsRef<[u8]> + Send + Sync>;

struct Face {
    key: String,
    source: FontBytes,
    index: u32,
}

#[derive(Default)]
struct Selection {
    sans: Vec<Face>,
    serif: Vec<Face>,
    fallback: Vec<Face>,
}

impl Selection {
    fn definitions(&'static self) -> FontDefinitions {
        let mut defs = FontDefinitions::default();
        for face in self.sans.iter().chain(&self.serif).chain(&self.fallback) {
            defs.font_data.entry(face.key.clone()).or_insert_with(|| {
                let mut data = FontData::from_static(face.source.as_ref().as_ref());
                data.index = face.index;
                Arc::new(data)
            });
        }
        let keys = |faces: &[Face]| faces.iter().map(|f| f.key.clone()).collect::<Vec<_>>();
        let sans = keys(&self.sans);
        let fallback = keys(&self.fallback);
        let proportional = defs.families.entry(Family::Sans.egui()).or_default();
        proportional.splice(0..0, sans.iter().cloned());
        proportional.extend(fallback.iter().cloned());
        let mut serif = keys(&self.serif);
        for key in proportional.iter() {
            if !serif.contains(key) {
                serif.push(key.clone());
            }
        }
        defs.families.insert(Family::Serif.egui(), serif);
        defs.families
            .entry(Family::Mono.egui())
            .or_default()
            .extend(sans.into_iter().chain(fallback));
        defs
    }
}

pub fn definitions() -> &'static FontDefinitions {
    // FontData borrows bytes for egui. One process-owned selection keeps the
    // shared file allocations alive for both the window and export workers.
    static SELECTION: OnceLock<Selection> = OnceLock::new();
    static DEFINITIONS: OnceLock<FontDefinitions> = OnceLock::new();
    DEFINITIONS.get_or_init(|| SELECTION.get_or_init(resolve).definitions())
}

#[cfg(feature = "bundled-fonts")]
fn resolve() -> Selection {
    let bundled = |key: &str, bytes: &'static [u8]| Face {
        key: key.to_owned(),
        source: Arc::new(bytes),
        index: 0,
    };
    Selection {
        sans: vec![bundled(
            "noto-sans-cjk-sc",
            include_bytes!(concat!(env!("OUT_DIR"), "/NotoSansCJKsc-Regular.otf")),
        )],
        serif: vec![bundled(
            "source-serif-4-semibold",
            include_bytes!("../fonts/SourceSerif4-Semibold.otf"),
        )],
        fallback: Vec::new(),
    }
}

#[cfg(not(feature = "bundled-fonts"))]
fn resolve() -> Selection {
    system::load()
}

#[cfg(not(feature = "bundled-fonts"))]
mod system {
    use super::{Face, FontBytes, Selection};
    use fontdb::{Database, Family, ID, Query, Source, Style, Weight};
    use skrifa::{
        FontRef, MetadataProvider,
        raw::{
            TableProvider,
            tables::{head::Flags, sbix::HeaderFlags},
        },
    };
    use std::{collections::HashMap, path::Path, sync::Arc};

    // The named families in the Tauri stylesheet. Platform names for system-ui
    // follow its named CJK faces in that stylesheet.
    const SANS: &[&str] = &[
        "Inter",
        "SF Pro Text",
        "PingFang SC",
        "Microsoft YaHei",
        "Hiragino Sans",
        "Hiragino Kaku Gothic Pro",
        "Yu Gothic",
        "Meiryo",
        ".SF NS",
        "Segoe UI",
        "Helvetica Neue",
        "Noto Sans CJK SC",
        "Noto Sans",
        "DejaVu Sans",
    ];
    const SERIF: &[&str] = &[
        "Iowan Old Style",
        "Palatino Linotype",
        "Palatino",
        "Songti SC",
        "Noto Serif CJK SC",
        "Source Han Serif SC",
        "SimSun",
    ];

    fn named(database: &Database, names: &[&str], generic: Family<'_>, weight: Weight) -> Vec<ID> {
        let mut ids = Vec::new();
        for family in names.iter().map(|name| Family::Name(name)).chain([generic]) {
            if let Some(id) = database.query(&Query {
                families: &[family],
                weight,
                ..Default::default()
            }) && !ids.contains(&id)
            {
                ids.push(id);
            }
        }
        ids
    }

    // Only faces that extend a chain's Unicode coverage are retained. The
    // temporary bitset is bounded by Unicode and is discarded after startup.
    struct Coverage {
        words: Vec<u64>,
        include_private_use: bool,
    }

    impl Coverage {
        fn new() -> Self {
            Self {
                words: vec![0; (char::MAX as usize + 1).div_ceil(64)],
                include_private_use: true,
            }
        }

        fn extend(&mut self, data: &[u8], index: u32) -> bool {
            let Ok(font) = FontRef::from_index(data, index) else {
                return false;
            };
            // A Last Resort cmap advertises generic block placeholders. egui
            // uses outlines, so an unnamed bitmap strike cannot fill a gap.
            let placeholders = font
                .head()
                .is_ok_and(|h| h.flags().contains(Flags::LAST_RESORT_FONT));
            let bitmap_fallback = !self.include_private_use
                && font
                    .sbix()
                    .is_ok_and(|s| !s.flags().contains(HeaderFlags::DRAW_OUTLINES));
            if placeholders || bitmap_fallback || font.outline_glyphs().format().is_none() {
                return false;
            }
            let mut added = false;
            for (code, _) in font.charmap().mappings() {
                if !self.include_private_use
                    && matches!(code, 0xe000..=0xf8ff | 0xf0000..=0xffffd | 0x100000..=0x10fffd)
                {
                    continue;
                }
                if let Some(word) = self.words.get_mut(code as usize / 64) {
                    let mask = 1_u64 << (code % 64);
                    added |= *word & mask == 0;
                    *word |= mask;
                }
            }
            added
        }
    }

    struct Loader<'a> {
        database: &'a Database,
        files: HashMap<&'a Path, FontBytes>,
    }

    impl Loader<'_> {
        fn select(&mut self, id: ID, coverage: &mut Coverage) -> Option<Face> {
            let info = self.database.face(id)?;
            let source = match &info.source {
                Source::Binary(bytes) | Source::SharedFile(_, bytes) => coverage
                    .extend(bytes.as_ref().as_ref(), info.index)
                    .then(|| Arc::clone(bytes))?,
                Source::File(path) => match self.files.get(path.as_path()) {
                    Some(bytes) => coverage
                        .extend(bytes.as_ref().as_ref(), info.index)
                        .then(|| Arc::clone(bytes))?,
                    None => {
                        let bytes: FontBytes =
                            self.database.with_face_data(id, |data, index| {
                                coverage
                                    .extend(data, index)
                                    .then(|| Arc::new(data.to_vec()) as FontBytes)
                            })??;
                        self.files.insert(path.as_path(), Arc::clone(&bytes));
                        bytes
                    }
                },
            };
            Some(Face {
                key: format!("host-{id}"),
                index: info.index,
                source,
            })
        }
    }

    pub(super) fn load() -> Selection {
        let mut database = Database::new();
        database.load_system_fonts();
        select(&database, SANS, SERIF)
    }

    fn select(database: &Database, sans_names: &[&str], serif_names: &[&str]) -> Selection {
        let mut loader = Loader {
            database,
            files: HashMap::new(),
        };
        let sans_ids = named(database, sans_names, Family::SansSerif, Weight::NORMAL);
        let serif_ids = named(database, serif_names, Family::Serif, Weight::SEMIBOLD);
        let mut sans_coverage = Coverage::new();
        let mut serif_coverage = Coverage::new();
        let sans = sans_ids
            .iter()
            .filter_map(|&id| loader.select(id, &mut sans_coverage))
            .collect();
        let serif = serif_ids
            .into_iter()
            .filter_map(|id| loader.select(id, &mut serif_coverage))
            .collect();

        // egui's proportional defaults precede unnamed host fallbacks. Only
        // retain another file if it contributes a glyph to that actual chain.
        for bytes in [
            epaint_default_fonts::UBUNTU_LIGHT,
            epaint_default_fonts::NOTO_EMOJI_REGULAR,
            epaint_default_fonts::EMOJI_ICON,
        ] {
            sans_coverage.extend(bytes, 0);
        }
        // Private-use glyphs have a meaning only in a particular named family.
        sans_coverage.include_private_use = false;
        let mut candidates: Vec<_> = database
            .faces()
            .filter(|f| !sans_ids.contains(&f.id))
            .collect();
        // A new collection is copied only when its face adds coverage. Within
        // each style prefer already resident sources, then smaller files.
        candidates.sort_by_cached_key(|f| {
            let bytes = match &f.source {
                Source::File(path) if !loader.files.contains_key(path.as_path()) => {
                    path.metadata().map_or(u64::MAX, |stat| stat.len())
                }
                _ => 0,
            };
            (
                f.monospaced,
                f.style != Style::Normal,
                f.weight.0.abs_diff(Weight::NORMAL.0),
                bytes,
                &f.post_script_name,
                f.index,
            )
        });
        let fallback = candidates
            .into_iter()
            .filter_map(|f| loader.select(f.id, &mut sans_coverage))
            .collect();
        Selection {
            sans,
            serif,
            fallback,
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;

        fn fixture() -> Database {
            let mut db = Database::new();
            db.load_font_file(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fonts/fallback.ttc"
            ))
            .unwrap();
            db
        }

        #[test]
        fn named_faces_keep_extra_kana_and_share_one_collection() {
            let selected = select(
                &fixture(),
                &["Fixture Sans", "Fixture Japanese"],
                &["Fixture Serif"],
            );
            assert_eq!(
                selected.sans.iter().map(|f| f.index).collect::<Vec<_>>(),
                [0, 1]
            );
            assert_eq!(selected.serif[0].index, 2);
            assert_eq!(
                selected
                    .fallback
                    .iter()
                    .map(|f| f.index)
                    .collect::<Vec<_>>(),
                [3]
            );
            for face in selected
                .sans
                .iter()
                .chain(&selected.serif)
                .chain(&selected.fallback)
            {
                assert!(Arc::ptr_eq(&selected.sans[0].source, &face.source));
            }
        }

        #[test]
        fn a_monospaced_only_cjk_face_remains_a_last_resort() {
            let mut db = fixture();
            let unused: Vec<_> = db.faces().filter(|f| !f.monospaced).map(|f| f.id).collect();
            for id in unused {
                db.remove_face(id);
            }
            let selected = select(&db, &[], &[]);
            assert!(selected.sans.is_empty());
            assert_eq!(selected.fallback.len(), 1);
            let face = &selected.fallback[0];
            let font = FontRef::from_index(face.source.as_ref().as_ref(), face.index).unwrap();
            assert!("原顺序".chars().all(|ch| font.charmap().map(ch).is_some()));
        }
    }
}
