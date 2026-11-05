use super::AuthMode;
use crate::Progress;
use anyhow::{Context, Result, bail};
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

    /// Uses the saved token, allowing sign-in only for interactive runs.
    pub(super) fn connect(
        provider: P,
        path: &Path,
        progress: Progress,
        auth: AuthMode,
    ) -> Result<Session<P>> {
        let client = Client::builder()
            .timeout(Duration::from_secs(3600))
            .build()?;
        let token = match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(_) if auth == AuthMode::Interactive => provider.login(&client, progress)?,
            Err(error) => {
                return Err(error).context("saved sign-in is required for unattended export");
            }
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
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct Calls {
        login: AtomicUsize,
        refresh: AtomicUsize,
    }

    #[derive(Clone, Default)]
    struct FakeProvider {
        calls: Arc<Calls>,
        refresh_error: bool,
    }

    impl Provider for FakeProvider {
        fn login(&self, _client: &Client, _progress: Progress) -> Result<Token> {
            self.calls.login.fetch_add(1, Ordering::Relaxed);
            Ok(Token {
                access_token: "signed-in".into(),
                refresh_token: "refresh".into(),
                expires_at: now() + 3600,
            })
        }

        fn refresh(&self, _client: &Client, refresh_token: &str) -> Result<Token> {
            self.calls.refresh.fetch_add(1, Ordering::Relaxed);
            assert_eq!(refresh_token, "refresh");
            if self.refresh_error {
                bail!("refresh rejected");
            }
            Ok(Token {
                access_token: "refreshed".into(),
                refresh_token: "rotated".into(),
                expires_at: now() + 3600,
            })
        }
    }

    fn save_token(path: &Path, expires_at: u64) {
        let token = Token {
            access_token: "cached".into(),
            refresh_token: "refresh".into(),
            expires_at,
        };
        std::fs::write(path, serde_json::to_vec(&token).unwrap()).unwrap();
    }

    #[test]
    fn unattended_requires_valid_saved_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.json");
        let unreadable = dir.path().join("directory");
        std::fs::create_dir(&unreadable).unwrap();
        let invalid = dir.path().join("invalid.json");
        std::fs::write(&invalid, "invalid JSON").unwrap();
        let provider = FakeProvider::default();
        for path in [&missing, &unreadable, &invalid] {
            assert!(
                Session::connect(provider.clone(), path, &|_| {}, AuthMode::Unattended).is_err()
            );
        }
        assert!(!missing.exists());
        assert_eq!(provider.calls.login.load(Ordering::Relaxed), 0);
        assert_eq!(provider.calls.refresh.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn unattended_uses_cached_token() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token.json");
        save_token(&path, now() + 3600);
        let provider = FakeProvider::default();
        let session =
            Session::connect(provider.clone(), &path, &|_| {}, AuthMode::Unattended).unwrap();
        assert_eq!(session.access_token().unwrap(), "cached");
        assert_eq!(provider.calls.login.load(Ordering::Relaxed), 0);
        assert_eq!(provider.calls.refresh.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn unattended_refreshes_and_saves_expired_token() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token.json");
        save_token(&path, 0);
        let provider = FakeProvider::default();
        let session =
            Session::connect(provider.clone(), &path, &|_| {}, AuthMode::Unattended).unwrap();
        assert_eq!(session.access_token().unwrap(), "refreshed");
        let saved: Token = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved.access_token, "refreshed");
        assert_eq!(saved.refresh_token, "rotated");
        assert!(saved.expires_at > now() + 60);
        assert_eq!(session.access_token().unwrap(), "refreshed");
        assert_eq!(provider.calls.login.load(Ordering::Relaxed), 0);
        assert_eq!(provider.calls.refresh.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn unattended_refresh_failure_does_not_sign_in() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token.json");
        save_token(&path, 0);
        let provider = FakeProvider {
            refresh_error: true,
            ..Default::default()
        };
        let session =
            Session::connect(provider.clone(), &path, &|_| {}, AuthMode::Unattended).unwrap();
        assert_eq!(
            session.access_token().unwrap_err().to_string(),
            "refresh rejected"
        );
        let saved: Token = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved.access_token, "cached");
        assert_eq!(saved.refresh_token, "refresh");
        assert_eq!(provider.calls.login.load(Ordering::Relaxed), 0);
        assert_eq!(provider.calls.refresh.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn interactive_signs_in_without_saved_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token.json");
        let provider = FakeProvider::default();
        let session =
            Session::connect(provider.clone(), &path, &|_| {}, AuthMode::Interactive).unwrap();
        assert_eq!(session.access_token().unwrap(), "signed-in");
        let saved: Token = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved.access_token, "signed-in");
        assert_eq!(provider.calls.login.load(Ordering::Relaxed), 1);
        assert_eq!(provider.calls.refresh.load(Ordering::Relaxed), 0);
    }

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
