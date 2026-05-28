pub mod use_cases {
    pub mod rag_indexer;
    pub mod sort_pipeline;
}

pub use use_cases::rag_indexer::{RagChunk, RagIndexer};
pub use use_cases::sort_pipeline::MailSortingPipeline;
