//! Loopback forwards (`CHARET_RENDER_FORWARD`): when charet-render runs apart from charet-server (in
//! docker), previews are reached as `http://127.0.0.1:<port>` through a forward instead of
//! `http://server:<port>`. Browsers trust loopback origins: the preview's `Secure` token cookie
//! and its mock service worker only work on a trusted origin.

use std::net::SocketAddr;

use tokio::net::{TcpListener, TcpStream};

/// One forward: connections to `listen` (loopback) are relayed to `target` (`host:port`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Forward {
    pub listen: SocketAddr,
    pub target: String,
}

/// Parses `127.0.0.1:5281=server:5281[,…]`.
pub fn parse(text: &str) -> Result<Vec<Forward>, String> {
    text.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|item| {
            let (listen, target) = item
                .split_once('=')
                .ok_or_else(|| format!("CHARET_RENDER_FORWARD {item:?} is not listen=host:port"))?;
            let listen: SocketAddr = listen
                .parse()
                .map_err(|_| format!("CHARET_RENDER_FORWARD {listen:?} is not an address"))?;
            if !listen.ip().is_loopback() {
                return Err(format!(
                    "CHARET_RENDER_FORWARD {listen} must listen on loopback"
                ));
            }
            if !target.contains(':') {
                return Err(format!(
                    "CHARET_RENDER_FORWARD target {target:?} needs a port"
                ));
            }
            Ok(Forward {
                listen,
                target: target.to_owned(),
            })
        })
        .collect()
}

/// Starts relaying; fails when the address is taken.
pub async fn start(forward: Forward) -> Result<(), String> {
    let listener = TcpListener::bind(forward.listen)
        .await
        .map_err(|e| format!("cannot listen on {}: {e}", forward.listen))?;
    tokio::spawn(async move {
        loop {
            let Ok((mut inbound, _)) = listener.accept().await else {
                continue;
            };
            let target = forward.target.clone();
            tokio::spawn(async move {
                match TcpStream::connect(&target).await {
                    Ok(mut outbound) => {
                        let _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
                    }
                    Err(error) => eprintln!("charet-render: forward to {target}: {error}"),
                }
            });
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn parses_forwards() {
        assert_eq!(
            parse("127.0.0.1:5281=server:5281").unwrap(),
            [Forward {
                listen: "127.0.0.1:5281".parse().unwrap(),
                target: "server:5281".into()
            }]
        );
        assert!(parse("0.0.0.0:5281=server:5281").is_err());
        assert!(parse("127.0.0.1:5281=server").is_err());
        assert!(parse("server:5281").is_err());
        assert_eq!(parse("").unwrap(), []);
    }

    #[tokio::test]
    async fn relays_bytes() {
        let echo = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target = echo.local_addr().unwrap().to_string();
        tokio::spawn(async move {
            let (mut socket, _) = echo.accept().await.unwrap();
            let mut buf = [0u8; 5];
            socket.read_exact(&mut buf).await.unwrap();
            socket.write_all(&buf).await.unwrap();
        });
        let free = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let listen = free.local_addr().unwrap();
        drop(free);
        start(Forward { listen, target }).await.unwrap();
        let mut client = TcpStream::connect(listen).await.unwrap();
        client.write_all(b"hello").await.unwrap();
        let mut back = [0u8; 5];
        client.read_exact(&mut back).await.unwrap();
        assert_eq!(&back, b"hello");
    }
}
