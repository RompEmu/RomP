//! RomM 5.4's `/devices` Socket.IO namespace: how RomP hears about games sent to it the moment
//! they're sent.
use super::socketio::{self, Frame};
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

const NAMESPACE: &str = "/devices";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceEvent {
    Connected,
    InstallQueued { id: String, rom_id: i64 },
    InstallCancelled { id: String },
    Refused(String),
    Disconnected,
}

pub fn socket_url(base: &url::Url) -> url::Url {
    let mut url = base.join("ws/socket.io/").expect("relative url");
    let scheme = if base.scheme() == "https" {
        "wss"
    } else {
        "ws"
    };
    url.set_scheme(scheme).expect("websocket scheme");
    url.set_query(Some("EIO=4&transport=websocket"));
    url
}

/// How long the link may stay silent: RomM pings every `pingInterval` and gives up after
/// `pingTimeout` more, so nothing for that long means the connection is gone.
pub fn quiet_limit(open: &str) -> Duration {
    let fields = serde_json::from_str::<serde_json::Value>(open.get(1..).unwrap_or_default())
        .unwrap_or_default();
    let ms = |key: &str, default: u64| fields[key].as_u64().unwrap_or(default);
    Duration::from_millis(ms("pingInterval", 25_000) + ms("pingTimeout", 20_000))
}

pub fn backoff(attempt: u32) -> Duration {
    Duration::from_secs(2u64.saturating_pow(attempt).min(60))
}

fn jitter() -> Duration {
    let mut bytes = [0u8; 2];
    let _ = getrandom::fill(&mut bytes);
    Duration::from_millis(u64::from(u16::from_le_bytes(bytes)) % 500)
}

/// The connection, kept open, and reopened after a drop, until this is dropped.
pub struct DeviceSocket {
    task: tokio::task::AbortHandle,
}

impl Drop for DeviceSocket {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub fn spawn(
    rt: &tokio::runtime::Handle,
    base: url::Url,
    token: String,
    events: impl Fn(DeviceEvent) + Send + Sync + 'static,
) -> DeviceSocket {
    let task = rt.spawn(async move {
        let mut attempt = 0;
        loop {
            match run(&base, &token, &events, &mut attempt).await {
                // RomM said no: trying again would only be refused again.
                Ended::Refused => return,
                Ended::Dropped => {
                    events(DeviceEvent::Disconnected);
                    tokio::time::sleep(backoff(attempt) + jitter()).await;
                    attempt = attempt.saturating_add(1);
                }
            }
        }
    });
    DeviceSocket {
        task: task.abort_handle(),
    }
}

enum Ended {
    Refused,
    Dropped,
}

async fn run(
    base: &url::Url,
    token: &str,
    events: &impl Fn(DeviceEvent),
    attempt: &mut u32,
) -> Ended {
    let connector = tokio_tungstenite::Connector::Rustls(std::sync::Arc::new(tls_config()));
    let connected = tokio::time::timeout(
        CONNECT_TIMEOUT,
        tokio_tungstenite::connect_async_tls_with_config(
            socket_url(base).as_str(),
            None,
            false,
            Some(connector),
        ),
    )
    .await;
    let Ok(Ok((mut ws, _))) = connected else {
        return Ended::Dropped;
    };
    // Until the open packet says otherwise, RomM's defaults.
    let mut quiet = quiet_limit("0{}");
    // A link that dies without closing (sleep, a dropped Wi-Fi) shows only as silence.
    while let Ok(Some(Ok(message))) = tokio::time::timeout(quiet, ws.next()).await {
        let Message::Text(text) = message else {
            continue;
        };
        let reply = match socketio::parse(&text) {
            Frame::Open => {
                quiet = quiet_limit(&text);
                Some(socketio::connect(
                    NAMESPACE,
                    &serde_json::json!({ "token": token }),
                ))
            }
            Frame::Ping => Some(socketio::PONG.to_string()),
            Frame::Connected { namespace } if namespace == NAMESPACE => {
                *attempt = 0;
                events(DeviceEvent::Connected);
                None
            }
            Frame::ConnectError { namespace, message } if namespace == NAMESPACE => {
                events(DeviceEvent::Refused(message));
                return Ended::Refused;
            }
            Frame::Event {
                namespace,
                name,
                data,
            } if namespace == NAMESPACE => {
                let id = data["id"].as_str().unwrap_or_default().to_string();
                match (name.as_str(), data["rom_id"].as_i64()) {
                    ("install:queued", Some(rom_id)) => {
                        events(DeviceEvent::InstallQueued { id, rom_id });
                    }
                    ("install:cancelled", _) => events(DeviceEvent::InstallCancelled { id }),
                    _ => {}
                }
                None
            }
            Frame::Close => return Ended::Dropped,
            Frame::Disconnected { namespace } if namespace == NAMESPACE => return Ended::Dropped,
            _ => None,
        };
        if let Some(reply) = reply {
            if ws.send(Message::text(reply)).await.is_err() {
                return Ended::Dropped;
            }
        }
    }
    Ended::Dropped
}

/// Checks certificates the way reqwest does for RomP's other requests.
fn tls_config() -> rustls::ClientConfig {
    use rustls_platform_verifier::ConfigVerifierExt;
    rustls::ClientConfig::with_platform_verifier().expect("platform certificate verifier")
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use std::time::Duration;
    use tokio_tungstenite::tungstenite::Message;

    const OPEN: &str = r#"0{"sid":"s","pingInterval":25000,"pingTimeout":20000}"#;

    #[test]
    fn the_socket_lives_under_romms_address() {
        let url = |base: &str| socket_url(&url::Url::parse(base).unwrap()).to_string();
        assert_eq!(
            url("http://romm.home/"),
            "ws://romm.home/ws/socket.io/?EIO=4&transport=websocket"
        );
        assert_eq!(
            url("https://h/romm/"),
            "wss://h/romm/ws/socket.io/?EIO=4&transport=websocket"
        );
    }

    #[test]
    fn reconnecting_backs_off_to_a_minute() {
        assert_eq!(backoff(0).as_secs(), 1);
        assert_eq!(backoff(3).as_secs(), 8);
        assert_eq!(backoff(10).as_secs(), 60);
    }

    async fn romm() -> (tokio::net::TcpListener, url::Url) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = url::Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        (listener, base)
    }

    #[tokio::test]
    async fn events_come_through_and_pings_are_answered() {
        let (listener, base) = romm().await;
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
            ws.send(Message::text(OPEN)).await.unwrap();
            let connect = ws.next().await.unwrap().unwrap().into_text().unwrap();
            assert_eq!(connect.as_str(), r#"40/devices,{"token":"rmm_t"}"#);
            ws.send(Message::text(r#"40/devices,{"sid":"x"}"#))
                .await
                .unwrap();
            ws.send(Message::text("2")).await.unwrap();
            let pong = ws.next().await.unwrap().unwrap().into_text().unwrap();
            assert_eq!(pong.as_str(), "3");
            ws.send(Message::text(
                r#"42/devices,["install:queued",{"id":"r1","rom_id":7}]"#,
            ))
            .await
            .unwrap();
            ws.send(Message::text(
                r#"42/devices,["install:cancelled",{"id":"r1","rom_id":7}]"#,
            ))
            .await
            .unwrap();
            let _ = ws.next().await;
        });
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let _socket = spawn(
            &tokio::runtime::Handle::current(),
            base,
            "rmm_t".into(),
            move |e| {
                let _ = tx.send(e);
            },
        );
        assert_eq!(rx.recv().await, Some(DeviceEvent::Connected));
        assert_eq!(
            rx.recv().await,
            Some(DeviceEvent::InstallQueued {
                id: "r1".into(),
                rom_id: 7
            })
        );
        assert_eq!(
            rx.recv().await,
            Some(DeviceEvent::InstallCancelled { id: "r1".into() })
        );
        server.abort();
    }

    #[tokio::test]
    async fn a_connection_that_goes_quiet_is_dropped_and_reopened() {
        let (listener, base) = romm().await;
        tokio::spawn(async move {
            // RomM promises a ping every 50 ms, then never sends one, as a dead link would.
            let (tcp, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
            ws.send(Message::text(
                r#"0{"sid":"s","pingInterval":50,"pingTimeout":50}"#,
            ))
            .await
            .unwrap();
            let _ = ws.next().await;
            ws.send(Message::text(r#"40/devices,{"sid":"x"}"#))
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_secs(30)).await;
        });
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let _socket = spawn(
            &tokio::runtime::Handle::current(),
            base,
            "rmm_t".into(),
            move |e| {
                let _ = tx.send(e);
            },
        );
        assert_eq!(rx.recv().await, Some(DeviceEvent::Connected));
        let next = tokio::time::timeout(Duration::from_secs(3), rx.recv()).await;
        assert_eq!(next.ok().flatten(), Some(DeviceEvent::Disconnected));
    }

    #[test]
    fn the_open_packet_says_how_long_a_quiet_link_lasts() {
        assert_eq!(
            quiet_limit(r#"0{"sid":"s","pingInterval":25000,"pingTimeout":20000}"#),
            Duration::from_secs(45)
        );
        assert_eq!(quiet_limit("0{}"), Duration::from_secs(45));
    }

    #[tokio::test]
    async fn a_refusal_ends_the_connection_for_good() {
        let (listener, base) = romm().await;
        tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
            ws.send(Message::text(OPEN)).await.unwrap();
            let _ = ws.next().await;
            ws.send(Message::text(r#"44/devices,{"message":"unauthorized"}"#))
                .await
                .unwrap();
            let _ = ws.next().await;
        });
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let _socket = spawn(
            &tokio::runtime::Handle::current(),
            base,
            "rmm_t".into(),
            move |e| {
                let _ = tx.send(e);
            },
        );
        assert_eq!(
            rx.recv().await,
            Some(DeviceEvent::Refused("unauthorized".into()))
        );
        assert_eq!(rx.recv().await, None, "no reconnect after a refusal");
    }
}
