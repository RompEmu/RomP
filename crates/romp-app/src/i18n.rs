use std::sync::atomic::{AtomicUsize, Ordering};

#[expect(
    dead_code,
    reason = "the Language setting and right-to-left layouts read these"
)]
pub struct Language {
    pub code: &'static str,
    /// The language's name in itself, as the Language setting lists it.
    pub name: &'static str,
    pub rtl: bool,
}

pub const LANGUAGES: [Language; 4] = [
    Language {
        code: "fr",
        name: "Français",
        rtl: false,
    },
    Language {
        code: "es",
        name: "Español",
        rtl: false,
    },
    Language {
        code: "de",
        name: "Deutsch",
        rtl: false,
    },
    Language {
        code: "ar",
        name: "العربية",
        rtl: true,
    },
];

/// The chosen language, or else the system's if RomP has it; `None` is English.
pub fn choose(preference: u8, system: Option<&str>) -> Option<&'static Language> {
    if let Some(chosen) = usize::from(preference).checked_sub(1) {
        return LANGUAGES.get(chosen);
    }
    let base = system?.split(['-', '_', '.']).next()?.to_ascii_lowercase();
    LANGUAGES.iter().find(|l| l.code == base)
}

/// 0 is English; otherwise an index into `LANGUAGES`, plus one.
static CURRENT: AtomicUsize = AtomicUsize::new(0);

fn catalog(code: &str) -> Option<&'static [u8]> {
    Some(match code {
        "fr" => include_bytes!("../translations/fr/LC_MESSAGES/rust.po"),
        "es" => include_bytes!("../translations/es/LC_MESSAGES/rust.po"),
        "de" => include_bytes!("../translations/de/LC_MESSAGES/rust.po"),
        "ar" => include_bytes!("../translations/ar/LC_MESSAGES/rust.po"),
        _ => return None,
    })
}

/// `tr`'s own `.po` reader can't read from memory, but its `.mo` one can.
fn translator(po: &[u8]) -> Option<tr::MoTranslator> {
    use rspolib::prelude::*;
    let po = rspolib::pofile(std::str::from_utf8(po).ok()?).ok()?;
    let mo = rspolib::MOFile::from(&po);
    tr::MoTranslator::from_vec_u8(mo.as_bytes().into_owned()).ok()
}

/// `ROMP_LANGUAGE` first, so a language can be tried without changing the system's.
pub fn system() -> Option<String> {
    std::env::var("ROMP_LANGUAGE")
        .ok()
        .or_else(sys_locale::get_locale)
}

#[expect(dead_code, reason = "right-to-left layouts read this")]
pub fn current() -> Option<&'static Language> {
    CURRENT
        .load(Ordering::Relaxed)
        .checked_sub(1)
        .and_then(|i| LANGUAGES.get(i))
}

/// Switches what Slint and `tr!` show. Call it after the first window exists.
pub fn apply(language: Option<&'static Language>) {
    let translator = language.and_then(|l| catalog(l.code)).and_then(translator);
    match translator {
        Some(translator) => tr::set_translator!(translator),
        None => tr::unset_translator!(),
    }
    if let Err(e) = slint::select_bundled_translation(language.map_or("", |l| l.code)) {
        tracing::warn!("choosing the interface language: {e:?}");
    }
    let index = language
        .and_then(|l| LANGUAGES.iter().position(|x| x.code == l.code))
        .map_or(0, |i| i + 1);
    CURRENT.store(index, Ordering::Relaxed);
}

/// Translates a string from a `const` list, which was marked with `gettext_noop!`.
#[expect(dead_code, reason = "lists of names use this once translated")]
pub fn translate(english: &str) -> String {
    tr::internal::with_translator(module_path!(), |t| t.translate(english, None).into_owned())
}

/// Marks a string for translation where `tr!` can't run, such as a `const`; `xtr` looks for it.
#[macro_export]
macro_rules! gettext_noop {
    ($s:literal) => {
        $s
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_locales_map_to_their_base_language() {
        let code = |system: &str| choose(0, Some(system)).map(|l| l.code);
        assert_eq!(code("fr-FR"), Some("fr"));
        assert_eq!(code("fr-CA"), Some("fr"));
        assert_eq!(code("es-419"), Some("es"));
        assert_eq!(code("de_AT.UTF-8"), Some("de"));
        assert_eq!(code("ar_EG"), Some("ar"));
        assert_eq!(code("AR"), Some("ar"));
        assert_eq!(code("en-US"), None);
        assert_eq!(code("zh-Hant"), None);
        assert_eq!(choose(0, None).map(|l| l.code), None);
    }

    #[test]
    fn a_chosen_language_wins_over_the_system() {
        assert_eq!(choose(3, Some("fr-FR")).map(|l| l.code), Some("de"));
        assert_eq!(choose(1, None).map(|l| l.code), Some("fr"));
    }

    #[test]
    fn only_arabic_reads_right_to_left() {
        let rtl: Vec<&str> = LANGUAGES.iter().filter(|l| l.rtl).map(|l| l.code).collect();
        assert_eq!(rtl, ["ar"]);
    }

    const TEST_PO: &str = r#"msgid ""
msgstr ""
"Content-Type: text/plain; charset=UTF-8\n"
"Language: fr\n"
"Plural-Forms: nplurals=2; plural=(n > 1);\n"

msgid "Save"
msgstr "Enregistrer"

msgid "{0} on {1}"
msgstr "{1} : {0}"
"#;

    fn test_translator() -> tr::MoTranslator {
        translator(TEST_PO.as_bytes()).unwrap()
    }

    #[test]
    fn an_untranslated_string_falls_back_to_english() {
        use tr::Translator;
        let t = test_translator();
        assert_eq!(t.translate("Save", None), "Enregistrer");
        assert_eq!(t.translate("Load", None), "Load");
    }

    #[test]
    fn placeholders_survive_translation() {
        use tr::Translator;
        let t = test_translator();
        let format = t.translate("{0} on {1}", None);
        let text = tr::runtime_format!(format, "Tetris", "Game Boy");
        assert_eq!(text, "Game Boy : Tetris");
    }

    #[test]
    fn every_language_ships_both_catalogs() {
        for language in &LANGUAGES {
            assert!(catalog(language.code).is_some(), "{}", language.code);
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("translations")
                .join(language.code)
                .join("LC_MESSAGES");
            assert!(dir.join("romp-app.po").is_file(), "{}", language.code);
            assert!(dir.join("rust.po").is_file(), "{}", language.code);
        }
    }
}
