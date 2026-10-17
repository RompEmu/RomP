//! The text frames of Socket.IO 5 over an Engine.IO 4 WebSocket: just what RomP's device
//! connection needs.
use serde_json::Value;

pub const PONG: &str = "3";

#[derive(Debug, Clone, PartialEq)]
pub enum Frame {
    Open,
    Ping,
    Close,
    Connected {
        namespace: String,
    },
    ConnectError {
        namespace: String,
        message: String,
    },
    Event {
        namespace: String,
        name: String,
        data: Value,
    },
    Disconnected {
        namespace: String,
    },
    Other,
}

pub fn connect(namespace: &str, auth: &Value) -> String {
    format!("40{namespace},{auth}")
}

/// `/devices,rest`, or `rest` for the main namespace.
fn split_namespace(packet: &str) -> (String, &str) {
    if !packet.starts_with('/') {
        return ("/".to_string(), packet);
    }
    match packet.split_once(',') {
        Some((namespace, rest)) => (namespace.to_string(), rest),
        None => (packet.to_string(), ""),
    }
}

pub fn parse(text: &str) -> Frame {
    let mut chars = text.chars();
    match chars.next() {
        Some('0') => Frame::Open,
        Some('1') => Frame::Close,
        Some('2') => Frame::Ping,
        Some('4') => socket_packet(chars.as_str()),
        _ => Frame::Other,
    }
}

fn socket_packet(packet: &str) -> Frame {
    let mut chars = packet.chars();
    let kind = chars.next();
    let (namespace, rest) = split_namespace(chars.as_str());
    match kind {
        Some('0') => Frame::Connected { namespace },
        Some('1') => Frame::Disconnected { namespace },
        Some('4') => {
            let message = serde_json::from_str::<Value>(rest)
                .ok()
                .and_then(|v| v["message"].as_str().map(str::to_string))
                .unwrap_or_default();
            Frame::ConnectError { namespace, message }
        }
        Some('2') => {
            // An acknowledgement id may come before the array.
            let body = rest.trim_start_matches(|c: char| c.is_ascii_digit());
            match serde_json::from_str::<Value>(body) {
                Ok(Value::Array(items)) => {
                    let mut items = items.into_iter();
                    match items.next() {
                        Some(Value::String(name)) => Frame::Event {
                            namespace,
                            name,
                            data: items.next().unwrap_or(Value::Null),
                        },
                        _ => Frame::Other,
                    }
                }
                _ => Frame::Other,
            }
        }
        _ => Frame::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_frames_are_told_apart() {
        assert_eq!(
            parse(r#"0{"sid":"a","pingInterval":25000,"pingTimeout":20000}"#),
            Frame::Open
        );
        assert_eq!(parse("2"), Frame::Ping);
        assert_eq!(parse("1"), Frame::Close);
        assert_eq!(parse("6"), Frame::Other);
    }

    #[test]
    fn namespace_packets_carry_their_namespace() {
        assert_eq!(
            parse(r#"40/devices,{"sid":"x"}"#),
            Frame::Connected {
                namespace: "/devices".into()
            }
        );
        assert_eq!(
            parse(r#"44/devices,{"message":"unauthorized"}"#),
            Frame::ConnectError {
                namespace: "/devices".into(),
                message: "unauthorized".into()
            }
        );
        assert_eq!(
            parse("41/devices,"),
            Frame::Disconnected {
                namespace: "/devices".into()
            }
        );
    }

    #[test]
    fn events_have_a_name_and_data_with_or_without_an_ack_id() {
        let event = |text| match parse(text) {
            Frame::Event {
                namespace,
                name,
                data,
            } => (namespace, name, data),
            other => panic!("{other:?}"),
        };
        let (ns, name, data) = event(r#"42/devices,["install:queued",{"id":"r1","rom_id":7}]"#);
        assert_eq!((ns.as_str(), name.as_str()), ("/devices", "install:queued"));
        assert_eq!(data["rom_id"], 7);
        let (_, name, _) = event(r#"42/devices,12["install:cancelled",{"id":"r1","rom_id":7}]"#);
        assert_eq!(name, "install:cancelled");
        let (ns, name, _) = event(r#"42["hello",{}]"#);
        assert_eq!((ns.as_str(), name.as_str()), ("/", "hello"));
    }

    #[test]
    fn connecting_sends_the_token_to_the_namespace() {
        assert_eq!(
            connect("/devices", &serde_json::json!({"token": "rmm_x"})),
            r#"40/devices,{"token":"rmm_x"}"#
        );
    }

    #[test]
    fn garbage_is_other() {
        assert_eq!(parse(""), Frame::Other);
        assert_eq!(parse("42/devices,not json"), Frame::Other);
    }
}
