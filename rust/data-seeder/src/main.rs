use std::env;
use std::sync::Arc;

use aws_config::{BehaviorVersion, Region};
use aws_sdk_dsql::auth_token::{AuthTokenGenerator, Config};
use shared::models::model::CacheableItem;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;

async fn load_data(pool: &PgPool) {
    let mut children = vec![];

    for c in 0..100 {
        let clone_pool = pool.clone();
        let handle = tokio::spawn(async move {
            for j in 0..1000 {
                let i = CacheableItem::default();

                let result = sqlx::query("INSERT INTO CacheableTable (id, first_name, last_name, created_at, updated_at) VALUES ($1, $2, $3, $4, $5)")
            .bind(i.id.to_owned())
            .bind(i.first_name.clone())
            .bind(i.last_name.clone())
            .bind(i.created_at)
            .bind(i.updated_at)
            .execute(&clone_pool)
            .await;

                match result {
                    Ok(_) => {
                        println!("(Item)={:?}", i);
                    }
                    Err(e) => {
                        println!("Error saving entity: {}", e);
                        //break;
                    }
                }
            }
        });
        children.push(handle);
    }

    for t in children {
        t.await.unwrap();
    }
}

#[tokio::main]
async fn main() -> Result<(), sqlx::Error> {
    let region = "us-east-1";
    let cluster_endpoint = env::var("CLUSTER_ENDPOINT").expect("CLUSTER_ENDPOINT required");
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
        .await;
    match pool {
        Ok(p) => {
            load_data(&p).await;
            Ok(())
        }
        Err(e) => {
            tracing::error!("Error creating pool: {:?}", e);
            Err(e)
        }
    }
}
