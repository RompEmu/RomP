/// The changelog this build was made with, so the notes match the version and show offline.
pub const CHANGELOG: &str = include_str!("../../../CHANGELOG.md");
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub title: String,
    pub items: Vec<String>,
}

/// One version's sections of the changelog, such as Added and Fixed, as plain text.
pub fn notes(changelog: &str, version: &str) -> Vec<Section> {
    let heading = format!("## [{version}]");
    let mut lines = changelog
        .lines()
        .skip_while(|line| !line.starts_with(&heading))
        .skip(1);
    let mut sections: Vec<Section> = Vec::new();
    for line in lines.by_ref() {
        if line.starts_with("## ") || line.starts_with('[') {
            break;
        }
        if let Some(title) = line.strip_prefix("### ") {
            sections.push(Section {
                title: title.trim().to_string(),
                items: Vec::new(),
            });
        } else if let Some(item) = line.strip_prefix("- ") {
            if let Some(section) = sections.last_mut() {
                section.items.push(item.trim().to_string());
            }
        } else if line.starts_with(' ') && !line.trim().is_empty() {
            if let Some(last) = sections.last_mut().and_then(|s| s.items.last_mut()) {
                last.push(' ');
                last.push_str(line.trim());
            }
        }
    }
    for section in &mut sections {
        for item in &mut section.items {
            *item = plain(item);
        }
    }
    sections.retain(|s| !s.items.is_empty());
    sections
}

/// Drops the changelog's Markdown: bold markers, and links reduced to their text.
fn plain(text: &str) -> String {
    let text = text.replace("**", "");
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find("](").map(|i| open + i) else {
            break;
        };
        let Some(end) = rest[close..].find(')').map(|i| close + i) else {
            break;
        };
        out.push_str(&rest[..open]);
        out.push_str(&rest[open + 1..close]);
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Whether to show the notes by themselves: the first time this version runs, unless turned off.
pub fn due(seen: Option<&str>, current: &str, enabled: bool) -> bool {
    enabled && seen != Some(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "# Changelog

Intro.

## [Unreleased]

### Added

- Not out yet.

## [0.20.0] - 2026-10-31

### Added

- Change discs from the game menu.
- Choose under **Settings → General → Appearance**, or see [the docs](https://example.com) for
  more.

### Fixed

- A held mouse button no longer stays held.

## [0.19.0] - 2026-10-24

### Added

- Older news.

[Unreleased]: https://github.com/RompEmu/RomP/compare/v0.20.0...HEAD
";

    #[test]
    fn a_versions_notes_are_its_own_sections_and_items() {
        let notes = notes(LOG, "0.20.0");
        let titles: Vec<&str> = notes.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(titles, ["Added", "Fixed"]);
        assert_eq!(
            notes[0].items,
            [
                "Change discs from the game menu.",
                "Choose under Settings → General → Appearance, or see the docs for more."
            ]
        );
        assert_eq!(
            notes[1].items,
            ["A held mouse button no longer stays held."]
        );
    }

    #[test]
    fn a_version_missing_from_the_changelog_has_no_notes() {
        assert!(notes(LOG, "0.21.0").is_empty());
        assert!(notes(LOG, "0.2").is_empty());
    }

    #[test]
    fn the_window_is_due_once_per_new_version_unless_turned_off() {
        assert!(due(None, "0.20.0", true), "a first install shows it too");
        assert!(due(Some("0.19.0"), "0.20.0", true));
        assert!(!due(Some("0.20.0"), "0.20.0", true));
        assert!(!due(Some("0.19.0"), "0.20.0", false));
    }

    #[test]
    fn this_versions_notes_are_in_the_changelog_it_was_built_with() {
        assert!(!notes(CHANGELOG, VERSION).is_empty());
    }
}
