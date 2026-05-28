use async_trait::async_trait;
use graphon_core::entities::Email;
use graphon_core::error::GraphonError;
use graphon_core::ports::ClassifierPort;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

pub struct ClassifierAdapter {
    pylos_base_url: String,
    pylos_api_key: Option<String>,
    client: reqwest::Client,
    model: String,
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    max_tokens: u32,
    temperature: f32,
}

#[derive(Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChatMessage,
}

#[derive(Deserialize)]
struct ChatMessage {
    content: String,
}

impl ClassifierAdapter {
    pub fn new(pylos_base_url: String, pylos_api_key: Option<String>, model: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_default();
        Self {
            pylos_base_url,
            pylos_api_key,
            client,
            model,
        }
    }

    async fn llm_classify(&self, prompt: &str, email: &Email) -> Result<String, GraphonError> {
        let Some(ref api_key) = self.pylos_api_key else {
            return Err(GraphonError::Internal("No Pylos API key configured".into()));
        };

        let text = format!("Subject: {}\nBody:\n{}", email.subject, email.body);
        let messages = vec![
            Message {
                role: "system".into(),
                content: prompt.to_string(),
            },
            Message {
                role: "user".into(),
                content: text,
            },
        ];

        let payload = ChatRequest {
            model: self.model.clone(),
            messages,
            max_tokens: 64,
            temperature: 0.0,
        };

        let url = format!(
            "{}/v1/chat/completions",
            self.pylos_base_url.trim_end_matches('/')
        );

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .json(&payload)
            .send()
            .await
            .map_err(GraphonError::Network)?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            warn!("Pylos API error {}: {}", status, body);
            return Err(GraphonError::Internal(format!(
                "Pylos returned {}: {}",
                status, body
            )));
        }

        let chat_resp: ChatResponse = resp.json().await.map_err(GraphonError::Network)?;

        let content = chat_resp
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .unwrap_or_default();

        Ok(content.trim().to_lowercase())
    }
}

#[async_trait]
impl ClassifierPort for ClassifierAdapter {
    async fn is_spam_or_promo(&self, email: &Email) -> Result<bool, GraphonError> {
        info!(
            "Checking if email ID {} is spam or promotional...",
            email.id
        );

        // Fast path: keyword check before LLM call
        let text_to_check = format!("{} {}", email.subject, email.body).to_lowercase();
        let promo_keywords = [
            "unsubscribe",
            "opt out",
            "promotion",
            "sale",
            "discount",
            "offer",
            "deal",
            "limited time",
            "newsletter",
        ];
        if promo_keywords.iter().any(|k| text_to_check.contains(k)) {
            info!("Keyword match — classified as promo");
            return Ok(true);
        }

        // LLM classification via Pylos / DeepSeek
        match self
            .llm_classify(
                "You are an email classifier. Reply with exactly one word: SPAM, PROMO, or OK.\n\
                 SPAM = unsolicited bulk email, scams, phishing.\n\
                 PROMO = marketing, newsletters, sales, promotions.\n\
                 OK = personal, transactional, or work emails.\n\
                 Answer with only the word.",
                email,
            )
            .await
        {
            Ok(answer) => {
                let is_promo = answer.contains("promo") || answer.contains("spam");
                info!("LLM classification: {} -> promo={}", answer, is_promo);
                Ok(is_promo)
            }
            Err(e) => {
                warn!(
                    "LLM classification failed, falling back to keyword-only: {:?}",
                    e
                );
                Ok(false)
            }
        }
    }

    async fn classify_importance(&self, email: &Email) -> Result<String, GraphonError> {
        info!("Classifying importance of email ID {}...", email.id);

        // Fast path: urgent keyword check
        let text_to_check = format!("{} {}", email.subject, email.body).to_lowercase();
        let urgent_keywords = [
            "urgent",
            "asap",
            "action required",
            "attention",
            "important",
            "critical",
            "meeting",
            "interview",
            "invoice",
            "deadline",
        ];
        if urgent_keywords.iter().any(|k| text_to_check.contains(k)) {
            info!("Keyword match — classified as URGENT");
            return Ok("URGENT".to_string());
        }

        // LLM classification via Pylos / DeepSeek
        match self
            .llm_classify(
                "You are an email prioritizer. Reply with exactly one word: URGENT or NORMAL.\n\
                 URGENT = time-sensitive, requires immediate action, deadlines, meetings, invoices.\n\
                 NORMAL = routine, informational, not time-critical.\n\
                 Answer with only the word.",
                email,
            )
            .await
        {
            Ok(answer) => {
                let is_urgent = answer.contains("urgent");
                info!("LLM importance: {} -> urgent={}", answer, is_urgent);
                Ok(if is_urgent { "URGENT" } else { "NORMAL" }.to_string())
            }
            Err(e) => {
                warn!("LLM classification failed, falling back to keyword-only: {:?}", e);
                Ok("NORMAL".to_string())
            }
        }
    }
}
