use std::sync::RwLock;

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

/// The preference for English; 0 follows the system, and from 2 on are `LANGUAGES`.
pub const ENGLISH: u8 = 1;

/// The chosen language, or else the system's if RomP has translations for it; `None` is English.
pub fn choose(
    preference: u8,
    system: Option<&str>,
    translated: impl Fn(&Language) -> bool,
) -> Option<&'static Language> {
    if preference == ENGLISH {
        return None;
    }
    if let Some(chosen) = usize::from(preference).checked_sub(2) {
        return LANGUAGES.get(chosen);
    }
    let base = system?.split(['-', '_', '.']).next()?.to_ascii_lowercase();
    #[cfg(debug_assertions)]
    if base == PSEUDO.code {
        return Some(&PSEUDO);
    }
    LANGUAGES
        .iter()
        .find(|l| l.code == base)
        .filter(|l| translated(l))
}

/// The language to use for a saved preference on this computer.
pub fn pick(preference: u8) -> Option<&'static Language> {
    choose(preference, system().as_deref(), has_translations)
}

/// Whether a language has any translated string yet, so a system set to it isn't shown English
/// laid out for another language.
fn has_translations(language: &Language) -> bool {
    [catalog(language.code), slint_catalog(language.code)]
        .into_iter()
        .flatten()
        .any(translated_any)
}

fn translated_any(po: &[u8]) -> bool {
    std::str::from_utf8(po)
        .ok()
        .and_then(|text| rspolib::pofile(text).ok())
        .is_some_and(|po| !po.translated_entries().is_empty())
}

fn slint_catalog(code: &str) -> Option<&'static [u8]> {
    Some(match code {
        "fr" => include_bytes!("../translations/fr/LC_MESSAGES/romp-app.po"),
        "es" => include_bytes!("../translations/es/LC_MESSAGES/romp-app.po"),
        "de" => include_bytes!("../translations/de/LC_MESSAGES/romp-app.po"),
        "ar" => include_bytes!("../translations/ar/LC_MESSAGES/romp-app.po"),
        _ => return None,
    })
}

/// Every string stretched and accented, from `make pseudo`, to find text that gets cut off.
#[cfg(debug_assertions)]
const PSEUDO: Language = Language {
    code: "xx",
    name: "Pseudo",
    rtl: false,
};

/// `None` is English; `Some` once a language has been applied.
static CURRENT: RwLock<Option<Option<&'static Language>>> = RwLock::new(None);

fn catalog(code: &str) -> Option<&'static [u8]> {
    Some(match code {
        "fr" => include_bytes!("../translations/fr/LC_MESSAGES/rust.po"),
        "es" => include_bytes!("../translations/es/LC_MESSAGES/rust.po"),
        "de" => include_bytes!("../translations/de/LC_MESSAGES/rust.po"),
        "ar" => include_bytes!("../translations/ar/LC_MESSAGES/rust.po"),
        #[cfg(debug_assertions)]
        "xx" => std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/translations/xx/LC_MESSAGES/rust.po"
        ))
        .ok()?
        .leak(),
        _ => return None,
    })
}

/// `tr`'s own `.po` reader can't read from memory, but its `.mo` one can. It also only finds
/// the plural rule when nothing follows the `;` before it, which no translation tool writes.
fn translator(po: &[u8]) -> Option<tr::MoTranslator> {
    use rspolib::prelude::*;
    let mut po = rspolib::pofile(std::str::from_utf8(po).ok()?).ok()?;
    if let Some(rule) = po.metadata.get_mut("Plural-Forms") {
        *rule = rule.split(';').map(str::trim).collect::<Vec<_>>().join(";");
    }
    let mo = rspolib::MOFile::from(&po);
    tr::MoTranslator::from_vec_u8(mo.as_bytes().into_owned()).ok()
}

/// `ROMP_LANGUAGE` first, so a language can be tried without changing the system's.
pub fn system() -> Option<String> {
    std::env::var("ROMP_LANGUAGE")
        .ok()
        .or_else(sys_locale::get_locale)
}

/// Whether the interface reads right to left; debug builds force it with `ROMP_RTL`.
pub fn rtl() -> bool {
    current().is_some_and(|l| l.rtl)
        || cfg!(debug_assertions) && std::env::var_os("ROMP_RTL").is_some()
}

pub fn current() -> Option<&'static Language> {
    CURRENT.read().unwrap().flatten()
}

/// Whether a language has been applied yet, which games started from the library rely on.
pub fn applied() -> bool {
    CURRENT.read().unwrap().is_some()
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
    *CURRENT.write().unwrap() = Some(language);
}

/// Translates a string from a `const` list, which was marked with `gettext_noop!`.
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

    fn all(_: &Language) -> bool {
        true
    }

    /// The placeholders in a string, with `{}` counted by position as `tr!` does.
    fn placeholders(text: &str) -> Vec<String> {
        let mut found = Vec::new();
        let mut position = 0;
        let mut rest = text.replace("{{", "");
        while let Some(start) = rest.find('{') {
            let Some(end) = rest[start..].find('}') else {
                break;
            };
            let name = rest[start + 1..start + end].trim().to_string();
            if name.is_empty() {
                found.push(position.to_string());
                position += 1;
            } else {
                found.push(name);
            }
            rest = rest[start + end + 1..].to_string();
        }
        found.sort();
        found
    }

    /// What's wrong with a translation's placeholders, if anything. A plural form may leave out
    /// the count, as a language's "one" form often does, but must keep everything else.
    fn placeholder_problem(source: &str, translated: &str, plural: bool) -> Option<String> {
        let want = placeholders(source);
        let got = placeholders(translated);
        let ok = if plural {
            got.iter().all(|p| want.contains(p))
                && want.iter().filter(|p| *p != "n").all(|p| got.contains(p))
        } else {
            want == got
        };
        (!ok).then(|| format!("expected {want:?}, found {got:?} in {translated:?}"))
    }

    #[test]
    fn system_locales_map_to_their_base_language() {
        let code = |system: &str| choose(0, Some(system), all).map(|l| l.code);
        assert_eq!(code("fr-FR"), Some("fr"));
        assert_eq!(code("fr-CA"), Some("fr"));
        assert_eq!(code("es-419"), Some("es"));
        assert_eq!(code("de_AT.UTF-8"), Some("de"));
        assert_eq!(code("ar_EG"), Some("ar"));
        assert_eq!(code("AR"), Some("ar"));
        assert_eq!(code("en-US"), None);
        assert_eq!(code("zh-Hant"), None);
        assert_eq!(choose(0, None, all).map(|l| l.code), None);
    }

    #[test]
    fn a_chosen_language_wins_over_the_system() {
        assert_eq!(choose(4, Some("fr-FR"), all).map(|l| l.code), Some("de"));
        assert_eq!(choose(2, None, all).map(|l| l.code), Some("fr"));
    }

    #[test]
    fn english_can_be_chosen_over_a_system_language_romp_has() {
        assert_eq!(choose(ENGLISH, Some("fr-FR"), all).map(|l| l.code), None);
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
            let po = catalog(language.code).expect(language.code);
            assert!(translator(po).is_some(), "{} loads", language.code);
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("translations")
                .join(language.code)
                .join("LC_MESSAGES");
            assert!(dir.join("romp-app.po").is_file(), "{}", language.code);
            assert!(dir.join("rust.po").is_file(), "{}", language.code);
        }
    }

    #[test]
    fn counts_use_the_languages_plural_rule() {
        use tr::Translator;
        let po = format!(
            "{TEST_PO}\nmsgid \"{{n}} game\"\nmsgid_plural \"{{n}} games\"\nmsgstr[0] \"{{n}} jeu\"\nmsgstr[1] \"{{n}} jeux\"\n"
        );
        let t = translator(po.as_bytes()).unwrap();
        assert_eq!(t.ntranslate(1, "{n} game", "{n} games", None), "{n} jeu");
        assert_eq!(t.ntranslate(2, "{n} game", "{n} games", None), "{n} jeux");
        assert_eq!(
            t.ntranslate(0, "{n} game", "{n} games", None),
            "{n} jeu",
            "French counts 0 as one"
        );
    }

    #[test]
    fn the_pseudo_language_is_never_offered() {
        assert!(LANGUAGES.iter().all(|l| l.code != "xx"));
    }

    #[test]
    fn the_pseudo_language_is_only_chosen_by_name_in_debug_builds() {
        assert_eq!(
            choose(0, Some("xx"), all).map(|l| l.code),
            cfg!(debug_assertions).then_some("xx")
        );
    }

    #[test]
    fn arabic_turns_the_layout_around() {
        assert!(choose(5, None, all).is_some_and(|l| l.rtl));
        assert!(!choose(2, None, all).is_some_and(|l| l.rtl));
        assert!(!choose(ENGLISH, Some("ar"), all).is_some_and(|l| l.rtl));
    }

    #[test]
    fn the_system_language_is_only_followed_once_it_has_translations() {
        let none = |_: &Language| false;
        assert_eq!(choose(0, Some("ar_EG"), none).map(|l| l.code), None);
        let only_french = |l: &Language| l.code == "fr";
        assert_eq!(
            choose(0, Some("fr-CA"), only_french).map(|l| l.code),
            Some("fr")
        );
        assert_eq!(choose(0, Some("de-DE"), only_french).map(|l| l.code), None);
        assert_eq!(
            choose(2, None, none).map(|l| l.code),
            Some("fr"),
            "a chosen language still applies"
        );
    }

    #[test]
    fn a_catalog_with_only_its_header_has_no_translations() {
        let header = TEST_PO.split("\n\n").next().unwrap();
        assert!(!translated_any(header.as_bytes()));
        assert!(translated_any(TEST_PO.as_bytes()));
    }

    #[test]
    fn a_plural_missing_some_forms_falls_back_to_english() {
        use tr::Translator;
        let po = format!(
            "{TEST_PO}\nmsgid \"{{n}} day\"\nmsgid_plural \"{{n}} days\"\nmsgstr[0] \"{{n}} jour\"\nmsgstr[1] \"\"\n"
        );
        let t = translator(po.as_bytes()).unwrap();
        assert_eq!(t.ntranslate(5, "{n} day", "{n} days", None), "{n} days");
        assert_eq!(t.ntranslate(1, "{n} day", "{n} days", None), "{n} day");
    }

    #[test]
    fn a_translation_must_keep_its_placeholders() {
        assert_eq!(
            placeholder_problem("Sync failed: {e}", "Échec : {e}", false),
            None
        );
        assert_eq!(placeholder_problem("{0} on {1}", "{1} : {0}", false), None);
        assert_eq!(
            placeholder_problem("Slot {}", "Emplacement {0}", false),
            None
        );
        assert!(placeholder_problem("Sync failed: {e}", "Échec : {erreur}", false).is_some());
        assert!(placeholder_problem("Sync failed: {e}", "Échec", false).is_some());
        assert_eq!(
            placeholder_problem("{n} minutes ago", "دقيقة واحدة", true),
            None
        );
        assert!(placeholder_problem("{n} achievements for {title}", "{n} succès", true).is_some());
    }

    #[test]
    fn shipped_translations_keep_their_placeholders() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("translations");
        for language in &LANGUAGES {
            for file in ["romp-app.po", "rust.po"] {
                let path = root.join(language.code).join("LC_MESSAGES").join(file);
                let po = rspolib::pofile(path.to_str().unwrap()).unwrap();
                for entry in po.translated_entries() {
                    let problem = match &entry.msgid_plural {
                        Some(plural) => entry
                            .msgstr_plural
                            .iter()
                            .find_map(|form| placeholder_problem(plural, form, true)),
                        None => entry
                            .msgstr
                            .as_deref()
                            .and_then(|text| placeholder_problem(&entry.msgid, text, false)),
                    };
                    assert_eq!(problem, None, "{}/{file}: {:?}", language.code, entry.msgid);
                }
            }
        }
    }
}
