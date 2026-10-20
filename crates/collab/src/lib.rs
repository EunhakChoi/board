//! Realtime hub: presence relay and ordered charet events (`docs/design/interfaces.md` §2).
//!
//! One room per project. Presence (cursor, selection, viewport, follow, spotlight, chat) is
//! relayed as it comes and only the latest per connection is kept, for newcomers. Events get a
//! sequence number per room; the last [`LOG`] are kept so a client that reconnects with `since`
//! catches up, and one that fell further behind is told to reload.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Mutex;

use charet_engine::{Joined, Kick, Outgoing, Peer, Realtime};
use charet_model::{Person, Role};
use serde_json::{Value, json};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

/// Events kept per room for catching up.
pub const LOG: usize = 500;

/// The hub. One per server, shared by every connection.
#[derive(Debug, Default)]
pub struct Hub {
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    next: u64,
    rooms: HashMap<String, Room>,
}

#[derive(Debug, Default)]
struct Room {
    seq: u64,
    log: VecDeque<(u64, Value)>,
    clients: BTreeMap<u64, Client>,
    /// People working from their own checkout, by person id.
    working: BTreeMap<String, String>,
}

#[derive(Debug)]
struct Client {
    peer: Peer,
    sender: UnboundedSender<Outgoing>,
    presence: Option<Value>,
}

impl Room {
    fn send_all(&self, message: &Value, except: Option<u64>) {
        let text = message.to_string();
        for (id, client) in &self.clients {
            if Some(*id) != except {
                let _ = client.sender.send(Outgoing::Text(text.clone()));
            }
        }
    }

    fn event(&mut self, kind: &str, data: &Value) {
        self.seq += 1;
        let message = json!({ "type": "event", "seq": self.seq, "kind": kind, "data": data });
        self.send_all(&message, None);
        self.log.push_back((self.seq, message));
        while self.log.len() > LOG {
            self.log.pop_front();
        }
    }

    fn working_list(&self) -> Vec<Value> {
        self.working
            .iter()
            .map(|(id, name)| json!({ "person": id, "name": name }))
            .collect()
    }
}

impl Hub {
    pub fn new() -> Self {
        Self::default()
    }

    fn with<T>(&self, f: impl FnOnce(&mut State) -> T) -> T {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        f(&mut state)
    }

    /// How many connections a project room has (for tests and status).
    pub fn connections(&self, project: &str) -> usize {
        self.with(|state| state.rooms.get(project).map_or(0, |r| r.clients.len()))
    }
}

impl Realtime for Hub {
    fn join(&self, project: &str, mut peer: Peer, since: Option<u64>) -> Joined {
        let (sender, receiver) = unbounded_channel();
        self.with(|state| {
            state.next += 1;
            let client = state.next;
            peer.id = format!("c{client}");
            let room = state.rooms.entry(project.to_owned()).or_default();
            let peers: Vec<Value> = room
                .clients
                .values()
                .map(|c| json!({ "peer": c.peer, "presence": c.presence }))
                .collect();
            // Missed events, or a reload when the log no longer reaches back that far.
            let (missed, reload) = match since {
                Some(seen) if seen < room.seq => {
                    let oldest = room.log.front().map_or(room.seq + 1, |(s, _)| *s);
                    if seen + 1 < oldest {
                        (Vec::new(), true)
                    } else {
                        let missed = room
                            .log
                            .iter()
                            .filter(|(s, _)| *s > seen)
                            .map(|(_, m)| m.clone())
                            .collect();
                        (missed, false)
                    }
                }
                Some(seen) if seen > room.seq => (Vec::new(), true),
                _ => (Vec::new(), false),
            };
            let hello = json!({
                "type": "hello",
                "you": peer,
                "seq": room.seq,
                "peers": peers,
                "missed": missed,
                "reload": reload,
                "working": room.working_list(),
            })
            .to_string();
            room.send_all(&json!({ "type": "join", "peer": peer }), None);
            room.clients.insert(
                client,
                Client {
                    peer,
                    sender,
                    presence: None,
                },
            );
            Joined {
                client,
                hello,
                receiver,
            }
        })
    }

    fn presence(&self, project: &str, client: u64, data: Value) {
        self.with(|state| {
            let Some(room) = state.rooms.get_mut(project) else {
                return;
            };
            let Some(me) = room.clients.get_mut(&client) else {
                return;
            };
            me.presence = Some(data.clone());
            let from = me.peer.id.clone();
            room.send_all(
                &json!({ "type": "presence", "from": from, "data": data }),
                Some(client),
            );
        });
    }

    fn leave(&self, project: &str, client: u64) {
        self.with(|state| {
            let Some(room) = state.rooms.get_mut(project) else {
                return;
            };
            if let Some(gone) = room.clients.remove(&client) {
                room.send_all(&json!({ "type": "leave", "id": gone.peer.id }), None);
            }
            if room.clients.is_empty() && room.working.is_empty() && room.seq == 0 {
                state.rooms.remove(project);
            }
        });
    }

    fn publish(&self, project: &str, kind: &str, data: Value) {
        self.with(|state| {
            state
                .rooms
                .entry(project.to_owned())
                .or_default()
                .event(kind, &data);
        });
    }

    fn publish_admins(&self, kind: &str, data: Value) {
        // Not part of any room's log: admins refetch what they show when they reconnect.
        let message = json!({ "type": "admin", "kind": kind, "data": data }).to_string();
        self.with(|state| {
            for room in state.rooms.values() {
                for client in room.clients.values().filter(|c| c.peer.role == Role::Admin) {
                    let _ = client.sender.send(Outgoing::Text(message.clone()));
                }
            }
        });
    }

    fn notify(&self, people: &[String]) {
        // Only a nudge: clients refetch their inbox.
        let message = json!({ "type": "inbox" }).to_string();
        self.with(|state| {
            for room in state.rooms.values() {
                for client in room
                    .clients
                    .values()
                    .filter(|c| people.contains(&c.peer.person))
                {
                    let _ = client.sender.send(Outgoing::Text(message.clone()));
                }
            }
        });
    }

    fn kick(&self, who: Kick) {
        self.with(|state| {
            for room in state.rooms.values_mut() {
                let gone: Vec<u64> = room
                    .clients
                    .iter()
                    .filter(|(_, c)| match &who {
                        Kick::Person(person) => &c.peer.person == person,
                        Kick::Visitors => c.peer.kind == "visitor",
                    })
                    .map(|(id, _)| *id)
                    .collect();
                for id in gone {
                    if let Some(client) = room.clients.remove(&id) {
                        let _ = client.sender.send(Outgoing::Close);
                        room.send_all(&json!({ "type": "leave", "id": client.peer.id }), None);
                    }
                }
            }
        });
    }

    fn working(&self, project: &str, person: &Person, on: bool) {
        self.with(|state| {
            let room = state.rooms.entry(project.to_owned()).or_default();
            let changed = if on {
                room.working
                    .insert(person.id.clone(), person.name.clone())
                    .is_none()
            } else {
                room.working.remove(&person.id).is_some()
            };
            if changed {
                let working = room.working_list();
                room.send_all(&json!({ "type": "working", "working": working }), None);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::mpsc::UnboundedReceiver;

    use super::*;

    fn peer(person: &str, kind: &str, role: Role) -> Peer {
        Peer {
            id: String::new(),
            person: person.into(),
            name: person.to_uppercase(),
            kind: kind.into(),
            role,
        }
    }

    fn next(receiver: &mut UnboundedReceiver<Outgoing>) -> Value {
        match receiver.try_recv().expect("a message") {
            Outgoing::Text(text) => serde_json::from_str(&text).unwrap(),
            Outgoing::Close => json!({ "type": "closed" }),
        }
    }

    fn hello(joined: &Joined) -> Value {
        serde_json::from_str(&joined.hello).unwrap()
    }

    #[test]
    fn presence_is_relayed_and_newcomers_get_the_latest() {
        let hub = Hub::new();
        let mut ann = hub.join("web", peer("u1", "user", Role::Editor), None);
        assert_eq!(hello(&ann)["peers"], json!([]));
        hub.presence("web", ann.client, json!({ "cursor": [1, 2] }));
        let mut bob = hub.join("web", peer("u2", "user", Role::Viewer), None);
        let bob_hello = hello(&bob);
        assert_eq!(
            bob_hello["peers"][0]["presence"],
            json!({ "cursor": [1, 2] })
        );
        assert_eq!(bob_hello["you"]["id"], format!("c{}", bob.client));
        assert_eq!(next(&mut ann.receiver)["type"], "join");

        hub.presence("web", bob.client, json!({ "cursor": [5, 5] }));
        let relayed = next(&mut ann.receiver);
        assert_eq!(
            (relayed["type"].as_str(), relayed["from"].clone()),
            (Some("presence"), json!(format!("c{}", bob.client)))
        );
        assert!(
            bob.receiver.try_recv().is_err(),
            "nobody hears their own presence"
        );

        hub.leave("web", bob.client);
        assert_eq!(next(&mut ann.receiver)["type"], "leave");
        assert_eq!(hub.connections("web"), 1);
    }

    #[test]
    fn events_are_numbered_and_missed_ones_come_with_hello() {
        let hub = Hub::new();
        let mut ann = hub.join("web", peer("u1", "user", Role::Editor), None);
        hub.publish("web", "comment.added", json!({ "id": 1 }));
        hub.publish("web", "comment.added", json!({ "id": 2 }));
        hub.publish("other", "comment.added", json!({ "id": 9 }));
        assert_eq!(next(&mut ann.receiver)["seq"], 1);
        assert_eq!(next(&mut ann.receiver)["data"]["id"], 2);
        assert!(ann.receiver.try_recv().is_err(), "other rooms stay apart");

        let late = hub.join("web", peer("u2", "user", Role::Editor), Some(1));
        let late = hello(&late);
        assert_eq!(late["seq"], 2);
        assert_eq!(late["missed"].as_array().unwrap().len(), 1);
        assert_eq!(late["reload"], false);
        for _ in 0..LOG + 5 {
            hub.publish("web", "x", json!({}));
        }
        let lost = hello(&hub.join("web", peer("u3", "user", Role::Editor), Some(2)));
        assert_eq!(lost["reload"], true);
    }

    #[test]
    fn inbox_nudges_reach_only_those_people() {
        let hub = Hub::new();
        let mut ann = hub.join("web", peer("u1", "user", Role::Editor), None);
        let mut bob = hub.join("other", peer("u2", "user", Role::Editor), None);
        hub.notify(&["u2".into()]);
        assert_eq!(next(&mut bob.receiver)["type"], "inbox");
        assert!(ann.receiver.try_recv().is_err());
    }

    #[test]
    fn admin_events_kicks_and_working() {
        let hub = Hub::new();
        let mut admin = hub.join("web", peer("u1", "user", Role::Admin), None);
        let mut guest = hub.join("web", peer("visitor:g1", "visitor", Role::Viewer), None);
        let _ = next(&mut admin.receiver); // visitor joined
        hub.publish_admins("access.changed", json!({}));
        assert_eq!(next(&mut admin.receiver)["kind"], "access.changed");
        assert!(guest.receiver.try_recv().is_err());

        hub.working(
            "web",
            &Person {
                id: "u1".into(),
                name: "Ann".into(),
            },
            true,
        );
        assert_eq!(next(&mut admin.receiver)["working"][0]["name"], "Ann");
        assert_eq!(next(&mut guest.receiver)["type"], "working");

        hub.kick(Kick::Visitors);
        assert_eq!(next(&mut guest.receiver)["type"], "closed");
        assert_eq!(next(&mut admin.receiver)["type"], "leave");
        assert_eq!(hub.connections("web"), 1);
        hub.kick(Kick::Person("u1".into()));
        assert_eq!(next(&mut admin.receiver)["type"], "closed");
    }
}
