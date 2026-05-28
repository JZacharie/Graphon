use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub id: String,
    pub filename: String,
    pub mime_type: String,
    pub size: usize,
    pub data: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Email {
    pub id: String,
    pub thread_id: String,
    pub from: String,
    pub to: Vec<String>,
    pub subject: String,
    pub body: String,
    pub date: DateTime<Utc>,
    pub labels: Vec<String>,
    pub attachments: Vec<Attachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionRule {
    pub id: String,
    pub query: String, // Gmail query style, e.g. "from:alerts@system.com"
    pub duration_days: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RagChunk {
    pub email_id: String,
    pub subject: String,
    pub sender: String,
    pub chunk_index: usize,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub email_id: String,
    pub subject: String,
    pub sender: String,
    pub chunk_index: usize,
    pub content: String,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    pub query_text: String,
    pub limit: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelInfo {
    pub id: String,
    pub name: String,
    pub label_type: String,
    pub messages_total: Option<i64>,
    pub messages_unread: Option<i64>,
    pub threads_total: Option<i64>,
}

impl LabelInfo {
    pub fn is_system(&self) -> bool {
        matches!(
            self.name.as_str(),
            "INBOX"
                | "SPAM"
                | "TRASH"
                | "UNREAD"
                | "STARRED"
                | "IMPORTANT"
                | "DRAFT"
                | "SENT"
                | "CHAT"
                | "CATEGORY_PERSONAL"
                | "CATEGORY_SOCIAL"
                | "CATEGORY_PROMOTIONS"
                | "CATEGORY_UPDATES"
                | "CATEGORY_FORUMS"
        )
    }

    pub fn is_empty(&self) -> bool {
        self.messages_total.unwrap_or(0) == 0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelCategory {
    pub name: String,
    pub labels: Vec<LabelInfo>,
    pub description: String,
    pub action: String,
}
