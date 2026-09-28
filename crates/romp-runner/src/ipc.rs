use crate::input::InputState;
use parking_lot::Mutex;
use romp_proto::msg::{AppMsg, RunnerMsg};
use romp_proto::socket::UnixStream;
use romp_proto::wire;
use std::path::Path;
use std::sync::mpsc::{self, Receiver};

pub struct Link {
    writer: Mutex<UnixStream>,
    inbox: Receiver<AppMsg>,
}

impl Link {
    pub fn connect(path: &Path, input: InputState) -> std::io::Result<Self> {
        Self::from_stream(UnixStream::connect(path)?, input)
    }

    pub fn from_stream(stream: UnixStream, input: InputState) -> std::io::Result<Self> {
        let mut reader = stream.try_clone()?;
        let (tx, inbox) = mpsc::channel();
        std::thread::spawn(move || loop {
            match wire::read_msg::<_, AppMsg>(&mut reader) {
                Ok(AppMsg::Pad { port, state }) => input.apply_pad(port, state),
                Ok(AppMsg::Pointer { x, y, pressed }) => {
                    input.apply_pointer(crate::input::Pointer { x, y, pressed })
                }
                Ok(AppMsg::Mouse { dx, dy, buttons }) => input.apply_mouse(dx, dy, buttons),
                Ok(msg) => {
                    if tx.send(msg).is_err() {
                        return;
                    }
                }
                Err(_) => {
                    let _ = tx.send(AppMsg::Shutdown);
                    return;
                }
            }
        });
        Ok(Self {
            writer: Mutex::new(stream),
            inbox,
        })
    }

    pub fn send(&self, msg: &RunnerMsg) {
        if let Err(e) = wire::write_msg(&mut *self.writer.lock(), msg) {
            tracing::warn!("send to app: {e}");
        }
    }

    pub fn drain(&self) -> Vec<AppMsg> {
        self.inbox.try_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use romp_proto::msg::PadState;
    use std::time::{Duration, Instant};

    fn wait_for(mut cond: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !cond() {
            assert!(Instant::now() < deadline, "timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn pad_goes_to_input_and_controls_to_inbox() {
        let (mut app, runner) = UnixStream::pair().unwrap();
        let input = InputState::default();
        let link = Link::from_stream(runner, input.clone()).unwrap();
        let state = PadState {
            buttons: 1 << 8,
            axes: [0; 6],
        };
        wire::write_msg(&mut app, &AppMsg::Pad { port: 0, state }).unwrap();
        wire::write_msg(&mut app, &AppMsg::SaveSlot(2)).unwrap();
        wait_for(|| input.is_pressed(0, 8));
        let mut got = Vec::new();
        wait_for(|| {
            got.extend(link.drain());
            !got.is_empty()
        });
        assert_eq!(got, vec![AppMsg::SaveSlot(2)]);
    }

    #[test]
    fn eof_from_app_becomes_shutdown() {
        let (app, runner) = UnixStream::pair().unwrap();
        let link = Link::from_stream(runner, InputState::default()).unwrap();
        drop(app);
        let mut got = Vec::new();
        wait_for(|| {
            got.extend(link.drain());
            !got.is_empty()
        });
        assert_eq!(got, vec![AppMsg::Shutdown]);
    }

    #[test]
    fn send_reaches_app() {
        let (mut app, runner) = UnixStream::pair().unwrap();
        let link = Link::from_stream(runner, InputState::default()).unwrap();
        link.send(&RunnerMsg::StateWritten { slot: 1, ok: true });
        let msg: RunnerMsg = wire::read_msg(&mut app).unwrap();
        assert_eq!(msg, RunnerMsg::StateWritten { slot: 1, ok: true });
    }
}
