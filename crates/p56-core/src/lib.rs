//! `p56-core` — protocol, transcode and networking for Holoscope / P56 POV hologram fans.
//!
//! See `PROTOCOL.md` for the reverse-engineered wire format. Layers:
//! - [`protocol`]: packet builders and parser (pure, no I/O)
//! - [`transcode`]: image → fan-layout JPEG, modelled on Holoscope.exe
//! - [`net`]: UDP discovery, TCP server, commands, upload and live cast
//! - [`video`]: frame source via an external `ffmpeg` binary (optional, desktop)

pub mod net;
pub mod protocol;
pub mod transcode;
pub mod video;

pub use net::{Config, Device, Event, Link, UploadOutcome};
pub use protocol::{Bluetooth, ColorMode, FanError, MachineInfo, Packet, Status};
pub use transcode::{FanType, Frame, Placement, Support};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no fan connected")]
    NotConnected,
    #[error("i/o: {0}")]
    Io(String),
    #[error("invalid: {0}")]
    Invalid(String),
    #[error("fan reports: {0}")]
    Fan(FanError),
    #[error("image: {0}")]
    Image(#[from] image::ImageError),
}
