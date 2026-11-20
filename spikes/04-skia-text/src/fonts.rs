use skia_safe::{
    Data, FontArguments, FontMgr, Typeface,
    font_arguments::{VariationPosition, variation_position::Coordinate},
    textlayout::{FontCollection, TypefaceFontProvider},
};
use std::path::Path;

pub const EMOJI_FAMILY: &str = "Noto Color Emoji";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MgrKind {
    FreeType,
    System,
}

pub fn make_mgr(kind: MgrKind) -> Option<FontMgr> {
    match kind {
        MgrKind::FreeType => FontMgr::custom_empty(),
        MgrKind::System => Some(FontMgr::new()),
    }
}

pub fn preferred_mgr() -> (FontMgr, MgrKind) {
    let forced = std::env::var("SPIKE_FONTMGR").ok();
    if forced.as_deref() != Some("system") {
        if let Some(m) = FontMgr::custom_empty() {
            return (m, MgrKind::FreeType);
        }
    }
    (FontMgr::new(), MgrKind::System)
}

pub struct Fonts {
    pub collection: FontCollection,
    pub provider: TypefaceFontProvider,
    pub mgr: FontMgr,
    pub kind: MgrKind,
    pub emoji_file: String,
}

fn wght(tf: &Typeface, w: f32) -> Typeface {
    let coords = [Coordinate { axis: skia_safe::FourByteTag::from_chars('w', 'g', 'h', 't'), value: w }];
    let args = FontArguments::new().set_variation_design_position(VariationPosition { coordinates: &coords });
    tf.clone_with_arguments(&args).expect("variation instance")
}

pub fn load_typeface(mgr: &FontMgr, path: &Path, index: u32) -> Option<Typeface> {
    let bytes = std::fs::read(path).ok()?;
    mgr.new_from_data(Data::new_copy(&bytes), Some(index))
}

pub fn load_bundled(dir: &Path, emoji_file: &str) -> Fonts {
    let (mgr, kind) = preferred_mgr();
    load_bundled_with(dir, emoji_file, mgr, kind)
}

pub fn load_bundled_with(dir: &Path, emoji_file: &str, mgr: FontMgr, kind: MgrKind) -> Fonts {
    let mut provider = TypefaceFontProvider::new();
    let instances: [(&str, f32, &str); 4] = [
        ("Oswald[wght].ttf", 700.0, "Oswald Bold"),
        ("Fredoka[wdth,wght].ttf", 600.0, "Fredoka SemiBold"),
        ("Caveat[wght].ttf", 700.0, "Caveat Bold"),
        ("Oswald[wght].ttf", 400.0, "Oswald"),
    ];
    for (file, w, alias) in instances {
        let base = load_typeface(&mgr, &dir.join(file), 0).unwrap_or_else(|| panic!("load {file}"));
        provider.register_typeface(wght(&base, w), alias);
    }
    let emoji = load_typeface(&mgr, &dir.join(emoji_file), 0);
    match emoji {
        Some(tf) => {
            provider.register_typeface(tf, EMOJI_FAMILY);
        }
        None => eprintln!("WARNING: font manager {kind:?} could not load {emoji_file}"),
    }
    let mut collection = FontCollection::new();
    collection.set_asset_font_manager(Some(provider.clone().into()));
    collection.set_default_font_manager(None, None);
    collection.disable_font_fallback();
    collection.paragraph_cache_mut().turn_on(false);
    Fonts { collection, provider, mgr, kind, emoji_file: emoji_file.to_string() }
}

impl Fonts {
    pub fn add_alias(&mut self, tf: Typeface, alias: &str) {
        self.provider.register_typeface(tf, alias);
        let mut collection = FontCollection::new();
        collection.set_asset_font_manager(Some(self.provider.clone().into()));
        collection.set_default_font_manager(None, None);
        collection.disable_font_fallback();
        collection.paragraph_cache_mut().turn_on(false);
        self.collection = collection;
    }
}

pub struct SystemFontProbe {
    pub faces: usize,
    pub families: usize,
    pub sample_families: Vec<String>,
    pub chosen: Option<(String, String, u32)>,
    pub scan_ms: f64,
}

pub fn fontdb_probe(fonts: &mut Fonts) -> SystemFontProbe {
    let t = std::time::Instant::now();
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    let scan_ms = t.elapsed().as_secs_f64() * 1000.0;
    let mut fams: Vec<String> = db
        .faces()
        .filter_map(|f| f.families.first().map(|(n, _)| n.clone()))
        .collect();
    fams.sort();
    fams.dedup();
    let sample_families = fams.iter().take(8).cloned().collect();
    let query = fontdb::Query {
        families: &[fontdb::Family::Name("Georgia"), fontdb::Family::Serif, fontdb::Family::SansSerif],
        weight: fontdb::Weight::BOLD,
        ..Default::default()
    };
    let mut chosen = None;
    if let Some(id) = db.query(&query) {
        let face = db.face(id).unwrap();
        let name = face.families.first().map(|(n, _)| n.clone()).unwrap_or_default();
        if let fontdb::Source::File(path) = &face.source {
            if let Some(tf) = load_typeface(&fonts.mgr, path, face.index) {
                fonts.add_alias(tf, "System Pick");
                chosen = Some((name, path.display().to_string(), face.index));
            }
        }
    }
    SystemFontProbe { faces: db.len(), families: fams.len(), sample_families, chosen, scan_ms }
}
