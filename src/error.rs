use std::io;
use std::sync::mpsc;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Could not initialize terminal")]
    Io(#[from] io::Error),
    #[error("Could not read input")]
    StreamingReceive(#[from] mpsc::RecvError),
    #[error("Could not send input to terminal")]
    StreamingSend,
}
