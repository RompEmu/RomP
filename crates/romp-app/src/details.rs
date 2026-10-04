use crate::gettext_noop;
use crate::store::GameDetail;
use tr::tr;

const MONTHS: [&str; 12] = [
    gettext_noop!("Jan"),
    gettext_noop!("Feb"),
    gettext_noop!("Mar"),
    gettext_noop!("Apr"),
    gettext_noop!("May"),
    gettext_noop!("Jun"),
    gettext_noop!("Jul"),
    gettext_noop!("Aug"),
    gettext_noop!("Sep"),
    gettext_noop!("Oct"),
    gettext_noop!("Nov"),
    gettext_noop!("Dec"),
];

pub fn human_size(bytes: i64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes.max(0) as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < units.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", units[unit])
    }
}

fn date_parts(ms: i64) -> Option<(i64, u32, u32)> {
    (ms != 0).then(|| crate::sync::civil_from_days(ms.div_euclid(86_400_000)))
}

pub fn release_date(ms: i64) -> Option<String> {
    let (y, m, d) = date_parts(ms)?;
    Some(tr!(
        "{d} {month} {y}",
        d,
        month = crate::i18n::translate(MONTHS[m as usize - 1]),
        y
    ))
}

pub fn players(count: &str) -> Option<String> {
    match count.trim() {
        "" => None,
        "1" => Some(tr!("1 player")),
        n => Some(tr!("{} players", n.replace('-', "–"))),
    }
}

fn release_ms(detail: &GameDetail) -> Option<i64> {
    detail.meta.first_release_date
}

/// The platform categories RomM takes from IGDB.
const PLATFORM_CATEGORIES: [&str; 6] = [
    gettext_noop!("Console"),
    gettext_noop!("Portable Console"),
    gettext_noop!("Arcade"),
    gettext_noop!("Computer"),
    gettext_noop!("Operating System"),
    gettext_noop!("Platform"),
];

/// A platform's category as shown, translated when it's one RomM is known to send.
pub fn category(name: &str) -> String {
    if PLATFORM_CATEGORIES.contains(&name) {
        crate::i18n::translate(name)
    } else {
        name.to_string()
    }
}

const DVD_CASE: f32 = 0.71;

const BOX_SHAPES: [(&[&str], f32); 12] = [
    (&["gb", "gbc", "gba", "dc"], 1.0),
    (&["snes"], 1.38),
    (&["n64"], 1.37),
    (&["psx"], 1.16),
    (&["nds", "3ds"], 1.11),
    (&["virtualboy"], 1.12),
    (&["sfam"], 0.55),
    (&["psp"], 0.58),
    (&["saturn"], 0.65),
    (&["wonderswan", "wonderswan-color"], 0.69),
    (&["nes", "famicom", "gamegear", "atari2600", "arcade"], 0.73),
    (&["lynx", "dos"], 0.8),
];

pub fn box_aspect(platform_slug: &str) -> f32 {
    BOX_SHAPES
        .iter()
        .find(|(slugs, _)| slugs.contains(&platform_slug))
        .map_or(DVD_CASE, |(_, aspect)| *aspect)
}

pub const CARD_WIDTH: f32 = 172.0;
pub const SHELF_HEIGHT: f32 = 230.0;

pub fn box_height(aspect: f32) -> f32 {
    (CARD_WIDTH.min(SHELF_HEIGHT * aspect) / aspect).round()
}

pub fn shelf_height(aspects: impl IntoIterator<Item = f32>) -> f32 {
    aspects
        .into_iter()
        .map(box_height)
        .reduce(f32::max)
        .unwrap_or(SHELF_HEIGHT)
}

pub fn subtitle(detail: &GameDetail) -> String {
    let year = release_ms(detail)
        .and_then(date_parts)
        .map(|(y, _, _)| y.to_string());
    let players = detail.meta.player_count.as_deref().and_then(players);
    std::iter::once(detail.platform.clone())
        .chain(year)
        .chain(players)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

fn list(one: &'static str, many: &'static str, items: &[String]) -> Option<(&'static str, String)> {
    match items.len() {
        0 => None,
        1 => Some((one, items[0].clone())),
        _ => Some((many, items.join(", "))),
    }
}

pub fn facts(detail: &GameDetail) -> Vec<(&'static str, String)> {
    let meta = &detail.meta;
    let companies = if meta.developers.is_empty() && meta.publishers.is_empty() {
        list(
            gettext_noop!("Company"),
            gettext_noop!("Companies"),
            &meta.companies,
        )
    } else {
        None
    };
    [
        release_ms(detail)
            .and_then(release_date)
            .map(|d| (gettext_noop!("Released"), d)),
        list(
            gettext_noop!("Genre"),
            gettext_noop!("Genres"),
            &meta.genres,
        ),
        list(
            gettext_noop!("Developer"),
            gettext_noop!("Developers"),
            &meta.developers,
        ),
        list(
            gettext_noop!("Publisher"),
            gettext_noop!("Publishers"),
            &meta.publishers,
        ),
        companies,
        list(
            gettext_noop!("Franchise"),
            gettext_noop!("Franchises"),
            &meta.franchises,
        ),
        meta.average_rating
            .filter(|r| *r > 0.0)
            .map(|r| (gettext_noop!("Rating"), format!("{} / 100", r.round()))),
        (detail.size_bytes > 0).then(|| (gettext_noop!("Size"), human_size(detail.size_bytes))),
    ]
    .into_iter()
    .flatten()
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::romm::types::RomMetadata;

    #[test]
    fn a_row_is_as_tall_as_its_tallest_box() {
        assert_eq!(box_height(1.0), 172.0);
        assert_eq!(box_height(1.38), 125.0);
        assert_eq!(box_height(0.55), 230.0);
        assert_eq!(shelf_height([1.0, 1.38]), 172.0);
        assert_eq!(shelf_height([1.0, 0.71]), 230.0);
        assert_eq!(shelf_height([]), 230.0);
    }

    #[test]
    fn boxes_take_the_shape_of_each_consoles_packaging() {
        for slug in ["gb", "gbc", "gba", "dc"] {
            assert_eq!(box_aspect(slug), 1.0, "{slug}");
        }
        assert_eq!(box_aspect("snes"), 1.38);
        assert_eq!(box_aspect("n64"), 1.37);
        assert_eq!(box_aspect("nds"), 1.11);
        assert_eq!(box_aspect("sfam"), 0.55);
        assert_eq!(box_aspect("psp"), 0.58);
        assert_eq!(box_aspect("nes"), 0.73);
        assert_eq!(box_aspect("ps2"), 0.71);
        assert_eq!(box_aspect("unknown-console"), 0.71);
    }

    fn detail(meta: RomMetadata) -> GameDetail {
        GameDetail {
            id: 1,
            title: "Chrono Trigger".into(),
            platform_id: 1,
            platform_slug: "snes".into(),
            platform: "Super Nintendo".into(),
            platform_category: Some("Console".into()),
            summary: None,
            size_bytes: 4_194_304,
            cover_small: None,
            cover_large: None,
            local_path: None,
            meta,
            screenshots: Vec::new(),
        }
    }

    #[test]
    fn release_dates_are_utc_days_and_zero_is_unset() {
        assert_eq!(
            release_date(795_052_800_000).as_deref(),
            Some("13 Mar 1995")
        );
        assert_eq!(
            release_date(795_139_199_999).as_deref(),
            Some("13 Mar 1995")
        );
        assert_eq!(release_date(0), None);
    }

    #[test]
    fn player_counts_read_naturally() {
        assert_eq!(players("1").as_deref(), Some("1 player"));
        assert_eq!(players(" 1-4 ").as_deref(), Some("1–4 players"));
        assert_eq!(players("2").as_deref(), Some("2 players"));
        assert_eq!(players(""), None);
    }

    #[test]
    fn subtitle_joins_platform_year_and_players() {
        let d = detail(RomMetadata {
            first_release_date: Some(795_052_800_000),
            player_count: Some("1".into()),
            ..RomMetadata::default()
        });
        assert_eq!(subtitle(&d), "Super Nintendo · 1995 · 1 player");
        assert_eq!(subtitle(&detail(RomMetadata::default())), "Super Nintendo");
    }

    #[test]
    fn facts_skip_empty_values_and_prefer_developers_over_companies() {
        let d = detail(RomMetadata {
            genres: vec!["Role-playing (RPG)".into(), "Adventure".into()],
            developers: vec!["Square".into()],
            companies: vec!["Square".into(), "Nintendo".into()],
            average_rating: Some(92.4),
            first_release_date: Some(795_052_800_000),
            ..RomMetadata::default()
        });
        assert_eq!(
            facts(&d),
            [
                ("Released", "13 Mar 1995".to_string()),
                ("Genres", "Role-playing (RPG), Adventure".to_string()),
                ("Developer", "Square".to_string()),
                ("Rating", "92 / 100".to_string()),
                ("Size", "4.2 MB".to_string()),
            ]
        );
        let d = detail(RomMetadata {
            companies: vec!["Sega".into()],
            publishers: vec!["Sega".into(), "Tec Toy".into()],
            franchises: vec!["Sonic".into()],
            ..RomMetadata::default()
        });
        assert_eq!(
            facts(&d),
            [
                ("Publishers", "Sega, Tec Toy".to_string()),
                ("Franchise", "Sonic".to_string()),
                ("Size", "4.2 MB".to_string()),
            ]
        );
        let d = detail(RomMetadata {
            companies: vec!["Sega".into()],
            ..RomMetadata::default()
        });
        assert_eq!(facts(&d)[0], ("Company", "Sega".to_string()));
    }

    #[test]
    fn sizes_are_human_readable() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(46_857), "46.9 KB");
        assert_eq!(human_size(627_135_698), "627.1 MB");
        assert_eq!(human_size(4_700_000_000), "4.7 GB");
    }
}
