use graphon_core::entities::{LabelCategory, LabelInfo};
use graphon_core::error::GraphonError;
use graphon_core::ports::GmailPort;
use std::sync::Arc;
use tracing::{info, warn};

pub struct LabelOrganizer {
    gmail_client: Arc<dyn GmailPort>,
}

// Labels created and managed by Graphon itself — protected from deletion
const GRAPHON_LABELS: &[&str] = &["PROMO", "NORMAL", "URGENT"];

// Gmail color/star labels
const STAR_LABELS: &[&str] = &[
    "GREEN_CIRCLE",
    "ORANGE_CIRCLE",
    "RED_CIRCLE",
    "YELLOW_CIRCLE",
    "BLUE_STAR",
    "GREEN_STAR",
    "ORANGE_STAR",
    "PURPLE_STAR",
    "RED_STAR",
    "YELLOW_STAR",
];

fn is_graphon_label(name: &str) -> bool {
    GRAPHON_LABELS.iter().any(|&l| l.eq_ignore_ascii_case(name))
}

fn is_star_label(name: &str) -> bool {
    STAR_LABELS.iter().any(|&l| l.eq_ignore_ascii_case(name))
}

fn is_nested_label(name: &str) -> bool {
    name.contains('/') || name.contains('\\')
}

fn match_promo_keywords(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains("promo")
        || lower.contains("newsletter")
        || lower.contains("notification")
        || lower.contains("market")
        || lower.contains("shop")
        || lower.contains("deal")
        || lower.contains("offer")
}

impl LabelOrganizer {
    pub fn new(gmail_client: Arc<dyn GmailPort>) -> Self {
        Self { gmail_client }
    }

    pub async fn analyze_labels(&self) -> Result<Vec<LabelCategory>, GraphonError> {
        info!("Analyzing existing Gmail labels...");
        let labels = self.gmail_client.get_all_labels().await?;

        let mut assigned = vec![false; labels.len()];

        // Helper: find unassigned indices matching a predicate, mark them, return cloned labels
        let mut extract = |pred: fn(&LabelInfo) -> bool| -> Vec<LabelInfo> {
            let mut out = Vec::new();
            for (i, l) in labels.iter().enumerate() {
                if !assigned[i] && pred(l) {
                    assigned[i] = true;
                    out.push(l.clone());
                }
            }
            out
        };

        let system = extract(LabelInfo::is_system);
        let graphon_labels = extract(|l| is_graphon_label(&l.name));
        let star_labels = extract(|l| is_star_label(&l.name));
        let nested = extract(|l| is_nested_label(&l.name));
        let empty = extract(|l| l.is_empty());
        let promo = extract(|l| match_promo_keywords(&l.name));
        let low_usage = extract(|l| !l.is_empty() && l.messages_total.unwrap_or(0) < 5);
        let active = extract(|l| l.messages_total.unwrap_or(0) >= 5);

        let mut categories = Vec::new();

        macro_rules! push_cat {
            ($labels:expr, $name:expr, $desc:expr, $action:expr) => {
                if !$labels.is_empty() {
                    categories.push(LabelCategory {
                        name: $name.to_string(),
                        description: $desc.to_string(),
                        action: $action.to_string(),
                        labels: $labels,
                    });
                }
            };
        }

        push_cat!(
            system,
            "system",
            "Étiquettes système Gmail (conservées automatiquement)",
            "keep"
        );
        push_cat!(
            graphon_labels,
            "graphon",
            "Étiquettes gérées par Graphon (tri automatique)",
            "keep"
        );
        push_cat!(
            star_labels,
            "stars",
            "Étiquettes d'étoiles/couleurs Gmail",
            "keep"
        );
        push_cat!(
            nested,
            "nested",
            "Étiquettes hiérarchiques (organisation manuelle)",
            "keep"
        );
        push_cat!(
            empty,
            "empty_unused",
            "Étiquettes plates sans messages (nettoyable)",
            "delete"
        );
        push_cat!(
            promo,
            "promotional",
            "Étiquettes promotionnelles/newsletters (peuvent être consolidées)",
            "consolidate"
        );
        push_cat!(
            low_usage,
            "low_usage",
            "Étiquettes peu utilisées (< 5 messages)",
            "review"
        );
        push_cat!(active, "active", "Étiquettes actives avec contenu", "keep");

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
            if label.is_system() || is_graphon_label(&label.name) || is_nested_label(&label.name) {
                continue;
            }
            if label.is_empty() {
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

    pub async fn delete_label_by_name(&self, name: &str) -> Result<String, GraphonError> {
        info!("Deleting label by name: {}", name);
        let labels = self.gmail_client.get_all_labels().await?;
        let label = labels
            .iter()
            .find(|l| l.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| GraphonError::NotFound(format!("Label '{}' not found", name)))?;
        self.gmail_client.delete_label(&label.id).await?;
        info!("Deleted label '{}' ({})", label.name, label.id);
        Ok(label.name.clone())
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
