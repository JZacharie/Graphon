use chrono::Utc;
use graphon_core::error::GraphonError;
use graphon_core::ports::{ClassifierPort, GmailPort, StoragePort};
use std::sync::Arc;
use tracing::{debug, info};

pub struct MailSortingPipeline {
    gmail_client: Arc<dyn GmailPort>,
    classifier: Arc<dyn ClassifierPort>,
    storage: Arc<dyn StoragePort>,
}

impl MailSortingPipeline {
    pub fn new(
        gmail_client: Arc<dyn GmailPort>,
        classifier: Arc<dyn ClassifierPort>,
        storage: Arc<dyn StoragePort>,
    ) -> Self {
        Self {
            gmail_client,
            classifier,
            storage,
        }
    }

    pub async fn run(&self) -> Result<(), GraphonError> {
        info!("Starting mail sorting pipeline...");

        // 1. Fetch unread emails
        let emails = self.gmail_client.fetch_unread_emails().await?;
        info!("Fetched {} unread emails", emails.len());

        for email in emails {
            info!(
                "Processing email ID: {}, Subject: {}",
                email.id, email.subject
            );

            let process_result = async {
                // Step 1: Nettoyage / Détection pub & spams
                let is_spam_or_promo = self.classifier.is_spam_or_promo(&email).await?;
                debug!(
                    "Email ID {} - is_spam_or_promo: {}",
                    email.id, is_spam_or_promo
                );
                if is_spam_or_promo {
                    info!("Email classified as spam or promo. Moving to PROMO.");
                    self.gmail_client
                        .apply_labels(&email.id, &["PROMO".to_string()])
                        .await?;
                    self.gmail_client
                        .remove_labels(&email.id, &["UNREAD".to_string()])
                        .await?;
                    return Ok::<(), GraphonError>(());
                }

                // Step 2: Classification d'importance
                let label = self.classifier.classify_importance(&email).await?;
                debug!(
                    "Email ID {} - importance classification: {}",
                    email.id, label
                );
                info!("Email importance classification: {}", label);
                self.gmail_client.apply_labels(&email.id, &[label]).await?;
                self.gmail_client
                    .remove_labels(&email.id, &["UNREAD".to_string()])
                    .await?;

                // Step 3: Persistance pour le RAG
                debug!("Saving email ID {} to storage...", email.id);
                self.storage.save_email(&email).await?;
                debug!("Email ID {} saved to storage.", email.id);
                Ok(())
            }.await;

            if let Err(e) = process_result {
                tracing::error!("Failed to process email ID {}: {:?}", email.id, e);
            }
        }

        // 2. Apply Retention Rules (Purge)
        info!("Running retention rule purges...");
        let rules = self.storage.get_retention_rules().await?;
        for rule in rules {
            info!(
                "Evaluating rule: {} (duration: {} days)",
                rule.query, rule.duration_days
            );
            let target_emails = match self.gmail_client.fetch_emails_by_query(&rule.query).await {
                Ok(emails) => emails,
                Err(e) => {
                    tracing::error!("Failed to fetch emails for retention rule query '{}': {:?}", rule.query, e);
                    continue;
                }
            };
            let now = Utc::now();
            for email in target_emails {
                let age = now.signed_duration_since(email.date);
                if age.num_days() >= rule.duration_days {
                    info!(
                        "Email ID {} is older than retention limit. Trashing it.",
                        email.id
                    );
                    if let Err(e) = self.gmail_client.trash_email(&email.id).await {
                        tracing::error!("Failed to trash email ID {}: {:?}", email.id, e);
                    }
                }
            }
        }

        info!("Mail sorting pipeline completed.");
        Ok(())
    }
}
