use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PadState {
    pub buttons: u16,
    pub axes: [i16; 6],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AppMsg {
    Pad { port: u8, state: PadState },
    Pointer { x: i16, y: i16, pressed: bool },
    Mouse { dx: i16, dy: i16, buttons: u8 },
    PortDevice { port: u8, device: u32 },
    Pause(bool),
    Volume(u8),
    SaveSlot(u8),
    LoadSlot(u8),
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RunnerMsg {
    Started {
        core_name: String,
        core_version: String,
        fps: f64,
        sample_rate: f64,
    },
    Controllers {
        ports: Vec<Vec<(String, u32)>>,
    },
    SramWritten,
    StateWritten {
        slot: u8,
        ok: bool,
    },
    StateLoaded {
        slot: u8,
        ok: bool,
    },
    Exited {
        error: Option<String>,
    },
}
