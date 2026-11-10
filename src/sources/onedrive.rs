use super::oauth::{Provider, Session, Token, now};
use super::{AuthMode, Source};
use crate::Progress;
use crate::photo::{Asset, Camera};
use anyhow::{Context, Result, bail};
use chrono::DateTime;
use reqwest::blocking::{Client, Response};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

const AUTH: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0";
const GRAPH: &str = "https://graph.microsoft.com/v1.0/me/drive";
const SCOPE: &str = "Files.Read offline_access";
const SELECT: &str = "$select=id,name,file,folder,deleted,photo,location,description,fileSystemInfo,createdDateTime,cTag,eTag";

struct Microsoft {
    client_id: String,
}

pub(crate) struct OneDrive {
    session: Session<Microsoft>,
}

impl Provider for Microsoft {
    fn login(&self, client: &Client, progress: Progress) -> Result<Token> {
        device_login(client, &self.client_id, progress)
    }

    fn refresh(&self, client: &Client, refresh_token: &str) -> Result<Token> {
        let v: Value = client
            .post(format!("{AUTH}/token"))
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
                ("scope", SCOPE),
            ])
            .send()?
            .json()?;
        token_from(&v)
    }
}

fn token_from(v: &Value) -> Result<Token> {
    let field = |k: &str| {
        v[k].as_str()
            .map(str::to_owned)
            .with_context(|| format!("token response without {k}: {v}"))
    };
    Ok(Token {
        access_token: field("access_token")?,
        refresh_token: field("refresh_token")?,
        expires_at: now() + v["expires_in"].as_u64().unwrap_or(3600),
    })
}

fn item_asset(v: &Value) -> Option<Asset> {
    if !v["deleted"].is_null() || !v["folder"].is_null() {
        return None;
    }
    let mime = v["file"]["mimeType"].as_str()?;
    let video = mime.starts_with("video/");
    if !video && !mime.starts_with("image/") {
        return None;
    }
    let text = |x: &Value| x.as_str().filter(|s| !s.is_empty()).map(str::to_owned);
    let photo = &v["photo"];
    let mut a = Asset {
        uuid: v["id"].as_str()?.to_owned(),
        revision: text(&v["cTag"]).or_else(|| text(&v["eTag"])),
        filename: v["name"].as_str()?.to_owned(),
        description: text(&v["description"]),
        taken: DateTime::parse_from_rfc3339(photo["takenDateTime"].as_str().unwrap_or("")).ok(),
        uploaded: [
            &v["fileSystemInfo"]["createdDateTime"],
            &v["createdDateTime"],
        ]
        .into_iter()
        .find_map(|d| DateTime::parse_from_rfc3339(d.as_str()?).ok()),
        latitude: v["location"]["latitude"].as_f64(),
        longitude: v["location"]["longitude"].as_f64(),
        video,
        camera: Camera {
            make: text(&photo["cameraMake"]),
            model: text(&photo["cameraModel"]),
            lens: None,
        },
        ..Default::default()
    };
    a.fill_place();
    Some(a)
}

fn device_login(client: &Client, client_id: &str, progress: Progress) -> Result<Token> {
    let start: Value = client
        .post(format!("{AUTH}/devicecode"))
        .form(&[("client_id", client_id), ("scope", SCOPE)])
        .send()?
        .json()?;
    if let Some(e) = start["error"].as_str() {
        bail!("{e}: {}", start["error_description"]);
    }
    let code = start["user_code"].as_str().context("user_code")?;
    // The consumers endpoint returns https://www.microsoft.com/link, which
    // routes to the login.live.com "link a device" page and rejects the
    // code. /devicelogin lands on the /common deviceauth page, which
    // accepts consumer codes and prefills from ?otc=.
    let url = format!("https://microsoft.com/devicelogin?otc={code}");
    progress(crate::Event::SignIn {
        url: url.clone(),
        code: code.to_owned(),
    });
    if let Err(e) = open::that(&url) {
        progress(crate::Event::error("could not open browser", e));
    }
    let device_code = start["device_code"].as_str().context("device_code")?;
    let mut interval = start["interval"].as_u64().unwrap_or(5);
    loop {
        std::thread::sleep(Duration::from_secs(interval));
        let v: Value = client
            .post(format!("{AUTH}/token"))
            .form(&[
                ("client_id", client_id),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("device_code", device_code),
            ])
            .send()?
            .json()?;
        match v["error"].as_str() {
            None => return token_from(&v),
            Some("authorization_pending") => {}
            Some("slow_down") => interval += 5,
            Some(e) => bail!("{e}: {}", v["error_description"]),
        }
    }
}

impl OneDrive {
    pub(crate) fn connect(
        client_id: &str,
        token_path: &Path,
        progress: Progress,
        auth: AuthMode,
    ) -> Result<OneDrive> {
        let provider = Microsoft {
            client_id: client_id.to_owned(),
        };
        Ok(OneDrive {
            session: Session::connect(provider, token_path, progress, auth)?,
        })
    }

    fn get(&self, url: &str) -> Result<Response> {
        self.session.send(url, || self.session.client.get(url))
    }

    pub(crate) fn list(
        &self,
        folder: &str,
        delta: Option<String>,
    ) -> Result<(HashMap<String, Asset>, String)> {
        let mut url = match delta {
            Some(d) => d,
            None if folder.is_empty() => format!("{GRAPH}/root/delta?{SELECT}"),
            None => {
                let item: Value = self.get(&format!("{GRAPH}/root:/{folder}"))?.json()?;
                let id = item["id"].as_str().context("folder id")?;
                format!("{GRAPH}/items/{id}/delta?{SELECT}")
            }
        };
        let mut assets = HashMap::new();
        loop {
            let page: Value = self.get(&url)?.json()?;
            for item in page["value"].as_array().into_iter().flatten() {
                if let Some(a) = item_asset(item) {
                    assets.insert(a.uuid.clone(), a);
                }
            }
            match (
                page["@odata.nextLink"].as_str(),
                page["@odata.deltaLink"].as_str(),
            ) {
                (Some(next), _) => url = next.to_owned(),
                (None, Some(d)) => return Ok((assets, d.to_owned())),
                (None, None) => bail!("delta page without next or delta link"),
            }
        }
    }
}

impl Source for OneDrive {
    fn write(&self, local_id: &str, _index: usize, dest: &Path) -> Result<String> {
        let url = format!("{GRAPH}/items/{local_id}/content");
        self.session
            .download(dest, || self.session.client.get(&url))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_items() {
        let a = item_asset(&json!({
            "id": "X1", "name": "IMG_1.HEIC", "description": "d",
            "cTag": "content-1", "eTag": "metadata-2",
            "file": {"mimeType": "image/heic"},
            "photo": {"takenDateTime": "2019-05-03T18:27:59Z", "cameraMake": "Apple", "cameraModel": "iPhone"},
            "location": {"latitude": 48.8566, "longitude": 2.3522}
        }))
        .unwrap();
        assert_eq!((a.uuid.as_str(), a.filename.as_str()), ("X1", "IMG_1.HEIC"));
        assert_eq!(a.revision.as_deref(), Some("content-1"));
        assert_eq!(
            a.taken.unwrap().format("%Y-%m-%d").to_string(),
            "2019-05-03"
        );
        assert_eq!(a.camera.make.as_deref(), Some("Apple"));
        assert_eq!(a.place.country.as_deref(), Some("FR"));
        assert!(!a.video);

        let v = item_asset(&json!({"id": "V", "name": "a.mp4", "eTag": "version-1", "file": {"mimeType": "video/mp4"}}))
            .unwrap();
        assert_eq!(v.revision.as_deref(), Some("version-1"));
        assert!(v.video && v.taken.is_none());
        assert!(item_asset(&json!({"id": "F", "name": "f", "folder": {}})).is_none());
        assert!(item_asset(&json!({"id": "D", "name": "a.jpg", "file": {"mimeType": "image/jpeg"}, "deleted": {}})).is_none());
        assert!(
            item_asset(&json!({"id": "T", "name": "a.txt", "file": {"mimeType": "text/plain"}}))
                .is_none()
        );
    }

    #[test]
    fn parses_tokens() {
        let t = token_from(&json!({"access_token": "a", "refresh_token": "r", "expires_in": 10}))
            .unwrap();
        assert_eq!(
            (t.access_token.as_str(), t.refresh_token.as_str()),
            ("a", "r")
        );
        assert!(t.expires_at >= now() + 9);
        assert!(token_from(&json!({"access_token": "a"})).is_err());
    }
}
