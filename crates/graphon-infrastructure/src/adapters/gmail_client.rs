use async_trait::async_trait;
use chrono::{DateTime, Utc};
use graphon_core::entities::{Attachment, Email};
use graphon_core::error::GraphonError;
use graphon_core::ports::GmailPort;
use serde::Deserialize;
use tracing::{debug, error, info, warn};

fn encode_query(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            ' ' => "%20".to_string(),
            _ => format!("%{:02X}", c as u32),
        })
        .collect()
}

use std::sync::RwLock;

pub struct GmailClient {
    client: reqwest::Client,
    _api_url: String,
    token: RwLock<Option<String>>,
}

#[derive(Deserialize, Debug)]
struct Label {
    id: String,
    name: String,
}

#[derive(Deserialize, Debug)]
struct LabelsListResponse {
    labels: Option<Vec<Label>>,
}

#[derive(Deserialize, Debug)]
struct MessageListResponse {
    messages: Option<Vec<MessageRef>>,
}

#[derive(Deserialize, Debug)]
struct MessageRef {
    id: String,
    #[serde(rename = "threadId")]
    _thread_id: String,
}

#[derive(Deserialize, Debug)]
struct MessageDetail {
    id: String,
    #[serde(rename = "threadId")]
    thread_id: String,
    label_ids: Option<Vec<String>>,
    payload: Option<MessagePayload>,
}

#[derive(Deserialize, Debug)]
struct MessagePayload {
    headers: Option<Vec<Header>>,
    body: Option<BodyData>,
    parts: Option<Vec<MessagePart>>,
}

#[derive(Deserialize, Debug)]
struct MessagePart {
    filename: Option<String>,
    #[serde(rename = "mimeType")]
    mime_type: Option<String>,
    body: Option<BodyData>,
    parts: Option<Vec<MessagePart>>,
}

#[derive(Deserialize, Debug)]
struct BodyData {
    data: Option<String>,
    #[serde(rename = "attachmentId")]
    attachment_id: Option<String>,
    size: Option<i32>,
}

#[derive(Deserialize, Debug)]
struct Header {
    name: String,
    value: String,
}

impl GmailClient {
    pub fn new(token: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(15))
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .unwrap_or_default();
        Self {
            client,
            _api_url: "https://gmail.googleapis.com/gmail/v1".to_string(),
            token: RwLock::new(token),
        }
    }

    pub fn set_token(&self, new_token: String) {
        match self.token.write() {
            Ok(mut guard) => *guard = Some(new_token),
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                *guard = Some(new_token);
            }
        }
    }

    pub fn get_token(&self) -> Option<String> {
        self.token.read().ok().and_then(|g| g.clone())
    }

    async fn fetch_message_ids(&self, query: &str) -> Result<Vec<MessageRef>, GraphonError> {
        let token = {
            let token_guard = self.token.read().unwrap();
            match &*token_guard {
                Some(t) => t.clone(),
                None => return Err(GraphonError::Gmail("No OAuth token provided".to_string())),
            }
        };

        let url = format!(
            "{}/users/me/messages?q={}",
            self._api_url,
            encode_query(query)
        );
        let response = self.client.get(&url).bearer_auth(token).send().await?;

        if !response.status().is_success() {
            let err_body = response.text().await.unwrap_or_default();
            return Err(GraphonError::Gmail(format!(
                "Failed to list messages: {}",
                err_body
            )));
        }

        let list: MessageListResponse = response.json().await?;
        Ok(list.messages.unwrap_or_default())
    }

    async fn fetch_message_details(&self, id: &str) -> Result<Email, GraphonError> {
        let token = self
            .token
            .read()
            .ok()
            .and_then(|g| g.clone())
            .ok_or_else(|| GraphonError::Gmail("No OAuth token available".to_string()))?;
        let url = format!("{}/users/me/messages/{}?format=full", self._api_url, id);

        let response = self.client.get(&url).bearer_auth(token).send().await?;

        if !response.status().is_success() {
            let err_body = response.text().await.unwrap_or_default();
            return Err(GraphonError::Gmail(format!(
                "Failed to get message {}: {}",
                id, err_body
            )));
        }

        let detail: MessageDetail = response.json().await?;

        // Extract headers
        let mut from = String::new();
        let mut to = Vec::new();
        let mut subject = String::new();
        let mut date = Utc::now();

        if let Some(payload) = &detail.payload {
            if let Some(headers) = &payload.headers {
                for header in headers {
                    match header.name.to_lowercase().as_str() {
                        "from" => from = header.value.clone(),
                        "to" => {
                            to = header
                                .value
                                .split(',')
                                .map(|s| s.trim().to_string())
                                .collect()
                        }
                        "subject" => subject = header.value.clone(),
                        "date" => {
                            if let Ok(parsed_date) = DateTime::parse_from_rfc2822(&header.value) {
                                date = parsed_date.with_timezone(&Utc);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // Extract body and attachments
        let mut body = String::new();
        let mut attachments = Vec::new();

        if let Some(payload) = &detail.payload {
            self.parse_parts(payload, &mut body, &mut attachments);
        }

        Ok(Email {
            id: detail.id,
            thread_id: detail.thread_id,
            from,
            to,
            subject,
            body,
            date,
            labels: detail.label_ids.unwrap_or_default(),
            attachments,
        })
    }

    fn parse_parts(
        &self,
        part: &MessagePayload,
        body: &mut String,
        attachments: &mut Vec<Attachment>,
    ) {
        if let Some(body_data) = &part.body {
            if let Some(data) = &body_data.data {
                if let Some(decoded) = decode_base64url(data) {
                    if let Ok(text) = String::from_utf8(decoded) {
                        body.push_str(&text);
                    }
                }
            }
        }

        if let Some(parts) = &part.parts {
            for sub_part in parts {
                self.parse_part_recursive(sub_part, body, attachments);
            }
        }
    }

    fn parse_part_recursive(
        &self,
        part: &MessagePart,
        body: &mut String,
        attachments: &mut Vec<Attachment>,
    ) {
        if let Some(filename) = &part.filename {
            if !filename.is_empty() {
                if let Some(body_data) = &part.body {
                    if let Some(att_id) = &body_data.attachment_id {
                        attachments.push(Attachment {
                            id: att_id.clone(),
                            filename: filename.clone(),
                            mime_type: part
                                .mime_type
                                .clone()
                                .unwrap_or_else(|| "application/octet-stream".to_string()),
                            size: body_data.size.unwrap_or(0) as usize,
                            data: None,
                        });
                    }
                }
            }
        }

        if part.filename.as_ref().map(|f| f.is_empty()).unwrap_or(true) {
            if let Some(body_data) = &part.body {
                if let Some(data) = &body_data.data {
                    if let Some(decoded) = decode_base64url(data) {
                        if let Ok(text) = String::from_utf8(decoded) {
                            body.push_str(&text);
                        }
                    }
                }
            }
        }

        if let Some(parts) = &part.parts {
            for sub_part in parts {
                self.parse_part_recursive(sub_part, body, attachments);
            }
        }
    }

    async fn get_or_create_label_id(
        &self,
        token: &str,
        name: &str,
    ) -> Result<String, GraphonError> {
        let list_url = format!("{}/users/me/labels", self._api_url);
        let response = self.client.get(&list_url).bearer_auth(token).send().await?;
        if response.status().is_success() {
            let list: LabelsListResponse = response.json().await?;
            if let Some(labels) = list.labels {
                for label in labels {
                    if label.name.eq_ignore_ascii_case(name) || label.id.eq_ignore_ascii_case(name)
                    {
                        debug!(
                            "Resolved label name '{}' to existing ID '{}'",
                            name, label.id
                        );
                        return Ok(label.id);
                    }
                }
            }
        }

        if is_system_label(name) {
            return Ok(name.to_string());
        }

        info!("Label '{}' not found. Creating it...", name);
        let create_url = format!("{}/users/me/labels", self._api_url);
        let body = serde_json::json!({
            "name": name,
            "labelListVisibility": "labelShow",
            "messageListVisibility": "show"
        });
        let create_response = self
            .client
            .post(&create_url)
            .bearer_auth(token)
            .json(&body)
            .send()
            .await?;

        if create_response.status().is_success() {
            let new_label: Label = create_response.json().await?;
            Ok(new_label.id)
        } else {
            let err_body = create_response.text().await.unwrap_or_default();
            if err_body.contains("alreadyExists") || err_body.contains("Label name exists") {
                let response = self.client.get(&list_url).bearer_auth(token).send().await?;
                if response.status().is_success() {
                    let list: LabelsListResponse = response.json().await?;
                    if let Some(labels) = list.labels {
                        for label in labels {
                            if label.name.eq_ignore_ascii_case(name) {
                                return Ok(label.id);
                            }
                        }
                    }
                }
            }
            Err(GraphonError::Gmail(format!(
                "Failed to create or retrieve label ID for '{}': {}",
                name, err_body
            )))
        }
    }
}

fn decode_base64url(s: &str) -> Option<Vec<u8>> {
    let mut clean = s.replace('-', "+").replace('_', "/");
    while !clean.len().is_multiple_of(4) {
        clean.push('=');
    }

    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut map = [0u8; 256];
    for (i, &c) in ALPHABET.iter().enumerate() {
        map[c as usize] = i as u8;
    }

    let bytes = clean.as_bytes();
    let mut output = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'=' {
            break;
        }
        if i + 3 >= bytes.len() {
            return None;
        }

        let b1 = map[bytes[i] as usize] as u32;
        let b2 = map[bytes[i + 1] as usize] as u32;
        let b3 = if bytes[i + 2] == b'=' {
            0
        } else {
            map[bytes[i + 2] as usize] as u32
        };
        let b4 = if bytes[i + 3] == b'=' {
            0
        } else {
            map[bytes[i + 3] as usize] as u32
        };

        let triple = (b1 << 18) + (b2 << 12) + (b3 << 6) + b4;
        output.push(((triple >> 16) & 0xFF) as u8);
        if bytes[i + 2] != b'=' {
            output.push(((triple >> 8) & 0xFF) as u8);
        }
        if bytes[i + 3] != b'=' {
            output.push((triple & 0xFF) as u8);
        }
        i += 4;
    }
    Some(output)
}

fn read_token(token: &RwLock<Option<String>>) -> Option<String> {
    token.read().ok().and_then(|g| g.clone())
}

#[async_trait]
impl GmailPort for GmailClient {
    fn get_token(&self) -> Option<String> {
        read_token(&self.token)
    }

    async fn fetch_unread_emails(&self) -> Result<Vec<Email>, GraphonError> {
        info!("Fetching unread emails from Gmail...");
        if read_token(&self.token).is_none() {
            warn!("Gmail OAuth token not provided. Falling back to mock emails for local run.");
            return Ok(vec![
                Email {
                    id: "msg123".to_string(),
                    thread_id: "thread123".to_string(),
                    from: "newsletter@shop.com".to_string(),
                    to: vec!["user@example.com".to_string()],
                    subject: "50% off all items today only!".to_string(),
                    body: "Click here to unsubscribe from this sale newsletter.".to_string(),
                    date: Utc::now(),
                    labels: vec!["UNREAD".to_string()],
                    attachments: vec![],
                },
                Email {
                    id: "msg456".to_string(),
                    thread_id: "thread456".to_string(),
                    from: "boss@company.com".to_string(),
                    to: vec!["user@example.com".to_string()],
                    subject: "URGENT: Review quarterly report".to_string(),
                    body: "Please review the attached PDF as soon as possible and let me know your thoughts.".to_string(),
                    date: Utc::now(),
                    labels: vec!["UNREAD".to_string()],
                    attachments: vec![
                        Attachment {
                            id: "att1".to_string(),
                            filename: "quarterly_report.pdf".to_string(),
                            mime_type: "application/pdf".to_string(),
                            size: 1024,
                            data: None,
                        }
                    ],
                }
            ]);
        }

        let refs = self.fetch_message_ids("is:unread").await?;
        let mut emails = Vec::new();
        for r in refs {
            match self.fetch_message_details(&r.id).await {
                Ok(email) => emails.push(email),
                Err(e) => error!("Failed to fetch details for message {}: {:?}", r.id, e),
            }
        }
        Ok(emails)
    }

    async fn fetch_emails_by_query(&self, query: &str) -> Result<Vec<Email>, GraphonError> {
        info!("Fetching emails matching Gmail query: '{}'", query);
        if read_token(&self.token).is_none() {
            return Ok(vec![Email {
                id: "msg789".to_string(),
                thread_id: "thread789".to_string(),
                from: "alerts@system.com".to_string(),
                to: vec!["user@example.com".to_string()],
                subject: "System CPU Alert".to_string(),
                body: "CPU usage exceeded 90% threshold.".to_string(),
                date: Utc::now() - chrono::Duration::days(15),
                labels: vec![],
                attachments: vec![],
            }]);
        }

        let refs = self.fetch_message_ids(query).await?;
        let mut emails = Vec::new();
        for r in refs {
            match self.fetch_message_details(&r.id).await {
                Ok(email) => emails.push(email),
                Err(e) => error!(
                    "Failed to fetch details for query message {}: {:?}",
                    r.id, e
                ),
            }
        }
        Ok(emails)
    }

    async fn apply_labels(&self, email_id: &str, labels: &[String]) -> Result<(), GraphonError> {
        info!("Applying labels {:?} to email ID {}", labels, email_id);
        let token = match read_token(&self.token) {
            Some(t) => t,
            None => return Ok(()),
        };

        let mut label_ids = Vec::new();
        for label in labels {
            match self.get_or_create_label_id(&token, label).await {
                Ok(id) => label_ids.push(id),
                Err(e) => {
                    warn!("Could not resolve label ID for '{}': {:?}", label, e);
                    label_ids.push(label.clone());
                }
            }
        }

        let url = format!("{}/users/me/messages/{}/modify", self._api_url, email_id);
        let body = serde_json::json!({
            "addLabelIds": label_ids
        });

        let response = self
            .client
            .post(&url)
            .bearer_auth(token)
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let err_body = response.text().await.unwrap_or_default();
            return Err(GraphonError::Gmail(format!(
                "Failed to apply labels: {}",
                err_body
            )));
        }
        Ok(())
    }

    async fn remove_labels(&self, email_id: &str, labels: &[String]) -> Result<(), GraphonError> {
        info!("Removing labels {:?} from email ID {}", labels, email_id);
        let token = match read_token(&self.token) {
            Some(t) => t,
            None => return Ok(()),
        };

        let mut label_ids = Vec::new();
        for label in labels {
            match self.get_or_create_label_id(&token, label).await {
                Ok(id) => label_ids.push(id),
                Err(e) => {
                    warn!("Could not resolve label ID for '{}': {:?}", label, e);
                    label_ids.push(label.clone());
                }
            }
        }

        let url = format!("{}/users/me/messages/{}/modify", self._api_url, email_id);
        let body = serde_json::json!({
            "removeLabelIds": label_ids
        });

        let response = self
            .client
            .post(&url)
            .bearer_auth(token)
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let err_body = response.text().await.unwrap_or_default();
            return Err(GraphonError::Gmail(format!(
                "Failed to remove labels: {}",
                err_body
            )));
        }
        Ok(())
    }

    async fn trash_email(&self, email_id: &str) -> Result<(), GraphonError> {
        info!("Trashing email ID {}", email_id);
        let token = match read_token(&self.token) {
            Some(t) => t,
            None => return Ok(()),
        };
        let url = format!("{}/users/me/messages/{}/trash", self._api_url, email_id);

        let response = self.client.post(&url).bearer_auth(token).send().await?;

        if !response.status().is_success() {
            let err_body = response.text().await.unwrap_or_default();
            return Err(GraphonError::Gmail(format!(
                "Failed to trash email: {}",
                err_body
            )));
        }
        Ok(())
    }
}

fn is_system_label(label: &str) -> bool {
    matches!(
        label,
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
