/// `fs_name` without its extension. Multi-file games are folders with none, so a dot
/// followed by anything but a short alphanumeric suffix belongs to the name.
pub fn stem(fs_name: &str) -> &str {
    match fs_name.rsplit_once('.') {
        Some((stem, ext))
            if !stem.is_empty()
                && (1..=5).contains(&ext.len())
                && ext.bytes().all(|b| b.is_ascii_alphanumeric()) =>
        {
            stem
        }
        _ => fs_name,
    }
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
        let tag = rest[open + 1..open + 1 + len].trim();
        if !tag.is_empty() {
            tags.push(tag);
        }
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

    #[test]
    fn folder_names_keep_their_dots() {
        assert_eq!(label("Dr. Mario (USA)"), "USA");
        assert_eq!(label("Dr. Mario"), "Dr. Mario");
        assert_eq!(label("FF VII (USA) (v1.1)"), "USA, v1.1");
    }

    #[test]
    fn empty_tags_are_skipped() {
        assert_eq!(label("Foo () (USA).zip"), "USA");
    }
}
