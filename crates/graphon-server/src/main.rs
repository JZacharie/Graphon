use axum::{
    http::StatusCode,
    response::{Html, IntoResponse, Redirect},
    routing::{delete, get, post},
    Extension, Json, Router,
};
use clap::Parser;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::time::{timeout, Duration};
use tower_http::trace::TraceLayer;
use tracing::{error, info, warn, Level};
use tracing_subscriber::FmtSubscriber;

use graphon_application::{LabelOrganizer, MailSortingPipeline, RagIndexer};
use graphon_core::ports::{ClassifierPort, GmailPort, StoragePort, VectorStorePort};
use graphon_infrastructure::{ClassifierAdapter, DatabaseAdapter, GmailClient, QdrantAdapter};

#[derive(Parser, Debug)]
#[command(name = "graphon-server", about = "Graphon CLI & API Server")]
struct Args {
    /// Start HTTP server mode
    #[arg(long)]
    server: bool,

    /// Execute the mail sorting and cleanup pipeline immediately
    #[arg(long)]
    sync: bool,

    /// Clean retention rules (run automatically in sync)
    #[arg(long)]
    clean: bool,

    /// Enable verbose/debug logs
    #[arg(long)]
    debug: bool,
}

struct ServerMetrics {
    sync_requests_total: AtomicU64,
    sync_errors_total: AtomicU64,
    rag_index_requests_total: AtomicU64,
    rag_index_errors_total: AtomicU64,
    rag_reindex_requests_total: AtomicU64,
    rag_reindex_errors_total: AtomicU64,
    rag_search_requests_total: AtomicU64,
    rag_search_errors_total: AtomicU64,
}

impl ServerMetrics {
    fn new() -> Self {
        Self {
            sync_requests_total: AtomicU64::new(0),
            sync_errors_total: AtomicU64::new(0),
            rag_index_requests_total: AtomicU64::new(0),
            rag_index_errors_total: AtomicU64::new(0),
            rag_reindex_requests_total: AtomicU64::new(0),
            rag_reindex_errors_total: AtomicU64::new(0),
            rag_search_requests_total: AtomicU64::new(0),
            rag_search_errors_total: AtomicU64::new(0),
        }
    }
}

struct AppState {
    gmail_client: Arc<dyn GmailPort>,
    gmail_client_adapter: Arc<GmailClient>,
    classifier: Arc<dyn ClassifierPort>,
    storage: Arc<dyn StoragePort>,
    rag_indexer: Arc<RagIndexer>,
    label_organizer: Arc<LabelOrganizer>,
    metrics: Arc<ServerMetrics>,
    google_client_id: Option<String>,
    google_client_secret: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let is_debug = args.debug
        || std::env::var("DEBUG")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);
    let log_level = if is_debug { Level::DEBUG } else { Level::INFO };

    // Setup logger
    let subscriber = FmtSubscriber::builder().with_max_level(log_level).finish();
    tracing::subscriber::set_global_default(subscriber)?;

    // Load environment variables / configurations
    let database_url = std::env::var("DATABASE_URL").ok();
    let gmail_token = std::env::var("GMAIL_TOKEN").ok();
    let llm_key = std::env::var("LLM_API_KEY").ok();
    let google_client_id = std::env::var("GOOGLE_CLIENT_ID").ok();
    let google_client_secret = std::env::var("GOOGLE_CLIENT_SECRET").ok();
    let pylos_base_url =
        std::env::var("PYLOS_BASE_URL").unwrap_or_else(|_| "http://localhost:3000".into());
    let pylos_api_key = std::env::var("PYLOS_API_KEY").ok();
    let pylos_model = std::env::var("PYLOS_MODEL").unwrap_or_else(|_| "deepseek-v4-flash".into());
    let qdrant_url = std::env::var("QDRANT_URL").ok();
    let qdrant_collection = std::env::var("QDRANT_COLLECTION").ok();
    let qdrant_vector_size = std::env::var("QDRANT_VECTOR_SIZE")
        .ok()
        .and_then(|v| v.parse::<usize>().ok());

    // Initialize adapters
    let gmail_client_adapter = Arc::new(GmailClient::new(gmail_token));
    let gmail_client = gmail_client_adapter.clone() as Arc<dyn GmailPort>;
    let classifier = Arc::new(ClassifierAdapter::new(
        pylos_base_url,
        pylos_api_key,
        pylos_model,
    ));
    let storage = Arc::new(DatabaseAdapter::new(database_url.as_deref()).await?);
    let qdrant_adapter = Arc::new(QdrantAdapter::new(
        qdrant_url,
        qdrant_collection,
        qdrant_vector_size,
        llm_key,
    ));
    let vector_store = qdrant_adapter.clone() as Arc<dyn VectorStorePort>;
    let rag_indexer = Arc::new(RagIndexer::new(storage.clone(), vector_store));

    // Ensure Qdrant collection exists at startup
    info!(
        "Ensuring Qdrant collection '{}' exists...",
        qdrant_adapter.collection_name()
    );
    if let Err(e) = qdrant_adapter.create_collection().await {
        warn!("Failed to create Qdrant collection at startup: {:?}", e);
    }
    let metrics = Arc::new(ServerMetrics::new());

    let label_organizer = Arc::new(LabelOrganizer::new(gmail_client.clone()));
    let app_state = Arc::new(AppState {
        gmail_client: gmail_client.clone(),
        gmail_client_adapter,
        classifier: classifier.clone(),
        storage: storage.clone(),
        rag_indexer,
        label_organizer,
        metrics,
        google_client_id,
        google_client_secret,
    });

    if args.sync || args.clean {
        info!("Executing one-off CLI sync job...");
        let pipeline = MailSortingPipeline::new(gmail_client, classifier, storage);
        if let Err(e) = pipeline.run().await {
            error!("CLI pipeline run failed: {:?}", e);
            std::process::exit(1);
        }
        info!("CLI job finished.");
        return Ok(());
    }

    if args.server {
        // Build router
        let app = Router::new()
            .route("/", get(dashboard_handler))
            .route("/sso/complete/google-oauth2/", get(oauth_callback_handler))
            .route("/api/stats", get(api_stats_handler))
            .route("/api/emails", get(api_emails_handler))
            .route("/health", get(health_handler))
            .route("/metrics", get(metrics_handler))
            .route("/sync", post(sync_handler))
            .route("/rag/index/:id", post(rag_index_handler))
            .route("/rag/reindex", post(rag_reindex_handler))
            .route("/rag/search", post(rag_search_handler))
            .route("/api/labels", get(api_labels_handler))
            .route("/api/labels/:name", delete(api_labels_delete_handler))
            .route("/api/labels/cleanup", post(api_labels_cleanup_handler))
            .route(
                "/api/labels/consolidate/:prefix/:target",
                post(api_labels_consolidate_handler),
            )
            .layer(TraceLayer::new_for_http())
            .layer(Extension(app_state));

        // Use localhost/127.0.0.1 for testing security standard, but allow override (e.g. 0.0.0.0) for Kubernetes
        let host_str = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
        let addr: SocketAddr = format!("{}:8080", host_str).parse()?;
        info!("Graphon API server running at http://{}", addr);

        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app).await?;
    } else {
        println!(
            "No action specified. Use --sync to run once, or --server to run the HTTP service."
        );
    }

    Ok(())
}

async fn health_handler() -> (StatusCode, &'static str) {
    (StatusCode::OK, "OK")
}

async fn metrics_handler(Extension(state): Extension<Arc<AppState>>) -> impl IntoResponse {
    let sync_total = state.metrics.sync_requests_total.load(Ordering::Relaxed);
    let sync_errors = state.metrics.sync_errors_total.load(Ordering::Relaxed);
    let rag_total = state
        .metrics
        .rag_index_requests_total
        .load(Ordering::Relaxed);
    let rag_errors = state.metrics.rag_index_errors_total.load(Ordering::Relaxed);
    let reindex_total = state
        .metrics
        .rag_reindex_requests_total
        .load(Ordering::Relaxed);
    let reindex_errors = state
        .metrics
        .rag_reindex_errors_total
        .load(Ordering::Relaxed);
    let search_total = state
        .metrics
        .rag_search_requests_total
        .load(Ordering::Relaxed);
    let search_errors = state
        .metrics
        .rag_search_errors_total
        .load(Ordering::Relaxed);

    let body = format!(
        "# HELP graphon_sync_requests_total Total number of mail sync requests\n\
         # TYPE graphon_sync_requests_total counter\n\
         graphon_sync_requests_total {}\n\
         # HELP graphon_sync_errors_total Total number of failed mail sync requests\n\
         # TYPE graphon_sync_errors_total counter\n\
         graphon_sync_errors_total {}\n\
         # HELP graphon_rag_index_requests_total Total number of RAG indexing requests\n\
         # TYPE graphon_rag_index_requests_total counter\n\
         graphon_rag_index_requests_total {}\n\
         # HELP graphon_rag_index_errors_total Total number of failed RAG indexing requests\n\
         # TYPE graphon_rag_index_errors_total counter\n\
         graphon_rag_index_errors_total {}\n\
         # HELP graphon_rag_reindex_requests_total Total number of RAG reindex requests\n\
         # TYPE graphon_rag_reindex_requests_total counter\n\
         graphon_rag_reindex_requests_total {}\n\
         # HELP graphon_rag_reindex_errors_total Total number of failed RAG reindex requests\n\
         # TYPE graphon_rag_reindex_errors_total counter\n\
         graphon_rag_reindex_errors_total {}\n\
         # HELP graphon_rag_search_requests_total Total number of RAG search requests\n\
         # TYPE graphon_rag_search_requests_total counter\n\
         graphon_rag_search_requests_total {}\n\
         # HELP graphon_rag_search_errors_total Total number of failed RAG search requests\n\
         # TYPE graphon_rag_search_errors_total counter\n\
         graphon_rag_search_errors_total {}\n",
        sync_total,
        sync_errors,
        rag_total,
        rag_errors,
        reindex_total,
        reindex_errors,
        search_total,
        search_errors
    );

    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        body,
    )
}

async fn sync_handler(
    Extension(state): Extension<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    state
        .metrics
        .sync_requests_total
        .fetch_add(1, Ordering::Relaxed);
    let pipeline = MailSortingPipeline::new(
        state.gmail_client.clone(),
        state.classifier.clone(),
        state.storage.clone(),
    );

    let sync_timeout = std::env::var("SYNC_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(300);

    match timeout(Duration::from_secs(sync_timeout), pipeline.run()).await {
        Ok(Ok(_)) => Ok(Json(
            serde_json::json!({ "status": "success", "message": "Mail sync completed." }),
        )),
        Ok(Err(err)) => {
            state
                .metrics
                .sync_errors_total
                .fetch_add(1, Ordering::Relaxed);
            error!("Sync handler error: {:?}", err);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    serde_json::json!({ "status": "error", "message": "An internal system error occurred during sync." }),
                ),
            ))
        }
        Err(_) => {
            state
                .metrics
                .sync_errors_total
                .fetch_add(1, Ordering::Relaxed);
            error!("Sync handler timed out after {} seconds.", sync_timeout);
            Err((
                StatusCode::GATEWAY_TIMEOUT,
                Json(
                    serde_json::json!({ "status": "error", "message": "Sync timed out. Gmail API may be slow." }),
                ),
            ))
        }
    }
}

async fn rag_index_handler(
    axum::extract::Path(id): axum::extract::Path<String>,
    Extension(state): Extension<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    state
        .metrics
        .rag_index_requests_total
        .fetch_add(1, Ordering::Relaxed);
    match state.rag_indexer.index_email_for_rag(&id).await {
        Ok(chunks) => Ok(Json(serde_json::json!({
            "status": "success",
            "email_id": id,
            "chunks_count": chunks.len(),
            "chunks": chunks
        }))),
        Err(err) => {
            state
                .metrics
                .rag_index_errors_total
                .fetch_add(1, Ordering::Relaxed);
            error!("RAG Indexer error: {:?}", err);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    serde_json::json!({ "status": "error", "message": "Failed to index email for RAG." }),
                ),
            ))
        }
    }
}

async fn rag_reindex_handler(
    Extension(state): Extension<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    state
        .metrics
        .rag_reindex_requests_total
        .fetch_add(1, Ordering::Relaxed);
    match state.rag_indexer.reindex_all(500).await {
        Ok(total_chunks) => Ok(Json(serde_json::json!({
            "status": "success",
            "chunks_count": total_chunks
        }))),
        Err(err) => {
            state
                .metrics
                .rag_reindex_errors_total
                .fetch_add(1, Ordering::Relaxed);
            error!("RAG Reindex error: {:?}", err);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    serde_json::json!({ "status": "error", "message": "Failed to reindex emails for RAG." }),
                ),
            ))
        }
    }
}

#[derive(serde::Deserialize)]
struct SearchInput {
    query: String,
    #[serde(default = "default_limit")]
    limit: u64,
}

fn default_limit() -> u64 {
    10
}

async fn rag_search_handler(
    Extension(state): Extension<Arc<AppState>>,
    Json(input): Json<SearchInput>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    state
        .metrics
        .rag_search_requests_total
        .fetch_add(1, Ordering::Relaxed);
    match state.rag_indexer.search(&input.query, input.limit).await {
        Ok(results) => Ok(Json(serde_json::json!({
            "status": "success",
            "query": input.query,
            "results": results
        }))),
        Err(err) => {
            state
                .metrics
                .rag_search_errors_total
                .fetch_add(1, Ordering::Relaxed);
            error!("RAG Search error: {:?}", err);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    serde_json::json!({ "status": "error", "message": "Failed to search RAG index." }),
                ),
            ))
        }
    }
}

async fn dashboard_handler(Extension(state): Extension<Arc<AppState>>) -> impl IntoResponse {
    let current_token = state.gmail_client.get_token();
    let is_token_valid = match current_token {
        Some(ref t) => !t.is_empty() && t != "placeholder_gmail_token",
        None => false,
    };

    if !is_token_valid {
        if let Some(ref client_id) = state.google_client_id {
            let auth_url = format!(
                "https://accounts.google.com/o/oauth2/auth?\
                 client_id={}&\
                 redirect_uri=https://graphon.p.zacharie.org/sso/complete/google-oauth2/&\
                 response_type=code&\
                 scope=https://mail.google.com/&\
                 access_type=offline&\
                 prompt=consent",
                client_id
            );
            return Redirect::temporary(&auth_url).into_response();
        } else {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Google Client ID not configured. Please check your Vault secret (ai/graphon) or environment.",
            )
                .into_response();
        }
    }

    Html(include_str!("dashboard.html")).into_response()
}

#[derive(serde::Deserialize)]
struct CallbackParams {
    code: String,
}

async fn oauth_callback_handler(
    axum::extract::Query(params): axum::extract::Query<CallbackParams>,
    Extension(state): Extension<Arc<AppState>>,
) -> impl IntoResponse {
    let client_id = match &state.google_client_id {
        Some(id) => id,
        None => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Missing GOOGLE_CLIENT_ID",
            )
                .into_response()
        }
    };
    let client_secret = match &state.google_client_secret {
        Some(secret) => secret,
        None => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Missing GOOGLE_CLIENT_SECRET",
            )
                .into_response()
        }
    };

    let client = reqwest::Client::new();
    let token_res = match client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("code", &params.code),
            ("client_id", client_id),
            ("client_secret", client_secret),
            (
                "redirect_uri",
                &"https://graphon.p.zacharie.org/sso/complete/google-oauth2/".to_string(),
            ),
            ("grant_type", &"authorization_code".to_string()),
        ])
        .send()
        .await
    {
        Ok(res) => res,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to send token request: {:?}", e),
            )
                .into_response()
        }
    };

    if !token_res.status().is_success() {
        let err_text = token_res.text().await.unwrap_or_default();
        return (
            StatusCode::BAD_REQUEST,
            format!("Token exchange failed: {}", err_text),
        )
            .into_response();
    }

    #[derive(serde::Deserialize)]
    struct TokenResponse {
        access_token: String,
    }

    let tokens: TokenResponse = match token_res.json().await {
        Ok(t) => t,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to parse token response: {:?}", e),
            )
                .into_response()
        }
    };

    // Update in-memory token
    state
        .gmail_client_adapter
        .set_token(tokens.access_token.clone());

    // Sync back to Vault
    if let Ok(vault_token) = std::env::var("VAULT_TOKEN") {
        let payload = serde_json::json!({
            "data": {
                "gmail_token": tokens.access_token,
                "client_id": client_id,
                "client_secret": client_secret,
                "project_id": "graphon-497704"
            }
        });

        let _ = client
            .post("https://vault.p.zacharie.org/v1/secret/data/ai/graphon")
            .header("X-Vault-Token", &vault_token)
            .json(&payload)
            .send()
            .await;
    } else {
        info!("VAULT_TOKEN not configured; skipping sync to Vault.");
    }

    Redirect::temporary("/").into_response()
}

async fn api_stats_handler(Extension(state): Extension<Arc<AppState>>) -> impl IntoResponse {
    let total_syncs = state.metrics.sync_requests_total.load(Ordering::Relaxed);
    let sync_errors = state.metrics.sync_errors_total.load(Ordering::Relaxed);
    let emails_count = match state.storage.get_emails_count().await {
        Ok(count) => count,
        Err(e) => {
            error!("Failed to get emails count: {:?}", e);
            0
        }
    };

    Json(serde_json::json!({
        "total_syncs": total_syncs,
        "sync_errors": sync_errors,
        "emails_count": emails_count
    }))
}

async fn api_emails_handler(Extension(state): Extension<Arc<AppState>>) -> impl IntoResponse {
    match state.storage.get_recent_emails(50).await {
        Ok(emails) => Json(serde_json::json!(emails)).into_response(),
        Err(err) => {
            error!("Failed to list emails: {:?}", err);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "status": "error",
                    "message": format!("Failed to load emails: {}", err)
                })),
            )
                .into_response()
        }
    }
}

async fn api_labels_handler(
    Extension(state): Extension<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match state.label_organizer.analyze_labels().await {
        Ok(categories) => Ok(Json(serde_json::json!({
            "status": "success",
            "categories": categories
        }))),
        Err(err) => {
            error!("Label analysis error: {:?}", err);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "status": "error", "message": format!("{}", err) })),
            ))
        }
    }
}

async fn api_labels_delete_handler(
    axum::extract::Path(name): axum::extract::Path<String>,
    Extension(state): Extension<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match state.label_organizer.delete_label_by_name(&name).await {
        Ok(label_name) => Ok(Json(serde_json::json!({
            "status": "success",
            "deleted": label_name
        }))),
        Err(err) => {
            error!("Label deletion error: {:?}", err);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "status": "error", "message": format!("{}", err) })),
            ))
        }
    }
}

async fn api_labels_cleanup_handler(
    Extension(state): Extension<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match state.label_organizer.cleanup_empty_labels().await {
        Ok(deleted) => Ok(Json(serde_json::json!({
            "status": "success",
            "deleted": deleted
        }))),
        Err(err) => {
            error!("Label cleanup error: {:?}", err);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "status": "error", "message": format!("{}", err) })),
            ))
        }
    }
}

async fn api_labels_consolidate_handler(
    axum::extract::Path((prefix, target)): axum::extract::Path<(String, String)>,
    Extension(state): Extension<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match state
        .label_organizer
        .consolidate_labels(&prefix, &target)
        .await
    {
        Ok(consolidated) => Ok(Json(serde_json::json!({
            "status": "success",
            "consolidated": consolidated
        }))),
        Err(err) => {
            error!("Label consolidation error: {:?}", err);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "status": "error", "message": format!("{}", err) })),
            ))
        }
    }
}
