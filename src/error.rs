use std::io;
use std::sync::mpsc;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Could not initialize terminal")]
    IoErr(#[from] io::Error),
    #[error("Could not read input")]
    StraemingReceiveError(#[from] mpsc::RecvError),
    #[error("Could not send input to terminal")]
    StreamingSendError,
}
