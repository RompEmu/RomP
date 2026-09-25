use serde::{de::DeserializeOwned, Serialize};
use std::io::{self, Read, Write};

const MAX_LEN: u32 = 1 << 20;

pub fn write_msg<W: Write, T: Serialize>(w: &mut W, msg: &T) -> io::Result<()> {
    let body =
        postcard::to_stdvec(msg).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    w.write_all(&(body.len() as u32).to_le_bytes())?;
    w.write_all(&body)?;
    w.flush()
}

pub fn read_msg<R: Read, T: DeserializeOwned>(r: &mut R) -> io::Result<T> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len);
    if len > MAX_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "message too large",
        ));
    }
    let mut body = vec![0u8; len as usize];
    r.read_exact(&mut body)?;
    postcard::from_bytes(&body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::msg::{AppMsg, PadState, RunnerMsg};
    use std::io::Cursor;

    #[test]
    fn round_trips_several_messages_in_order() {
        let mut buf = Vec::new();
        let pad = PadState {
            buttons: 0b1001,
            axes: [1, -2, 3, -4, 5, 6],
        };
        write_msg(
            &mut buf,
            &AppMsg::Pad {
                port: 1,
                state: pad,
            },
        )
        .unwrap();
        write_msg(&mut buf, &AppMsg::SaveSlot(3)).unwrap();
        let mut r = Cursor::new(buf);
        assert_eq!(
            read_msg::<_, AppMsg>(&mut r).unwrap(),
            AppMsg::Pad {
                port: 1,
                state: pad
            }
        );
        assert_eq!(read_msg::<_, AppMsg>(&mut r).unwrap(), AppMsg::SaveSlot(3));
    }

    #[test]
    fn round_trips_runner_messages_with_strings() {
        let mut buf = Vec::new();
        let msg = RunnerMsg::Exited {
            error: Some("core rejected the game".into()),
        };
        write_msg(&mut buf, &msg).unwrap();
        assert_eq!(
            read_msg::<_, RunnerMsg>(&mut Cursor::new(buf)).unwrap(),
            msg
        );
    }

    #[test]
    fn eof_is_unexpected_eof() {
        let err = read_msg::<_, AppMsg>(&mut Cursor::new(Vec::new())).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn truncated_body_is_unexpected_eof() {
        let mut buf = Vec::new();
        write_msg(&mut buf, &AppMsg::LoadSlot(1)).unwrap();
        buf.pop();
        let err = read_msg::<_, AppMsg>(&mut Cursor::new(buf)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn oversized_length_is_rejected() {
        let buf = u32::MAX.to_le_bytes().to_vec();
        let err = read_msg::<_, AppMsg>(&mut Cursor::new(buf)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn garbage_body_is_invalid_data() {
        let mut buf = 2u32.to_le_bytes().to_vec();
        buf.extend_from_slice(&[0xff, 0xff]);
        let err = read_msg::<_, AppMsg>(&mut Cursor::new(buf)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }
}
