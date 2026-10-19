//! Accounts, sessions, the setup token and the allowlist.

use board_engine::{Accounts, AllowRule, NewUser, StoreError};
use board_model::{Role, User, UserStatus};
use rusqlite::{OptionalExtension, Row, params};

use crate::{Db, err, parse, text};

const USER_FIELDS: &str = "id, email, name, avatar_url, role, status, created_at";

fn user_row(row: &Row<'_>) -> rusqlite::Result<User> {
    Ok(User {
        id: row.get(0)?,
        email: row.get(1)?,
        name: row.get(2)?,
        avatar_url: row.get(3)?,
        role: parse(row, 4)?,
        status: parse(row, 5)?,
        created_at: row.get(6)?,
    })
}

impl Accounts for Db {
    fn user(&self, id: &str) -> Result<Option<User>, StoreError> {
        self.lock()?
            .query_row(
                &format!("SELECT {USER_FIELDS} FROM users WHERE id = ?1"),
                params![id],
                user_row,
            )
            .optional()
            .map_err(err)
    }

    fn user_by_subject(&self, provider: &str, subject: &str) -> Result<Option<User>, StoreError> {
        self.lock()?
            .query_row(
                &format!("SELECT {USER_FIELDS} FROM users WHERE provider = ?1 AND subject = ?2"),
                params![provider, subject],
                user_row,
            )
            .optional()
            .map_err(err)
    }

    fn insert_user(&self, user: &NewUser) -> Result<User, StoreError> {
        self.lock()?
            .query_row(
                &format!(
                    "INSERT INTO users (id, provider, subject, email, name, avatar_url, role, status)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) RETURNING {USER_FIELDS}"
                ),
                params![
                    user.id,
                    user.provider,
                    user.subject,
                    user.email,
                    user.name,
                    user.avatar_url,
                    text(user.role),
                    text(user.status)
                ],
                user_row,
            )
            .map_err(err)
    }

    fn update_profile(
        &self,
        id: &str,
        email: &str,
        name: &str,
        avatar_url: Option<&str>,
    ) -> Result<(), StoreError> {
        self.lock()?
            .execute(
                "UPDATE users SET email = ?2, name = ?3, avatar_url = ?4,
                   updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
                params![id, email, name, avatar_url],
            )
            .map(drop)
            .map_err(err)
    }

    fn set_access(&self, id: &str, role: Role, status: UserStatus) -> Result<(), StoreError> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE users SET role = ?2, status = ?3,
               updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
            params![id, text(role), text(status)],
        )
        .map_err(err)?;
        if status != UserStatus::Active && status != UserStatus::Pending {
            // Turned down or removed: every open session ends now.
            conn.execute("DELETE FROM sessions WHERE user_id = ?1", params![id])
                .map_err(err)?;
        }
        Ok(())
    }

    fn has_admin(&self) -> Result<bool, StoreError> {
        self.lock()?
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM users WHERE role = 'admin' AND status = 'active')",
                [],
                |row| row.get(0),
            )
            .map_err(err)
    }

    fn insert_session(
        &self,
        token_hash: &str,
        user: &str,
        expires_at: u64,
    ) -> Result<(), StoreError> {
        let expires_at = i64::try_from(expires_at).map_err(err)?;
        let conn = self.lock()?;
        // Expired sessions are swept whenever a new one starts.
        conn.execute(
            "DELETE FROM sessions WHERE expires_at <= CAST(strftime('%s', 'now') AS INTEGER)",
            [],
        )
        .map_err(err)?;
        conn.execute(
            "INSERT INTO sessions (token_hash, user_id, expires_at) VALUES (?1, ?2, ?3)",
            params![token_hash, user, expires_at],
        )
        .map(drop)
        .map_err(err)
    }

    fn session_user(&self, token_hash: &str, now: u64) -> Result<Option<User>, StoreError> {
        let now = i64::try_from(now).map_err(err)?;
        let fields = USER_FIELDS
            .split(", ")
            .map(|f| format!("u.{f}"))
            .collect::<Vec<_>>()
            .join(", ");
        self.lock()?
            .query_row(
                &format!(
                    "SELECT {fields} FROM sessions s JOIN users u ON u.id = s.user_id
                     WHERE s.token_hash = ?1 AND s.expires_at > ?2"
                ),
                params![token_hash, now],
                user_row,
            )
            .optional()
            .map_err(err)
    }

    fn delete_session(&self, token_hash: &str) -> Result<(), StoreError> {
        self.lock()?
            .execute(
                "DELETE FROM sessions WHERE token_hash = ?1",
                params![token_hash],
            )
            .map(drop)
            .map_err(err)
    }

    fn set_setup_token(&self, token_hash: &str) -> Result<(), StoreError> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM setup_tokens WHERE used_at IS NULL", [])
            .map_err(err)?;
        conn.execute(
            "INSERT INTO setup_tokens (token_hash) VALUES (?1)",
            params![token_hash],
        )
        .map(drop)
        .map_err(err)
    }

    fn take_setup_token(&self, token_hash: &str) -> Result<bool, StoreError> {
        let used = self
            .lock()?
            .execute(
                "UPDATE setup_tokens SET used_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE token_hash = ?1 AND used_at IS NULL",
                params![token_hash],
            )
            .map_err(err)?;
        Ok(used == 1)
    }

    fn allow_rules(&self) -> Result<Vec<AllowRule>, StoreError> {
        let conn = self.lock()?;
        let mut statement = conn
            .prepare("SELECT id, kind, value FROM allow_rules ORDER BY kind, value")
            .map_err(err)?;
        let rows = statement
            .query_map([], |row| {
                Ok(AllowRule {
                    id: row.get(0)?,
                    kind: row.get(1)?,
                    value: row.get(2)?,
                })
            })
            .map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)
    }

    fn remove_allow_rule(&self, id: i64) -> Result<bool, StoreError> {
        let removed = self
            .lock()?
            .execute("DELETE FROM allow_rules WHERE id = ?1", params![id])
            .map_err(err)?;
        Ok(removed == 1)
    }

    fn users(&self) -> Result<Vec<User>, StoreError> {
        let conn = self.lock()?;
        let mut statement = conn
            .prepare(&format!(
                "SELECT {USER_FIELDS} FROM users ORDER BY created_at, id"
            ))
            .map_err(err)?;
        let rows = statement.query_map([], user_row).map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)
    }

    fn add_allow_rule(&self, kind: &str, value: &str) -> Result<AllowRule, StoreError> {
        self.lock()?
            .query_row(
                "INSERT INTO allow_rules (kind, value) VALUES (?1, lower(?2))
                 ON CONFLICT (kind, value) DO UPDATE SET value = excluded.value
                 RETURNING id, kind, value",
                params![kind, value.trim()],
                |row| {
                    Ok(AllowRule {
                        id: row.get(0)?,
                        kind: row.get(1)?,
                        value: row.get(2)?,
                    })
                },
            )
            .map_err(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_user(id: &str, subject: &str, role: Role, status: UserStatus) -> NewUser {
        NewUser {
            id: id.into(),
            provider: "google".into(),
            subject: subject.into(),
            email: format!("{subject}@example.com"),
            name: subject.into(),
            avatar_url: None,
            role,
            status,
        }
    }

    #[test]
    fn users_sessions_and_removal() {
        let db = Db::memory().unwrap();
        assert!(!db.has_admin().unwrap());
        let ann = db
            .insert_user(&new_user("u1", "ann", Role::Admin, UserStatus::Active))
            .unwrap();
        assert!(db.has_admin().unwrap());
        assert_eq!(
            db.user_by_subject("google", "ann").unwrap(),
            Some(ann.clone())
        );
        db.update_profile("u1", "ann@new.example", "Ann", Some("https://pic"))
            .unwrap();
        assert_eq!(db.user("u1").unwrap().unwrap().name, "Ann");

        db.insert_session("h1", "u1", 2_000_000_000).unwrap();
        db.insert_session("h0", "u1", 1).unwrap();
        db.insert_session("h2", "u1", 2_000_000_000).unwrap();
        assert!(
            db.session_user("h1", 1_000).unwrap().is_some(),
            "live sessions survive the sweep"
        );
        assert_eq!(
            db.session_user("h0", 0).unwrap(),
            None,
            "expired ones are swept"
        );
        assert_eq!(db.session_user("h1", 1_000).unwrap().unwrap().id, "u1");
        assert_eq!(db.session_user("h1", 2_000_000_001).unwrap(), None);
        db.set_access("u1", Role::Admin, UserStatus::Removed)
            .unwrap();
        assert_eq!(
            db.session_user("h1", 1_000).unwrap(),
            None,
            "removal ends sessions"
        );
        assert!(!db.has_admin().unwrap());
    }

    #[test]
    fn setup_token_works_once() {
        let db = Db::memory().unwrap();
        db.set_setup_token("old").unwrap();
        db.set_setup_token("new").unwrap();
        assert!(!db.take_setup_token("old").unwrap(), "replaced");
        assert!(db.take_setup_token("new").unwrap());
        assert!(!db.take_setup_token("new").unwrap(), "used up");
    }

    #[test]
    fn allow_rules_are_lowercase_and_unique() {
        let db = Db::memory().unwrap();
        db.add_allow_rule("domain", "Example.COM").unwrap();
        db.add_allow_rule("domain", "example.com").unwrap();
        let rules = db.allow_rules().unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].value, "example.com");
        assert!(db.remove_allow_rule(rules[0].id).unwrap());
        assert!(!db.remove_allow_rule(rules[0].id).unwrap());
    }
}
