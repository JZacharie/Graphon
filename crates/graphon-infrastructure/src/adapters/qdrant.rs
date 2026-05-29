use async_trait::async_trait;
use graphon_core::entities::{RagChunk, SearchQuery, SearchResult};
use graphon_core::error::GraphonError;
use graphon_core::ports::VectorStorePort;
use qdrant_client::qdrant::{
    CreateCollectionBuilder, Distance, PointStruct, QueryPointsBuilder, UpsertPointsBuilder,
    VectorParamsBuilder,
};
use qdrant_client::Qdrant;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::info;

pub struct QdrantAdapter {
    qdrant_client: Qdrant,
    client: reqwest::Client,
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

        let qdrant_client = Qdrant::from_url(&qdrant_url).build().unwrap_or_else(|e| {
            panic!("Failed to initialize Qdrant client: {}", e);
        });

        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_default();

        Self {
            qdrant_client,
            client,
            collection_name,
            vector_size,
            llm_api_key,
            pylos_base_url,
            pylos_api_key,
            pylos_embedding_model,
        }
    }

    pub fn collection_name(&self) -> &str {
        &self.collection_name
    }

    async fn get_embeddings(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, GraphonError> {
        if let Some(ref model) = self.pylos_embedding_model {
            let base_url = self
                .pylos_base_url
                .as_deref()
                .unwrap_or("http://localhost:3000");
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
                None => return Err(GraphonError::Classifier(
                    "LLM_API_KEY/PYLOS_EMBEDDING_MODEL not configured. Cannot generate embeddings."
                        .to_string(),
                )),
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
            let base_url = self
                .pylos_base_url
                .as_deref()
                .unwrap_or("http://localhost:3000");
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
            let embedding = res
                .data
                .into_iter()
                .next()
                .map(|d| d.embedding)
                .ok_or_else(|| {
                    GraphonError::Classifier("Pylos embedding returned empty results".to_string())
                })?;
            Ok(embedding)
        } else {
            let api_key = match &self.llm_api_key {
                Some(key) => key,
                None => return Err(GraphonError::Classifier(
                    "LLM_API_KEY/PYLOS_EMBEDDING_MODEL not configured. Cannot generate embeddings."
                        .to_string(),
                )),
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

        let exists = self
            .qdrant_client
            .collection_exists(&self.collection_name)
            .await
            .map_err(|e| GraphonError::Internal(e.to_string()))?;

        if !exists {
            self.qdrant_client
                .create_collection(
                    CreateCollectionBuilder::new(&self.collection_name).vectors_config(
                        VectorParamsBuilder::new(self.vector_size as u64, Distance::Cosine),
                    ),
                )
                .await
                .map_err(|e| GraphonError::Internal(e.to_string()))?;
            info!(
                "Qdrant collection '{}' created successfully.",
                self.collection_name
            );
        } else {
            info!(
                "Qdrant collection '{}' already exists.",
                self.collection_name
            );
        }

        Ok(())
    }

    async fn delete_collection(&self) -> Result<(), GraphonError> {
        info!("Deleting Qdrant collection '{}'...", self.collection_name);
        let exists = self
            .qdrant_client
            .collection_exists(&self.collection_name)
            .await
            .map_err(|e| GraphonError::Internal(e.to_string()))?;

        if exists {
            self.qdrant_client
                .delete_collection(&self.collection_name)
                .await
                .map_err(|e| GraphonError::Internal(e.to_string()))?;
            info!("Qdrant collection '{}' deleted.", self.collection_name);
        }
        Ok(())
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
            let vector = embeddings[i].clone();

            let mut payload: HashMap<String, qdrant_client::qdrant::Value> = HashMap::new();
            payload.insert("email_id".to_string(), chunk.email_id.clone().into());
            payload.insert("subject".to_string(), chunk.subject.clone().into());
            payload.insert("sender".to_string(), chunk.sender.clone().into());
            payload.insert("chunk_index".to_string(), (chunk.chunk_index as i64).into());
            payload.insert("content".to_string(), chunk.content.clone().into());

            points.push(PointStruct::new(point_id, vector, payload));
        }

        self.qdrant_client
            .upsert_points(UpsertPointsBuilder::new(&self.collection_name, points))
            .await
            .map_err(|e| GraphonError::Internal(e.to_string()))?;

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

        let request = QueryPointsBuilder::new(&self.collection_name)
            .query(query_vector)
            .limit(query.limit)
            .with_payload(true)
            .build();

        let search_result = self
            .qdrant_client
            .query(request)
            .await
            .map_err(|e| GraphonError::Internal(e.to_string()))?;

        let mut results = Vec::new();
        for point in search_result.result {
            let payload = point.payload;
            let score = point.score;

            let email_id = payload
                .get("email_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default();

            let subject = payload
                .get("subject")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default();

            let sender = payload
                .get("sender")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default();

            let chunk_index = payload
                .get("chunk_index")
                .and_then(|v| v.as_integer())
                .unwrap_or(0) as usize;

            let content = payload
                .get("content")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default();

            results.push(SearchResult {
                email_id,
                subject,
                sender,
                chunk_index,
                content,
                score: score as f64,
            });
        }

        info!("Found {} results from Qdrant search.", results.len());
        Ok(results)
    }

    async fn health_check(&self) -> Result<(), GraphonError> {
        self.qdrant_client
            .health_check()
            .await
            .map_err(|e| GraphonError::Internal(e.to_string()))?;
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
