//! The languages TypeScript 7.1's messages are translated into (tsgo
//! `locale.Locale` and `diagnostics.Localize`): the thirteen catalogs tsgo
//! embeds as gzip-compressed JSON, matched from a BCP 47 language tag and
//! read on first use.

use std::collections::HashMap;
use std::sync::OnceLock;

use tsc_diagnostics::MessageCatalog;

/// A language for the messages: English (the catalog's own text) or one of
/// tsgo's translations.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum Locale {
    #[default]
    English,
    Czech,
    German,
    Spanish,
    French,
    Italian,
    Japanese,
    Korean,
    Polish,
    PortugueseBrazil,
    Russian,
    Turkish,
    ChineseSimplified,
    ChineseTraditional,
}

const TRANSLATIONS: usize = 13;

macro_rules! catalog_bytes {
    ($name:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../vendor/typescript-native/7.1.0-dev-aa814927/upstream/tsc/internal/diagnostics/loc/",
            $name,
            ".json.gz"
        ))
    };
}

/// The compressed catalogs, in [`Locale::index`] order.
static CATALOG_BYTES: [&[u8]; TRANSLATIONS] = [
    catalog_bytes!("cs-CZ"),
    catalog_bytes!("de-DE"),
    catalog_bytes!("es-ES"),
    catalog_bytes!("fr-FR"),
    catalog_bytes!("it-IT"),
    catalog_bytes!("ja-JP"),
    catalog_bytes!("ko-KR"),
    catalog_bytes!("pl-PL"),
    catalog_bytes!("pt-BR"),
    catalog_bytes!("ru-RU"),
    catalog_bytes!("tr-TR"),
    catalog_bytes!("zh-CN"),
    catalog_bytes!("zh-TW"),
];

static CATALOGS: [OnceLock<Catalog>; TRANSLATIONS] = [const { OnceLock::new() }; TRANSLATIONS];

/// One translation: message key to template.
#[derive(Debug, Default)]
pub struct Catalog {
    templates: HashMap<String, String>,
}

impl MessageCatalog for Catalog {
    fn template(&self, key: &str) -> Option<&str> {
        self.templates.get(key).map(String::as_str)
    }
}

impl Locale {
    /// tsgo `locale.Parse` followed by its catalog match: `None` for a tag
    /// that is not a well-formed BCP 47 language tag, English for one no
    /// translation matches.
    pub fn parse(tag: &str) -> Option<Self> {
        let subtags = tag.split(['-', '_']).collect::<Vec<_>>();
        let language = subtags.first()?.to_ascii_lowercase();
        if !(2..=3).contains(&language.len()) || !language.bytes().all(|b| b.is_ascii_alphabetic())
        {
            return None;
        }
        let mut script = None;
        let mut region = None;
        let mut rest = subtags[1..]
            .iter()
            .map(|subtag| subtag.to_ascii_lowercase());
        let mut pending = rest.next();
        // extlang (three letters, up to three), script, region, variants,
        // extensions and private use, in that order.
        let mut extlangs = 0;
        while let Some(subtag) = pending.as_deref() {
            if extlangs < 3
                && script.is_none()
                && subtag.len() == 3
                && subtag.bytes().all(|b| b.is_ascii_alphabetic())
            {
                extlangs += 1;
            } else {
                break;
            }
            pending = rest.next();
        }
        if let Some(subtag) = pending.as_deref() {
            if subtag.len() == 4 && subtag.bytes().all(|b| b.is_ascii_alphabetic()) {
                script = Some(subtag.to_owned());
                pending = rest.next();
            }
        }
        if let Some(subtag) = pending.as_deref() {
            if (subtag.len() == 2 && subtag.bytes().all(|b| b.is_ascii_alphabetic()))
                || (subtag.len() == 3 && subtag.bytes().all(|b| b.is_ascii_digit()))
            {
                region = Some(subtag.to_owned());
                pending = rest.next();
            }
        }
        while let Some(subtag) = pending.take() {
            let alphanumeric = subtag.bytes().all(|b| b.is_ascii_alphanumeric());
            let variant = alphanumeric
                && ((5..=8).contains(&subtag.len())
                    || (subtag.len() == 4 && subtag.as_bytes()[0].is_ascii_digit()));
            if variant {
                pending = rest.next();
                continue;
            }
            // An extension or private use: a singleton, then subtags.
            if subtag.len() == 1 && alphanumeric {
                let private = subtag == "x";
                let mut any = false;
                for next in rest.by_ref() {
                    let fits = if private {
                        (1..=8).contains(&next.len())
                    } else {
                        (2..=8).contains(&next.len())
                    };
                    if !fits || !next.bytes().all(|b| b.is_ascii_alphanumeric()) {
                        return None;
                    }
                    any = true;
                }
                return any
                    .then(|| Self::matching(&language, script.as_deref(), region.as_deref()));
            }
            return None;
        }
        Some(Self::matching(
            &language,
            script.as_deref(),
            region.as_deref(),
        ))
    }

    /// The translation tsgo's language matcher picks for a tag.
    fn matching(language: &str, script: Option<&str>, region: Option<&str>) -> Self {
        match language {
            "cs" => Self::Czech,
            "de" => Self::German,
            "es" => Self::Spanish,
            "fr" => Self::French,
            "it" => Self::Italian,
            "ja" => Self::Japanese,
            "ko" => Self::Korean,
            "pl" => Self::Polish,
            "pt" => Self::PortugueseBrazil,
            "ru" => Self::Russian,
            "tr" => Self::Turkish,
            "zh" => {
                if script == Some("hant") || matches!(region, Some("tw" | "hk" | "mo")) {
                    Self::ChineseTraditional
                } else {
                    Self::ChineseSimplified
                }
            }
            _ => Self::English,
        }
    }

    fn index(self) -> Option<usize> {
        Some(match self {
            Self::English => return None,
            Self::Czech => 0,
            Self::German => 1,
            Self::Spanish => 2,
            Self::French => 3,
            Self::Italian => 4,
            Self::Japanese => 5,
            Self::Korean => 6,
            Self::Polish => 7,
            Self::PortugueseBrazil => 8,
            Self::Russian => 9,
            Self::Turkish => 10,
            Self::ChineseSimplified => 11,
            Self::ChineseTraditional => 12,
        })
    }

    /// The translation's catalog (`None` for English), decompressed and
    /// parsed on first use. A catalog that cannot be read translates
    /// nothing.
    pub fn catalog(self) -> Option<&'static Catalog> {
        let index = self.index()?;
        Some(CATALOGS[index].get_or_init(|| read_catalog(CATALOG_BYTES[index]).unwrap_or_default()))
    }

    /// The catalog as the renderers take it.
    pub fn messages(self) -> Option<&'static dyn MessageCatalog> {
        self.catalog().map(|catalog| catalog as &dyn MessageCatalog)
    }
}

/// A gzip member's JSON object of message templates.
fn read_catalog(gzip: &[u8]) -> Option<Catalog> {
    let json = gunzip(gzip)?;
    let templates: HashMap<String, String> = serde_json::from_slice(&json).ok()?;
    Some(Catalog { templates })
}

/// The data of a gzip member (RFC 1952): the header's optional fields are
/// skipped and the deflate stream inflated.
fn gunzip(bytes: &[u8]) -> Option<Vec<u8>> {
    const FHCRC: u8 = 1 << 1;
    const FEXTRA: u8 = 1 << 2;
    const FNAME: u8 = 1 << 3;
    const FCOMMENT: u8 = 1 << 4;
    if bytes.len() < 18 || bytes[0] != 0x1f || bytes[1] != 0x8b || bytes[2] != 8 {
        return None;
    }
    let flags = bytes[3];
    let mut offset = 10;
    if flags & FEXTRA != 0 {
        let length = usize::from(*bytes.get(offset)?) | usize::from(*bytes.get(offset + 1)?) << 8;
        offset += 2 + length;
    }
    for flag in [FNAME, FCOMMENT] {
        if flags & flag != 0 {
            offset += bytes.get(offset..)?.iter().position(|&byte| byte == 0)? + 1;
        }
    }
    if flags & FHCRC != 0 {
        offset += 2;
    }
    miniz_oxide::inflate::decompress_to_vec(bytes.get(offset..bytes.len() - 8)?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsc_diagnostics::gen;

    #[test]
    fn tags_parse_as_bcp_47_and_match_tsgos_translations() {
        assert_eq!(Locale::parse("en"), Some(Locale::English));
        assert_eq!(Locale::parse("cs"), Some(Locale::Czech));
        assert_eq!(Locale::parse("ja-jp"), Some(Locale::Japanese));
        assert_eq!(Locale::parse("pt-BR"), Some(Locale::PortugueseBrazil));
        assert_eq!(Locale::parse("zh-TW"), Some(Locale::ChineseTraditional));
        assert_eq!(Locale::parse("zh-Hant"), Some(Locale::ChineseTraditional));
        assert_eq!(Locale::parse("zh"), Some(Locale::ChineseSimplified));
        assert_eq!(Locale::parse("en-US-x-private"), Some(Locale::English));
        assert_eq!(Locale::parse("whoops"), None);
        assert_eq!(Locale::parse(""), None);
        assert_eq!(Locale::parse("en-"), None);
    }

    #[test]
    fn catalogs_translate_by_message_key() {
        let czech = Locale::Czech.messages();
        assert_eq!(gen::Version_0.template_in(czech), "Verze {0}");
        assert_eq!(
            gen::Version_0.format_in(Locale::Japanese.messages(), &["7.1"]),
            "バージョン 7.1"
        );
        assert_eq!(
            gen::Version_0.template_in(Locale::English.messages()),
            "Version {0}"
        );
    }
}
