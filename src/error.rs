use std::io;
use std::sync::mpsc;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum CpgError {
    #[error("Could not initialize terminal")]
    IoErr(#[from] io::Error),
    #[error("Could not read input")]
    StremingReceiveError(#[from] mpsc::RecvError),
    #[error("Could not send input to terminal")]
    StreamingSendError,
}
