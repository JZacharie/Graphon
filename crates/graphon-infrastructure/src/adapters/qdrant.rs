use async_trait::async_trait;
use graphon_core::entities::{RagChunk, SearchQuery, SearchResult};
use graphon_core::error::GraphonError;
use graphon_core::ports::VectorStorePort;
use serde::{Deserialize, Serialize};
use tracing::info;

pub struct QdrantAdapter {
    client: reqwest::Client,
    qdrant_url: String,
    collection_name: String,
    vector_size: usize,
    llm_api_key: Option<String>,
    pylos_base_url: Option<String>,
    pylos_api_key: Option<String>,
    pylos_embedding_model: Option<String>,
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

#[derive(Serialize)]
struct PylosEmbedRequest {
    model: String,
    input: Vec<String>,
}

#[derive(Deserialize)]
struct PylosEmbedResponse {
    data: Vec<PylosEmbeddingData>,
}

#[derive(Deserialize)]
struct PylosEmbeddingData {
    index: usize,
    embedding: Vec<f32>,
}

#[derive(Deserialize)]
struct QdrantSearchResponse {
    result: Vec<QdrantScoredPoint>,
}

#[derive(Deserialize)]
struct QdrantScoredPoint {
    #[allow(dead_code)]
    id: Option<serde_json::Value>,
    #[allow(dead_code)]
    version: Option<f64>,
    score: Option<f64>,
    payload: Option<serde_json::Value>,
}

impl QdrantAdapter {
    pub fn new(
        qdrant_url: Option<String>,
        collection_name: Option<String>,
        vector_size: Option<usize>,
        llm_api_key: Option<String>,
        pylos_base_url: Option<String>,
        pylos_api_key: Option<String>,
        pylos_embedding_model: Option<String>,
    ) -> Self {
        let qdrant_url =
            qdrant_url.unwrap_or_else(|| "http://qdrant.qdrant.svc.cluster.local:6333".to_string());
        let collection_name = collection_name.unwrap_or_else(|| "emails".to_string());
        let vector_size = vector_size.unwrap_or(768);
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_default();
        Self {
            client,
            qdrant_url,
            collection_name,
            vector_size,
            llm_api_key,
            pylos_base_url,
            pylos_api_key,
            pylos_embedding_model,
        }
    }

    pub fn collection_url(&self) -> String {
        format!("{}/collections/{}", self.qdrant_url, self.collection_name)
    }

    pub fn points_url(&self) -> String {
        format!(
            "{}/collections/{}/points",
            self.qdrant_url, self.collection_name
        )
    }

    pub fn collection_name(&self) -> &str {
        &self.collection_name
    }

    async fn get_embeddings(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, GraphonError> {
        if let Some(ref model) = self.pylos_embedding_model {
            let base_url = self.pylos_base_url.as_deref().unwrap_or("http://localhost:3000");
            let url = format!("{}/v1/embeddings", base_url.trim_end_matches('/'));
            let body = PylosEmbedRequest {
                model: model.clone(),
                input: texts.to_vec(),
            };
            let mut req = self.client.post(&url).json(&body);
            if let Some(ref key) = self.pylos_api_key {
                req = req.header("Authorization", format!("Bearer {}", key));
            }
            let response = req.send().await?;
            if !response.status().is_success() {
                let err = response.text().await.unwrap_or_default();
                return Err(GraphonError::Classifier(format!(
                    "Pylos Embedding API error: {}",
                    err
                )));
            }
            let res: PylosEmbedResponse = response.json().await?;
            let mut data = res.data;
            data.sort_by_key(|d| d.index);
            Ok(data.into_iter().map(|d| d.embedding).collect())
        } else {
            let api_key = match &self.llm_api_key {
                Some(key) => key,
                None => {
                    return Err(GraphonError::Classifier(
                        "LLM_API_KEY/PYLOS_EMBEDDING_MODEL not configured. Cannot generate embeddings.".to_string(),
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

    async fn get_single_embedding(&self, text: &str) -> Result<Vec<f32>, GraphonError> {
        if let Some(ref model) = self.pylos_embedding_model {
            let base_url = self.pylos_base_url.as_deref().unwrap_or("http://localhost:3000");
            let url = format!("{}/v1/embeddings", base_url.trim_end_matches('/'));
            let body = PylosEmbedRequest {
                model: model.clone(),
                input: vec![text.to_string()],
            };
            let mut req = self.client.post(&url).json(&body);
            if let Some(ref key) = self.pylos_api_key {
                req = req.header("Authorization", format!("Bearer {}", key));
            }
            let response = req.send().await?;
            if !response.status().is_success() {
                let err = response.text().await.unwrap_or_default();
                return Err(GraphonError::Classifier(format!(
                    "Pylos Embedding API error: {}",
                    err
                )));
            }
            let res: PylosEmbedResponse = response.json().await?;
            let embedding = res.data.into_iter().next().map(|d| d.embedding).ok_or_else(|| {
                GraphonError::Classifier("Pylos embedding returned empty results".to_string())
            })?;
            Ok(embedding)
        } else {
            let api_key = match &self.llm_api_key {
                Some(key) => key,
                None => {
                    return Err(GraphonError::Classifier(
                        "LLM_API_KEY/PYLOS_EMBEDDING_MODEL not configured. Cannot generate embeddings.".to_string(),
                    ))
                }
            };

            let url = format!(
                "https://generativelanguage.googleapis.com/v1beta/models/text-embedding-004:embedContent?key={}",
                api_key
            );

            let body = serde_json::json!({
                "model": "models/text-embedding-004",
                "content": {
                    "parts": [{"text": text}]
                }
            });

            let response = self.client.post(&url).json(&body).send().await?;

            if !response.status().is_success() {
                let err = response.text().await.unwrap_or_default();
                return Err(GraphonError::Classifier(format!(
                    "Gemini Embedding API error: {}",
                    err
                )));
            }

            #[derive(Deserialize)]
            struct EmbedResponse {
                embedding: Embedding,
            }

            let res: EmbedResponse = response.json().await?;
            Ok(res.embedding.values)
        }
    }
}

#[async_trait]
impl VectorStorePort for QdrantAdapter {
    async fn create_collection(&self) -> Result<(), GraphonError> {
        info!(
            "Creating/ensuring Qdrant collection '{}'...",
            self.collection_name
        );
        let create_url = self.collection_url();
        let create_body = serde_json::json!({
            "vectors": {
                "size": self.vector_size,
                "distance": "Cosine"
            }
        });

        let response = self
            .client
            .put(&create_url)
            .json(&create_body)
            .send()
            .await?;

        if response.status().is_success() {
            info!("Qdrant collection '{}' ready.", self.collection_name);
            Ok(())
        } else {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            if status == 409 || body.contains("already exists") {
                info!(
                    "Qdrant collection '{}' already exists.",
                    self.collection_name
                );
                return Ok(());
            }
            Err(GraphonError::Internal(format!(
                "Failed to create Qdrant collection '{}': {} {}",
                self.collection_name, status, body
            )))
        }
    }

    async fn delete_collection(&self) -> Result<(), GraphonError> {
        info!("Deleting Qdrant collection '{}'...", self.collection_name);
        let delete_url = self.collection_url();
        let response = self.client.delete(&delete_url).send().await?;

        if response.status().is_success() || response.status() == 404 {
            info!("Qdrant collection '{}' deleted.", self.collection_name);
            Ok(())
        } else {
            let body = response.text().await.unwrap_or_default();
            Err(GraphonError::Internal(format!(
                "Failed to delete Qdrant collection '{}': {}",
                self.collection_name, body
            )))
        }
    }

    async fn index_chunks(&self, chunks: &[RagChunk]) -> Result<(), GraphonError> {
        if chunks.is_empty() {
            return Ok(());
        }

        info!(
            "Starting Qdrant indexing for {} chunks into '{}'...",
            chunks.len(),
            self.collection_name
        );

        let texts: Vec<String> = chunks.iter().map(|c| c.content.clone()).collect();
        let embeddings = self.get_embeddings(&texts).await?;

        if embeddings.len() != chunks.len() {
            return Err(GraphonError::Classifier(format!(
                "Mismatch: expected {} embeddings, received {}",
                chunks.len(),
                embeddings.len()
            )));
        }

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

        let points_url = format!("{}?wait=true", self.points_url());
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

        info!(
            "Successfully indexed {} chunks into '{}'.",
            chunks.len(),
            self.collection_name
        );
        Ok(())
    }

    async fn search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>, GraphonError> {
        info!(
            "Searching '{}' collection for: '{}'",
            self.collection_name, query.query_text
        );

        let query_vector = self.get_single_embedding(&query.query_text).await?;

        let search_url = format!("{}/points/search", self.collection_url());
        let search_body = serde_json::json!({
            "vector": query_vector,
            "limit": query.limit,
            "with_payload": true,
            "params": {
                "hnsw_ef": 128,
                "exact": false
            }
        });

        let response = self
            .client
            .post(&search_url)
            .json(&search_body)
            .send()
            .await?;

        if !response.status().is_success() {
            let err = response.text().await.unwrap_or_default();
            return Err(GraphonError::Internal(format!(
                "Qdrant search error: {}",
                err
            )));
        }

        let res: QdrantSearchResponse = response.json().await?;

        let results: Vec<SearchResult> = res
            .result
            .into_iter()
            .filter_map(|sp| {
                let payload = sp.payload?;
                let score = sp.score?;
                let email_id = payload.get("email_id")?.as_str()?.to_string();
                let subject = payload.get("subject")?.as_str()?.to_string();
                let sender = payload.get("sender")?.as_str()?.to_string();
                let chunk_index = payload.get("chunk_index")?.as_u64()? as usize;
                let content = payload.get("content")?.as_str()?.to_string();
                Some(SearchResult {
                    email_id,
                    subject,
                    sender,
                    chunk_index,
                    content,
                    score,
                })
            })
            .collect();

        info!("Found {} results from Qdrant search.", results.len());
        Ok(results)
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
