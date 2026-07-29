use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum StoreError {
    #[error("{0} was not found")]
    NotFound(&'static str),
    #[error("{0} already exists")]
    Conflict(&'static str),
    #[error("tenant scope violation")]
    TenantScope,
    #[error("database operation failed")]
    Database(String),
}

pub type StoreResult<T> = Result<T, StoreError>;
