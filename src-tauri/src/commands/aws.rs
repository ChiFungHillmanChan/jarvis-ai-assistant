use crate::aws::credentials::{AwsCredentials, AwsCredsRepo};
use crate::db::Database;
use std::sync::Arc;
use tauri::State;

#[derive(serde::Serialize)]
pub struct AwsCredsStatus {
    pub configured: bool,
    pub region: Option<String>,
}

/// Bedrock cross-region inference profile ids are prefixed with the geography
/// they run in (`apac.`, `us.`, `eu.`), and a profile from one geography is not
/// valid against another geography's endpoint. Storing a hardcoded "apac" while
/// the user picks us-east-1 in Settings records a combination that cannot work.
/// Unrecognised regions keep "apac", which is the column default in V13 and the
/// deployment this app targets.
fn inference_profile_for_region(region: &str) -> &'static str {
    if region.starts_with("us-") {
        "us"
    } else if region.starts_with("eu-") {
        "eu"
    } else {
        "apac"
    }
}

#[tauri::command]
pub async fn set_aws_credentials(
    access_key_id: String,
    secret_access_key: String,
    region: String,
    db: State<'_, Arc<Database>>,
) -> Result<(), String> {
    let inference_profile = inference_profile_for_region(&region).to_string();
    let creds = AwsCredentials {
        access_key_id,
        secret_access_key,
        region,
        inference_profile,
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

/// The Bedrock model picker's list: from the user's AWS account for the
/// saved region, or the built-in list with the reason. Called by Settings
/// when the Models section shows a Bedrock row, and when the user asks to
/// load it again; never at start-up or in the background. Nothing is stored.
#[tauri::command]
pub async fn list_bedrock_models(
    db: State<'_, Arc<Database>>,
) -> Result<crate::aws::model_list::ModelList, String> {
    Ok(crate::aws::model_list::list_for(db.inner(), crate::aws::model_list::LIST_TIME_LIMIT).await)
}

#[cfg(test)]
mod tests {
    use super::inference_profile_for_region;

    /// Every region the Settings dropdown offers must store the profile that
    /// matches it, not a fixed "apac".
    #[test]
    fn profile_follows_the_selected_region() {
        assert_eq!(inference_profile_for_region("ap-southeast-1"), "apac");
        assert_eq!(inference_profile_for_region("ap-northeast-1"), "apac");
        assert_eq!(inference_profile_for_region("us-east-1"), "us");
        assert_eq!(inference_profile_for_region("eu-west-1"), "eu");
    }

    #[test]
    fn unknown_regions_keep_the_schema_default() {
        assert_eq!(inference_profile_for_region("sa-east-1"), "apac");
        assert_eq!(inference_profile_for_region(""), "apac");
    }
}
