//! HTTP(S) and file:// fetching with hash verification and bounded parallelism.

use crate::hash::sha1_bytes;
use crate::progress::{Event, Reporter};
use anyhow::{bail, Context, Result};
use futures::StreamExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use url::Url;

pub const PARALLEL: usize = 12;

#[derive(Clone)]
pub struct Http {
    pub client: reqwest::Client,
}

impl Default for Http {
    fn default() -> Self {
        Self::new()
    }
}

impl Http {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .user_agent(crate::USER_AGENT)
            .connect_timeout(std::time::Duration::from_secs(15))
            .read_timeout(std::time::Duration::from_secs(60))
            .build()
            .expect("http client");
        Self { client }
    }

    pub async fn bytes(&self, url: &str) -> Result<Vec<u8>> {
        let u = Url::parse(url).with_context(|| format!("bad url {url}"))?;
        if u.scheme() == "file" {
            let p = u.to_file_path().map_err(|_| anyhow::anyhow!("bad file url {url}"))?;
            return tokio::fs::read(&p).await.with_context(|| format!("reading {}", p.display()));
        }
        let mut last = None;
        for attempt in 0..3 {
            match self.client.get(u.clone()).send().await.and_then(|r| r.error_for_status()) {
                Ok(r) => match r.bytes().await {
                    Ok(b) => return Ok(b.to_vec()),
                    Err(e) => last = Some(e),
                },
                Err(e) => {
                    let fatal = e.status().map(|s| s.is_client_error()).unwrap_or(false);
                    last = Some(e);
                    if fatal {
                        break;
                    }
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(500 * (attempt + 1))).await;
        }
        Err(anyhow::Error::from(last.unwrap())).with_context(|| format!("downloading {url}"))
    }

    pub async fn json<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T> {
        let b = self.bytes(url).await?;
        serde_json::from_slice(&b).with_context(|| format!("parsing {url}"))
    }

    /// Downloads `url` to `dest` (atomically), verifying sha1/size when given.
    pub async fn to_file(&self, url: &str, dest: &Path, sha1: Option<&str>, size: Option<u64>) -> Result<()> {
        let data = self.bytes(url).await?;
        if let Some(s) = size {
            if data.len() as u64 != s {
                bail!("{url}: expected {s} bytes, got {}", data.len());
            }
        }
        if let Some(h) = sha1 {
            let got = sha1_bytes(&data);
            if !got.eq_ignore_ascii_case(h) {
                bail!("{url}: hash mismatch (expected {h}, got {got})");
            }
        }
        write_atomic(dest, &data).await
    }
}

pub async fn write_atomic(dest: &Path, data: &[u8]) -> Result<()> {
    if let Some(p) = dest.parent() {
        tokio::fs::create_dir_all(p).await?;
    }
    let tmp = dest.with_extension(format!("part{}", std::process::id()));
    tokio::fs::write(&tmp, data).await?;
    tokio::fs::rename(&tmp, dest).await?;
    Ok(())
}

/// Resolves a possibly relative URL against a base URL.
pub fn resolve(base: &str, rel: &str) -> Result<String> {
    if let Ok(u) = Url::parse(rel) {
        return Ok(u.to_string());
    }
    let b = Url::parse(base).with_context(|| format!("bad base url {base}"))?;
    Ok(b.join(rel)?.to_string())
}

/// Turns a user-entered location (URL or local path) into a URL.
pub fn to_url(loc: &str) -> Result<String> {
    if let Ok(u) = Url::parse(loc) {
        if u.scheme().len() > 1 {
            return Ok(u.to_string());
        }
    }
    let p = std::fs::canonicalize(loc).with_context(|| format!("{loc} is neither a URL nor an existing path"))?;
    Url::from_file_path(&p)
        .map(|u| u.to_string())
        .map_err(|_| anyhow::anyhow!("cannot turn {} into a URL", p.display()))
}

#[derive(Clone, Debug)]
pub struct Job {
    pub urls: Vec<String>,
    pub dest: PathBuf,
    pub sha1: Option<String>,
    pub size: Option<u64>,
}

/// Runs download jobs in parallel. Jobs whose destination already exists with
/// the right hash are skipped. Reports byte progress over all jobs.
pub async fn run_jobs(http: &Http, jobs: Vec<Job>, r: &Reporter) -> Result<()> {
    let total: u64 = jobs.iter().map(|j| j.size.unwrap_or(0)).sum();
    let done = Arc::new(AtomicU64::new(0));
    r(Event::Progress { done: 0, total });
    let results: Vec<Result<()>> = futures::stream::iter(jobs.into_iter().map(|job| {
        let http = http.clone();
        let done = done.clone();
        let r = r.clone();
        async move {
            let ok = match &job.sha1 {
                Some(h) => {
                    let dest = job.dest.clone();
                    let h = h.clone();
                    tokio::task::spawn_blocking(move || {
                        crate::hash::sha1_file(&dest).ok().flatten().map(|g| g.eq_ignore_ascii_case(&h)).unwrap_or(false)
                    })
                    .await
                    .unwrap_or(false)
                }
                None => job.dest.exists(),
            };
            if !ok {
                let mut err = None;
                for u in &job.urls {
                    match http.to_file(u, &job.dest, job.sha1.as_deref(), job.size).await {
                        Ok(()) => {
                            err = None;
                            break;
                        }
                        Err(e) => err = Some(e),
                    }
                }
                if let Some(e) = err {
                    return Err(e);
                }
                if job.urls.is_empty() {
                    bail!("{} is missing and has no download url", job.dest.display());
                }
            }
            let d = done.fetch_add(job.size.unwrap_or(0), Ordering::Relaxed) + job.size.unwrap_or(0);
            r(Event::Progress { done: d, total });
            Ok(())
        }
    }))
    .buffer_unordered(PARALLEL)
    .collect()
    .await;
    let errs: Vec<_> = results.into_iter().filter_map(|r| r.err()).collect();
    if let Some(first) = errs.into_iter().next() {
        return Err(first);
    }
    Ok(())
}
