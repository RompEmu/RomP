use std::path::Path;

pub fn stem(fs_name: &str) -> &str {
    Path::new(fs_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(fs_name)
}

/// The tags in a No-Intro or Redump style name, like "USA, Disc 1", or the name itself.
pub fn label(fs_name: &str) -> String {
    let stem = stem(fs_name);
    let mut tags = Vec::new();
    let mut rest = stem;
    while let Some(open) = rest.find(['(', '[']) {
        let close = if rest[open..].starts_with('(') {
            ')'
        } else {
            ']'
        };
        let Some(len) = rest[open + 1..].find(close) else {
            break;
        };
        tags.push(rest[open + 1..open + 1 + len].trim());
        rest = &rest[open + 1 + len + 1..];
    }
    if tags.is_empty() {
        stem.to_string()
    } else {
        tags.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_come_from_the_file_names_tags() {
        assert_eq!(label("Final Fantasy VII (USA) (Disc 1).chd"), "USA, Disc 1");
        assert_eq!(label("Tetris (World) (Rev 1) [!].gb"), "World, Rev 1, !");
        assert_eq!(label("Tetris.gb"), "Tetris");
    }
}
