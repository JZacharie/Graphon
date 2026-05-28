use async_trait::async_trait;
use graphon_core::entities::Email;
use graphon_core::error::GraphonError;
use graphon_core::ports::ClassifierPort;
use tracing::info;

pub struct ClassifierAdapter {
    llm_api_key: Option<String>,
}

impl ClassifierAdapter {
    pub fn new(llm_api_key: Option<String>) -> Self {
        Self { llm_api_key }
    }
}

#[async_trait]
impl ClassifierPort for ClassifierAdapter {
    async fn is_spam_or_promo(&self, email: &Email) -> Result<bool, GraphonError> {
        info!(
            "Checking if email ID {} is spam or promotional...",
            email.id
        );

        // Simple, clean heuristic classification
        let text_to_check = format!("{} {}", email.subject, email.body).to_lowercase();

        let promo_keywords = vec![
            "unsubscribe",
            "opt out",
            "promotion",
            "sale",
            "discount",
            "offer",
            "deal",
            "limited time",
            "special offer",
            "newsletter",
        ];

        for keyword in promo_keywords {
            if text_to_check.contains(keyword) {
                info!(
                    "Found promotion keyword '{}'. Classified as promo.",
                    keyword
                );
                return Ok(true);
            }
        }

        // LLM based check fallback if API key is present
        if let Some(_key) = &self.llm_api_key {
            info!("Running LLM-based promo classification (simulated)...");
            // If we had LiteLLM / OpenAI config, we'd query it here.
        }

        Ok(false)
    }

    async fn classify_importance(&self, email: &Email) -> Result<String, GraphonError> {
        info!("Classifying importance of email ID {}...", email.id);

        let text_to_check = format!("{} {}", email.subject, email.body).to_lowercase();

        // Check for urgent indicators
        let urgent_keywords = vec![
            "urgent",
            "asap",
            "action required",
            "attention",
            "important",
            "critical",
            "schedule",
            "meeting",
            "interview",
            "invoice",
        ];

        for keyword in urgent_keywords {
            if text_to_check.contains(keyword) {
                info!(
                    "Found priority keyword '{}'. Classified as URGENT.",
                    keyword
                );
                return Ok("URGENT".to_string());
            }
        }

        Ok("NORMAL".to_string())
    }
}
