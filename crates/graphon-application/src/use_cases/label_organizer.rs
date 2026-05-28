use graphon_core::entities::{LabelCategory, LabelInfo};
use graphon_core::error::GraphonError;
use graphon_core::ports::GmailPort;
use std::sync::Arc;
use tracing::{info, warn};

pub struct LabelOrganizer {
    gmail_client: Arc<dyn GmailPort>,
}

impl LabelOrganizer {
    pub fn new(gmail_client: Arc<dyn GmailPort>) -> Self {
        Self { gmail_client }
    }

    pub async fn analyze_labels(&self) -> Result<Vec<LabelCategory>, GraphonError> {
        info!("Analyzing existing Gmail labels...");
        let labels = self.gmail_client.get_all_labels().await?;

        let mut categories = Vec::new();

        // System labels that should be kept
        let system: Vec<LabelInfo> = labels.iter().filter(|l| l.is_system()).cloned().collect();
        categories.push(LabelCategory {
            name: "system".to_string(),
            description: "Étiquettes système Gmail (conservées automatiquement)".to_string(),
            action: "keep".to_string(),
            labels: system,
        });

        // Empty user labels (candidates for cleanup)
        let empty: Vec<LabelInfo> = labels
            .iter()
            .filter(|l| !l.is_system() && l.is_empty())
            .cloned()
            .collect();
        if !empty.is_empty() {
            categories.push(LabelCategory {
                name: "empty".to_string(),
                description: "Étiquettes sans messages (nettoyage recommandé)".to_string(),
                action: "delete".to_string(),
                labels: empty,
            });
        }

        // Promotional/newsletter labels
        let promo_keywords = [
            "promo",
            "newsletter",
            "market",
            "shop",
            "alert",
            "notification",
        ];
        let promo: Vec<LabelInfo> = labels
            .iter()
            .filter(|l| {
                !l.is_system()
                    && promo_keywords
                        .iter()
                        .any(|k| l.name.to_lowercase().contains(k))
            })
            .cloned()
            .collect();
        if !promo.is_empty() {
            categories.push(LabelCategory {
                name: "promotional".to_string(),
                description: "Étiquettes promotionnelles/alertes (peuvent être consolidées)"
                    .to_string(),
                action: "consolidate".to_string(),
                labels: promo,
            });
        }

        // Archived/inactive labels (no recent messages)
        let archived: Vec<LabelInfo> = labels
            .iter()
            .filter(|l| {
                !l.is_system()
                    && !promo_keywords
                        .iter()
                        .any(|k| l.name.to_lowercase().contains(k))
                    && !l.is_empty()
                    && l.messages_total.unwrap_or(0) < 5
            })
            .cloned()
            .collect();
        if !archived.is_empty() {
            categories.push(LabelCategory {
                name: "low_usage".to_string(),
                description: "Étiquettes peu utilisées (< 5 messages)".to_string(),
                action: "review".to_string(),
                labels: archived,
            });
        }

        // Active user labels
        let active: Vec<LabelInfo> = labels
            .iter()
            .filter(|l| {
                !l.is_system()
                    && !promo_keywords
                        .iter()
                        .any(|k| l.name.to_lowercase().contains(k))
                    && !l.is_empty()
                    && l.messages_total.unwrap_or(0) >= 5
            })
            .cloned()
            .collect();
        if !active.is_empty() {
            categories.push(LabelCategory {
                name: "active".to_string(),
                description: "Étiquettes actives avec contenu".to_string(),
                action: "keep".to_string(),
                labels: active,
            });
        }

        info!(
            "Label analysis complete: {} categories, {} total labels",
            categories.len(),
            labels.len()
        );
        Ok(categories)
    }

    pub async fn cleanup_empty_labels(&self) -> Result<usize, GraphonError> {
        info!("Cleaning up empty labels...");
        let labels = self.gmail_client.get_all_labels().await?;
        let mut deleted = 0;

        for label in &labels {
            if !label.is_system() && label.is_empty() {
                info!("Deleting empty label: {} ({})", label.name, label.id);
                if let Err(e) = self.gmail_client.delete_label(&label.id).await {
                    warn!("Failed to delete label '{}': {:?}", label.name, e);
                } else {
                    deleted += 1;
                }
            }
        }

        info!("Deleted {} empty labels.", deleted);
        Ok(deleted)
    }

    pub async fn consolidate_labels(
        &self,
        prefix: &str,
        target: &str,
    ) -> Result<usize, GraphonError> {
        info!(
            "Consolidating labels with prefix '{}' into '{}'...",
            prefix, target
        );
        let labels = self.gmail_client.get_all_labels().await?;
        let mut consolidated = 0;

        for label in &labels {
            if !label.is_system()
                && label
                    .name
                    .to_lowercase()
                    .starts_with(&prefix.to_lowercase())
                && label.name != target
            {
                info!("Consolidating label '{}' -> '{}'", label.name, target);
                if let Err(e) = self.gmail_client.delete_label(&label.id).await {
                    warn!("Failed to consolidate label '{}': {:?}", label.name, e);
                } else {
                    consolidated += 1;
                }
            }
        }

        info!("Consolidated {} labels into '{}'.", consolidated, target);
        Ok(consolidated)
    }
}
