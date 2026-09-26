use crate::store::GameDetail;

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
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
    Some(format!("{d} {} {y}", MONTHS[m as usize - 1]))
}

pub fn players(count: &str) -> Option<String> {
    match count.trim() {
        "" => None,
        "1" => Some("1 player".into()),
        n => Some(format!("{} players", n.replace('-', "–"))),
    }
}

fn release_ms(detail: &GameDetail) -> Option<i64> {
    detail.meta.first_release_date
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
        list("Company", "Companies", &meta.companies)
    } else {
        None
    };
    [
        release_ms(detail)
            .and_then(release_date)
            .map(|d| ("Released", d)),
        list("Genre", "Genres", &meta.genres),
        list("Developer", "Developers", &meta.developers),
        list("Publisher", "Publishers", &meta.publishers),
        companies,
        list("Franchise", "Franchises", &meta.franchises),
        meta.average_rating
            .filter(|r| *r > 0.0)
            .map(|r| ("Rating", format!("{} / 100", r.round()))),
        (detail.size_bytes > 0).then(|| ("Size", human_size(detail.size_bytes))),
    ]
    .into_iter()
    .flatten()
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::romm::types::RomMetadata;

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
