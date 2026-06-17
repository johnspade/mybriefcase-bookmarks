#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("{0}")]
    NotFound(String),

    #[error("{0}")]
    Validation(String),

    #[error("{0}")]
    DocumentCorrupted(String),

    #[error(transparent)]
    Automerge(#[from] automerge::AutomergeError),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}
