//! Who hears about what: inbox notifications for members and signed webhook POSTs
//! (story 03 §1: a comment reaches people in the web, through webhooks and `board inbox`).
//!
//! - A new comment: everyone who can see the project.
//! - A reply, resolve or reopen: the people in that thread.
//! - An access request: the admins.
//!
//! The one who acted is never told about their own action. Guests have no inbox. Webhooks get
//! the events they chose; a private project's events only reach webhooks set to that project.

use std::time::Duration;

use board_model::{
    Actor, Comment, Notification, Project, ReplyKind, Role, User, UserStatus, Visibility, Webhook,
};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

use crate::access::admin;
use crate::auth::{hex, random_hex};
use crate::{Engine, EngineError, NewNotification, NewWebhook};

/// Events a webhook can choose.
pub const WEBHOOK_EVENTS: &[&str] = &[
    "comment.added",
    "comment.replied",
    "comment.resolved",
    "comment.reopened",
    "version.created",
    "access.requested",
];

/// Waits between delivery attempts; three attempts in all.
const RETRY: [Duration; 2] = [Duration::from_secs(2), Duration::from_secs(10)];

/// The longest piece of a comment quoted in a notification.
const QUOTE: usize = 120;

/// A member's notifications and how many are unread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inbox {
    pub notifications: Vec<Notification>,
    pub unread: u32,
}

/// A webhook just created or given a new secret. `secret` is shown this once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebhookSecret {
    pub webhook: Webhook,
    pub secret: String,
}

/// Something to tell people about.
struct Event<'a> {
    kind: &'static str,
    project: Option<&'a Project>,
    actor: &'a Actor,
    text: String,
    link: String,
    data: serde_json::Value,
    /// User ids to notify (the actor is left out later).
    people: Vec<String>,
}

impl Engine {
    /// The caller's notifications, newest first. Empty for local mode and guests.
    pub fn inbox(
        &self,
        actor: &Actor,
        unread_only: bool,
        limit: u32,
    ) -> Result<Inbox, EngineError> {
        let Actor::User { id, .. } = actor else {
            return Ok(Inbox {
                notifications: Vec::new(),
                unread: 0,
            });
        };
        Ok(Inbox {
            notifications: self.ports.notices.notifications(
                id,
                unread_only,
                limit.clamp(1, 200),
            )?,
            unread: self.ports.notices.unread_count(id)?,
        })
    }

    /// Marks these notifications (or all, with `None`) read; returns how many stay unread.
    pub fn mark_read(&self, actor: &Actor, ids: Option<&[u64]>) -> Result<u32, EngineError> {
        let Actor::User { id, .. } = actor else {
            return Ok(0);
        };
        self.ports.notices.mark_read(id, ids)?;
        Ok(self.ports.notices.unread_count(id)?)
    }

    pub fn webhooks(&self, actor: &Actor) -> Result<Vec<Webhook>, EngineError> {
        admin(actor)?;
        Ok(self
            .ports
            .notices
            .webhooks()?
            .into_iter()
            .map(|(hook, _)| hook)
            .collect())
    }

    /// Adds a webhook for `events` (all of them when empty), optionally for one project.
    pub fn create_webhook(
        &self,
        actor: &Actor,
        url: &str,
        events: &[String],
        project: Option<&str>,
    ) -> Result<WebhookSecret, EngineError> {
        admin(actor)?;
        let url = url.trim();
        if !(url.starts_with("https://") || url.starts_with("http://")) || url.len() > 2000 {
            return Err(EngineError::Invalid(
                "a webhook address starts with https:// (or http://)".into(),
            ));
        }
        if let Some(bad) = events
            .iter()
            .find(|e| !WEBHOOK_EVENTS.contains(&e.as_str()))
        {
            return Err(EngineError::Invalid(format!(
                "unknown event {bad:?}; choose from {}",
                WEBHOOK_EVENTS.join(", ")
            )));
        }
        let events = if events.is_empty() {
            WEBHOOK_EVENTS.iter().map(|&e| e.to_owned()).collect()
        } else {
            events.to_vec()
        };
        if let Some(key) = project {
            self.visible(actor, key, crate::policy::Command::Admin)?;
        }
        let id = format!("w{}", &random_hex()[..10]);
        let salt = random_hex();
        let secret = self.webhook_secret(&id, &salt)?;
        let webhook = self.ports.notices.insert_webhook(&NewWebhook {
            id,
            url: url.to_owned(),
            events,
            project: project.map(str::to_owned),
            salt,
            secret_last4: last4(&secret),
            created_by: actor.name().to_owned(),
        })?;
        Ok(WebhookSecret { webhook, secret })
    }

    pub fn delete_webhook(&self, actor: &Actor, id: &str) -> Result<(), EngineError> {
        admin(actor)?;
        if self.ports.notices.delete_webhook(id)? {
            Ok(())
        } else {
            Err(EngineError::NotFound("webhook"))
        }
    }

    /// Gives a webhook a new signing secret; the old one stops working at once.
    pub fn rotate_webhook_secret(
        &self,
        actor: &Actor,
        id: &str,
    ) -> Result<WebhookSecret, EngineError> {
        admin(actor)?;
        let salt = random_hex();
        let secret = self.webhook_secret(id, &salt)?;
        if !self
            .ports
            .notices
            .set_webhook_salt(id, &salt, &last4(&secret))?
        {
            return Err(EngineError::NotFound("webhook"));
        }
        Ok(WebhookSecret {
            webhook: self.webhook(id)?.0,
            secret,
        })
    }

    /// Sends a `ping` now, once, and returns the webhook with the outcome recorded.
    pub fn test_webhook(&self, actor: &Actor, id: &str) -> Result<Webhook, EngineError> {
        admin(actor)?;
        let (hook, salt) = self.webhook(id)?;
        let body = serde_json::json!({
            "event": "ping",
            "project": null,
            "actor": actor.name(),
            "text": format!("board: test delivery from {}", actor.name()),
            "url": self.tool_url(),
            "data": {},
        });
        let secret = self.webhook_secret(&hook.id, &salt)?;
        let outcome = send(self, &hook.url, &secret, "ping", &body);
        self.record_outcome(&hook.id, &outcome);
        Ok(self.webhook(id)?.0)
    }

    pub(crate) fn comment_added(&self, actor: &Actor, project: &Project, comment: &Comment) {
        let people = self.audience(project).into_iter().map(|u| u.id).collect();
        self.announce(&Event {
            kind: "comment.added",
            project: Some(project),
            actor,
            text: format!(
                "{} commented on {} {}: {}",
                actor.name(),
                comment.kind,
                comment.target,
                quote(&comment.body)
            ),
            link: comment_link(&project.key, comment),
            data: comment_data(comment, &comment.body),
            people,
        });
    }

    pub(crate) fn comment_answered(
        &self,
        actor: &Actor,
        project: &Project,
        comment: &Comment,
        kind: ReplyKind,
        body: &str,
    ) {
        let (event, verb) = match kind {
            ReplyKind::Reply => ("comment.replied", "replied to"),
            ReplyKind::Resolve => ("comment.resolved", "resolved"),
            ReplyKind::Reopen => ("comment.reopened", "reopened"),
        };
        // The thread: whoever wrote the comment or answered it, if they can still see it.
        let audience: Vec<String> = self.audience(project).into_iter().map(|u| u.id).collect();
        let mut people: Vec<String> = std::iter::once(&comment.author)
            .chain(comment.replies.iter().map(|r| &r.author))
            .map(|p| p.id.clone())
            .filter(|id| audience.contains(id))
            .collect();
        people.sort();
        people.dedup();
        let mut text = format!(
            "{} {verb} c{} on {}",
            actor.name(),
            comment.id,
            comment.target
        );
        if !body.is_empty() {
            text = format!("{text}: {}", quote(body));
        }
        self.announce(&Event {
            kind: event,
            project: Some(project),
            actor,
            text,
            link: comment_link(&project.key, comment),
            data: comment_data(comment, body),
            people,
        });
    }

    pub(crate) fn version_created(
        &self,
        actor: &Actor,
        project: &Project,
        number: u32,
        note: &str,
    ) {
        let mut text = format!("{} made v{number} of {}", actor.name(), project.title);
        if !note.is_empty() {
            text = format!("{text}: {}", quote(note));
        }
        self.announce(&Event {
            kind: "version.created",
            project: Some(project),
            actor,
            text,
            link: format!("/p/{}/versions", project.key),
            data: serde_json::json!({ "number": number, "note": note }),
            // On the version timeline, not in anyone's inbox.
            people: Vec::new(),
        });
    }

    pub(crate) fn access_requested(&self, user: &User) {
        let admins = self.ports.accounts.users().unwrap_or_default();
        let people = admins
            .into_iter()
            .filter(|u| u.role == Role::Admin && u.status == UserStatus::Active)
            .map(|u| u.id)
            .collect();
        self.announce(&Event {
            kind: "access.requested",
            project: None,
            actor: &user.actor(),
            text: format!("{} ({}) asked to join", user.name, user.email),
            link: "/team".into(),
            data: serde_json::json!({ "user": user.id, "email": user.email, "name": user.name }),
            people,
        });
    }

    /// Stores the notifications, nudges the people connected, and starts webhook deliveries.
    /// The command that caused it has already happened: failures here do not undo it.
    fn announce(&self, event: &Event<'_>) {
        let me = event.actor.person().id;
        let people: Vec<String> = event
            .people
            .iter()
            .filter(|id| **id != me)
            .cloned()
            .collect();
        let project = event.project.map(|p| p.key.clone());
        if !people.is_empty() {
            let rows: Vec<NewNotification> = people
                .iter()
                .map(|user| NewNotification {
                    user: user.clone(),
                    kind: event.kind.to_owned(),
                    project: project.clone(),
                    actor: event.actor.name().to_owned(),
                    text: event.text.clone(),
                    link: event.link.clone(),
                    data: event.data.clone(),
                })
                .collect();
            if self.ports.notices.insert_notifications(&rows).is_ok() {
                self.ports.realtime.notify(&people);
            }
        }
        self.deliver(event, project.as_deref());
    }

    /// Webhooks that want this event, each delivered (with retries) on its own thread.
    fn deliver(&self, event: &Event<'_>, project: Option<&str>) {
        let Ok(hooks) = self.ports.notices.webhooks() else {
            return;
        };
        let private = event
            .project
            .is_some_and(|p| p.visibility == Visibility::Private);
        let body = serde_json::json!({
            "event": event.kind,
            "project": project,
            "actor": event.actor.name(),
            "text": event.text,
            "url": self.tool_url().map(|base| format!("{}{}", base.trim_end_matches('/'), event.link)),
            "data": event.data,
        });
        for (hook, salt) in hooks {
            let wanted = hook.events.iter().any(|e| e == event.kind)
                && match (&hook.project, project) {
                    (Some(only), Some(key)) => only == key,
                    (Some(_), None) => false,
                    (None, _) => !private,
                };
            if !wanted {
                continue;
            }
            let Ok(secret) = self.webhook_secret(&hook.id, &salt) else {
                continue;
            };
            let engine = self.clone();
            let body = body.clone();
            let kind = event.kind;
            std::thread::spawn(move || {
                let mut outcome = send(&engine, &hook.url, &secret, kind, &body);
                for wait in RETRY {
                    if matches!(outcome, Ok(status) if status < 500 && status != 429) {
                        break;
                    }
                    std::thread::sleep(wait);
                    outcome = send(&engine, &hook.url, &secret, kind, &body);
                }
                engine.record_outcome(&hook.id, &outcome);
            });
        }
    }

    fn record_outcome(&self, id: &str, outcome: &Result<u16, String>) {
        let _ = match outcome {
            Ok(status) => self.ports.notices.record_delivery(id, Some(*status), None),
            Err(error) => self.ports.notices.record_delivery(id, None, Some(error)),
        };
    }

    fn webhook(&self, id: &str) -> Result<(Webhook, String), EngineError> {
        self.ports
            .notices
            .webhooks()?
            .into_iter()
            .find(|(hook, _)| hook.id == id)
            .ok_or(EngineError::NotFound("webhook"))
    }

    /// HMAC of the server key over the webhook id and salt: nothing secret sits in the database.
    fn webhook_secret(&self, id: &str, salt: &str) -> Result<String, EngineError> {
        let key = self.secret.as_deref().ok_or_else(|| {
            EngineError::Unavailable("the server has no secret key for webhooks".into())
        })?;
        let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC takes keys of any size");
        mac.update(format!("webhook:{id}:{salt}").as_bytes());
        Ok(format!("whsec_{}", hex(&mac.finalize().into_bytes())))
    }

    /// Active members who can see `project` (admins always can).
    fn audience(&self, project: &Project) -> Vec<User> {
        let users = self.ports.accounts.users().unwrap_or_default();
        let members = if project.visibility == Visibility::Private {
            self.ports.records.members(&project.key).unwrap_or_default()
        } else {
            Vec::new()
        };
        users
            .into_iter()
            .filter(|u| u.status == UserStatus::Active)
            .filter(|u| {
                project.visibility != Visibility::Private
                    || u.role == Role::Admin
                    || members.contains(&u.id)
            })
            .collect()
    }
}

/// One POST, signed: `X-Board-Signature: sha256=<hex HMAC of "<timestamp>.<body>">`.
fn send(
    engine: &Engine,
    url: &str,
    secret: &str,
    kind: &str,
    body: &serde_json::Value,
) -> Result<u16, String> {
    let body = body.to_string();
    let timestamp = crate::auth::now().to_string();
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC takes keys of any size");
    mac.update(timestamp.as_bytes());
    mac.update(b".");
    mac.update(body.as_bytes());
    let signature = format!("sha256={}", hex(&mac.finalize().into_bytes()));
    engine.ports.webhooks.post(
        url,
        &[
            ("content-type", "application/json".to_owned()),
            ("x-board-event", kind.to_owned()),
            ("x-board-delivery", format!("d{}", &random_hex()[..16])),
            ("x-board-timestamp", timestamp),
            ("x-board-signature", signature),
        ],
        body.as_bytes(),
    )
}

fn last4(secret: &str) -> String {
    secret[secret.len().saturating_sub(4)..].to_owned()
}

fn quote(text: &str) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() > QUOTE {
        format!("{}…", line.chars().take(QUOTE).collect::<String>())
    } else {
        line
    }
}

fn comment_link(key: &str, comment: &Comment) -> String {
    format!(
        "/p/{key}/t/{}/{}?comment={}",
        comment.kind, comment.target, comment.id
    )
}

fn comment_data(comment: &Comment, body: &str) -> serde_json::Value {
    serde_json::json!({
        "comment": comment.id,
        "kind": comment.kind,
        "target": comment.target,
        "body": quote(body),
    })
}

#[cfg(test)]
mod tests {
    use board_model::{ReplyKind, Visibility};

    use super::*;
    use crate::testing::{FakeIdentity, allow, file, signed_in_with_fakes};
    use crate::{At, Import, SignIn};

    fn join(engine: &Engine, idp: &FakeIdentity, who: &str) -> User {
        engine.begin_sign_in(None, None).unwrap();
        idp.next(who, &format!("{who}@example.com"), true);
        match engine.finish_sign_in(&idp.last_state(), "c").unwrap() {
            SignIn::Session { user, .. } => user,
            SignIn::Refused(r) => panic!("{r:?}"),
        }
    }

    fn project(engine: &Engine, admin: &Actor) {
        engine
            .create_project(
                admin,
                "web",
                "Web",
                Some(Import {
                    files: vec![file("a.txt", "a")],
                    source: "t".into(),
                }),
            )
            .unwrap();
    }

    fn kinds(engine: &Engine, user: &User) -> Vec<String> {
        engine
            .inbox(&user.actor(), false, 50)
            .unwrap()
            .notifications
            .into_iter()
            .map(|n| n.kind)
            .collect()
    }

    #[test]
    fn comments_reach_the_team_and_answers_reach_the_thread() {
        let (engine, idp, _) = signed_in_with_fakes(&["boss@example.com"]);
        allow(&engine, "domain", "example.com");
        let boss = join(&engine, &idp, "boss");
        let ann = join(&engine, &idp, "ann");
        let bob = join(&engine, &idp, "bob");
        project(&engine, &boss.actor());

        let at = At::Point { x: 5, y: 5 };
        let c = engine
            .add_comment(
                &ann.actor(),
                "web",
                "screen",
                "home",
                "light",
                &at,
                "Too   tight",
            )
            .unwrap();
        assert_eq!(
            kinds(&engine, &ann),
            Vec::<String>::new(),
            "not told about my own"
        );
        let inbox = engine.inbox(&bob.actor(), true, 50).unwrap();
        assert_eq!(inbox.unread, 1);
        let n = &inbox.notifications[0];
        assert_eq!(n.text, "ann commented on screen home: Too tight");
        assert_eq!(n.link, format!("/p/web/t/screen/home?comment={}", c.id));
        assert_eq!(n.project.as_deref(), Some("web"));

        // Boss replies: only the thread (Ann) hears, not Bob.
        engine
            .answer_comment(&boss.actor(), "web", c.id, c.rev, ReplyKind::Reply, "On it")
            .unwrap();
        assert_eq!(kinds(&engine, &ann), ["comment.replied"]);
        assert_eq!(kinds(&engine, &bob), ["comment.added"]);
        // Ann resolves: Boss (who answered) hears.
        let c = engine.comment(&ann.actor(), "web", c.id).unwrap();
        engine
            .answer_comment(&ann.actor(), "web", c.id, c.rev, ReplyKind::Resolve, "")
            .unwrap();
        assert_eq!(kinds(&engine, &boss), ["comment.resolved", "comment.added"]);

        assert_eq!(engine.mark_read(&bob.actor(), None).unwrap(), 0);
        assert_eq!(engine.inbox(&bob.actor(), true, 50).unwrap().unread, 0);
    }

    #[test]
    fn private_projects_tell_only_their_members_and_requests_go_to_admins() {
        let (engine, idp, fakes) = signed_in_with_fakes(&["boss@example.com"]);
        allow(&engine, "email", "ann@example.com");
        let boss = join(&engine, &idp, "boss");
        let ann = join(&engine, &idp, "ann");
        project(&engine, &boss.actor());
        engine
            .update_project(&boss.actor(), "web", None, Some(Visibility::Private))
            .unwrap();
        let at = At::Point { x: 5, y: 5 };
        engine
            .add_comment(&boss.actor(), "web", "screen", "home", "light", &at, "x")
            .unwrap();
        assert_eq!(kinds(&engine, &ann), Vec::<String>::new());

        let eve = join(&engine, &idp, "eve");
        assert_eq!(kinds(&engine, &boss), ["access.requested"]);
        assert_eq!(kinds(&engine, &eve), Vec::<String>::new());
        assert!(
            fakes
                .realtime
                .events
                .lock()
                .unwrap()
                .contains(&("*".into(), format!("notify {}", boss.id)))
        );
    }

    #[test]
    fn webhooks_get_signed_posts_for_the_events_they_chose() {
        let (engine, idp, fakes) = signed_in_with_fakes(&["boss@example.com"]);
        let boss = join(&engine, &idp, "boss");
        let admin = boss.actor();
        project(&engine, &admin);
        assert!(matches!(
            engine.create_webhook(&admin, "ftp://x", &[], None),
            Err(EngineError::Invalid(_))
        ));
        assert!(matches!(
            engine.create_webhook(&admin, "https://x", &["nope".into()], None),
            Err(EngineError::Invalid(_))
        ));
        let created = engine
            .create_webhook(
                &admin,
                "https://hooks.test/a",
                &["comment.added".into()],
                None,
            )
            .unwrap();
        assert!(created.secret.starts_with("whsec_"));
        assert_eq!(created.webhook.secret_last4, last4(&created.secret));

        let at = At::Point { x: 5, y: 5 };
        engine
            .add_comment(&admin, "web", "screen", "home", "light", &at, "Hello")
            .unwrap();
        let posts = fakes.sender.wait_for(1);
        assert_eq!(posts.len(), 1);
        let (url, headers, body) = &posts[0];
        assert_eq!(url, "https://hooks.test/a");
        let header = |name: &str| {
            headers
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
                .unwrap()
        };
        assert_eq!(header("x-board-event"), "comment.added");
        let mut mac = Hmac::<Sha256>::new_from_slice(created.secret.as_bytes()).unwrap();
        mac.update(format!("{}.{body}", header("x-board-timestamp")).as_bytes());
        assert_eq!(
            header("x-board-signature"),
            format!("sha256={}", hex(&mac.finalize().into_bytes()))
        );
        let body: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(body["event"], "comment.added");
        assert_eq!(body["data"]["body"], "Hello");

        // Versions were not chosen; a private project's events stay inside.
        engine
            .update_project(&admin, "web", None, Some(Visibility::Private))
            .unwrap();
        engine
            .add_comment(&admin, "web", "screen", "home", "light", &at, "Secret")
            .unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(fakes.sender.0.lock().unwrap().len(), 1);

        let tested = engine.test_webhook(&admin, &created.webhook.id).unwrap();
        assert_eq!(tested.last_status, Some(200));
        let rotated = engine
            .rotate_webhook_secret(&admin, &created.webhook.id)
            .unwrap();
        assert_ne!(rotated.secret, created.secret);
        engine.delete_webhook(&admin, &created.webhook.id).unwrap();
        assert_eq!(engine.webhooks(&admin).unwrap(), vec![]);
        assert_eq!(
            engine.webhooks(&Actor::Guest {
                id: "g".into(),
                name: "G".into(),
                role: Role::Editor,
                project: "web".into()
            }),
            Err(EngineError::Forbidden)
        );
    }
}
