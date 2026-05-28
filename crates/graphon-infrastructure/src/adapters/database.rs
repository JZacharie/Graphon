use async_trait::async_trait;
use graphon_core::entities::{Attachment, Email, RetentionRule};
use graphon_core::error::GraphonError;
use graphon_core::ports::StoragePort;
use sqlx::{PgPool, Row};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::{info, warn};

pub enum DatabaseConnection {
    Postgres(PgPool),
    Mock(Arc<Mutex<MockStorage>>),
}

pub struct MockStorage {
    emails: HashMap<String, Email>,
    rules: Vec<RetentionRule>,
}

pub struct DatabaseAdapter {
    connection: DatabaseConnection,
}

impl DatabaseAdapter {
    pub async fn new(database_url: Option<&str>) -> Result<Self, GraphonError> {
        if let Some(url) = database_url {
            info!("Connecting to PostgreSQL database...");
            let pool = PgPool::connect(url).await?;

            // Automatically initialize tables if they do not exist
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS emails (
                    id TEXT PRIMARY KEY,
                    thread_id TEXT NOT NULL,
                    sender TEXT NOT NULL,
                    recipient TEXT[] NOT NULL,
                    subject TEXT NOT NULL,
                    body TEXT NOT NULL,
                    date TIMESTAMPTZ NOT NULL,
                    labels TEXT[] NOT NULL
                );",
            )
            .execute(&pool)
            .await?;

            sqlx::query(
                "CREATE TABLE IF NOT EXISTS attachments (
                    id TEXT PRIMARY KEY,
                    email_id TEXT REFERENCES emails(id) ON DELETE CASCADE,
                    filename TEXT NOT NULL,
                    mime_type TEXT NOT NULL,
                    size INT NOT NULL,
                    data BYTEA
                );",
            )
            .execute(&pool)
            .await?;

            sqlx::query(
                "CREATE TABLE IF NOT EXISTS retention_rules (
                    id TEXT PRIMARY KEY,
                    query TEXT NOT NULL,
                    duration_days INT NOT NULL
                );",
            )
            .execute(&pool)
            .await?;

            Ok(Self {
                connection: DatabaseConnection::Postgres(pool),
            })
        } else {
            warn!("No database URL provided. Initializing in-memory mock storage.");
            Ok(Self {
                connection: DatabaseConnection::Mock(Arc::new(Mutex::new(MockStorage {
                    emails: HashMap::new(),
                    rules: vec![RetentionRule {
                        id: "rule1".to_string(),
                        query: "from:alerts@system.com".to_string(),
                        duration_days: 7,
                    }],
                }))),
            })
        }
    }
}

#[async_trait]
impl StoragePort for DatabaseAdapter {
    async fn save_email(&self, email: &Email) -> Result<(), GraphonError> {
        match &self.connection {
            DatabaseConnection::Postgres(pool) => {
                // TODO(security): Use parameterized query to prevent SQL injection
                sqlx::query(
                    "INSERT INTO emails (id, thread_id, sender, recipient, subject, body, date, labels)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                     ON CONFLICT (id) DO UPDATE SET
                     thread_id = EXCLUDED.thread_id, sender = EXCLUDED.sender, recipient = EXCLUDED.recipient,
                     subject = EXCLUDED.subject, body = EXCLUDED.body, date = EXCLUDED.date, labels = EXCLUDED.labels;"
                )
                .bind(&email.id)
                .bind(&email.thread_id)
                .bind(&email.from)
                .bind(&email.to)
                .bind(&email.subject)
                .bind(&email.body)
                .bind(email.date)
                .bind(&email.labels)
                .execute(pool)
                .await?;

                for attachment in &email.attachments {
                    sqlx::query(
                        "INSERT INTO attachments (id, email_id, filename, mime_type, size, data)
                         VALUES ($1, $2, $3, $4, $5, $6)
                         ON CONFLICT (id) DO NOTHING;",
                    )
                    .bind(&attachment.id)
                    .bind(&email.id)
                    .bind(&attachment.filename)
                    .bind(&attachment.mime_type)
                    .bind(attachment.size as i32)
                    .bind(&attachment.data)
                    .execute(pool)
                    .await?;
                }
                Ok(())
            }
            DatabaseConnection::Mock(store) => {
                let mut store = store.lock().unwrap();
                store.emails.insert(email.id.clone(), email.clone());
                Ok(())
            }
        }
    }

    async fn get_email(&self, email_id: &str) -> Result<Option<Email>, GraphonError> {
        match &self.connection {
            DatabaseConnection::Postgres(pool) => {
                let email_row = sqlx::query(
                    "SELECT id, thread_id, sender, recipient, subject, body, date, labels FROM emails WHERE id = $1;"
                )
                .bind(email_id)
                .fetch_optional(pool)
                .await?;

                if let Some(row) = email_row {
                    let attachments_rows = sqlx::query(
                        "SELECT id, filename, mime_type, size, data FROM attachments WHERE email_id = $1;"
                    )
                    .bind(email_id)
                    .fetch_all(pool)
                    .await?;

                    let attachments = attachments_rows
                        .into_iter()
                        .map(|r| Attachment {
                            id: r.get("id"),
                            filename: r.get("filename"),
                            mime_type: r.get("mime_type"),
                            size: r.get::<i32, _>("size") as usize,
                            data: r.get("data"),
                        })
                        .collect();

                    Ok(Some(Email {
                        id: row.get("id"),
                        thread_id: row.get("thread_id"),
                        from: row.get("sender"),
                        to: row.get("recipient"),
                        subject: row.get("subject"),
                        body: row.get("body"),
                        date: row.get("date"),
                        labels: row.get("labels"),
                        attachments,
                    }))
                } else {
                    Ok(None)
                }
            }
            DatabaseConnection::Mock(store) => {
                let store = store.lock().unwrap();
                Ok(store.emails.get(email_id).cloned())
            }
        }
    }

    async fn save_retention_rule(&self, rule: &RetentionRule) -> Result<(), GraphonError> {
        match &self.connection {
            DatabaseConnection::Postgres(pool) => {
                sqlx::query(
                    "INSERT INTO retention_rules (id, query, duration_days) VALUES ($1, $2, $3)
                     ON CONFLICT (id) DO UPDATE SET query = EXCLUDED.query, duration_days = EXCLUDED.duration_days;"
                )
                .bind(&rule.id)
                .bind(&rule.query)
                .bind(rule.duration_days as i32)
                .execute(pool)
                .await?;
                Ok(())
            }
            DatabaseConnection::Mock(store) => {
                let mut store = store.lock().unwrap();
                store.rules.push(rule.clone());
                Ok(())
            }
        }
    }

    async fn get_retention_rules(&self) -> Result<Vec<RetentionRule>, GraphonError> {
        match &self.connection {
            DatabaseConnection::Postgres(pool) => {
                let rows = sqlx::query("SELECT id, query, duration_days FROM retention_rules;")
                    .fetch_all(pool)
                    .await?;

                let rules = rows
                    .into_iter()
                    .map(|r| RetentionRule {
                        id: r.get("id"),
                        query: r.get("query"),
                        duration_days: r.get::<i32, _>("duration_days") as i64,
                    })
                    .collect();
                Ok(rules)
            }
            DatabaseConnection::Mock(store) => {
                let store = store.lock().unwrap();
                Ok(store.rules.clone())
            }
        }
    }
}
