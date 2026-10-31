//! Logins from OpenCode's store (`opencode.db`, table `credential`), opened
//! read only. GitHub device-flow tokens do not expire, so a Copilot copy is
//! safe. A copied Codex login refreshes on its own, and a refresh spends the
//! token OpenCode holds too: whichever side refreshes second gets signed out.
use crate::accounts::OauthCreds;
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct CopilotLogin {
    pub id: String,
    pub label: String,
    pub token: String,
}

fn db_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".local")
        .join("share")
        .join("opencode")
        .join("opencode.db")
}

pub fn read_copilot(conn: &Connection) -> rusqlite::Result<Vec<CopilotLogin>> {
    let mut stmt = conn.prepare(
        "SELECT id, label, value FROM credential \
         WHERE integration_id = 'github-copilot' ORDER BY time_created",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<String>>(2)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, label, value) = row?;
        // NULL, non-JSON or token-less rows are skipped.
        let token = value
            .and_then(|v| serde_json::from_str::<Value>(&v).ok())
            .and_then(|v| v.get("access").and_then(Value::as_str).map(str::to_string))
            .filter(|t| !t.is_empty());
        let Some(token) = token else { continue };
        let label = label.unwrap_or_default();
        let label = label.strip_prefix("Copilot - ").unwrap_or(&label).trim();
        out.push(CopilotLogin {
            id,
            label: if label.is_empty() { "OpenCode".into() } else { label.to_string() },
            token,
        });
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodexLogin {
    pub id: String,
    pub label: String,
    pub creds: OauthCreds,
}

pub fn read_codex(conn: &Connection) -> rusqlite::Result<Vec<CodexLogin>> {
    let mut stmt = conn.prepare(
        "SELECT id, label, value FROM credential          WHERE integration_id = 'openai' ORDER BY time_created",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<String>>(2)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, label, value) = row?;
        // NULL, non-JSON, API-key or token-less rows are skipped.
        let Some(v) = value.and_then(|v| serde_json::from_str::<Value>(&v).ok()) else { continue };
        let text = |k: &str| v.get(k).and_then(Value::as_str).filter(|t| !t.is_empty()).map(str::to_string);
        let Some(access) = text("access") else { continue };
        let label = label.unwrap_or_default();
        let label = label.trim();
        out.push(CodexLogin {
            id,
            // OpenCode names its ChatGPT login just "OAuth".
            label: if label.is_empty() || label == "OAuth" { "Codex".into() } else { label.to_string() },
            creds: OauthCreds {
                access,
                refresh: text("refresh").unwrap_or_default(),
                expires_at: v.get("expires").and_then(Value::as_i64).unwrap_or(0),
                account_id: v
                    .pointer("/metadata/accountID")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            },
        });
    }
    Ok(out)
}

fn open() -> Result<Connection, String> {
    let path = db_path();
    if !path.exists() {
        return Err(format!("OpenCode store not found at {}", path.display()));
    }
    Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e| e.to_string())
}

pub fn list() -> Result<Vec<CopilotLogin>, String> {
    read_copilot(&open()?).map_err(|e| e.to_string())
}

pub fn codex_logins() -> Result<Vec<CodexLogin>, String> {
    read_codex(&open()?).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_copilot_rows_only() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            r#"
            CREATE TABLE credential (id TEXT, integration_id TEXT, label TEXT, value TEXT, time_created INTEGER);
            INSERT INTO credential VALUES ('c2','github-copilot','Copilot - Tecnomat','{"type":"oauth","access":"gho_b","refresh":"gho_b","expires":0}',2);
            INSERT INTO credential VALUES ('c1','github-copilot','Copilot - BIP','{"type":"oauth","access":"gho_a","refresh":"gho_a","expires":0}',1);
            INSERT INTO credential VALUES ('c3','openai','OAuth','{"type":"oauth","access":"x"}',3);
            INSERT INTO credential VALUES ('c4','github-copilot',NULL,'{"type":"oauth"}',4);
            INSERT INTO credential VALUES ('c5','github-copilot',NULL,'{"type":"oauth","access":"gho_c"}',5);
            INSERT INTO credential VALUES ('c6','github-copilot','Copilot - Null',NULL,6);
            "#,
        )
        .unwrap();
        assert_eq!(
            read_copilot(&c).unwrap(),
            vec![
                CopilotLogin { id: "c1".into(), label: "BIP".into(), token: "gho_a".into() },
                CopilotLogin { id: "c2".into(), label: "Tecnomat".into(), token: "gho_b".into() },
                CopilotLogin { id: "c5".into(), label: "OpenCode".into(), token: "gho_c".into() },
            ]
        );
    }

    #[test]
    fn reads_codex_rows_only() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            r#"
            CREATE TABLE credential (id TEXT, integration_id TEXT, label TEXT, value TEXT, time_created INTEGER);
            INSERT INTO credential VALUES ('o1','openai','OAuth','{"type":"oauth","access":"at","refresh":"rt","expires":1700,"metadata":{"accountID":"acc-1"}}',1);
            INSERT INTO credential VALUES ('o2','openai','API key','{"type":"api","key":"sk"}',2);
            INSERT INTO credential VALUES ('o3','openai',NULL,NULL,3);
            INSERT INTO credential VALUES ('g1','github-copilot','Copilot - BIP','{"type":"oauth","access":"gho_a"}',4);
            "#,
        )
        .unwrap();
        let logins = read_codex(&c).unwrap();
        assert_eq!(logins.len(), 1);
        assert_eq!(logins[0].id, "o1");
        assert_eq!(logins[0].label, "Codex");
        assert_eq!(logins[0].creds.access, "at");
        assert_eq!(logins[0].creds.refresh, "rt");
        assert_eq!(logins[0].creds.expires_at, 1700);
        assert_eq!(logins[0].creds.account_id.as_deref(), Some("acc-1"));
    }
}
