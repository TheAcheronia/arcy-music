use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LyricLine {
    pub time_secs: f32,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TrackLyrics {
    pub track_id: String,
    pub plain_lyrics: Option<String>,
    pub synced_lyrics: Vec<LyricLine>,
}

#[derive(Deserialize)]
struct LrclibItem {
    #[serde(rename = "plainLyrics")]
    plain_lyrics: Option<String>,
    #[serde(rename = "syncedLyrics")]
    synced_lyrics: Option<String>,
}

pub fn parse_lrc(lrc_text: &str) -> Vec<LyricLine> {
    let mut lines = Vec::new();
    for line in lrc_text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if let Some(close_bracket) = trimmed.find(']') {
                let time_str = &trimmed[1..close_bracket];
                let text = trimmed[close_bracket + 1..].trim().to_string();
                
                let parts: Vec<&str> = time_str.split(':').collect();
                if parts.len() == 2 {
                    if let (Ok(mins), Ok(secs)) = (parts[0].parse::<f32>(), parts[1].parse::<f32>()) {
                        let total_secs = mins * 60.0 + secs;
                        if !text.is_empty() {
                            lines.push(LyricLine {
                                time_secs: total_secs,
                                text,
                            });
                        }
                    }
                }
            }
        }
    }
    lines.sort_by(|a, b| a.time_secs.partial_cmp(&b.time_secs).unwrap());
    lines
}

pub async fn fetch_lyrics(title: &str, artist: &str) -> Option<TrackLyrics> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .ok()?;

    // 1. Direct match query
    let mut url = reqwest::Url::parse("https://lrclib.net/api/get").ok()?;
    url.query_pairs_mut()
        .append_pair("track_name", title)
        .append_pair("artist_name", artist);

    let res = client.get(url).send().await.ok()?;

    if res.status().is_success() {
        if let Ok(item) = res.json::<LrclibItem>().await {
            let synced = item.synced_lyrics.as_deref().map(parse_lrc).unwrap_or_default();
            return Some(TrackLyrics {
                track_id: String::new(),
                plain_lyrics: item.plain_lyrics,
                synced_lyrics: synced,
            });
        }
    }

    // 2. Fuzzy search fallback
    let search_query = format!("{} {}", artist, title);
    let mut search_url = reqwest::Url::parse("https://lrclib.net/api/search").ok()?;
    search_url.query_pairs_mut().append_pair("q", &search_query);

    let res = client.get(search_url).send().await.ok()?;

    if res.status().is_success() {
        if let Ok(items) = res.json::<Vec<LrclibItem>>().await {
            if let Some(first) = items.into_iter().next() {
                let synced = first.synced_lyrics.as_deref().map(parse_lrc).unwrap_or_default();
                return Some(TrackLyrics {
                    track_id: String::new(),
                    plain_lyrics: first.plain_lyrics,
                    synced_lyrics: synced,
                });
            }
        }
    }

    None
}
