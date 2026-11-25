//! Demo accounts, ported from `backend/internal/seed/seed.go`.

use azahar_auth::hash_password;
use azahar_store::{Store, User};
use chrono::{DateTime, TimeDelta, Utc};

use crate::{Options, StageResult};

const SAMPLE_PASSWORD: &str = "azahar-sample-customer-2026";
const SAMPLE_CUSTOMERS: usize = 12;

struct SeedUser {
    id: String,
    name: String,
    email: String,
    role: &'static str,
    created_at: DateTime<Utc>,
}

pub(crate) async fn seed_users(store: &Store, opts: &Options) -> StageResult<usize> {
    if store.count_users().await? > 0 {
        return Ok(0);
    }

    let now = Utc::now();
    let admin_hash = hash_password(&opts.admin_password)?;
    let customer_hash = hash_password(&opts.customer_password)?;
    let employee_hash = hash_password(&opts.employee_password)?;
    let sample_hash = hash_password(SAMPLE_PASSWORD)?;

    let mut users = vec![
        SeedUser {
            id: "usr_admin".to_string(),
            name: "Azahar admin".to_string(),
            email: opts.admin_email.clone(),
            role: "admin",
            created_at: now - TimeDelta::days(60),
        },
        SeedUser {
            id: "usr_staff".to_string(),
            name: "Azahar staff".to_string(),
            email: opts.employee_email.clone(),
            role: "employee",
            created_at: now - TimeDelta::days(55),
        },
        SeedUser {
            id: "usr_client".to_string(),
            name: "Demo client".to_string(),
            email: opts.customer_email.clone(),
            role: "customer",
            created_at: now - TimeDelta::days(50),
        },
    ];
    for index in 1..=SAMPLE_CUSTOMERS {
        users.push(SeedUser {
            id: format!("usr_sample_{index:02}"),
            name: format!("Sample customer {index:02}"),
            email: format!("sample{index:02}@azahar.example"),
            role: "customer",
            created_at: now - TimeDelta::days((index * 3) as i64),
        });
    }

    let tx = store.begin_write().await?;
    for user in &users {
        let hash = if user.role == "admin" {
            &admin_hash
        } else if user.role == "employee" {
            &employee_hash
        } else if user.email == opts.customer_email {
            &customer_hash
        } else {
            &sample_hash
        };
        store
            .create_user(
                tx.conn(),
                &User {
                    id: user.id.clone(),
                    name: user.name.clone(),
                    email: user.email.clone(),
                    password_hash: hash.clone(),
                    role: user.role.to_string(),
                    theme_preset: "cellar".to_string(),
                    created_at: Some(user.created_at),
                    updated_at: Some(user.created_at),
                },
            )
            .await?;
    }
    tx.commit().await?;

    Ok(users.len())
}
