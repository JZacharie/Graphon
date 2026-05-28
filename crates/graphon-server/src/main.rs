use axum::{
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Extension, Json, Router,
};
use clap::Parser;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tower_http::trace::TraceLayer;
use tracing::{error, info, Level};
use tracing_subscriber::FmtSubscriber;

use graphon_application::{MailSortingPipeline, RagIndexer};
use graphon_core::ports::{ClassifierPort, GmailPort, StoragePort};
use graphon_infrastructure::{ClassifierAdapter, DatabaseAdapter, GmailClient};

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
}

struct ServerMetrics {
    sync_requests_total: AtomicU64,
    sync_errors_total: AtomicU64,
    rag_index_requests_total: AtomicU64,
    rag_index_errors_total: AtomicU64,
}

impl ServerMetrics {
    fn new() -> Self {
        Self {
            sync_requests_total: AtomicU64::new(0),
            sync_errors_total: AtomicU64::new(0),
            rag_index_requests_total: AtomicU64::new(0),
            rag_index_errors_total: AtomicU64::new(0),
        }
    }
}

struct AppState {
    gmail_client: Arc<dyn GmailPort>,
    classifier: Arc<dyn ClassifierPort>,
    storage: Arc<dyn StoragePort>,
    rag_indexer: Arc<RagIndexer>,
    metrics: Arc<ServerMetrics>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup logger
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    let args = Args::parse();

    // Load environment variables / configurations
    let database_url = std::env::var("DATABASE_URL").ok();
    let gmail_token = std::env::var("GMAIL_TOKEN").ok();
    let llm_key = std::env::var("LLM_API_KEY").ok();

    // Initialize adapters
    let gmail_client = Arc::new(GmailClient::new(gmail_token));
    let classifier = Arc::new(ClassifierAdapter::new(llm_key));
    let storage = Arc::new(DatabaseAdapter::new(database_url.as_deref()).await?);
    let rag_indexer = Arc::new(RagIndexer::new(storage.clone()));
    let metrics = Arc::new(ServerMetrics::new());

    let app_state = Arc::new(AppState {
        gmail_client: gmail_client.clone(),
        classifier: classifier.clone(),
        storage: storage.clone(),
        rag_indexer,
        metrics,
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
            .route("/health", get(health_handler))
            .route("/metrics", get(metrics_handler))
            .route("/sync", post(sync_handler))
            .route("/rag/index/:id", post(rag_index_handler))
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
         graphon_rag_index_errors_total {}\n",
        sync_total, sync_errors, rag_total, rag_errors
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

    match pipeline.run().await {
        Ok(_) => Ok(Json(
            serde_json::json!({ "status": "success", "message": "Mail sync completed." }),
        )),
        Err(err) => {
            state
                .metrics
                .sync_errors_total
                .fetch_add(1, Ordering::Relaxed);
            error!("Sync handler error: {:?}", err);
            // Secure response: do not expose underlying database/system errors to user
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    serde_json::json!({ "status": "error", "message": "An internal system error occurred during sync." }),
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
