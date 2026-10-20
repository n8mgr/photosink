use crate::Progress;
use anyhow::{Result, bail};
use reqwest::blocking::{Client, RequestBuilder, Response};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize)]
pub(super) struct Token {
    pub(super) access_token: String,
    pub(super) refresh_token: String,
    pub(super) expires_at: u64,
}

/// How a service signs in and refreshes its tokens.
pub(super) trait Provider: Sync {
    fn login(&self, client: &Client, progress: Progress) -> Result<Token>;
    fn refresh(&self, client: &Client, refresh_token: &str) -> Result<Token>;
}

/// An HTTP client holding a service's token, saved to `path` and refreshed
/// before it expires.
pub(super) struct Session<P> {
    pub(super) client: Client,
    provider: P,
    path: PathBuf,
    token: Mutex<Token>,
}

impl<P: Provider> Session<P> {
    fn save(&self) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(
            &self.path,
            serde_json::to_vec(&*self.token.lock().unwrap())?,
        )?;
        Ok(())
    }

    fn access_token(&self) -> Result<String> {
        let refresh = {
            let t = self.token.lock().unwrap();
            if t.expires_at > now() + 60 {
                return Ok(t.access_token.clone());
            }
            t.refresh_token.clone()
        };
        let token = self.provider.refresh(&self.client, &refresh)?;
        let access = token.access_token.clone();
        *self.token.lock().unwrap() = token;
        self.save()?;
        Ok(access)
    }

    /// Uses the token saved at `path`, or signs in when there is none.
    pub(super) fn connect(provider: P, path: &Path, progress: Progress) -> Result<Session<P>> {
        let client = Client::builder()
            .timeout(Duration::from_secs(3600))
            .build()?;
        let token = match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(_) => provider.login(&client, progress)?,
        };
        let session = Session {
            client,
            provider,
            path: path.to_owned(),
            token: Mutex::new(token),
        };
        session.save()?;
        Ok(session)
    }

    pub(super) fn download(
        &self,
        dest: &Path,
        request: impl Fn() -> RequestBuilder,
    ) -> Result<String> {
        super::retry::download(dest, || {
            let response = request().bearer_auth(self.access_token()?).send()?;
            let wait = matches!(response.status().as_u16(), 429 | 503).then(|| {
                Duration::from_secs(
                    response
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok()?.parse().ok())
                        .unwrap_or(5),
                )
            });
            let response = response.error_for_status().map_err(|error| {
                let error = anyhow::Error::new(error);
                match wait {
                    Some(wait) => error.context(super::retry::RetryDelay(wait)),
                    None => error,
                }
            })?;
            crate::export::write_hashed(response, dest)
        })
    }

    /// Sends the request with the access token, waiting out 429 and 503
    /// responses up to five times.
    pub(super) fn send(
        &self,
        what: &str,
        request: impl Fn() -> RequestBuilder,
    ) -> Result<Response> {
        for _ in 0..5 {
            let resp = request().bearer_auth(self.access_token()?).send()?;
            if matches!(resp.status().as_u16(), 429 | 503) {
                let wait = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok()?.parse().ok())
                    .unwrap_or(5);
                std::thread::sleep(Duration::from_secs(wait));
                continue;
            }
            if !resp.status().is_success() {
                let status = resp.status();
                bail!("{what}: {status} {}", resp.text().unwrap_or_default());
            }
            return Ok(resp);
        }
        bail!("{what}: throttled")
    }
}

pub(super) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_saved_token_files() {
        let t: Token =
            serde_json::from_str(r#"{"access_token":"a","refresh_token":"r","expires_at":12}"#)
                .unwrap();
        assert_eq!(
            (
                t.access_token.as_str(),
                t.refresh_token.as_str(),
                t.expires_at
            ),
            ("a", "r", 12)
        );
    }
}
