//! Members, their tokens and browser sessions.

use charet_engine::{Accounts, NewUser, SessionRow, StoreError};
use charet_model::{Role, User};
use rusqlite::{OptionalExtension, Row, params};

use crate::{Db, err, parse, text};

const USER_FIELDS: &str = "u.id, u.name, u.role, u.created_at, u.last_used_at";

fn user_row(row: &Row<'_>) -> rusqlite::Result<User> {
    Ok(User {
        id: row.get(0)?,
        name: row.get(1)?,
        role: parse(row, 2)?,
        created_at: row.get(3)?,
        last_used_at: row.get(4)?,
    })
}

impl Accounts for Db {
    fn user(&self, id: &str) -> Result<Option<User>, StoreError> {
        self.lock()?
            .query_row(
                &format!("SELECT {USER_FIELDS} FROM users u WHERE u.id = ?1"),
                params![id],
                user_row,
            )
            .optional()
            .map_err(err)
    }

    fn users(&self) -> Result<Vec<User>, StoreError> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {USER_FIELDS} FROM users u ORDER BY u.created_at, u.rowid"
            ))
            .map_err(err)?;
        stmt.query_map([], user_row)
            .map_err(err)?
            .collect::<Result<_, _>>()
            .map_err(err)
    }

    fn insert_user(&self, user: &NewUser) -> Result<User, StoreError> {
        self.lock()?
            .execute(
                "INSERT INTO users (id, name, role, token) VALUES (?1, ?2, ?3, ?4)",
                params![user.id, user.name, text(user.role), user.token],
            )
            .map_err(err)?;
        self.user(&user.id)?
            .ok_or_else(|| StoreError("member vanished after insert".into()))
    }

    fn update_user(&self, id: &str, name: &str, role: Role) -> Result<bool, StoreError> {
        let changed = self
            .lock()?
            .execute(
                "UPDATE users SET name = ?2, role = ?3 WHERE id = ?1",
                params![id, name, text(role)],
            )
            .map_err(err)?;
        Ok(changed > 0)
    }

    fn delete_user(&self, id: &str) -> Result<bool, StoreError> {
        let changed = self
            .lock()?
            .execute("DELETE FROM users WHERE id = ?1", params![id])
            .map_err(err)?;
        Ok(changed > 0)
    }

    fn user_token(&self, id: &str) -> Result<Option<String>, StoreError> {
        self.lock()?
            .query_row(
                "SELECT token FROM users WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()
            .map_err(err)
    }

    fn set_user_token(&self, id: &str, token: &str) -> Result<bool, StoreError> {
        let changed = self
            .lock()?
            .execute(
                "UPDATE users SET token = ?2 WHERE id = ?1",
                params![id, token],
            )
            .map_err(err)?;
        Ok(changed > 0)
    }

    fn user_by_token(&self, token: &str) -> Result<Option<User>, StoreError> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE users SET last_used_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE token = ?1",
            params![token],
        )
        .map_err(err)?;
        conn.query_row(
            &format!("SELECT {USER_FIELDS} FROM users u WHERE u.token = ?1"),
            params![token],
            user_row,
        )
        .optional()
        .map_err(err)
    }

    fn insert_session(
        &self,
        token_hash: &str,
        user: &str,
        expires_at: u64,
    ) -> Result<(), StoreError> {
        let expires = i64::try_from(expires_at).map_err(err)?;
        self.lock()?
            .execute(
                "INSERT INTO sessions (token_hash, user_id, expires_at) VALUES (?1, ?2, ?3)",
                params![token_hash, user, expires],
            )
            .map_err(err)?;
        Ok(())
    }

    fn session(&self, token_hash: &str, now: u64) -> Result<Option<SessionRow>, StoreError> {
        let now = i64::try_from(now).map_err(err)?;
        self.lock()?
            .query_row(
                &format!(
                    "SELECT {USER_FIELDS}, s.replaced FROM sessions s
                     JOIN users u ON u.id = s.user_id
                     WHERE s.token_hash = ?1 AND s.expires_at > ?2"
                ),
                params![token_hash, now],
                |row| {
                    Ok(SessionRow {
                        user: user_row(row)?,
                        replaced: row.get::<_, i64>(5)? != 0,
                    })
                },
            )
            .optional()
            .map_err(err)
    }

    fn replace_sessions(&self, user: &str, except_hash: &str) -> Result<(), StoreError> {
        self.lock()?
            .execute(
                "UPDATE sessions SET replaced = 1 WHERE user_id = ?1 AND token_hash != ?2",
                params![user, except_hash],
            )
            .map_err(err)?;
        Ok(())
    }

    fn delete_session(&self, token_hash: &str) -> Result<(), StoreError> {
        self.lock()?
            .execute(
                "DELETE FROM sessions WHERE token_hash = ?1",
                params![token_hash],
            )
            .map_err(err)?;
        Ok(())
    }

    fn delete_sessions(&self, user: &str) -> Result<(), StoreError> {
        self.lock()?
            .execute("DELETE FROM sessions WHERE user_id = ?1", params![user])
            .map_err(err)?;
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn new_user(id: &str, role: Role) -> NewUser {
        NewUser {
            id: id.into(),
            name: format!("{id} name"),
            role,
            token: format!("tok-{id}"),
        }
    }

    #[test]
    fn members_tokens_and_removal() {
        let db = Db::memory().unwrap();
        let ann = db.insert_user(&new_user("u1", Role::Editor)).unwrap();
        assert_eq!((ann.name.as_str(), ann.role), ("u1 name", Role::Editor));
        assert_eq!(ann.last_used_at, None);
        db.insert_user(&new_user("u2", Role::Admin)).unwrap();
        assert!(
            db.insert_user(&NewUser {
                id: "u3".into(),
                ..new_user("u1", Role::Viewer)
            })
            .is_err(),
            "tokens are unique"
        );
        assert_eq!(
            db.users()
                .unwrap()
                .iter()
                .map(|u| u.id.as_str())
                .collect::<Vec<_>>(),
            ["u1", "u2"]
        );

        let used = db.user_by_token("tok-u1").unwrap().unwrap();
        assert_eq!(used.id, "u1");
        assert!(used.last_used_at.is_some());
        assert_eq!(db.user_by_token("nope").unwrap(), None);

        assert!(db.update_user("u1", "Ann", Role::Commenter).unwrap());
        assert!(!db.update_user("zz", "x", Role::Viewer).unwrap());
        assert_eq!(db.user("u1").unwrap().unwrap().name, "Ann");

        assert!(db.set_user_token("u1", "fresh").unwrap());
        assert_eq!(db.user_token("u1").unwrap().as_deref(), Some("fresh"));
        assert_eq!(db.user_by_token("tok-u1").unwrap(), None);

        assert!(db.delete_user("u1").unwrap());
        assert!(!db.delete_user("u1").unwrap());
        assert_eq!(db.user("u1").unwrap(), None);
    }

    #[test]
    fn sessions_expire_get_replaced_and_go_with_the_member() {
        let db = Db::memory().unwrap();
        db.insert_user(&new_user("u1", Role::Editor)).unwrap();
        db.insert_session("h1", "u1", 100).unwrap();
        db.insert_session("h2", "u1", 100).unwrap();
        let row = db.session("h1", 50).unwrap().unwrap();
        assert_eq!((row.user.id.as_str(), row.replaced), ("u1", false));
        assert_eq!(db.session("h1", 100).unwrap(), None, "expired");

        db.replace_sessions("u1", "h2").unwrap();
        assert!(db.session("h1", 50).unwrap().unwrap().replaced);
        assert!(!db.session("h2", 50).unwrap().unwrap().replaced);

        db.delete_session("h1").unwrap();
        assert_eq!(db.session("h1", 50).unwrap(), None);
        db.delete_sessions("u1").unwrap();
        assert_eq!(db.session("h2", 50).unwrap(), None);
        db.insert_session("h3", "u1", 100).unwrap();
        db.delete_user("u1").unwrap();
        assert_eq!(db.session("h3", 50).unwrap(), None);
    }

    #[test]
    fn project_members_and_visibility() {
        use charet_engine::Records;
        let db = Db::memory().unwrap();
        db.insert_project(&charet_model::Project {
            key: "web".into(),
            title: "Web".into(),
            visibility: charet_model::Visibility::Team,
            visitors: None,
            visitor_access: charet_model::VisitorAccess::View,
            origin: charet_model::Origin::Empty,
            created_at: String::new(),
        })
        .unwrap();
        db.insert_user(&new_user("u1", Role::Editor)).unwrap();
        db.update_project("web", "Web app", charet_model::Visibility::Private)
            .unwrap();
        let project = db.project("web").unwrap().unwrap();
        assert_eq!(
            (project.title.as_str(), project.visibility),
            ("Web app", charet_model::Visibility::Private)
        );
        db.set_member("web", "u1", true).unwrap();
        db.set_member("web", "u1", true).unwrap();
        assert_eq!(db.members("web").unwrap(), ["u1"]);
        assert!(db.is_member("web", "u1").unwrap());
        db.set_member("web", "u1", false).unwrap();
        assert!(!db.is_member("web", "u1").unwrap());
    }
}
