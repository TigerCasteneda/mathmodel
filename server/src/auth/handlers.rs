use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{extract::State, routing::post, Json, Router};
use chrono::Utc;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use uuid::Uuid;

use super::model::*;
use crate::{config::Config, error::AppError, AppState};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/register", post(register))
        .route("/login", post(login))
        .route("/refresh", post(refresh))
}

fn generate_tokens(user_id: &str, config: &Config) -> Result<(String, String), AppError> {
    let now = Utc::now().timestamp() as usize;

    let claims = Claims {
        sub: user_id.to_string(),
        exp: now + 86400,
        iat: now,
        token_type: "access".into(),
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(config.jwt_secret.as_bytes()),
    )?;

    let refresh_claims = Claims {
        sub: user_id.to_string(),
        exp: now + 604800,
        iat: now,
        token_type: "refresh".into(),
    };
    let refresh_token = encode(
        &Header::default(),
        &refresh_claims,
        &EncodingKey::from_secret(config.jwt_secret.as_bytes()),
    )?;

    Ok((token, refresh_token))
}

fn hash_password(password: &str) -> Result<String, AppError> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)?
        .to_string();
    Ok(hash)
}

fn verify_password(password: &str, hash: &str) -> Result<bool, AppError> {
    let parsed_hash = PasswordHash::new(hash)?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok())
}

async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> Result<Json<AuthResponse>, AppError> {
    if req.email.is_empty() || req.password.len() < 6 || req.display_name.is_empty() {
        return Err(AppError::BadRequest("invalid input".into()));
    }

    let user_id = Uuid::new_v4().to_string();
    let now = Utc::now().timestamp();
    let password_hash = hash_password(&req.password)?;

    sqlx::query(
        "INSERT INTO users (id, email, password_hash, display_name, created_at) VALUES (?, ?, ?, ?, ?)"
    )
    .bind(&user_id)
    .bind(&req.email)
    .bind(&password_hash)
    .bind(&req.display_name)
    .bind(now)
    .execute(&state.pool)
    .await?;

    let (token, refresh_token) = generate_tokens(&user_id, &state.config)?;

    Ok(Json(AuthResponse {
        token,
        refresh_token,
        user: UserProfile {
            id: user_id,
            email: req.email,
            display_name: req.display_name,
        },
    }))
}

async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<AuthResponse>, AppError> {
    let user: super::model::User = sqlx::query_as("SELECT * FROM users WHERE email = ?")
        .bind(&req.email)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::Unauthorized("invalid credentials".into()))?;

    if !verify_password(&req.password, &user.password_hash)? {
        return Err(AppError::Unauthorized("invalid credentials".into()));
    }

    let (token, refresh_token) = generate_tokens(&user.id, &state.config)?;

    Ok(Json(AuthResponse {
        token,
        refresh_token,
        user: UserProfile {
            id: user.id,
            email: user.email,
            display_name: user.display_name,
        },
    }))
}

async fn refresh(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<AuthResponse>, AppError> {
    let refresh_str = body["refresh_token"]
        .as_str()
        .ok_or_else(|| AppError::BadRequest("refresh_token required".into()))?;

    let claims = decode::<Claims>(
        refresh_str,
        &DecodingKey::from_secret(state.config.jwt_secret.as_bytes()),
        &Validation::default(),
    )?;

    if claims.claims.token_type != "refresh" {
        return Err(AppError::Unauthorized("invalid refresh token".into()));
    }

    let user: super::model::User = sqlx::query_as("SELECT * FROM users WHERE id = ?")
        .bind(&claims.claims.sub)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::Unauthorized("user not found".into()))?;

    let (token, refresh_token) = generate_tokens(&user.id, &state.config)?;

    Ok(Json(AuthResponse {
        token,
        refresh_token,
        user: UserProfile {
            id: user.id,
            email: user.email,
            display_name: user.display_name,
        },
    }))
}
