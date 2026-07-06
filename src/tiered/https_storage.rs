//! HTTPS storage backend for turbolite.
//!
//! Reads page groups and manifests from a plain HTTPS endpoint using HTTP Range
//! requests. This is a **read-only** backend: `put`, `delete`, and CAS
//! operations all return errors. Use it to query a turbolite database that is
//! published as static files on any HTTPS server (S3 static website, CDN,
//! GitHub Releases, etc.).
//!
//! Uses [`ureq`] (synchronous) wrapped in [`tokio::task::spawn_blocking`] to
//! satisfy the async `StorageBackend` trait without introducing a new async
//! HTTP dependency.
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
//! travel over the network. Servers that honour the `Range` header return HTTP
//! 206; servers that ignore it return 200 and the response body is sliced
//! locally (at the cost of fetching extra bytes).
//!
//! # Authentication
//!
//! An optional bearer token can be supplied for authenticated endpoints.
//! Set it with [`HttpsStorageBuilder::bearer_token`].

use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use hadb_storage::{CasResult, StorageBackend};

/// How many times to retry a failed request before giving up.
const MAX_RETRIES: u32 = 3;
/// Initial retry back-off in milliseconds (doubles on each attempt).
const RETRY_BASE_MS: u64 = 100;

/// Shared state held inside an `Arc` so it is cheaply cloned into
/// `spawn_blocking` closures.
struct Inner {
    agent: ureq::Agent,
    base_url: String,
    bearer_token: Option<String>,
}

impl Inner {
    fn url(&self, key: &str) -> String {
        format!("{}/{}", self.base_url, key)
    }

    fn add_auth(&self, req: ureq::Request) -> ureq::Request {
        if let Some(token) = &self.bearer_token {
            req.set("Authorization", &format!("Bearer {}", token))
        } else {
            req
        }
    }

    fn get_bytes(&self, url: &str) -> Result<Option<Vec<u8>>> {
        let mut attempt = 0u32;
        loop {
            let req = self.add_auth(self.agent.get(url));
            match req.call() {
                Ok(resp) => {
                    let mut buf = Vec::new();
                    resp.into_reader().read_to_end(&mut buf)?;
                    return Ok(Some(buf));
                }
                Err(ureq::Error::Status(404, _)) => return Ok(None),
                Err(ureq::Error::Status(status, _)) => {
                    attempt += 1;
                    if attempt >= MAX_RETRIES {
                        return Err(anyhow!(
                            "HTTPS GET {} returned HTTP {} after {} attempts",
                            url,
                            status,
                            attempt
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(RETRY_BASE_MS * (1 << attempt)));
                }
                Err(ureq::Error::Transport(e)) => {
                    attempt += 1;
                    if attempt >= MAX_RETRIES {
                        return Err(anyhow!(
                            "HTTPS GET {} transport error after {} attempts: {}",
                            url,
                            attempt,
                            e
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(RETRY_BASE_MS * (1 << attempt)));
                }
            }
        }
    }

    fn range_get_bytes(&self, url: &str, start: u64, len: u32) -> Result<Option<Vec<u8>>> {
        let range_header = format!("bytes={}-{}", start, start + len as u64 - 1);
        let mut attempt = 0u32;
        loop {
            let req = self
                .add_auth(self.agent.get(url))
                .set("Range", &range_header);
            match req.call() {
                Ok(resp) => {
                    let status = resp.status();
                    let mut buf = Vec::new();
                    resp.into_reader().read_to_end(&mut buf)?;
                    if status == 206 {
                        return Ok(Some(buf));
                    }
                    // Server returned 200 (ignores Range) — slice locally.
                    let s = start as usize;
                    let e = (s + len as usize).min(buf.len());
                    if s >= buf.len() {
                        return Ok(Some(Vec::new()));
                    }
                    return Ok(Some(buf[s..e].to_vec()));
                }
                Err(ureq::Error::Status(404, _)) => return Ok(None),
                Err(ureq::Error::Status(status, _)) => {
                    attempt += 1;
                    if attempt >= MAX_RETRIES {
                        return Err(anyhow!(
                            "HTTPS Range GET {} ({}) returned HTTP {} after {} attempts",
                            url,
                            range_header,
                            status,
                            attempt
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(RETRY_BASE_MS * (1 << attempt)));
                }
                Err(ureq::Error::Transport(e)) => {
                    attempt += 1;
                    if attempt >= MAX_RETRIES {
                        return Err(anyhow!(
                            "HTTPS Range GET {} transport error after {} attempts: {}",
                            url,
                            attempt,
                            e
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(RETRY_BASE_MS * (1 << attempt)));
                }
            }
        }
    }

    fn head_exists(&self, url: &str) -> Result<bool> {
        let req = self.add_auth(self.agent.head(url));
        match req.call() {
            Ok(_) => Ok(true),
            Err(ureq::Error::Status(404, _)) => Ok(false),
            // Some servers don't support HEAD; fall back to a GET.
            Err(ureq::Error::Status(405, _)) => Ok(self.get_bytes(url)?.is_some()),
            Err(ureq::Error::Status(status, _)) => Err(anyhow!(
                "HTTPS HEAD {} returned HTTP {}",
                url,
                status
            )),
            Err(ureq::Error::Transport(e)) => {
                Err(anyhow!("HTTPS HEAD {} transport error: {}", url, e))
            }
        }
    }
}

/// Read-only HTTPS storage backend.
///
/// Implements [`StorageBackend`] by translating `get` / `range_get` /
/// `exists` calls into HTTPS requests via [`ureq`]. Write operations
/// (`put`, `delete`, `put_if_absent`, `put_if_match`) always return an
/// error — this backend is intentionally read-only.
///
/// Create via [`HttpsStorage::new`] or the builder returned by
/// [`HttpsStorage::builder`].
pub struct HttpsStorage {
    inner: Arc<Inner>,
}

impl HttpsStorage {
    /// Create a new `HttpsStorage` pointing at `base_url`.
    ///
    /// `base_url` should NOT have a trailing slash, e.g.
    /// `https://cdn.example.com/mydb`.
    pub fn new(base_url: impl Into<String>) -> Result<Self> {
        Self::builder(base_url).build()
    }

    /// Return a builder for more control over client options.
    pub fn builder(base_url: impl Into<String>) -> HttpsStorageBuilder {
        HttpsStorageBuilder::new(base_url)
    }
}

#[async_trait]
impl StorageBackend for HttpsStorage {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let inner = Arc::clone(&self.inner);
        let url = inner.url(key);
        tokio::task::spawn_blocking(move || inner.get_bytes(&url))
            .await
            .map_err(|e| anyhow!("spawn_blocking: {}", e))?
    }

    async fn range_get(&self, key: &str, start: u64, len: u32) -> Result<Option<Vec<u8>>> {
        let inner = Arc::clone(&self.inner);
        let url = inner.url(key);
        tokio::task::spawn_blocking(move || inner.range_get_bytes(&url, start, len))
            .await
            .map_err(|e| anyhow!("spawn_blocking: {}", e))?
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        let inner = Arc::clone(&self.inner);
        let url = inner.url(key);
        tokio::task::spawn_blocking(move || inner.head_exists(&url))
            .await
            .map_err(|e| anyhow!("spawn_blocking: {}", e))?
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

    /// Set a bearer token for authenticated endpoints.
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
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(self.timeout_secs))
            .build();
        Ok(HttpsStorage {
            inner: Arc::new(Inner {
                agent,
                base_url: self.base_url,
                bearer_token: self.bearer_token,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_no_trailing_slash() {
        let s = HttpsStorage::new("https://example.com/mydb").unwrap();
        assert_eq!(
            s.inner.url("manifest.msgpack"),
            "https://example.com/mydb/manifest.msgpack"
        );
        assert_eq!(
            s.inner.url("p/d/0_v1"),
            "https://example.com/mydb/p/d/0_v1"
        );
    }

    #[test]
    fn url_trailing_slash_stripped() {
        let s = HttpsStorage::new("https://example.com/mydb/").unwrap();
        assert_eq!(
            s.inner.url("manifest.msgpack"),
            "https://example.com/mydb/manifest.msgpack"
        );
    }

    #[test]
    fn builder_sets_token() {
        let s = HttpsStorage::builder("https://example.com/mydb")
            .bearer_token("tok123")
            .build()
            .unwrap();
        assert_eq!(s.inner.bearer_token.as_deref(), Some("tok123"));
    }

    #[test]
    fn builder_sets_timeout() {
        let s = HttpsStorage::builder("https://example.com/mydb")
            .timeout_secs(60)
            .build()
            .unwrap();
        let _ = s;
    }
}
