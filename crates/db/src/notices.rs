//! Inbox notifications and webhooks.

use charet_engine::{NewNotification, NewWebhook, Notices, StoreError};
use charet_model::{Notification, Webhook};
use rusqlite::{Row, params, params_from_iter};

use crate::{Db, err};

const NOTIFICATION_FIELDS: &str = "id, kind, project, actor, text, link, data, created_at, read_at";

fn notification_row(row: &Row<'_>) -> rusqlite::Result<Notification> {
    let id: i64 = row.get(0)?;
    let data: String = row.get(6)?;
    Ok(Notification {
        id: u64::try_from(id).unwrap_or(0),
        kind: row.get(1)?,
        project: row.get(2)?,
        actor: row.get(3)?,
        text: row.get(4)?,
        link: row.get(5)?,
        data: serde_json::from_str(&data).unwrap_or_default(),
        created_at: row.get(7)?,
        read_at: row.get(8)?,
    })
}

const WEBHOOK_FIELDS: &str = "id, url, events, project, secret_last4, created_by, created_at, \
     last_status, last_error, last_delivery_at, salt";

fn webhook_row(row: &Row<'_>) -> rusqlite::Result<(Webhook, String)> {
    let events: String = row.get(2)?;
    let status: Option<i64> = row.get(7)?;
    Ok((
        Webhook {
            id: row.get(0)?,
            url: row.get(1)?,
            events: serde_json::from_str(&events).unwrap_or_default(),
            project: row.get(3)?,
            secret_last4: row.get(4)?,
            created_by: row.get(5)?,
            created_at: row.get(6)?,
            last_status: status.and_then(|s| u16::try_from(s).ok()),
            last_error: row.get(8)?,
            last_delivery_at: row.get(9)?,
        },
        row.get(10)?,
    ))
}

impl Notices for Db {
    fn insert_notifications(&self, rows: &[NewNotification]) -> Result<(), StoreError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction().map_err(err)?;
        for n in rows {
            tx.execute(
                "INSERT INTO notifications (user_id, kind, project, actor, text, link, data)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    n.user,
                    n.kind,
                    n.project,
                    n.actor,
                    n.text,
                    n.link,
                    n.data.to_string()
                ],
            )
            .map_err(err)?;
        }
        tx.commit().map_err(err)
    }

    fn notifications(
        &self,
        user: &str,
        unread_only: bool,
        limit: u32,
    ) -> Result<Vec<Notification>, StoreError> {
        let conn = self.lock()?;
        let mut statement = conn
            .prepare(&format!(
                "SELECT {NOTIFICATION_FIELDS} FROM notifications
                 WHERE user_id = ?1 AND (?2 = 0 OR read_at IS NULL)
                 ORDER BY id DESC LIMIT ?3"
            ))
            .map_err(err)?;
        let rows = statement
            .query_map(params![user, unread_only, limit], notification_row)
            .map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)
    }

    fn unread_count(&self, user: &str) -> Result<u32, StoreError> {
        self.lock()?
            .query_row(
                "SELECT COUNT(*) FROM notifications WHERE user_id = ?1 AND read_at IS NULL",
                params![user],
                |row| row.get(0),
            )
            .map_err(err)
    }

    fn mark_read(&self, user: &str, ids: Option<&[u64]>) -> Result<(), StoreError> {
        let conn = self.lock()?;
        let set = "UPDATE notifications SET read_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                   WHERE user_id = ?1 AND read_at IS NULL";
        match ids {
            None => conn.execute(set, params![user]),
            Some([]) => return Ok(()),
            Some(ids) => {
                let marks = vec!["?"; ids.len()].join(", ");
                let values = std::iter::once(user.to_owned()).chain(ids.iter().map(u64::to_string));
                conn.execute(
                    &format!("{set} AND CAST(id AS TEXT) IN ({marks})"),
                    params_from_iter(values),
                )
            }
        }
        .map(|_| ())
        .map_err(err)
    }

    fn webhooks(&self) -> Result<Vec<(Webhook, String)>, StoreError> {
        let conn = self.lock()?;
        let mut statement = conn
            .prepare(&format!(
                "SELECT {WEBHOOK_FIELDS} FROM webhooks ORDER BY created_at, id"
            ))
            .map_err(err)?;
        let rows = statement.query_map([], webhook_row).map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)
    }

    fn insert_webhook(&self, hook: &NewWebhook) -> Result<Webhook, StoreError> {
        self.lock()?
            .query_row(
                &format!(
                    "INSERT INTO webhooks (id, url, events, project, salt, secret_last4, created_by)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) RETURNING {WEBHOOK_FIELDS}"
                ),
                params![
                    hook.id,
                    hook.url,
                    serde_json::to_string(&hook.events).map_err(err)?,
                    hook.project,
                    hook.salt,
                    hook.secret_last4,
                    hook.created_by
                ],
                webhook_row,
            )
            .map(|(hook, _)| hook)
            .map_err(err)
    }

    fn delete_webhook(&self, id: &str) -> Result<bool, StoreError> {
        let deleted = self
            .lock()?
            .execute("DELETE FROM webhooks WHERE id = ?1", params![id])
            .map_err(err)?;
        Ok(deleted > 0)
    }

    fn set_webhook_salt(&self, id: &str, salt: &str, last4: &str) -> Result<bool, StoreError> {
        let changed = self
            .lock()?
            .execute(
                "UPDATE webhooks SET salt = ?2, secret_last4 = ?3 WHERE id = ?1",
                params![id, salt, last4],
            )
            .map_err(err)?;
        Ok(changed > 0)
    }

    fn record_delivery(
        &self,
        id: &str,
        status: Option<u16>,
        error: Option<&str>,
    ) -> Result<(), StoreError> {
        self.lock()?
            .execute(
                "UPDATE webhooks SET last_status = ?2, last_error = ?3,
                   last_delivery_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE id = ?1",
                params![id, status, error],
            )
            .map(|_| ())
            .map_err(err)
    }
}

#[cfg(test)]
mod tests {
    use charet_engine::Accounts;
    use charet_model::Role;

    use super::*;

    fn note(user: &str, text: &str) -> NewNotification {
        NewNotification {
            user: user.into(),
            kind: "comment.added".into(),
            project: None,
            actor: "Ann".into(),
            text: text.into(),
            link: "/p/web".into(),
            data: serde_json::json!({ "comment": 1 }),
        }
    }

    #[test]
    fn notifications_newest_first_and_marked_read() {
        let db = Db::memory().unwrap();
        db.insert_user(&crate::accounts::tests::new_user("u1", Role::Editor))
            .unwrap();
        db.insert_notifications(&[note("u1", "one"), note("u1", "two"), note("u1", "three")])
            .unwrap();
        let all = db.notifications("u1", false, 10).unwrap();
        assert_eq!(
            all.iter().map(|n| n.text.as_str()).collect::<Vec<_>>(),
            ["three", "two", "one"]
        );
        assert_eq!(all[0].data["comment"], 1);
        db.mark_read("u1", Some(&[all[0].id])).unwrap();
        assert_eq!(db.unread_count("u1").unwrap(), 2);
        assert_eq!(db.notifications("u1", true, 1).unwrap()[0].text, "two");
        db.mark_read("u1", None).unwrap();
        assert_eq!(db.unread_count("u1").unwrap(), 0);
        assert_eq!(db.notifications("u2", false, 10).unwrap(), vec![]);
    }

    #[test]
    fn webhooks_keep_their_salt_and_last_delivery() {
        let db = Db::memory().unwrap();
        let hook = db
            .insert_webhook(&NewWebhook {
                id: "w1".into(),
                url: "https://hooks.test".into(),
                events: vec!["comment.added".into()],
                project: None,
                salt: "s1".into(),
                secret_last4: "abcd".into(),
                created_by: "Ann".into(),
            })
            .unwrap();
        assert_eq!(hook.events, ["comment.added"]);
        assert!(db.set_webhook_salt("w1", "s2", "efgh").unwrap());
        db.record_delivery("w1", Some(500), None).unwrap();
        let (hook, salt) = db.webhooks().unwrap().remove(0);
        assert_eq!((salt.as_str(), hook.secret_last4.as_str()), ("s2", "efgh"));
        assert_eq!(hook.last_status, Some(500));
        assert!(hook.last_delivery_at.is_some());
        assert!(db.delete_webhook("w1").unwrap());
        assert!(!db.delete_webhook("w1").unwrap());
    }
}
