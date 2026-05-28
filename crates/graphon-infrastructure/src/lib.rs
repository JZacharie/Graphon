pub mod adapters {
    pub mod classifier;
    pub mod database;
    pub mod gmail_client;
}

pub use adapters::classifier::ClassifierAdapter;
pub use adapters::database::DatabaseAdapter;
pub use adapters::gmail_client::GmailClient;
