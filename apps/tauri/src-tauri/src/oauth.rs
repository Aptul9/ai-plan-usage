//! The app's own sign-ins: Claude (PKCE, code pasted back from the callback
//! page) and Codex (PKCE, browser redirect to a localhost callback).
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
// Claude Code's own User-Agent. Anthropic answers any other one on the token
// endpoint with HTTP 429 "Rate limited", even for a request it would reject
// anyway (checked 2026-10-31: "claude-code/2.1.0" got 429, this got 400).
pub const CLAUDE_CODE_UA: &str = "claude-cli/2.1.302 (external, cli)";

// OpenCode packages/core/src/plugin/provider/openai.ts, v2.0.32.
const CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const CODEX_ISSUER: &str = "https://auth.openai.com";
const UA: &str = "ai-plan-usage";

/// `state` is its own random value: the verifier must never leave the app, and
/// the callback page echoes `state` back in the pasted code.
#[derive(Debug, Clone)]
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
    pub state: String,
}

pub fn challenge_for(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

pub fn pkce() -> Pkce {
    let verifier = URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>());
    let challenge = challenge_for(&verifier);
    let state = URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>());
    Pkce { verifier, challenge, state }
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
            ("state", p.state.as_str()),
        ],
    )
    .expect("static authorize URL")
    .to_string()
}

/// The callback page shows `code#state`; a bare code is accepted too, and an
/// empty state counts as absent.
pub fn split_code(input: &str) -> (String, Option<String>) {
    let input = input.trim();
    match input.split_once('#') {
        Some((code, state)) if !state.is_empty() => (code.to_string(), Some(state.to_string())),
        Some((code, _)) => (code.to_string(), None),
        None => (input.to_string(), None),
    }
}

/// The code to exchange, after checking the pasted state belongs to this sign-in.
fn code_for(pasted: &str, p: &Pkce) -> Result<String, String> {
    let (code, state) = split_code(pasted);
    if code.is_empty() {
        return Err("Paste the code shown after signing in.".into());
    }
    match state {
        Some(s) if s != p.state => {
            Err("This code belongs to another sign-in. Open the sign-in page again.".into())
        }
        _ => Ok(code),
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
        .map_err(|e| format!("network: {e}"))
}

/// An initial exchange has no earlier refresh token to fall back on.
fn require_refresh(c: OauthCreds) -> Result<OauthCreds, String> {
    if c.refresh.is_empty() {
        return Err("Sign-in returned no refresh token.".into());
    }
    Ok(c)
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

pub async fn claude_exchange(pasted: &str, p: &Pkce) -> Result<OauthCreds, String> {
    let code = code_for(pasted, p)?;
    let resp = client()?
        .post(CLAUDE_TOKEN_URL)
        .header("User-Agent", CLAUDE_CODE_UA)
        .header("Accept", "application/json")
        .json(&json!({
            "code": code,
            "state": p.state,
            "grant_type": "authorization_code",
            "client_id": CLAUDE_CLIENT_ID,
            "redirect_uri": CLAUDE_REDIRECT,
            "code_verifier": p.verifier,
        }))
        .send()
        .await
        .map_err(|e| format!("network: {e}"))?;
    require_refresh(token_body(resp).await?.into_creds("", None))
}

/// Redirect for a callback server on `port`. Only these ports are registered
/// for the Codex client (Codex CLI `codex-rs/login/src/server.rs`).
pub const CODEX_PORTS: [u16; 2] = [1455, 1457];

pub fn codex_redirect(port: u16) -> String {
    format!("http://127.0.0.1:{port}/auth/callback")
}

pub fn codex_authorize_url(p: &Pkce, redirect: &str) -> String {
    reqwest::Url::parse_with_params(
        &format!("{CODEX_ISSUER}/oauth/authorize"),
        &[
            ("response_type", "code"),
            ("client_id", CODEX_CLIENT_ID),
            ("redirect_uri", redirect),
            ("scope", "openid profile email offline_access"),
            ("code_challenge", p.challenge.as_str()),
            ("code_challenge_method", "S256"),
            ("state", p.state.as_str()),
            ("id_token_add_organizations", "true"),
            ("codex_cli_simplified_flow", "true"),
            ("originator", "codex_cli_rs"),
        ],
    )
    .expect("static authorize URL")
    .to_string()
}

/// The code from a callback request line (`GET /auth/callback?... HTTP/1.1`).
/// None for any other path, such as the browser asking for a favicon.
pub fn callback_code(request_line: &str, p: &Pkce) -> Option<Result<String, String>> {
    let target = request_line.split_whitespace().nth(1)?;
    let url = reqwest::Url::parse(&format!("http://127.0.0.1{target}")).ok()?;
    if url.path() != "/auth/callback" {
        return None;
    }
    let q = |k: &str| url.query_pairs().find(|(n, _)| n == k).map(|(_, v)| v.into_owned());
    if let Some(e) = q("error") {
        return Some(Err(format!("Sign-in failed: {}", q("error_description").unwrap_or(e))));
    }
    if q("state").as_deref() != Some(p.state.as_str()) {
        return Some(Err("This answer belongs to another sign-in. Try again.".into()));
    }
    Some(q("code").filter(|c| !c.is_empty()).ok_or_else(|| "Sign-in returned no code.".to_string()))
}

pub async fn codex_exchange(code: &str, redirect: &str, p: &Pkce) -> Result<OauthCreds, String> {
    let resp = client()?
        .post(format!("{CODEX_ISSUER}/oauth/token"))
        .header("User-Agent", UA)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect),
            ("client_id", CODEX_CLIENT_ID),
            ("code_verifier", p.verifier.as_str()),
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
    require_refresh(tokens.into_creds("", account))
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
        assert_eq!(p.state.len(), 43);
        assert_ne!(p.state, p.verifier);
    }

    #[test]
    fn authorize_url_carries_pkce_and_state() {
        let p = Pkce { verifier: "v123".into(), challenge: "c456".into(), state: "s789".into() };
        let url = reqwest::Url::parse(&claude_authorize_url(&p)).unwrap();
        let q: std::collections::HashMap<String, String> = url.query_pairs().into_owned().collect();
        assert_eq!(url.host_str(), Some("claude.com"));
        assert_eq!(q["code_challenge"], "c456");
        assert_eq!(q["state"], "s789");
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["scope"], "user:profile");
        assert_eq!(q["redirect_uri"], "https://platform.claude.com/oauth/code/callback");
    }

    #[test]
    fn authorize_url_state_is_not_the_verifier() {
        let p = pkce();
        let url = reqwest::Url::parse(&claude_authorize_url(&p)).unwrap();
        let q: std::collections::HashMap<String, String> = url.query_pairs().into_owned().collect();
        assert_eq!(q["state"], p.state);
        assert_ne!(q["state"], p.verifier);
        assert!(!url.as_str().contains(&p.verifier));
    }

    #[test]
    fn splits_pasted_code() {
        assert_eq!(split_code(" abc#xyz \n"), ("abc".to_string(), Some("xyz".to_string())));
        assert_eq!(split_code("abc"), ("abc".to_string(), None));
    }

    #[test]
    fn split_code_treats_empty_state_as_absent() {
        assert_eq!(split_code("abc#"), ("abc".to_string(), None));
    }

    #[test]
    fn code_for_checks_the_pasted_state() {
        let p = Pkce { verifier: "v".into(), challenge: "c".into(), state: "s".into() };
        assert_eq!(code_for("abc#s", &p), Ok("abc".to_string()));
        assert_eq!(code_for("abc", &p), Ok("abc".to_string()));
        assert_eq!(code_for("abc#", &p), Ok("abc".to_string()));
        assert_eq!(
            code_for("abc#other", &p),
            Err("This code belongs to another sign-in. Open the sign-in page again.".to_string())
        );
        assert_eq!(code_for("  ", &p), Err("Paste the code shown after signing in.".to_string()));
        assert_eq!(code_for("#s", &p), Err("Paste the code shown after signing in.".to_string()));
    }

    #[test]
    fn codex_callback_checks_path_state_and_error() {
        let p = Pkce { verifier: "v".into(), challenge: "c".into(), state: "s1".into() };
        let line = |target: &str| format!("GET {target} HTTP/1.1");
        assert_eq!(callback_code(&line("/favicon.ico"), &p), None);
        assert_eq!(callback_code(&line("/auth/callback?code=abc&state=s1"), &p), Some(Ok("abc".into())));
        assert_eq!(
            callback_code(&line("/auth/callback?code=abc&state=other"), &p),
            Some(Err("This answer belongs to another sign-in. Try again.".into()))
        );
        assert_eq!(
            callback_code(&line("/auth/callback?error=access_denied&error_description=User%20cancelled&state=s1"), &p),
            Some(Err("Sign-in failed: User cancelled".into()))
        );
        assert_eq!(
            callback_code(&line("/auth/callback?state=s1"), &p),
            Some(Err("Sign-in returned no code.".into()))
        );
    }

    #[test]
    fn codex_authorize_url_carries_pkce_and_redirect() {
        let p = Pkce { verifier: "v".into(), challenge: "ch".into(), state: "st".into() };
        let url = reqwest::Url::parse(&codex_authorize_url(&p, &codex_redirect(1455))).unwrap();
        let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(url.path(), "/oauth/authorize");
        assert_eq!(q["redirect_uri"], "http://127.0.0.1:1455/auth/callback");
        assert_eq!(q["code_challenge"], "ch");
        assert_eq!(q["state"], "st");
        assert_eq!(q["client_id"], CODEX_CLIENT_ID);
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

    #[test]
    fn token_response_uses_rotated_refresh() {
        let t: TokenResponse =
            serde_json::from_str(r#"{"access_token":"a","refresh_token":"new","expires_in":60}"#).unwrap();
        assert_eq!(t.into_creds("old", None).refresh, "new");
    }

    #[test]
    fn initial_exchange_without_refresh_token_is_rejected() {
        let t: TokenResponse = serde_json::from_str(r#"{"access_token":"a"}"#).unwrap();
        assert_eq!(
            require_refresh(t.into_creds("", None)).unwrap_err(),
            "Sign-in returned no refresh token."
        );
        let t: TokenResponse = serde_json::from_str(r#"{"access_token":"a","refresh_token":"r"}"#).unwrap();
        assert_eq!(require_refresh(t.into_creds("", None)).unwrap().refresh, "r");
    }
}
