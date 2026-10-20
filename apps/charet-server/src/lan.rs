//! Sharing on the local network: the remote door also listens on this machine's LAN address.
//! For trying things out — the address people open is an IP over plain HTTP.

use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::Mutex;

use charet_engine::{ShareMode, TunnelMode, TunnelOptions, Tunnels};

/// The tunnels a local charet can run: the LAN listener here, the rest in `charet-tunnel`.
pub struct ServerTunnels {
    pub machine: charet_tunnel::Machine,
    pub lan: Lan,
}

impl Tunnels for ServerTunnels {
    fn modes(&self) -> Vec<TunnelMode> {
        let mut modes = self.machine.modes();
        modes.push(self.lan.mode());
        modes
    }

    fn open(&self, mode: ShareMode, options: &TunnelOptions) -> Result<String, String> {
        match mode {
            ShareMode::Lan => self.lan.open(),
            _ => self.machine.open(mode, options),
        }
    }

    fn close(&self, mode: ShareMode) -> Result<(), String> {
        match mode {
            ShareMode::Lan => self.lan.close(),
            _ => self.machine.close(mode),
        }
    }
}

/// A second listener for the remote door's routes, on the LAN address.
pub struct Lan {
    pub runtime: tokio::runtime::Handle,
    pub port: u16,
    pub app: axum::Router,
    /// In a container the LAN address is the container's own; publishing the port is the way.
    pub container: bool,
    pub running: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

impl Lan {
    fn address(&self) -> Result<SocketAddr, String> {
        if self.container {
            return Err(format!(
                "in Docker, publish port {} on the LAN instead (compose ports)",
                self.port
            ));
        }
        Ok(SocketAddr::new(lan_ip()?, self.port))
    }

    fn mode(&self) -> TunnelMode {
        let address = self.address();
        TunnelMode {
            mode: ShareMode::Lan,
            available: address.is_ok(),
            detail: match address {
                Ok(addr) => format!("http://{addr}"),
                Err(why) => why,
            },
        }
    }

    fn open(&self) -> Result<String, String> {
        let addr = self.address()?;
        self.close()?;
        let listener = self
            .runtime
            .block_on(tokio::net::TcpListener::bind(addr))
            .map_err(|e| format!("cannot listen on {addr}: {e}"))?;
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let app = self.app.clone();
        self.runtime.spawn(async move {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await;
        });
        *self.running.lock().map_err(|_| "poisoned")? = Some(stop);
        Ok(format!("http://{addr}"))
    }

    fn close(&self) -> Result<(), String> {
        if let Some(stop) = self.running.lock().map_err(|_| "poisoned")?.take() {
            let _ = stop.send(());
        }
        Ok(())
    }
}

/// The address this machine uses on its network: the source of a route to the outside (nothing
/// is sent; connecting a UDP socket only picks the route).
fn lan_ip() -> Result<IpAddr, String> {
    let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
    socket
        .connect("192.0.2.1:9")
        .map_err(|_| "this machine is not on a network".to_owned())?;
    let ip = socket.local_addr().map_err(|e| e.to_string())?.ip();
    if ip.is_loopback() || ip.is_unspecified() {
        return Err("this machine is not on a network".into());
    }
    Ok(ip)
}
