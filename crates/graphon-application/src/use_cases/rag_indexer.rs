use graphon_core::entities::{RagChunk, SearchQuery, SearchResult};
use graphon_core::error::GraphonError;
use graphon_core::ports::{StoragePort, VectorStorePort};
use std::sync::Arc;
use tracing::info;

pub struct RagIndexer {
    storage: Arc<dyn StoragePort>,
    vector_store: Arc<dyn VectorStorePort>,
}

impl RagIndexer {
    pub fn new(storage: Arc<dyn StoragePort>, vector_store: Arc<dyn VectorStorePort>) -> Self {
        Self {
            storage,
            vector_store,
        }
    }

    fn chunk_email(&self, email: &graphon_core::entities::Email) -> Vec<RagChunk> {
        let content_to_chunk = format!(
            "From: {}\nSubject: {}\nDate: {}\n\n{}",
            email.from, email.subject, email.date, email.body
        );

        let chunk_size = 1000;
        let overlap = 200;
        let mut chunks = Vec::new();
        let chars: Vec<char> = content_to_chunk.chars().collect();
        let mut start = 0;

        while start < chars.len() {
            let end = std::cmp::min(start + chunk_size, chars.len());
            let chunk_content: String = chars[start..end].iter().collect();

            chunks.push(RagChunk {
                email_id: email.id.clone(),
                subject: email.subject.clone(),
                sender: email.from.clone(),
                chunk_index: chunks.len(),
                content: chunk_content,
            });

            if end == chars.len() {
                break;
            }
            start += chunk_size - overlap;
        }

        chunks
    }

    pub async fn index_email_for_rag(&self, email_id: &str) -> Result<Vec<RagChunk>, GraphonError> {
        info!("Indexing email {} for RAG pipeline...", email_id);

        let email_opt = self.storage.get_email(email_id).await?;
        let email = match email_opt {
            Some(e) => e,
            None => {
                return Err(GraphonError::NotFound(format!(
                    "Email with ID {}",
                    email_id
                )))
            }
        };

        let chunks = self.chunk_email(&email);
        info!(
            "Created {} RAG chunks for email ID {}",
            chunks.len(),
            email_id
        );
        self.vector_store.index_chunks(&chunks).await?;
        Ok(chunks)
    }

    pub async fn reindex_all(&self, batch_size: usize) -> Result<usize, GraphonError> {
        info!("Reindexing all emails for RAG pipeline...");

        let emails = self.storage.get_recent_emails(batch_size).await?;
        let mut total_chunks = 0;

        for email in &emails {
            info!("Indexing email ID {}: {}", email.id, email.subject);
            let chunks = self.chunk_email(email);
            if !chunks.is_empty() {
                self.vector_store.index_chunks(&chunks).await?;
                total_chunks += chunks.len();
            }
        }

        info!(
            "Reindexing complete: {} emails processed, {} chunks indexed.",
            emails.len(),
            total_chunks
        );
        Ok(total_chunks)
    }

    pub async fn search(
        &self,
        query_text: &str,
        limit: u64,
    ) -> Result<Vec<SearchResult>, GraphonError> {
        info!("Searching RAG index for: '{}'", query_text);
        let query = SearchQuery {
            query_text: query_text.to_string(),
            limit,
        };
        self.vector_store.search(&query).await
    }
}
