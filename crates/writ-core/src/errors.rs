use thiserror::Error;

#[derive(Error, Debug)]
pub enum WritError {
    #[error("buffer not found: {id}")]
    BufferNotFound { id: String },

    #[error("buffer already exists: {id}")]
    BufferAlreadyExists { id: String },

    #[error("invalid config: {message}")]
    InvalidConfig { message: String },

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(String),
}

pub type WritResult<T> = Result<T, WritError>;
