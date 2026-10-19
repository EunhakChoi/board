//! An ngrok tunnel run inside the server (the person's authtoken; no ngrok program needed).

use std::sync::Mutex;
use std::time::Duration;

use board_engine::TunnelOptions;
use ngrok::config::ForwarderBuilder;
use ngrok::forwarder::Forwarder;
use ngrok::prelude::{EndpointInfo, TunnelCloser};
use ngrok::tunnel::HttpTunnel;

/// How long connecting to ngrok may take.
const TIMEOUT: Duration = Duration::from_secs(20);

/// The tunnel while it is open; dropping it closes it.
pub struct Ngrok {
    runtime: tokio::runtime::Handle,
    /// The remote door, e.g. `http://127.0.0.1:5284`.
    target: url::Url,
    open: Mutex<Option<Open>>,
}

struct Open {
    session: ngrok::Session,
    forwarder: Forwarder<HttpTunnel>,
}

impl std::fmt::Debug for Ngrok {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ngrok")
            .field("target", &self.target.as_str())
            .finish_non_exhaustive()
    }
}

impl Ngrok {
    /// Calls block on `runtime`, so they must come from outside it (a blocking thread).
    pub fn new(runtime: tokio::runtime::Handle, target: &str) -> Result<Self, String> {
        Ok(Self {
            runtime,
            target: url::Url::parse(target).map_err(|e| format!("{target}: {e}"))?,
            open: Mutex::new(None),
        })
    }

    pub(crate) fn open(&self, options: &TunnelOptions) -> Result<String, String> {
        let token = options
            .authtoken
            .clone()
            .or_else(|| std::env::var("BOARD_NGROK_AUTHTOKEN").ok())
            .filter(|t| !t.trim().is_empty())
            .ok_or("an ngrok authtoken, please (ngrok dashboard › Your Authtoken)")?;
        self.close()?;
        let target = self.target.clone();
        let domain = options.domain.clone();
        let open = self.runtime.block_on(async move {
            tokio::time::timeout(TIMEOUT, async move {
                let session = ngrok::Session::builder()
                    .authtoken(token)
                    .connect()
                    .await
                    .map_err(|e| format!("ngrok refused the session: {e}"))?;
                let mut endpoint = session.http_endpoint();
                if let Some(domain) = domain {
                    endpoint.domain(domain);
                }
                let forwarder = endpoint
                    .listen_and_forward(target)
                    .await
                    .map_err(|e| format!("ngrok refused the tunnel: {e}"))?;
                Ok::<_, String>(Open { session, forwarder })
            })
            .await
            .map_err(|_| "ngrok did not answer in time".to_owned())?
        })?;
        let url = open.forwarder.url().to_owned();
        *self.open.lock().map_err(|_| "poisoned")? = Some(open);
        Ok(url)
    }

    pub(crate) fn close(&self) -> Result<(), String> {
        let open = self.open.lock().map_err(|_| "poisoned")?.take();
        if let Some(Open {
            mut session,
            mut forwarder,
        }) = open
        {
            self.runtime.block_on(async move {
                let _ = forwarder.close().await;
                let _ = session.close().await;
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_an_authtoken_it_asks_for_one() {
        if std::env::var("BOARD_NGROK_AUTHTOKEN").is_ok() {
            return;
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let ngrok = Ngrok::new(runtime.handle().clone(), "http://127.0.0.1:5284").unwrap();
        let err = ngrok.open(&TunnelOptions::default()).unwrap_err();
        assert!(err.contains("authtoken"), "{err}");
        ngrok.close().unwrap();
        assert!(Ngrok::new(runtime.handle().clone(), "not a url").is_err());
    }
}
