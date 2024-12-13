use std::{env, str::FromStr, time::Duration};

use ::tracing::Instrument;
use aws_config::{BehaviorVersion, Region};
use aws_sdk_dsql::auth_token::{AuthTokenGenerator, Config};
use lambda_http::{
    http::{Response, StatusCode},
    run, service_fn, Error, IntoResponse, Request, RequestExt,
};
use momento::{cache::configurations, CacheClient, CredentialProvider, MomentoError};
use serde_json::json;
use shared::models::model::CacheableItem;
use sqlx::PgPool;
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    query_as,
};
use tracing::instrument;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, Registry};
use uuid::Uuid;

#[instrument(name = "Write Cache")]
async fn write_to_cache(client: &CacheClient, cache_name: String, item: CacheableItem) {
    let query_span = tracing::info_span!("Momento SET");

    let value = serde_json::to_string(&item).unwrap();
    let result = client
        .set(cache_name, item.id.to_string(), value.clone())
        .instrument(query_span)
        .await;

    match result {
        Ok(_) => {
            tracing::info!("Cache item set");
            tracing::info!("(Item)={:?}", value);
        }
        Err(e) => {
            tracing::error!("(CacheWriteError)={}", e);
        }
    }
}

#[instrument(name = "Query Cache")]
async fn query_cache(
    client: &CacheClient,
    cache_name: String,
    id: String,
) -> Option<CacheableItem> {
    let query_span = tracing::info_span!("Momento GET");
    let response = client.get(cache_name, id).instrument(query_span).await;

    match response {
        Ok(r) => {
            let item: Result<String, MomentoError> = r.try_into();

            match item {
                Ok(i) => {
                    let o: CacheableItem = serde_json::from_str(i.as_str()).unwrap();
                    tracing::info!("(CacheItem)={:?}", o);
                    Some(o)
                }
                Err(e) => {
                    tracing::info!("(Cache MISS)={}", e);
                    None
                }
            }
        }
        Err(e) => {
            tracing::error!("(GetResponseError)={}", e);
            None
        }
    }
}

#[instrument(name = "DSQL Query")]
async fn query_row(pool: &PgPool, u: Uuid) -> Option<CacheableItem> {
    let query_span = tracing::info_span!("DSQL Read");
    let item = query_as!(
        CacheableItem,
        "select id, first_name, last_name, created_at, updated_at from CacheableTable where id = $1",
        u
    )
    .fetch_optional(pool)
        .instrument(query_span)
    .await;

    item.unwrap_or_default()
}

#[instrument(name = "Function Handler")]
async fn function_handler(
    pool: &PgPool,
    cache_client: &CacheClient,
    cache_name: &str,
    request: Request,
) -> Result<impl IntoResponse, Error> {
    let id = request
        .query_string_parameters_ref()
        .and_then(|params| params.first("id"))
        .unwrap();

    let mut body = json!("").to_string();
    let mut status_code = StatusCode::OK;
    let u = Uuid::from_str(id).unwrap();
    let cache_item = query_cache(cache_client, cache_name.to_owned(), id.to_string()).await;

    match cache_item {
        Some(i) => {
            tracing::info!("Cache HIT!");
            body = serde_json::to_string(&i).unwrap();
        }
        None => {
            tracing::info!("Cache MISS!");
            let item = query_row(pool, u).await;
            match item {
                Some(i) => {
                    write_to_cache(cache_client, cache_name.to_owned(), i.clone()).await;
                    body = serde_json::to_string(&i).unwrap();
                }
                None => {
                    status_code = StatusCode::NOT_FOUND;
                }
            }
        }
    }

    let response = Response::builder()
        .status(status_code)
        .header("Content-Type", "application/json")
        .body(body)
        .map_err(Box::new)?;
    Ok(response)
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let tracer = opentelemetry_datadog::new_pipeline()
        .with_service_name("get-lambda")
        .with_agent_endpoint("http://127.0.0.1:8126")
        .with_api_version(opentelemetry_datadog::ApiVersion::Version05)
        .with_trace_config(
            opentelemetry_sdk::trace::config()
                .with_sampler(opentelemetry_sdk::trace::Sampler::AlwaysOn)
                .with_id_generator(opentelemetry_sdk::trace::RandomIdGenerator::default()),
        )
        .install_simple()
        .unwrap();
    let telemetry_layer = tracing_opentelemetry::layer().with_tracer(tracer);
    let logger = tracing_subscriber::fmt::layer().json().flatten_event(true);
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_target(false)
        .without_time();

    Registry::default()
        .with(fmt_layer)
        .with(telemetry_layer)
        .with(logger)
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let region = "us-east-1";
    let cluster_endpoint = env::var("CLUSTER_ENDPOINT").expect("CLUSTER_ENDPOINT required");
    let momento_key = env::var("MOMENTO_API_KEY").expect("MOMENTO_API_KEY required");
    let cache_name = env::var("CACHE_NAME").expect("CACHE_NAME required");

    // Generate auth token
    let sdk_config = aws_config::load_defaults(BehaviorVersion::latest()).await;
    let signer = AuthTokenGenerator::new(
        Config::builder()
            .hostname(&cluster_endpoint)
            .region(Region::new(region))
            .build()
            .unwrap(),
    );
    let password_token = signer
        .db_connect_admin_auth_token(&sdk_config)
        .await
        .unwrap();

    // Setup connections
    let connection_options = PgConnectOptions::new()
        .host(cluster_endpoint.as_str())
        .port(5432)
        .database("postgres")
        .username("admin")
        .password(password_token.as_str())
        .ssl_mode(sqlx::postgres::PgSslMode::VerifyFull);

    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect_with(connection_options.clone())
        .await?;
    let shared_pool = &pool;

    let cache_client = CacheClient::builder()
        .default_ttl(Duration::from_secs(5))
        .configuration(configurations::Lambda::latest())
        .credential_provider(CredentialProvider::from_string(momento_key).unwrap())
        .build()?;

    let shared_cache_client = &cache_client;
    let shared_cache_name = &cache_name;

    run(service_fn(move |event: Request| async move {
        function_handler(shared_pool, shared_cache_client, shared_cache_name, event).await
    }))
    .await
}
