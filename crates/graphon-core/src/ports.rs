use crate::entities::{Email, RetentionRule};
use crate::error::GraphonError;
use async_trait::async_trait;

#[async_trait]
pub trait GmailPort: Send + Sync {
    async fn fetch_unread_emails(&self) -> Result<Vec<Email>, GraphonError>;
    async fn fetch_emails_by_query(&self, query: &str) -> Result<Vec<Email>, GraphonError>;
    async fn apply_labels(&self, email_id: &str, labels: &[String]) -> Result<(), GraphonError>;
    async fn remove_labels(&self, email_id: &str, labels: &[String]) -> Result<(), GraphonError>;
    async fn trash_email(&self, email_id: &str) -> Result<(), GraphonError>;
}

#[async_trait]
pub trait ClassifierPort: Send + Sync {
    async fn is_spam_or_promo(&self, email: &Email) -> Result<bool, GraphonError>;
    async fn classify_importance(&self, email: &Email) -> Result<String, GraphonError>;
}

#[async_trait]
pub trait StoragePort: Send + Sync {
    async fn save_email(&self, email: &Email) -> Result<(), GraphonError>;
    async fn get_email(&self, email_id: &str) -> Result<Option<Email>, GraphonError>;
    async fn save_retention_rule(&self, rule: &RetentionRule) -> Result<(), GraphonError>;
    async fn get_retention_rules(&self) -> Result<Vec<RetentionRule>, GraphonError>;
}
