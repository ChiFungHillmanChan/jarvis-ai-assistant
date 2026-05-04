use crate::aws::credentials::{AwsCredentials, AwsCredsRepo};
use crate::db::Database;
use std::sync::Arc;
use tauri::State;

#[derive(serde::Serialize)]
pub struct AwsCredsStatus {
    pub configured: bool,
    pub region: Option<String>,
}

#[tauri::command]
pub async fn set_aws_credentials(
    access_key_id: String,
    secret_access_key: String,
    region: String,
    db: State<'_, Arc<Database>>,
) -> Result<(), String> {
    let creds = AwsCredentials {
        access_key_id,
        secret_access_key,
        region,
        inference_profile: "apac".to_string(),
    };
    AwsCredsRepo::new(db.inner().clone()).save(&creds).await
}

#[tauri::command]
pub async fn get_aws_credentials_status(
    db: State<'_, Arc<Database>>,
) -> Result<AwsCredsStatus, String> {
    let loaded = AwsCredsRepo::new(db.inner().clone()).load().await?;
    Ok(match loaded {
        Some(c) => AwsCredsStatus {
            configured: true,
            region: Some(c.region),
        },
        None => AwsCredsStatus {
            configured: false,
            region: None,
        },
    })
}

#[tauri::command]
pub async fn clear_aws_credentials(
    db: State<'_, Arc<Database>>,
) -> Result<(), String> {
    AwsCredsRepo::new(db.inner().clone()).clear().await
}
