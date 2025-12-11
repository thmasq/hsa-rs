use thiserror::Error;

#[derive(Error, Debug)]
pub enum HsaError {
    #[error("I/O Error: {0}")]
    Io(#[from] std::io::Error),

    #[error("KFD Driver Error: {0}")]
    Driver(String),

    #[error("Operation timed out")]
    WaitTimeout,

    #[error("Out of GPU Memory")]
    OutOfMemory,

    #[error("Invalid node ID: {0}")]
    InvalidNodeId(u32),

    #[error("General Thunk Error: {0}")]
    General(String),
}

// A convenient alias
pub type HsaResult<T> = Result<T, HsaError>;
