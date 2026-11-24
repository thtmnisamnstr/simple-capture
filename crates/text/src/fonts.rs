//! Fonts for text overlays (ADR-020): the bundled families, loaded through FreeType from the
//! fonts folder, any installed system family through fontdb, and Noto Color Emoji after every
//! family. Weights come from variable-font instances (`wght`), never synthetic bold.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use sc_core::{Error, Result};
use skia_safe::font_arguments::VariationPosition;
use skia_safe::font_arguments::variation_position::Coordinate;
use skia_safe::textlayout::{FontCollection, TypefaceFontProvider};
use skia_safe::{Data, FontArguments, FontMgr, FourByteTag, Typeface};

pub const EMOJI_FAMILY: &str = "Noto Color Emoji";
const EMOJI_FILE: &str = "Noto-COLRv1.ttf";

#[derive(Debug, Clone, Copy)]
pub struct BundledFace {
    pub file: &'static str,
    pub italic: bool,
    /// `(min, max)` of the `wght` axis, or the fixed weight twice.
    pub weights: (u32, u32),
}

#[derive(Debug, Clone, Copy)]
pub struct BundledFamily {
    pub name: &'static str,
    pub kind: &'static str,
    pub default_weight: u32,
    pub faces: &'static [BundledFace],
}

const fn face(file: &'static str, italic: bool, lo: u32, hi: u32) -> BundledFace {
    BundledFace {
        file,
        italic,
        weights: (lo, hi),
    }
}

pub const BUNDLED: &[BundledFamily] = &[
    BundledFamily {
        name: "Inter",
        kind: "Modern",
        default_weight: 700,
        faces: &[
            face("Inter[opsz,wght].ttf", false, 100, 900),
            face("Inter-Italic[opsz,wght].ttf", true, 100, 900),
        ],
    },
    BundledFamily {
        name: "Montserrat",
        kind: "Geometric",
        default_weight: 800,
        faces: &[
            face("Montserrat[wght].ttf", false, 100, 900),
            face("Montserrat-Italic[wght].ttf", true, 100, 900),
        ],
    },
    BundledFamily {
        name: "Oswald",
        kind: "Bold condensed",
        default_weight: 700,
        faces: &[face("Oswald[wght].ttf", false, 200, 700)],
    },
    BundledFamily {
        name: "Bebas Neue",
        kind: "Poster",
        default_weight: 400,
        faces: &[face("BebasNeue-Regular.ttf", false, 400, 400)],
    },
    BundledFamily {
        name: "Playfair Display",
        kind: "Serif",
        default_weight: 700,
        faces: &[
            face("PlayfairDisplay[wght].ttf", false, 400, 900),
            face("PlayfairDisplay-Italic[wght].ttf", true, 400, 900),
        ],
    },
    BundledFamily {
        name: "Abril Fatface",
        kind: "Display serif",
        default_weight: 400,
        faces: &[face("AbrilFatface-Regular.ttf", false, 400, 400)],
    },
    BundledFamily {
        name: "Courier Prime",
        kind: "Typewriter",
        default_weight: 400,
        faces: &[
            face("CourierPrime-Regular.ttf", false, 400, 400),
            face("CourierPrime-Bold.ttf", false, 700, 700),
            face("CourierPrime-Italic.ttf", true, 400, 400),
            face("CourierPrime-BoldItalic.ttf", true, 700, 700),
        ],
    },
    BundledFamily {
        name: "Special Elite",
        kind: "Old typewriter",
        default_weight: 400,
        faces: &[face("SpecialElite-Regular.ttf", false, 400, 400)],
    },
    BundledFamily {
        name: "Caveat",
        kind: "Handwriting",
        default_weight: 700,
        faces: &[face("Caveat[wght].ttf", false, 400, 700)],
    },
    BundledFamily {
        name: "Sedgwick Ave Display",
        kind: "Marker",
        default_weight: 400,
        faces: &[face("SedgwickAveDisplay-Regular.ttf", false, 400, 400)],
    },
    BundledFamily {
        name: "Pacifico",
        kind: "Script",
        default_weight: 400,
        faces: &[face("Pacifico-Regular.ttf", false, 400, 400)],
    },
    BundledFamily {
        name: "Tilt Neon",
        kind: "Neon",
        default_weight: 400,
        faces: &[face("TiltNeon[XROT,YROT].ttf", false, 400, 400)],
    },
    BundledFamily {
        name: "Baloo 2",
        kind: "Rounded",
        default_weight: 700,
        faces: &[face("Baloo2[wght].ttf", false, 400, 800)],
    },
];

pub fn bundled(name: &str) -> Option<&'static BundledFamily> {
    BUNDLED.iter().find(|f| f.name.eq_ignore_ascii_case(name))
}

/// `SC_FONTS_DIR`, the app bundle's `Resources/fonts`, `fonts` next to the executable, or the
/// repository's `assets/fonts`.
static RESOURCE_FONTS: OnceLock<PathBuf> = OnceLock::new();

/// The app's resource folder for fonts, as the bundle laid it out (Linux packages put resources
/// under /usr/lib, away from the executable).
pub fn set_resource_fonts_dir(dir: PathBuf) {
    let _ = RESOURCE_FONTS.set(dir);
}

pub fn default_fonts_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("SC_FONTS_DIR") {
        return PathBuf::from(d);
    }
    if let Some(d) = RESOURCE_FONTS.get().filter(|d| d.join(EMOJI_FILE).exists()) {
        return d.clone();
    }
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts");
    let exe = std::env::current_exe().ok();
    let beside = exe.as_ref().and_then(|e| e.parent()).map(|d| {
        [
            d.join("../Resources/fonts"),
            d.join("fonts"),
            d.join("resources/fonts"),
        ]
    });
    beside
        .into_iter()
        .flatten()
        .find(|d| d.join(EMOJI_FILE).exists())
        .unwrap_or(repo)
}

fn with_weight(tf: &Typeface, w: u32) -> Typeface {
    let coords = [Coordinate {
        axis: FourByteTag::from_chars('w', 'g', 'h', 't'),
        value: w as f32,
    }];
    let args = FontArguments::new().set_variation_design_position(VariationPosition {
        coordinates: &coords,
    });
    tf.clone_with_arguments(&args).unwrap_or_else(|| tf.clone())
}

/// Installed font families (fontdb), sorted, scanned once per process.
pub fn system_families() -> Vec<String> {
    let db = system_db();
    let mut fams: Vec<String> = db
        .faces()
        .filter_map(|f| f.families.first().map(|(n, _)| n.clone()))
        .filter(|n| !n.starts_with('.'))
        .collect();
    fams.sort_by_key(|a| a.to_lowercase());
    fams.dedup();
    fams
}

fn system_db() -> &'static fontdb::Database {
    static DB: OnceLock<fontdb::Database> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        db
    })
}

/// One thread's fonts: typefaces registered under aliases for each (family, weight, italic)
/// asked for, and the collection paragraphs lay out with.
pub struct FontLibrary {
    dir: PathBuf,
    mgr: FontMgr,
    provider: TypefaceFontProvider,
    collection: FontCollection,
    aliases: HashMap<(String, u32, bool), String>,
    dirty: bool,
}

impl FontLibrary {
    pub fn new() -> Result<FontLibrary> {
        FontLibrary::with_dir(&default_fonts_dir())
    }

    pub fn with_dir(dir: &Path) -> Result<FontLibrary> {
        let mgr = FontMgr::custom_empty().ok_or_else(|| {
            Error::Other("sc_text: the FreeType font manager is missing from this build".into())
        })?;
        let mut lib = FontLibrary {
            dir: dir.to_path_buf(),
            mgr,
            provider: TypefaceFontProvider::new(),
            collection: FontCollection::new(),
            aliases: HashMap::new(),
            dirty: true,
        };
        let emoji = lib.load(&dir.join(EMOJI_FILE), 0)?;
        lib.provider.register_typeface(emoji, EMOJI_FAMILY);
        Ok(lib)
    }

    fn load(&self, path: &Path, index: u32) -> Result<Typeface> {
        let data = Data::from_filename(path)
            .ok_or_else(|| Error::Other(format!("read font {}", path.display())))?;
        self.mgr
            .new_from_data(data, Some(index))
            .ok_or_else(|| Error::Invalid(format!("sc_text: cannot load font {}", path.display())))
    }

    /// The family name to lay out with for `family` at `weight` (100–900) and `italic`.
    /// Unknown families fall back to Inter.
    pub fn resolve(&mut self, family: &str, weight: u32, italic: bool) -> Result<String> {
        let key = (family.to_lowercase(), weight, italic);
        if let Some(a) = self.aliases.get(&key) {
            return Ok(a.clone());
        }
        let tf = match bundled(family) {
            Some(fam) => self.bundled_face(fam, weight, italic)?,
            None => match self.system_face(family, weight, italic) {
                Some(tf) => tf,
                None => {
                    let inter = bundled("Inter").ok_or_else(|| Error::Other("no Inter".into()))?;
                    self.bundled_face(inter, weight, italic)?
                }
            },
        };
        let alias = format!("sc:{}:{weight}:{}", family.to_lowercase(), u8::from(italic));
        self.provider.register_typeface(tf, Some(alias.as_str()));
        self.aliases.insert(key, alias.clone());
        self.dirty = true;
        Ok(alias)
    }

    fn bundled_face(&self, fam: &BundledFamily, weight: u32, italic: bool) -> Result<Typeface> {
        let pick = |want_italic: bool| {
            fam.faces
                .iter()
                .filter(|f| f.italic == want_italic)
                .min_by_key(|f| {
                    let (lo, hi) = f.weights;
                    if (lo..=hi).contains(&weight) {
                        0
                    } else {
                        weight.abs_diff(lo).min(weight.abs_diff(hi))
                    }
                })
        };
        let f = pick(italic)
            .or_else(|| pick(!italic))
            .ok_or_else(|| Error::Other(format!("sc_text: {} has no faces", fam.name)))?;
        let tf = self.load(&self.dir.join(f.file), 0)?;
        let (lo, hi) = f.weights;
        Ok(if lo < hi {
            with_weight(&tf, weight.clamp(lo, hi))
        } else {
            tf
        })
    }

    fn system_face(&self, family: &str, weight: u32, italic: bool) -> Option<Typeface> {
        let db = system_db();
        let query = fontdb::Query {
            families: &[fontdb::Family::Name(family)],
            weight: fontdb::Weight(weight.clamp(100, 900) as u16),
            style: if italic {
                fontdb::Style::Italic
            } else {
                fontdb::Style::Normal
            },
            ..Default::default()
        };
        let id = db.query(&query)?;
        let face = db.face(id)?;
        match &face.source {
            fontdb::Source::File(path) => self.load(path, face.index).ok(),
            _ => None,
        }
    }

    pub fn collection(&mut self) -> FontCollection {
        if self.dirty {
            let mut c = FontCollection::new();
            c.set_asset_font_manager(Some(self.provider.clone().into()));
            c.set_default_font_manager(None, None);
            c.disable_font_fallback();
            c.paragraph_cache_mut().turn_on(false);
            self.collection = c;
            self.dirty = false;
        }
        self.collection.clone()
    }
}
