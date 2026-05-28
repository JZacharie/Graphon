pub mod use_cases {
    pub mod label_organizer;
    pub mod rag_indexer;
    pub mod sort_pipeline;
}

pub use graphon_core::entities::{LabelCategory, LabelInfo, RagChunk, SearchQuery, SearchResult};
pub use use_cases::label_organizer::LabelOrganizer;
pub use use_cases::rag_indexer::RagIndexer;
pub use use_cases::sort_pipeline::MailSortingPipeline;
