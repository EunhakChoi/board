//! One-time invite and sign-in links.

use board_engine::{Invites, NewInvite, StoreError};
use board_model::Invite;
use rusqlite::{OptionalExtension, Row, params};

use crate::{Db, err, parse, text};

const FIELDS: &str =
    "id, role, user_id, label, created_by, created_at, expires_at, used_at, used_by";

fn row(row: &Row<'_>) -> rusqlite::Result<Invite> {
    let expires: i64 = row.get(6)?;
    Ok(Invite {
        id: row.get(0)?,
        role: parse(row, 1)?,
        user: row.get(2)?,
        label: row.get(3)?,
        created_by: row.get(4)?,
        created_at: row.get(5)?,
        expires_at: u64::try_from(expires).unwrap_or(0),
        used_at: row.get(7)?,
        used_by: row.get(8)?,
    })
}

impl Invites for Db {
    fn insert_invite(&self, invite: &NewInvite) -> Result<Invite, StoreError> {
        self.lock()?
            .query_row(
                &format!(
                    "INSERT INTO invites (id, token_hash, role, user_id, label, created_by, expires_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) RETURNING {FIELDS}"
                ),
                params![
                    invite.id,
                    invite.token_hash,
                    text(invite.role),
                    invite.user,
                    invite.label,
                    invite.created_by,
                    i64::try_from(invite.expires_at).unwrap_or(i64::MAX)
                ],
                row,
            )
            .map_err(err)
    }

    fn invite_by_token(&self, token_hash: &str) -> Result<Option<Invite>, StoreError> {
        self.lock()?
            .query_row(
                &format!("SELECT {FIELDS} FROM invites WHERE token_hash = ?1"),
                params![token_hash],
                row,
            )
            .optional()
            .map_err(err)
    }

    fn use_invite(&self, id: &str, user: &str) -> Result<bool, StoreError> {
        let changed = self
            .lock()?
            .execute(
                "UPDATE invites SET used_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), used_by = ?2
                 WHERE id = ?1 AND used_at IS NULL",
                params![id, user],
            )
            .map_err(err)?;
        Ok(changed > 0)
    }

    fn invites(&self) -> Result<Vec<Invite>, StoreError> {
        let conn = self.lock()?;
        let mut statement = conn
            .prepare(&format!(
                "SELECT {FIELDS} FROM invites WHERE user_id IS NULL ORDER BY created_at DESC, id"
            ))
            .map_err(err)?;
        let rows = statement.query_map([], row).map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)
    }

    fn delete_invite(&self, id: &str) -> Result<bool, StoreError> {
        let deleted = self
            .lock()?
            .execute("DELETE FROM invites WHERE id = ?1", params![id])
            .map_err(err)?;
        Ok(deleted > 0)
    }
}

#[cfg(test)]
mod tests {
    use board_model::Role;

    use super::*;

    #[test]
    fn invites_are_used_once() {
        let db = Db::memory().unwrap();
        let invite = db
            .insert_invite(&NewInvite {
                id: "i1".into(),
                token_hash: "h1".into(),
                role: Role::Commenter,
                user: None,
                label: "Dana".into(),
                created_by: "Boss".into(),
                expires_at: 2_000_000_000,
            })
            .unwrap();
        assert_eq!(
            (invite.role, invite.expires_at),
            (Role::Commenter, 2_000_000_000)
        );
        assert_eq!(db.invite_by_token("h1").unwrap(), Some(invite));
        assert!(db.use_invite("i1", "u1").unwrap());
        assert!(!db.use_invite("i1", "u2").unwrap());
        assert_eq!(db.invites().unwrap()[0].used_by.as_deref(), Some("u1"));
        assert!(db.delete_invite("i1").unwrap());
        assert_eq!(db.invite_by_token("h1").unwrap(), None);
    }
}
