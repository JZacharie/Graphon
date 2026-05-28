use async_trait::async_trait;
use graphon_core::entities::RagChunk;
use graphon_core::error::GraphonError;
use graphon_core::ports::VectorStorePort;
use serde::{Deserialize, Serialize};
use tracing::info;

pub struct QdrantAdapter {
    client: reqwest::Client,
    qdrant_url: String,
    llm_api_key: Option<String>,
}

#[derive(Serialize)]
struct EmbedRequest {
    model: String,
    content: EmbedContent,
}

#[derive(Serialize)]
struct EmbedContent {
    parts: Vec<EmbedPart>,
}

#[derive(Serialize)]
struct EmbedPart {
    text: String,
}

#[derive(Serialize)]
struct BatchEmbedRequest {
    requests: Vec<EmbedRequest>,
}

#[derive(Deserialize)]
struct BatchEmbedResponse {
    embeddings: Vec<Embedding>,
}

#[derive(Deserialize)]
struct Embedding {
    values: Vec<f32>,
}

impl QdrantAdapter {
    pub fn new(qdrant_url: Option<String>, llm_api_key: Option<String>) -> Self {
        let qdrant_url =
            qdrant_url.unwrap_or_else(|| "http://qdrant.qdrant.svc.cluster.local:6333".to_string());
        Self {
            client: reqwest::Client::new(),
            qdrant_url,
            llm_api_key,
        }
    }

    async fn get_embeddings(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, GraphonError> {
        let api_key = match &self.llm_api_key {
            Some(key) => key,
            None => {
                return Err(GraphonError::Classifier(
                    "LLM_API_KEY not configured. Cannot generate embeddings.".to_string(),
                ))
            }
        };

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/text-embedding-004:batchEmbedContents?key={}",
            api_key
        );

        let requests = texts
            .iter()
            .map(|t| EmbedRequest {
                model: "models/text-embedding-004".to_string(),
                content: EmbedContent {
                    parts: vec![EmbedPart { text: t.clone() }],
                },
            })
            .collect();

        let body = BatchEmbedRequest { requests };
        let response = self.client.post(&url).json(&body).send().await?;

        if !response.status().is_success() {
            let err = response.text().await.unwrap_or_default();
            return Err(GraphonError::Classifier(format!(
                "Gemini Embedding API error: {}",
                err
            )));
        }

        let res: BatchEmbedResponse = response.json().await?;
        Ok(res.embeddings.into_iter().map(|e| e.values).collect())
    }
}

#[async_trait]
impl VectorStorePort for QdrantAdapter {
    async fn index_chunks(&self, chunks: &[RagChunk]) -> Result<(), GraphonError> {
        if chunks.is_empty() {
            return Ok(());
        }

        info!("Starting Qdrant indexing for {} chunks...", chunks.len());

        // 1. Ensure collection exists
        let create_url = format!("{}/collections/emails", self.qdrant_url);
        let create_body = serde_json::json!({
            "vectors": {
                "size": 768,
                "distance": "Cosine"
            }
        });

        let _ = self.client.put(&create_url).json(&create_body).send().await;

        // 2. Fetch embeddings for all chunks in batch
        let texts: Vec<String> = chunks.iter().map(|c| c.content.clone()).collect();
        let embeddings = self.get_embeddings(&texts).await?;

        if embeddings.len() != chunks.len() {
            return Err(GraphonError::Classifier(format!(
                "Mismatch: expected {} embeddings, received {}",
                chunks.len(),
                embeddings.len()
            )));
        }

        // 3. Prepare points for Qdrant
        let mut points = Vec::new();
        for (i, chunk) in chunks.iter().enumerate() {
            let point_id = fnv1a_64(&format!("{}-{}", chunk.email_id, chunk.chunk_index));
            let vector = &embeddings[i];

            points.push(serde_json::json!({
                "id": point_id,
                "vector": vector,
                "payload": {
                    "email_id": chunk.email_id,
                    "subject": chunk.subject,
                    "sender": chunk.sender,
                    "chunk_index": chunk.chunk_index,
                    "content": chunk.content
                }
            }));
        }

        // 4. Push points to Qdrant
        let points_url = format!("{}/collections/emails/points?wait=true", self.qdrant_url);
        let points_body = serde_json::json!({ "points": points });

        let response = self
            .client
            .put(&points_url)
            .json(&points_body)
            .send()
            .await?;

        if !response.status().is_success() {
            let err = response.text().await.unwrap_or_default();
            return Err(GraphonError::Internal(format!(
                "Qdrant push points error: {}",
                err
            )));
        }

        info!("Successfully indexed {} chunks in Qdrant.", chunks.len());
        Ok(())
    }
}

fn fnv1a_64(s: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325;
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
