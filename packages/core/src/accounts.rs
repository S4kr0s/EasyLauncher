//! Minecraft accounts: Microsoft (device-code flow → Xbox Live → XSTS →
//! Minecraft services) and offline accounts for testing.
//!
//! Microsoft login needs an Azure app registration that Mojang has approved
//! for the Minecraft API; its client id is a setting (or the
//! `EASYLAUNCHER_MS_CLIENT_ID` build-time variable).

use crate::download::Http;
use crate::minecraft::LaunchAccount;
use crate::paths::Paths;
use anyhow::{anyhow, bail, Context, Result};
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const BUILTIN_CLIENT_ID: Option<&str> = option_env!("EASYLAUNCHER_MS_CLIENT_ID");
const SCOPE: &str = "XboxLive.signin offline_access";
const MS: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Microsoft,
    Offline,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    /// Minecraft UUID without dashes.
    pub id: String,
    pub name: String,
    pub kind: Kind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// Unix seconds when `access_token` expires.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xuid: Option<String>,
}

/// What the UI may see (no tokens).
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInfo {
    pub id: String,
    pub name: String,
    pub kind: Kind,
    pub active: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Accounts {
    #[serde(default)]
    pub accounts: Vec<Account>,
    #[serde(default)]
    pub active: Option<String>,
}

impl Accounts {
    pub fn load(paths: &Paths) -> Result<Self> {
        match std::fs::read(paths.accounts_file()) {
            Ok(b) => Ok(serde_json::from_slice(&b)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self, paths: &Paths) -> Result<()> {
        let f = paths.accounts_file();
        std::fs::create_dir_all(f.parent().unwrap())?;
        std::fs::write(&f, serde_json::to_vec_pretty(self)?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    pub fn upsert(&mut self, a: Account) {
        self.active = Some(a.id.clone());
        match self.accounts.iter_mut().find(|x| x.id == a.id) {
            Some(x) => *x = a,
            None => self.accounts.push(a),
        }
    }

    pub fn remove(&mut self, id: &str) {
        self.accounts.retain(|a| a.id != id);
        if self.active.as_deref() == Some(id) {
            self.active = self.accounts.first().map(|a| a.id.clone());
        }
    }

    pub fn info(&self) -> Vec<AccountInfo> {
        self.accounts
            .iter()
            .map(|a| AccountInfo { id: a.id.clone(), name: a.name.clone(), kind: a.kind.clone(), active: self.active.as_deref() == Some(&a.id) })
            .collect()
    }

    pub fn active(&self) -> Option<&Account> {
        let id = self.active.as_deref()?;
        self.accounts.iter().find(|a| a.id == id)
    }
}

/// Offline account with the same UUID scheme vanilla servers use in offline mode.
pub fn offline(name: &str) -> Result<Account> {
    if name.is_empty() || name.len() > 16 || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        bail!("player names are 1–16 letters, digits or _");
    }
    let mut h: [u8; 16] = Md5::digest(format!("OfflinePlayer:{name}").as_bytes()).into();
    h[6] = (h[6] & 0x0f) | 0x30;
    h[8] = (h[8] & 0x3f) | 0x80;
    Ok(Account {
        id: hex::encode(h),
        name: name.into(),
        kind: Kind::Offline,
        access_token: None,
        refresh_token: None,
        expires_at: None,
        xuid: None,
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCode {
    pub user_code: String,
    pub verification_uri: String,
    pub device_code: String,
    pub interval: u64,
    pub expires_in: u64,
}

pub async fn ms_start(http: &Http, client_id: &str) -> Result<DeviceCode> {
    let v: Value = http
        .client
        .post(format!("{MS}/devicecode"))
        .form(&[("client_id", client_id), ("scope", SCOPE)])
        .send()
        .await?
        .json()
        .await?;
    if let Some(e) = v.get("error_description") {
        bail!("Microsoft login: {}", e.as_str().unwrap_or_default());
    }
    Ok(DeviceCode {
        user_code: v["user_code"].as_str().context("no user_code")?.into(),
        verification_uri: v["verification_uri"].as_str().unwrap_or("https://www.microsoft.com/link").into(),
        device_code: v["device_code"].as_str().context("no device_code")?.into(),
        interval: v["interval"].as_u64().unwrap_or(5),
        expires_in: v["expires_in"].as_u64().unwrap_or(900),
    })
}

/// One poll of the device-code token endpoint. `Ok(None)` = player has not
/// finished signing in yet.
pub async fn ms_poll(http: &Http, client_id: &str, device_code: &str) -> Result<Option<Account>> {
    let v: Value = http
        .client
        .post(format!("{MS}/token"))
        .form(&[
            ("client_id", client_id),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("device_code", device_code),
        ])
        .send()
        .await?
        .json()
        .await?;
    match v.get("error").and_then(|e| e.as_str()) {
        Some("authorization_pending") | Some("slow_down") => return Ok(None),
        Some(e) => bail!("Microsoft login failed: {}", v["error_description"].as_str().unwrap_or(e)),
        None => {}
    }
    let ms_token = v["access_token"].as_str().context("no access_token")?;
    let refresh = v["refresh_token"].as_str().map(String::from);
    Ok(Some(minecraft_login(http, ms_token, refresh).await?))
}

async fn refresh_ms(http: &Http, client_id: &str, refresh_token: &str) -> Result<Account> {
    let v: Value = http
        .client
        .post(format!("{MS}/token"))
        .form(&[("client_id", client_id), ("grant_type", "refresh_token"), ("refresh_token", refresh_token), ("scope", SCOPE)])
        .send()
        .await?
        .json()
        .await?;
    if v.get("error").is_some() {
        bail!("your Microsoft login expired, please sign in again");
    }
    let ms_token = v["access_token"].as_str().context("no access_token")?;
    let refresh = v["refresh_token"].as_str().map(String::from).or(Some(refresh_token.to_string()));
    minecraft_login(http, ms_token, refresh).await
}

async fn post_json(http: &Http, url: &str, body: Value) -> Result<Value> {
    let r = http.client.post(url).header("Accept", "application/json").json(&body).send().await?;
    let status = r.status();
    let v: Value = r.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        if let Some(x) = v.get("XErr").and_then(|x| x.as_u64()) {
            bail!(match x {
                2148916233 => "this Microsoft account has no Xbox profile yet; sign in once at minecraft.net".to_string(),
                2148916238 => "this is a child account; it must be added to a Microsoft family".to_string(),
                _ => format!("Xbox Live refused the login (XErr {x})"),
            });
        }
        bail!("{url} returned {status}");
    }
    Ok(v)
}

async fn minecraft_login(http: &Http, ms_token: &str, refresh: Option<String>) -> Result<Account> {
    let xbl = post_json(
        http,
        "https://user.auth.xboxlive.com/user/authenticate",
        json!({"Properties":{"AuthMethod":"RPS","SiteName":"user.auth.xboxlive.com","RpsTicket":format!("d={ms_token}")},"RelyingParty":"http://auth.xboxlive.com","TokenType":"JWT"}),
    )
    .await?;
    let xbl_token = xbl["Token"].as_str().context("no Xbox token")?;
    let xsts = post_json(
        http,
        "https://xsts.auth.xboxlive.com/xsts/authorize",
        json!({"Properties":{"SandboxId":"RETAIL","UserTokens":[xbl_token]},"RelyingParty":"rp://api.minecraftservices.com/","TokenType":"JWT"}),
    )
    .await?;
    let xsts_token = xsts["Token"].as_str().context("no XSTS token")?;
    let uhs = xsts["DisplayClaims"]["xui"][0]["uhs"].as_str().context("no user hash")?;
    let mc = post_json(
        http,
        "https://api.minecraftservices.com/authentication/login_with_xbox",
        json!({"identityToken": format!("XBL3.0 x={uhs};{xsts_token}")}),
    )
    .await
    .context("Minecraft rejected the login (is the launcher's Azure app approved for Minecraft?)")?;
    let token = mc["access_token"].as_str().context("no Minecraft token")?.to_string();
    let expires_in = mc["expires_in"].as_i64().unwrap_or(86400);
    let profile = http.client.get("https://api.minecraftservices.com/minecraft/profile").bearer_auth(&token).send().await?;
    if profile.status().as_u16() == 404 {
        bail!("this Microsoft account does not own Minecraft Java Edition");
    }
    let p: Value = profile.error_for_status()?.json().await?;
    Ok(Account {
        id: p["id"].as_str().context("no profile id")?.into(),
        name: p["name"].as_str().unwrap_or("Player").into(),
        kind: Kind::Microsoft,
        access_token: Some(token),
        refresh_token: refresh,
        expires_at: Some(chrono::Utc::now().timestamp() + expires_in),
        xuid: xsts["DisplayClaims"]["xui"][0]["xid"].as_str().map(String::from),
    })
}

/// Returns launch credentials for the active account, refreshing tokens when
/// they are about to expire.
pub async fn launch_account(http: &Http, paths: &Paths, client_id: Option<&str>) -> Result<LaunchAccount> {
    let mut store = Accounts::load(paths)?;
    let a = store.active().cloned().ok_or_else(|| anyhow!("add an account first"))?;
    let a = match a.kind {
        Kind::Offline => a,
        Kind::Microsoft => {
            let fresh = a.expires_at.map(|t| t - 300 > chrono::Utc::now().timestamp()).unwrap_or(false);
            if fresh {
                a
            } else {
                let cid = client_id.ok_or_else(|| anyhow!("no Microsoft client id configured"))?;
                let rt = a.refresh_token.clone().ok_or_else(|| anyhow!("please sign in again"))?;
                let n = refresh_ms(http, cid, &rt).await?;
                store.upsert(n.clone());
                store.save(paths)?;
                n
            }
        }
    };
    Ok(LaunchAccount {
        name: a.name.clone(),
        uuid: a.id.clone(),
        token: a.access_token.clone().unwrap_or_else(|| "0".into()),
        user_type: if a.kind == Kind::Microsoft { "msa".into() } else { "legacy".into() },
        xuid: a.xuid.clone(),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn offline_uuid_matches_vanilla() {
        // Vanilla: UUID.nameUUIDFromBytes("OfflinePlayer:Notch")
        assert_eq!(super::offline("Notch").unwrap().id, "b50ad385829d3141a2167e7d7539ba7f");
    }
}
