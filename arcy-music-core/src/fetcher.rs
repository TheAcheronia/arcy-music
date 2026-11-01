use crate::Track;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum FetcherError {
    #[error("Network error: {0}")]
    NetworkError(String),
    #[error("Parse error: {0}")]
    ParseError(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Other error: {0}")]
    Other(String),
}

#[allow(async_fn_in_trait)]
pub trait MusicFetcher: Send + Sync {
    /// Name of the fetcher (e.g., "youtube", "spotify")
    fn name(&self) -> &'static str;

    /// Search for tracks based on a query string
    async fn search(&self, query: &str) -> Result<Vec<Track>, FetcherError>;

    /// Fetch a playlist or album by its unique URL or ID
    async fn fetch_playlist(&self, url_or_id: &str) -> Result<Vec<Track>, FetcherError>;
    
    /// Extract the direct streaming audio URL for a given track (if applicable)
    async fn get_stream_url(&self, track: &Track) -> Result<String, FetcherError>;
}
