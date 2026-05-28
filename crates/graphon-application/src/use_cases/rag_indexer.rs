use graphon_core::error::GraphonError;
use graphon_core::ports::StoragePort;
use std::sync::Arc;
use tracing::info;

pub struct RagIndexer {
    storage: Arc<dyn StoragePort>,
}

#[derive(serde::Serialize)]
pub struct RagChunk {
    pub email_id: String,
    pub subject: String,
    pub sender: String,
    pub chunk_index: usize,
    pub content: String,
}

impl RagIndexer {
    pub fn new(storage: Arc<dyn StoragePort>) -> Self {
        Self { storage }
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

        let content_to_chunk = format!(
            "From: {}\nSubject: {}\nDate: {}\n\n{}",
            email.from, email.subject, email.date, email.body
        );

        // Simple chunking strategy (e.g., character-based chunking with overlap)
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

        info!(
            "Created {} RAG chunks for email ID {}",
            chunks.len(),
            email_id
        );
        Ok(chunks)
    }
}
