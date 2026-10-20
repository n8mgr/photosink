use super::oauth::{Provider, Session, Token, now};
use super::{Source, is_media, is_video};
use crate::photo::Asset;
use crate::{Event, Progress};
use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::DateTime;
use reqwest::blocking::Client;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::Path;
use std::time::{Duration, Instant};

const AUTHORIZE: &str = "https://www.dropbox.com/oauth2/authorize";
const TOKEN: &str = "https://api.dropboxapi.com/oauth2/token";
const API: &str = "https://api.dropboxapi.com/2";
const CONTENT: &str = "https://content.dropboxapi.com/2";
/// Must be registered as a redirect URI on the Dropbox app.
const REDIRECT: &str = "http://localhost:53917/dropbox";
const LISTEN: &str = "127.0.0.1:53917";
const SIGN_IN_WAIT: Duration = Duration::from_secs(600);

struct DropboxAuth {
    app_key: String,
}

pub(crate) struct Dropbox {
    session: Session<DropboxAuth>,
}

impl Provider for DropboxAuth {
    fn login(&self, client: &Client, progress: Progress) -> Result<Token> {
        browser_login(client, &self.app_key, progress)
    }

    fn refresh(&self, client: &Client, refresh_token: &str) -> Result<Token> {
        let v: Value = client
            .post(TOKEN)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
                ("client_id", self.app_key.as_str()),
            ])
            .send()?
            .json()?;
        token_from(&v, Some(refresh_token))
    }
}

fn random(n: usize) -> Result<String> {
    let mut buf = vec![0u8; n];
    getrandom::fill(&mut buf).map_err(|e| anyhow::anyhow!("random: {e}"))?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

fn token_from(v: &Value, previous_refresh: Option<&str>) -> Result<Token> {
    if let Some(e) = v["error"].as_str() {
        bail!("{e}: {}", v["error_description"].as_str().unwrap_or(""));
    }
    Ok(Token {
        access_token: v["access_token"]
            .as_str()
            .with_context(|| format!("token response without access_token: {v}"))?
            .to_owned(),
        refresh_token: match (v["refresh_token"].as_str(), previous_refresh) {
            (Some(r), _) => r.to_owned(),
            (None, Some(r)) => r.to_owned(),
            (None, None) => bail!("token response without refresh_token: {v}"),
        },
        expires_at: now() + v["expires_in"].as_u64().unwrap_or(14400),
    })
}

fn entry_asset(v: &Value) -> Option<Asset> {
    if v[".tag"].as_str()? != "file" {
        return None;
    }
    let name = v["name"].as_str()?;
    if !is_media(name) {
        return None;
    }
    Some(Asset {
        uuid: v["id"].as_str()?.to_owned(),
        filename: name.to_owned(),
        uploaded: ["client_modified", "server_modified"]
            .into_iter()
            .find_map(|k| DateTime::parse_from_rfc3339(v[k].as_str()?).ok()),
        video: is_video(name),
        ..Default::default()
    })
}

/// The authorization code from the redirect's request line, after checking `state`.
fn code_from(request_line: &str, state: &str) -> Result<String> {
    let target = request_line
        .split_whitespace()
        .nth(1)
        .context("malformed redirect request")?;
    let url = url::Url::parse(&format!("http://127.0.0.1{target}"))?;
    let query: HashMap<_, _> = url.query_pairs().into_owned().collect();
    if let Some(e) = query.get("error") {
        bail!(
            "Dropbox sign-in failed: {e} {}",
            query.get("error_description").map_or("", |s| s)
        );
    }
    anyhow::ensure!(
        query.get("state").map(String::as_str) == Some(state),
        "sign-in state mismatch"
    );
    query
        .get("code")
        .cloned()
        .context("redirect without a code")
}

fn wait_for_code(listener: &TcpListener, state: &str) -> Result<String> {
    listener.set_nonblocking(true)?;
    let deadline = Instant::now() + SIGN_IN_WAIT;
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_nonblocking(false)?;
                stream.set_read_timeout(Some(Duration::from_secs(10)))?;
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line)?;
                if !line.contains("/dropbox") {
                    let _ =
                        stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
                    continue;
                }
                let result = code_from(&line, state);
                let body = match &result {
                    Ok(_) => {
                        "Photosink is signed in to Dropbox. You can close this tab.".to_owned()
                    }
                    Err(e) => format!("Photosink could not sign in to Dropbox: {e:#}"),
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                return result;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                anyhow::ensure!(
                    Instant::now() < deadline,
                    "timed out waiting for Dropbox sign-in"
                );
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => return Err(e.into()),
        }
    }
}

fn browser_login(client: &Client, app_key: &str, progress: Progress) -> Result<Token> {
    let listener = TcpListener::bind(LISTEN)
        .with_context(|| format!("listen on {LISTEN} for the sign-in redirect"))?;
    let verifier = random(48)?;
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state = random(16)?;
    let url = url::Url::parse_with_params(
        AUTHORIZE,
        [
            ("client_id", app_key),
            ("response_type", "code"),
            ("redirect_uri", REDIRECT),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
            ("token_access_type", "offline"),
            ("state", &state),
        ],
    )?
    .to_string();
    progress(Event::Authorize { url: url.clone() });
    if let Err(e) = open::that(&url) {
        progress(Event::error("could not open browser", e));
    }
    let code = wait_for_code(&listener, &state)?;
    let v: Value = client
        .post(TOKEN)
        .form(&[
            ("code", code.as_str()),
            ("grant_type", "authorization_code"),
            ("code_verifier", &verifier),
            ("client_id", app_key),
            ("redirect_uri", REDIRECT),
        ])
        .send()?
        .json()?;
    token_from(&v, None)
}

impl Dropbox {
    pub(crate) fn connect(app_key: &str, token_path: &Path, progress: Progress) -> Result<Dropbox> {
        anyhow::ensure!(
            !app_key.trim().is_empty(),
            "set dropbox_app_key in the config to your Dropbox app's key"
        );
        let provider = DropboxAuth {
            app_key: app_key.to_owned(),
        };
        Ok(Dropbox {
            session: Session::connect(provider, token_path, progress)?,
        })
    }

    fn rpc(&self, endpoint: &str, body: &Value) -> Result<Value> {
        let url = format!("{API}/{endpoint}");
        Ok(self
            .session
            .send(endpoint, || self.session.client.post(&url).json(body))?
            .json()?)
    }

    /// Media files under `folder` (no surrounding slashes, empty for the whole
    /// Dropbox), or only those
    /// changed since `cursor`, and the cursor to resume from next time.
    pub(crate) fn list(
        &self,
        folder: &str,
        cursor: Option<String>,
    ) -> Result<(HashMap<String, Asset>, String)> {
        let mut page = match cursor {
            Some(c) => self.rpc("files/list_folder/continue", &json!({ "cursor": c }))?,
            None => self.rpc(
                "files/list_folder",
                &json!({
                    "path": if folder.is_empty() { String::new() } else { format!("/{folder}") },
                    "recursive": true,
                    "include_deleted": false,
                    "limit": 2000,
                }),
            )?,
        };
        let mut assets = HashMap::new();
        loop {
            for entry in page["entries"].as_array().into_iter().flatten() {
                if let Some(a) = entry_asset(entry) {
                    assets.insert(a.uuid.clone(), a);
                }
            }
            let cursor = page["cursor"]
                .as_str()
                .context("list page without a cursor")?
                .to_owned();
            if !page["has_more"].as_bool().unwrap_or(false) {
                return Ok((assets, cursor));
            }
            page = self.rpc("files/list_folder/continue", &json!({ "cursor": cursor }))?;
        }
    }
}

impl Source for Dropbox {
    fn write(&self, local_id: &str, _index: usize, dest: &Path) -> Result<String> {
        let url = format!("{CONTENT}/files/download");
        let arg = json!({ "path": local_id }).to_string();
        self.session.download(dest, || {
            self.session
                .client
                .post(&url)
                .header("Dropbox-API-Arg", &arg)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_entries() {
        let a = entry_asset(&json!({
            ".tag": "file", "id": "id:abc", "name": "IMG_1.HEIC",
            "client_modified": "2019-05-03T18:27:59Z", "server_modified": "2019-06-01T00:00:00Z"
        }))
        .unwrap();
        assert_eq!(
            (a.uuid.as_str(), a.filename.as_str()),
            ("id:abc", "IMG_1.HEIC")
        );
        assert_eq!(
            a.uploaded.unwrap().format("%Y-%m-%d").to_string(),
            "2019-05-03"
        );
        assert!(
            a.taken.is_none(),
            "taken comes from the file's EXIF at export"
        );
        assert!(
            entry_asset(&json!({".tag": "file", "id": "id:v", "name": "a.MOV"}))
                .unwrap()
                .video
        );
        assert!(entry_asset(&json!({".tag": "folder", "id": "id:f", "name": "x.jpg"})).is_none());
        assert!(entry_asset(&json!({".tag": "deleted", "name": "x.jpg"})).is_none());
        assert!(entry_asset(&json!({".tag": "file", "id": "id:t", "name": "notes.txt"})).is_none());
    }

    #[test]
    fn reads_redirect() {
        let line = "GET /dropbox?code=C0DE&state=s1 HTTP/1.1\r\n";
        assert_eq!(code_from(line, "s1").unwrap(), "C0DE");
        assert!(code_from(line, "other").is_err());
        assert!(code_from("GET /dropbox?error=access_denied&state=s1 HTTP/1.1", "s1").is_err());
    }

    #[test]
    fn keeps_refresh_token_on_refresh() {
        let t = token_from(&json!({"access_token": "a", "expires_in": 10}), Some("r")).unwrap();
        assert_eq!(t.refresh_token, "r");
        assert!(token_from(&json!({"access_token": "a"}), None).is_err());
        assert!(token_from(&json!({"error": "invalid_grant"}), Some("r")).is_err());
    }
}
