// use std::path::PathBuf;
use std::path::PathBuf;

use ipc_channel::ipc::IpcSender;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    Handshake(IpcSender<Response>),
    MakeAudioThumb { path: PathBuf },
    MakeVideoThumb { path: PathBuf },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    AudioThumb(Vec<i16>),
    VideoThumb(Vec<u8>),
    Error(String),
}
