//! Joining a project's realtime room, and the events commands publish there.

use charet_model::Actor;

use crate::policy::Command;
use crate::{Engine, EngineError, Joined, Peer};

impl Engine {
    /// Joins the room of `project` (anyone who can see it). `since` is the last event sequence
    /// the client saw; missed events come with the hello.
    pub fn join_room(
        &self,
        actor: &Actor,
        project: &str,
        since: Option<u64>,
    ) -> Result<Joined, EngineError> {
        self.visible(actor, project, Command::ProjectView)?;
        let person = actor.person();
        let kind = match actor {
            Actor::Local => "local",
            Actor::User { .. } => "user",
            Actor::Visitor { .. } => "visitor",
        };
        let peer = Peer {
            id: String::new(),
            person: person.id,
            name: person.name,
            kind: kind.to_owned(),
            role: actor.role(),
        };
        Ok(self.ports.realtime.join(project, peer, since))
    }

    pub fn presence(&self, project: &str, client: u64, data: serde_json::Value) {
        self.ports.realtime.presence(project, client, data);
    }

    pub fn leave_room(&self, project: &str, client: u64) {
        self.ports.realtime.leave(project, client);
    }

    pub(crate) fn publish(&self, project: &str, kind: &str, data: serde_json::Value) {
        self.ports.realtime.publish(project, kind, data);
    }
}

#[cfg(test)]
mod tests {
    use charet_model::Actor;

    use crate::testing::{engine_with_fakes, file};

    #[test]
    fn commands_publish_events_and_working_marks() {
        let (engine, fakes) = engine_with_fakes();
        engine
            .create_project(
                &Actor::Local,
                "web",
                "Web",
                Some(crate::Import {
                    files: vec![file("a.txt", "a")],
                    source: "t".into(),
                }),
            )
            .unwrap();
        let checkout = engine.checkout(&Actor::Local, "web", None).unwrap();
        engine
            .push(
                &Actor::Local,
                "web",
                &checkout.commit,
                "m",
                &[file("a.txt", "b")],
            )
            .unwrap();
        engine
            .update_project(&Actor::Local, "web", Some("Web 2"), None)
            .unwrap();
        let events: Vec<String> = fakes
            .realtime
            .events
            .lock()
            .unwrap()
            .iter()
            .map(|(project, kind)| format!("{project} {kind}"))
            .collect();
        assert_eq!(
            events,
            [
                "web version.created",
                "web working local true",
                "web version.created",
                "web working local false",
                "web project.updated",
            ]
        );
    }
}
