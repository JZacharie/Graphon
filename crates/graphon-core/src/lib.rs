pub mod entities;
pub mod error;
pub mod ports;

pub use error::GraphonError;
pub type Result<T> = std::result::Result<T, GraphonError>;
