//! HTTPS storage backend for turbolite.
//!
//! Reads page groups and manifests from a plain HTTPS endpoint using HTTP Range
//! requests. This is a **read-only** backend: `put`, `delete`, and CAS
//! operations all return errors. Use it to query a turbolite database that is
//! published as static files on any HTTPS server (S3 static website, CDN,
//! GitHub Releases, etc.).
//!
//! # Layout
//!
//! The backend expects the server to serve turbolite objects at:
//!
//! ```text
//! {base_url}/{key}
//! ```
//!
//! For example, if `base_url` is `https://cdn.example.com/mydb`, the manifest
//! lives at `https://cdn.example.com/mydb/manifest.msgpack` and a page group at
//! `https://cdn.example.com/mydb/p/d/0_v1`.
//!
//! # Range requests
//!
//! Turbolite's sub-chunk prefetch path calls `range_get` rather than fetching
//! full page groups. This backend translates every `range_get(key, start, len)`
//! into a `Range: bytes=start-(start+len-1)` request so only the needed bytes
//! travel over the network. Servers must support `Accept-Ranges: bytes` for
//! this to work; if the server returns a full 200 instead of a 206, the
//! response is sliced locally (at the cost of fetching extra bytes).
//!
//! # Authentication
//!
//! An optional ****** can be supplied. Set it with
//! [`HttpsStorageBuilder::bearer_token`].

use std::time::Duration;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use hadb_storage::{CasResult, StorageBackend};

/// How many times to retry a failed request before giving up.
const MAX_RETRIES: u32 = 3;
/// Initial retry back-off (doubles on each attempt).
const RETRY_BASE_MS: u64 = 100;

/// Read-only HTTPS storage backend.
///
/// Implements [`StorageBackend`] by translating `get` / `range_get` /
/// `exists` calls into HTTPS requests. Write operations (`put`, `delete`,
/// `put_if_absent`, `put_if_match`) always return an error — this backend is
/// intentionally read-only.
///
/// Create via [`HttpsStorage::new`] or the builder returned by
/// [`HttpsStorage::builder`].
pub struct HttpsStorage {
    /// HTTP client (keep-alive connection pool, TLS session cache).
    client: reqwest::Client,
    /// Base URL, no trailing slash.
    base_url: String,
    /// Optional ****** for authenticated endpoints.
    bearer_token: Option<String>,
}

impl HttpsStorage {
    /// Create a new `HttpsStorage` pointing at `base_url`.
    ///
    /// `base_url` should NOT have a trailing slash, e.g.
    /// `https://cdn.example.com/mydb`.
    ///
    /// Returns an error if the reqwest client cannot be built (e.g. TLS
    /// initialisation failure).
    pub fn new(base_url: impl Into<String>) -> Result<Self> {
        Self::builder(base_url).build()
    }

    /// Return a builder for more control over client options.
    pub fn builder(base_url: impl Into<String>) -> HttpsStorageBuilder {
        HttpsStorageBuilder::new(base_url)
    }

    /// Construct the URL for a backend key.
    fn url(&self, key: &str) -> String {
        format!("{}/{}", self.base_url, key)
    }

    /// Add common request headers (auth, …) to a request builder.
    fn auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(token) = &self.bearer_token {
            req.bearer_auth(token)
        } else {
            req
        }
    }

    /// GET a full object, retrying on transient errors.
    ///
    /// Returns `Ok(None)` on 404, `Ok(Some(bytes))` on 200/206, and
    /// `Err` on persistent failure.
    async fn get_with_retry(&self, url: &str) -> Result<Option<Vec<u8>>> {
        let mut attempt = 0u32;
        loop {
            let req = self.auth(self.client.get(url));
            match req.send().await {
                Ok(resp) => match resp.status().as_u16() {
                    200 | 206 => {
                        let bytes = resp.bytes().await?.to_vec();
                        return Ok(Some(bytes));
                    }
                    404 => return Ok(None),
                    status => {
                        attempt += 1;
                        if attempt >= MAX_RETRIES {
                            return Err(anyhow!(
                                "HTTPS GET {} returned HTTP {} after {} attempts",
                                url,
                                status,
                                attempt
                            ));
                        }
                        let delay = RETRY_BASE_MS * (1 << attempt);
                        tokio::time::sleep(Duration::from_millis(delay)).await;
                    }
                },
                Err(e) if is_transient(&e) => {
                    attempt += 1;
                    if attempt >= MAX_RETRIES {
                        return Err(anyhow!(
                            "HTTPS GET {} failed after {} attempts: {}",
                            url,
                            attempt,
                            e
                        ));
                    }
                    let delay = RETRY_BASE_MS * (1 << attempt);
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                }
                Err(e) => return Err(anyhow!("HTTPS GET {}: {}", url, e)),
            }
        }
    }

    /// Range GET, retrying on transient errors.
    ///
    /// Sends `Range: bytes=start-(start+len-1)`. If the server honours the
    /// range and returns 206, we use the body directly. If the server ignores
    /// it and returns 200, we slice the body to the requested window. Returns
    /// `Ok(None)` on 404 and `Err` on persistent failure.
    async fn range_get_with_retry(
        &self,
        url: &str,
        start: u64,
        len: u32,
    ) -> Result<Option<Vec<u8>>> {
        let range_header = format!("bytes={}-{}", start, start + len as u64 - 1);
        let mut attempt = 0u32;
        loop {
            let req = self
                .auth(self.client.get(url))
                .header(reqwest::header::RANGE, &range_header);
            match req.send().await {
                Ok(resp) => match resp.status().as_u16() {
                    206 => {
                        let bytes = resp.bytes().await?.to_vec();
                        return Ok(Some(bytes));
                    }
                    200 => {
                        // Server does not support Range; slice locally.
                        let full = resp.bytes().await?;
                        let s = start as usize;
                        let e = (s + len as usize).min(full.len());
                        if s >= full.len() {
                            return Ok(Some(Vec::new()));
                        }
                        return Ok(Some(full[s..e].to_vec()));
                    }
                    404 => return Ok(None),
                    status => {
                        attempt += 1;
                        if attempt >= MAX_RETRIES {
                            return Err(anyhow!(
                                "HTTPS Range GET {} ({}) returned HTTP {} after {} attempts",
                                url,
                                range_header,
                                status,
                                attempt,
                            ));
                        }
                        let delay = RETRY_BASE_MS * (1 << attempt);
                        tokio::time::sleep(Duration::from_millis(delay)).await;
                    }
                },
                Err(e) if is_transient(&e) => {
                    attempt += 1;
                    if attempt >= MAX_RETRIES {
                        return Err(anyhow!(
                            "HTTPS Range GET {} failed after {} attempts: {}",
                            url,
                            attempt,
                            e
                        ));
                    }
                    let delay = RETRY_BASE_MS * (1 << attempt);
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                }
                Err(e) => return Err(anyhow!("HTTPS Range GET {}: {}", url, e)),
            }
        }
    }

    /// HEAD request to check existence, falling back to GET on servers that
    /// don't implement HEAD.
    async fn head_with_retry(&self, url: &str) -> Result<bool> {
        let mut attempt = 0u32;
        loop {
            let req = self.auth(self.client.head(url));
            match req.send().await {
                Ok(resp) => match resp.status().as_u16() {
                    200 | 206 => return Ok(true),
                    404 => return Ok(false),
                    405 => {
                        // HEAD not supported — fall back to GET.
                        return Ok(self.get_with_retry(url).await?.is_some());
                    }
                    status => {
                        attempt += 1;
                        if attempt >= MAX_RETRIES {
                            return Err(anyhow!(
                                "HTTPS HEAD {} returned HTTP {} after {} attempts",
                                url,
                                status,
                                attempt
                            ));
                        }
                        let delay = RETRY_BASE_MS * (1 << attempt);
                        tokio::time::sleep(Duration::from_millis(delay)).await;
                    }
                },
                Err(e) if is_transient(&e) => {
                    attempt += 1;
                    if attempt >= MAX_RETRIES {
                        return Err(anyhow!(
                            "HTTPS HEAD {} failed after {} attempts: {}",
                            url,
                            attempt,
                            e
                        ));
                    }
                    let delay = RETRY_BASE_MS * (1 << attempt);
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                }
                Err(e) => return Err(anyhow!("HTTPS HEAD {}: {}", url, e)),
            }
        }
    }
}

/// Classify reqwest errors as transient (worth retrying) or permanent.
fn is_transient(e: &reqwest::Error) -> bool {
    e.is_connect() || e.is_timeout() || e.is_request()
}

#[async_trait]
impl StorageBackend for HttpsStorage {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        self.get_with_retry(&self.url(key)).await
    }

    async fn range_get(&self, key: &str, start: u64, len: u32) -> Result<Option<Vec<u8>>> {
        self.range_get_with_retry(&self.url(key), start, len).await
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        self.head_with_retry(&self.url(key)).await
    }

    // ── Read-only: write operations return errors ──────────────────────

    async fn put(&self, key: &str, _data: &[u8]) -> Result<()> {
        Err(anyhow!(
            "HttpsStorage is read-only: put({}) not supported",
            key
        ))
    }

    async fn delete(&self, key: &str) -> Result<()> {
        Err(anyhow!(
            "HttpsStorage is read-only: delete({}) not supported",
            key
        ))
    }

    async fn list(&self, _prefix: &str, _after: Option<&str>) -> Result<Vec<String>> {
        // Plain HTTPS servers do not expose directory listings in the
        // StorageBackend key format. Return empty; the manifest (fetched via
        // `get`) is the only key the VFS needs to discover all page groups.
        Ok(Vec::new())
    }

    async fn put_if_absent(&self, key: &str, _data: &[u8]) -> Result<CasResult> {
        Err(anyhow!(
            "HttpsStorage is read-only: put_if_absent({}) not supported",
            key
        ))
    }

    async fn put_if_match(&self, key: &str, _data: &[u8], _etag: &str) -> Result<CasResult> {
        Err(anyhow!(
            "HttpsStorage is read-only: put_if_match({}) not supported",
            key
        ))
    }

    fn backend_name(&self) -> &str {
        "https"
    }
}

// ===== Builder =====

/// Builder for [`HttpsStorage`].
pub struct HttpsStorageBuilder {
    base_url: String,
    bearer_token: Option<String>,
    timeout_secs: u64,
}

impl HttpsStorageBuilder {
    fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            bearer_token: None,
            timeout_secs: 30,
        }
    }

    /// Set a ****** for authenticated endpoints.
    pub fn bearer_token(mut self, token: impl Into<String>) -> Self {
        self.bearer_token = Some(token.into());
        self
    }

    /// Override the per-request timeout (default: 30 s).
    pub fn timeout_secs(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    /// Build the [`HttpsStorage`] backend.
    pub fn build(self) -> Result<HttpsStorage> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(self.timeout_secs))
            .build()
            .map_err(|e| anyhow!("failed to build HTTPS client: {}", e))?;
        Ok(HttpsStorage {
            client,
            base_url: self.base_url,
            bearer_token: self.bearer_token,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_no_trailing_slash() {
        let s = HttpsStorage::new("https://example.com/mydb").unwrap();
        assert_eq!(s.url("manifest.msgpack"), "https://example.com/mydb/manifest.msgpack");
        assert_eq!(s.url("p/d/0_v1"), "https://example.com/mydb/p/d/0_v1");
    }

    #[test]
    fn url_trailing_slash_stripped() {
        let s = HttpsStorage::new("https://example.com/mydb/").unwrap();
        assert_eq!(s.url("manifest.msgpack"), "https://example.com/mydb/manifest.msgpack");
    }

    #[test]
    fn builder_sets_token() {
        let s = HttpsStorage::builder("https://example.com/mydb")
            .bearer_token("tok123")
            .build()
            .unwrap();
        assert_eq!(s.bearer_token.as_deref(), Some("tok123"));
    }

    #[test]
    fn builder_sets_timeout() {
        let s = HttpsStorage::builder("https://example.com/mydb")
            .timeout_secs(60)
            .build()
            .unwrap();
        // We can't easily inspect the timeout, but building succeeds.
        let _ = s;
    }
}
