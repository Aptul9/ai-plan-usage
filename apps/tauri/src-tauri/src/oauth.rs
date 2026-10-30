//! The app's own sign-ins: Claude (PKCE, code pasted back from the callback
//! page) and Codex (device code).
use crate::accounts::OauthCreds;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::Duration;

// Claude Code 2.1.302 values, read from claude.exe on 2026-10-30.
pub const CLAUDE_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const CLAUDE_AUTHORIZE: &str = "https://claude.com/cai/oauth/authorize";
const CLAUDE_REDIRECT: &str = "https://platform.claude.com/oauth/code/callback";
pub const CLAUDE_TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
// The usage endpoint needs user:profile. If the authorize page rejects it on
// its own, use "user:profile user:inference".
const CLAUDE_SCOPE: &str = "user:profile";
// Anthropic's Cloudflare edge 1010-blocks automation User-Agents on the token
// endpoint, so every request must look like the CLI.
pub const CLAUDE_CODE_UA: &str = "claude-code/2.1.0";

// OpenCode packages/core/src/plugin/provider/openai.ts, v2.0.32.
const CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const CODEX_ISSUER: &str = "https://auth.openai.com";
pub const CODEX_DEVICE_PAGE: &str = "https://auth.openai.com/codex/device";
const UA: &str = "ai-plan-usage";

pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

pub fn challenge_for(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

pub fn pkce() -> Pkce {
    let verifier = URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>());
    let challenge = challenge_for(&verifier);
    Pkce { verifier, challenge }
}

pub fn claude_authorize_url(p: &Pkce) -> String {
    reqwest::Url::parse_with_params(
        CLAUDE_AUTHORIZE,
        &[
            ("code", "true"),
            ("client_id", CLAUDE_CLIENT_ID),
            ("response_type", "code"),
            ("redirect_uri", CLAUDE_REDIRECT),
            ("scope", CLAUDE_SCOPE),
            ("code_challenge", p.challenge.as_str()),
            ("code_challenge_method", "S256"),
            ("state", p.verifier.as_str()),
        ],
    )
    .expect("static authorize URL")
    .to_string()
}

/// The callback page shows `code#state`; a bare code is accepted too.
pub fn split_code(input: &str) -> (String, Option<String>) {
    let input = input.trim();
    match input.split_once('#') {
        Some((code, state)) => (code.to_string(), Some(state.to_string())),
        None => (input.to_string(), None),
    }
}

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<i64>,
    pub id_token: Option<String>,
}

impl TokenResponse {
    /// `prev_refresh` is kept when the server does not rotate it.
    pub fn into_creds(self, prev_refresh: &str, account_id: Option<String>) -> OauthCreds {
        OauthCreds {
            expires_at: chrono::Utc::now().timestamp_millis()
                + self.expires_in.unwrap_or(3600) * 1000,
            refresh: self.refresh_token.unwrap_or_else(|| prev_refresh.to_string()),
            access: self.access_token,
            account_id,
        }
    }
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())
}

async fn token_body(resp: reqwest::Response) -> Result<TokenResponse, String> {
    let status = resp.status();
    let text = resp.text().await.map_err(|e| format!("network: {e}"))?;
    if !status.is_success() {
        let head: String = text.chars().take(300).collect();
        return Err(format!("HTTP {}: {head}", status.as_u16()));
    }
    serde_json::from_str(&text).map_err(|e| format!("token parse: {e}"))
}

pub async fn claude_exchange(pasted: &str, verifier: &str) -> Result<OauthCreds, String> {
    let (code, state) = split_code(pasted);
    if code.is_empty() {
        return Err("Paste the code shown after sign-in".into());
    }
    let resp = client()?
        .post(CLAUDE_TOKEN_URL)
        .header("User-Agent", CLAUDE_CODE_UA)
        .header("Accept", "application/json")
        .json(&json!({
            "code": code,
            "state": state.unwrap_or_else(|| verifier.to_string()),
            "grant_type": "authorization_code",
            "client_id": CLAUDE_CLIENT_ID,
            "redirect_uri": CLAUDE_REDIRECT,
            "code_verifier": verifier,
        }))
        .send()
        .await
        .map_err(|e| format!("network: {e}"))?;
    Ok(token_body(resp).await?.into_creds("", None))
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceCode {
    pub device_auth_id: String,
    pub user_code: String,
    pub interval_secs: u64,
}

pub fn parse_device_code(v: &Value) -> Option<DeviceCode> {
    // OpenAI sends the interval as a string.
    let interval = match v.get("interval") {
        Some(Value::String(s)) => s.parse().ok(),
        Some(n) => n.as_u64(),
        None => None,
    }
    .unwrap_or(5)
    .max(1);
    Some(DeviceCode {
        device_auth_id: v.get("device_auth_id")?.as_str()?.to_string(),
        user_code: v.get("user_code")?.as_str()?.to_string(),
        interval_secs: interval,
    })
}

pub async fn codex_device_start() -> Result<DeviceCode, String> {
    let resp = client()?
        .post(format!("{CODEX_ISSUER}/api/accounts/deviceauth/usercode"))
        .header("User-Agent", UA)
        .json(&json!({ "client_id": CODEX_CLIENT_ID }))
        .send()
        .await
        .map_err(|e| format!("network: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("device code HTTP {}", resp.status().as_u16()));
    }
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;
    parse_device_code(&v).ok_or_else(|| "device code response malformed".to_string())
}

/// Ok(None) while the code is not approved yet.
pub async fn codex_device_poll(d: &DeviceCode) -> Result<Option<OauthCreds>, String> {
    let c = client()?;
    let resp = c
        .post(format!("{CODEX_ISSUER}/api/accounts/deviceauth/token"))
        .header("User-Agent", UA)
        .json(&json!({ "device_auth_id": d.device_auth_id, "user_code": d.user_code }))
        .send()
        .await
        .map_err(|e| format!("network: {e}"))?;
    let status = resp.status().as_u16();
    if status == 403 || status == 404 {
        return Ok(None);
    }
    if !(200..300).contains(&status) {
        return Err(format!("device token HTTP {status}"));
    }
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;
    let code = v
        .get("authorization_code")
        .and_then(Value::as_str)
        .ok_or("no authorization_code")?;
    let verifier = v
        .get("code_verifier")
        .and_then(Value::as_str)
        .ok_or("no code_verifier")?;
    let redirect = format!("{CODEX_ISSUER}/deviceauth/callback");
    let resp = c
        .post(format!("{CODEX_ISSUER}/oauth/token"))
        .header("User-Agent", UA)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect.as_str()),
            ("client_id", CODEX_CLIENT_ID),
            ("code_verifier", verifier),
        ])
        .send()
        .await
        .map_err(|e| format!("network: {e}"))?;
    let tokens = token_body(resp).await?;
    let account = tokens
        .id_token
        .as_deref()
        .and_then(jwt_account_id)
        .or_else(|| jwt_account_id(&tokens.access_token));
    Ok(Some(tokens.into_creds("", account)))
}

pub async fn codex_refresh(creds: &OauthCreds) -> Result<OauthCreds, String> {
    let resp = client()?
        .post(format!("{CODEX_ISSUER}/oauth/token"))
        .header("User-Agent", UA)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", creds.refresh.as_str()),
            ("client_id", CODEX_CLIENT_ID),
        ])
        .send()
        .await
        .map_err(|e| format!("network: {e}"))?;
    let tokens = token_body(resp).await?;
    let account = tokens
        .id_token
        .as_deref()
        .and_then(jwt_account_id)
        .or_else(|| creds.account_id.clone());
    Ok(tokens.into_creds(&creds.refresh, account))
}

pub fn jwt_account_id(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    let claims: Value = serde_json::from_slice(&bytes).ok()?;
    claims
        .get("chatgpt_account_id")
        .or_else(|| {
            claims
                .get("https://api.openai.com/auth")
                .and_then(|a| a.get("chatgpt_account_id"))
        })
        .or_else(|| claims.get("organizations")?.get(0)?.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// Default browser, no shell in between (URLs carry `&`).
pub fn open_browser(url: &str) {
    let _ = std::process::Command::new("rundll32.exe")
        .args(["url.dll,FileProtocolHandler", url])
        .spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn challenge_matches_rfc7636_vector() {
        assert_eq!(
            challenge_for("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn pkce_verifier_is_43_url_safe_chars() {
        let p = pkce();
        assert_eq!(p.verifier.len(), 43);
        assert!(p
            .verifier
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert_eq!(p.challenge, challenge_for(&p.verifier));
    }

    #[test]
    fn authorize_url_carries_pkce_and_state() {
        let p = Pkce { verifier: "v123".into(), challenge: "c456".into() };
        let url = reqwest::Url::parse(&claude_authorize_url(&p)).unwrap();
        let q: std::collections::HashMap<String, String> = url.query_pairs().into_owned().collect();
        assert_eq!(url.host_str(), Some("claude.com"));
        assert_eq!(q["code_challenge"], "c456");
        assert_eq!(q["state"], "v123");
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["scope"], "user:profile");
        assert_eq!(q["redirect_uri"], "https://platform.claude.com/oauth/code/callback");
    }

    #[test]
    fn splits_pasted_code() {
        assert_eq!(split_code(" abc#xyz \n"), ("abc".to_string(), Some("xyz".to_string())));
        assert_eq!(split_code("abc"), ("abc".to_string(), None));
    }

    #[test]
    fn device_code_interval_accepts_string_or_number() {
        let a = parse_device_code(&json!({"device_auth_id": "d", "user_code": "U-1", "interval": "7"})).unwrap();
        assert_eq!(a.interval_secs, 7);
        let b = parse_device_code(&json!({"device_auth_id": "d", "user_code": "U-1", "interval": 0})).unwrap();
        assert_eq!(b.interval_secs, 1);
        assert!(parse_device_code(&json!({"user_code": "U"})).is_none());
    }

    #[test]
    fn reads_account_id_from_jwt() {
        let payload = URL_SAFE_NO_PAD.encode(br#"{"https://api.openai.com/auth":{"chatgpt_account_id":"acc-1"}}"#);
        assert_eq!(jwt_account_id(&format!("h.{payload}.s")), Some("acc-1".to_string()));
        assert_eq!(jwt_account_id("not-a-jwt"), None);
    }

    #[test]
    fn token_response_keeps_old_refresh_when_not_rotated() {
        let t: TokenResponse = serde_json::from_str(r#"{"access_token":"a","expires_in":60}"#).unwrap();
        let c = t.into_creds("old", None);
        assert_eq!(c.access, "a");
        assert_eq!(c.refresh, "old");
        assert!(c.expires_at > chrono::Utc::now().timestamp_millis());
    }
}
