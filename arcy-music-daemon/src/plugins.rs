use arcy_music_core::{Track, TrackSource, fetcher::{MusicFetcher, FetcherError}};
use tokio::process::Command;

#[allow(dead_code)]
pub struct SpotifyFetcher {
    client: reqwest::Client,
    // Note: To truly fetch liked songs we need an OAuth token.
    // For spotube-like behavior without login, we can use the Spotify Web API with an anonymous token
    // or parse public playlists.
}

#[allow(dead_code)]
impl SpotifyFetcher {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

impl MusicFetcher for SpotifyFetcher {
    fn name(&self) -> &'static str {
        "spotify"
    }

    async fn search(&self, _query: &str) -> Result<Vec<Track>, FetcherError> {
        // Implement Spotify Web API search
        Ok(vec![])
    }

    async fn fetch_playlist(&self, _url_or_id: &str) -> Result<Vec<Track>, FetcherError> {
        // Implement Spotify Web API playlist fetch
        Ok(vec![])
    }

    async fn get_stream_url(&self, _track: &Track) -> Result<String, FetcherError> {
        Err(FetcherError::Other("Spotify does not provide direct audio URLs".to_string()))
    }
}

pub struct YouTubeFetcher {
    client: reqwest::Client,
}

impl YouTubeFetcher {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap_or_default(),
        }
    }
}

impl MusicFetcher for YouTubeFetcher {
    fn name(&self) -> &'static str {
        "youtube"
    }

    async fn search(&self, query: &str) -> Result<Vec<Track>, FetcherError> {
        // use yt-dlp to search: yt-dlp "ytsearch5:query" --dump-json --flat-playlist --no-playlist
        let output = Command::new("yt-dlp")
            .arg(format!("ytsearch5:{}", query))
            .arg("--dump-json")
            .arg("--flat-playlist")
            .arg("--no-playlist")
            .arg("--no-warnings")
            .output()
            .await
            .map_err(|e| FetcherError::Other(e.to_string()))?;

        if !output.status.success() {
            return Err(FetcherError::Other("yt-dlp search failed".into()));
        }

        let thumbs_dir = dirs::data_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
            .join("arcy-music/thumbnails");
        let _ = tokio::fs::create_dir_all(&thumbs_dir).await;

        let mut tracks = Vec::new();
        let output_str = String::from_utf8_lossy(&output.stdout);
        
        for line in output_str.lines() {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
                if let (Some(id), Some(title)) = (json["id"].as_str(), json["title"].as_str()) {
                    let artist = json["uploader"].as_str().map(String::from);
                    let duration = json["duration"].as_u64()
                        .or_else(|| json["duration"].as_f64().map(|f| f as u64));
                    let raw_thumb = json["thumbnail"].as_str().map(String::from);
                    let id_str = id.to_string();

                    let thumb_file = thumbs_dir.join(format!("yt_{}.jpg", id_str));
                    let thumb_url = if thumb_file.exists() {
                        Some(thumb_file.to_string_lossy().to_string())
                    } else {
                        let candidate_url = raw_thumb.unwrap_or_else(|| format!("https://i.ytimg.com/vi/{}/hqdefault.jpg", id_str));
                        let client = self.client.clone();
                        let thumb_file_clone = thumb_file.clone();
                        let cand_clone = candidate_url.clone();
                        tokio::spawn(async move {
                            if let Ok(res) = client.get(&cand_clone).send().await {
                                if res.status().is_success() {
                                    if let Ok(bytes) = res.bytes().await {
                                        if !bytes.is_empty() {
                                            if let Ok(dyn_img) = image::load_from_memory(&bytes) {
                                                let _ = dyn_img.to_rgb8().save_with_format(&thumb_file_clone, image::ImageFormat::Jpeg);
                                            } else {
                                                let _ = tokio::fs::write(&thumb_file_clone, bytes).await;
                                            }
                                        }
                                    }
                                }
                            }
                        });
                        Some(candidate_url)
                    };

                    tracks.push(Track {
                        id: format!("yt:{}", id_str),
                        title: title.to_string(),
                        artist,
                        album: None,
                        duration_secs: duration,
                        source: TrackSource::YouTube { video_id: id_str },
                        thumbnail_url: thumb_url,
                    });
                }
            }
        }

        Ok(tracks)
    }

    async fn fetch_playlist(&self, _url_or_id: &str) -> Result<Vec<Track>, FetcherError> {
        // yt-dlp --flat-playlist --dump-json URL
        Ok(vec![])
    }

    async fn get_stream_url(&self, track: &Track) -> Result<String, FetcherError> {
        if let TrackSource::YouTube { video_id } = &track.source {
            if !arcy_music_core::is_valid_youtube_id(video_id) {
                return Err(FetcherError::Other("Invalid YouTube video ID".into()));
            }
            let output = Command::new("yt-dlp")
                .args([
                    "-g",
                    "-f", "251/140/bestaudio",
                    "--no-playlist",
                    "--no-warnings",
                    "--extractor-args", "youtube:player_client=visionos",
                    "--",
                    &format!("https://www.youtube.com/watch?v={}", video_id),
                ])
                .output()
                .await
                .map_err(|e| FetcherError::Other(e.to_string()))?;

            if !output.status.success() {
                return Err(FetcherError::Other("yt-dlp extract failed".into()));
            }

            let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
            Ok(url)
        } else {
            Err(FetcherError::Other("Not a youtube track".into()))
        }
    }
}
