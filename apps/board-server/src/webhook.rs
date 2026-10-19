//! Sends webhook POSTs over HTTP(S).

use std::time::Duration;

use board_engine::WebhookSender;

pub struct Sender {
    agent: ureq::Agent,
}

impl Sender {
    pub fn new() -> Self {
        Self {
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(10)))
                .http_status_as_error(false)
                .max_redirects(0)
                .user_agent(format!("board-webhook/{}", env!("CARGO_PKG_VERSION")))
                .build()
                .new_agent(),
        }
    }
}

impl WebhookSender for Sender {
    fn post(&self, url: &str, headers: &[(&str, String)], body: &[u8]) -> Result<u16, String> {
        let mut request = self.agent.post(url);
        for (name, value) in headers {
            request = request.header(*name, value);
        }
        request
            .send(body)
            .map(|response| response.status().as_u16())
            .map_err(|error| error.to_string())
    }
}
