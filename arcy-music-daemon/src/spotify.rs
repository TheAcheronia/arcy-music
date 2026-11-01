use arcy_music_core::{Track, TrackSource};
use reqwest::header::{HeaderMap, HeaderValue, COOKIE};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct AccessTokenResponse {
    #[serde(rename = "accessToken")]
    access_token: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SpotifySyncRequest {
    pub sp_dc: Option<String>,
    pub access_token: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SpotifySyncResponse {
    pub success: bool,
    pub synced_playlists: usize,
    pub synced_tracks: usize,
    pub message: String,
}

pub async fn get_bearer_token(sp_dc: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let sp_dc = sp_dc.trim().trim_start_matches("sp_dc=").trim_matches('"');
    let mut headers = HeaderMap::new();
    let cookie_val = format!("sp_dc={}", sp_dc);
    headers.insert(COOKIE, HeaderValue::from_str(&cookie_val)?);
    headers.insert(
        "User-Agent",
        HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/119.0.0.0 Safari/537.36"),
    );

    let client = reqwest::Client::builder().default_headers(headers).build()?;

    // First try the web player token endpoint
    let url = "https://open.spotify.com/get_access_token?reason=transport&productType=web_player";
    if let Ok(res) = client.get(url).send().await {
        if res.status().is_success() {
            if let Ok(json) = res.json::<AccessTokenResponse>().await {
                if !json.access_token.is_empty() {
                    return Ok(json.access_token);
                }
            }
        }
    }

    // Fallback: fetch homepage and extract token from HTML
    let res = client.get("https://open.spotify.com/").send().await?;
    let html = res.text().await?;

    if html.contains("authfailed") || html.contains("sp_dc=;") {
        return Err("Invalid sp_dc cookie or expired Spotify session".into());
    }

    let token_key = "\"accessToken\":\"";
    if let Some(idx) = html.find(token_key) {
        let start = idx + token_key.len();
        if let Some(end_idx) = html[start..].find('"') {
            let token = &html[start..start + end_idx];
            return Ok(token.to_string());
        }
    }

    Err("Could not extract Spotify access token from sp_dc cookie".into())
}

pub async fn fetch_liked_songs(token: &str) -> Result<Vec<Track>, Box<dyn std::error::Error + Send + Sync>> {
    let mut headers = HeaderMap::new();
    let auth_val = format!("Bearer {}", token);
    headers.insert("Authorization", HeaderValue::from_str(&auth_val)?);
    let client = reqwest::Client::builder().default_headers(headers).build()?;

    let res = client.get("https://api.spotify.com/v1/me/tracks?limit=50").send().await?;
    let json: serde_json::Value = res.json().await?;

    let mut tracks = Vec::new();
    if let Some(items) = json.get("items").and_then(|i| i.as_array()) {
        for item in items {
            if let Some(track) = item.get("track") {
                if let Some(t) = parse_spotify_track(track) {
                    tracks.push(t);
                }
            }
        }
    }

    Ok(tracks)
}

pub async fn fetch_user_playlists(token: &str) -> Result<Vec<(String, String, Option<String>)>, Box<dyn std::error::Error + Send + Sync>> {
    let mut headers = HeaderMap::new();
    let auth_val = format!("Bearer {}", token);
    headers.insert("Authorization", HeaderValue::from_str(&auth_val)?);
    let client = reqwest::Client::builder().default_headers(headers).build()?;

    let res = client.get("https://api.spotify.com/v1/me/playlists?limit=50").send().await?;
    let json: serde_json::Value = res.json().await?;

    let mut playlists = Vec::new();
    if let Some(items) = json.get("items").and_then(|i| i.as_array()) {
        for item in items {
            let name = item.get("name").and_then(|n| n.as_str()).unwrap_or("Unknown Playlist").to_string();
            let id = item.get("id").and_then(|n| n.as_str()).unwrap_or("").to_string();
            if id.is_empty() {
                continue;
            }

            let mut cover_url = None;
            if let Some(images) = item.get("images").and_then(|i| i.as_array()) {
                if let Some(first_img) = images.first() {
                    if let Some(url) = first_img.get("url").and_then(|u| u.as_str()) {
                        cover_url = Some(url.to_string());
                    }
                }
            }

            playlists.push((id, name, cover_url));
        }
    }

    Ok(playlists)
}

pub async fn fetch_playlist_tracks(token: &str, playlist_id: &str) -> Result<Vec<Track>, Box<dyn std::error::Error + Send + Sync>> {
    let mut headers = HeaderMap::new();
    let auth_val = format!("Bearer {}", token);
    headers.insert("Authorization", HeaderValue::from_str(&auth_val)?);
    let client = reqwest::Client::builder().default_headers(headers).build()?;

    let res = client.get(format!("https://api.spotify.com/v1/playlists/{}/tracks?limit=50", playlist_id)).send().await?;
    let json: serde_json::Value = res.json().await?;

    let mut tracks = Vec::new();
    if let Some(items) = json.get("items").and_then(|i| i.as_array()) {
        for item in items {
            if let Some(track) = item.get("track") {
                if let Some(t) = parse_spotify_track(track) {
                    tracks.push(t);
                }
            }
        }
    }

    Ok(tracks)
}

pub async fn search_spotify(token: &str, query: &str) -> Result<Vec<Track>, Box<dyn std::error::Error + Send + Sync>> {
    let mut headers = HeaderMap::new();
    let auth_val = format!("Bearer {}", token);
    headers.insert("Authorization", HeaderValue::from_str(&auth_val)?);
    let client = reqwest::Client::builder().default_headers(headers).build()?;

    let encoded_query = query.replace(' ', "%20").replace('&', "%26").replace('?', "%3F");
    let url = format!("https://api.spotify.com/v1/search?q={}&type=track&limit=20", encoded_query);
    let res = client.get(&url).send().await?;
    let json: serde_json::Value = res.json().await?;

    let mut tracks = Vec::new();
    if let Some(items) = json.get("tracks").and_then(|t| t.get("items")).and_then(|i| i.as_array()) {
        for track in items {
            if let Some(t) = parse_spotify_track(track) {
                tracks.push(t);
            }
        }
    }

    Ok(tracks)
}

fn parse_spotify_track(track: &serde_json::Value) -> Option<Track> {
    let title = track.get("name").and_then(|n| n.as_str())?.to_string();
    let artist = track.get("artists")
        .and_then(|a| a.as_array())
        .and_then(|a| a.first())
        .and_then(|f| f.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("Unknown Artist")
        .to_string();
    let album = track.get("album")
        .and_then(|a| a.get("name"))
        .and_then(|n| n.as_str())
        .map(|s| s.to_string());
    let id = track.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
    if id.is_empty() {
        return None;
    }
    let duration_ms = track.get("duration_ms").and_then(|d| d.as_f64()).unwrap_or(0.0);

    let mut thumbnail_url = None;
    if let Some(images) = track.get("album").and_then(|a| a.get("images")).and_then(|i| i.as_array()) {
        if let Some(first_img) = images.first() {
            if let Some(url) = first_img.get("url").and_then(|u| u.as_str()) {
                thumbnail_url = Some(url.to_string());
            }
        }
    }

    Some(Track {
        id: format!("sp_{}", id),
        title,
        artist: Some(artist),
        album,
        duration_secs: Some((duration_ms / 1000.0) as u64),
        source: TrackSource::Spotify { track_id: id },
        thumbnail_url,
    })
}
