use momento::cache::{configurations, CreateCacheResponse};
use momento::{CacheClient, CredentialProvider, MomentoError};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), MomentoError> {
    let cache_client = CacheClient::builder()
        .default_ttl(Duration::from_secs(60))
        .configuration(configurations::Laptop::latest())
        .credential_provider(CredentialProvider::from_env_var(
            "MOMENTO_API_KEY".to_string(),
        )?)
        .build()?;
    let cache_name = "CacheableTable";
    match cache_client.create_cache(cache_name).await? {
        CreateCacheResponse::Created => println!("Cache {} created", cache_name),
        CreateCacheResponse::AlreadyExists => println!("Cache {} already exists", cache_name),
    }
    Ok(())
}
