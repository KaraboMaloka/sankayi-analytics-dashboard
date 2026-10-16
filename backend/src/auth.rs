//! Dashboard authentication: Argon2 password hashing + opaque DB-backed
//! session tokens (no JWT — sessions are stored server-side so logout genuinely
//! revokes them). Public routes: `login`, `forgot-password`. Everything else is
//! wrapped in a bearer-auth middleware (see `validator`) that resolves the token
//! to the current user and stashes it in the request extensions.

use actix_web::{get, post, web, HttpMessage, HttpResponse};
use actix_web::dev::ServiceRequest;
use actix_web_httpauth::extractors::bearer::BearerAuth;
use argon2::password_hash::rand_core::{OsRng, RngCore};
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::AppError;

const SESSION_DAYS: i64 = 30;
const MIN_PASSWORD_LEN: usize = 8;

/// The authenticated user, injected into request extensions by `validator`.
#[derive(Clone)]
pub struct AuthedUser {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
}

// ---- password + token primitives ----

/// Argon2id hash of a password (includes a random per-password salt).
pub fn hash_password(pw: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(pw.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| anyhow::anyhow!("hashing failed: {e}"))
}

fn verify_password(pw: &str, hash: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default().verify_password(pw.as_bytes(), &parsed).is_ok(),
        Err(_) => false,
    }
}

/// A 256-bit opaque session token as hex.
fn new_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unauthorized(msg: &str) -> HttpResponse {
    HttpResponse::Unauthorized()
        .json(serde_json::json!({ "error": { "code": "unauthorized", "message": msg } }))
}

fn bad_request(msg: &str) -> HttpResponse {
    HttpResponse::BadRequest()
        .json(serde_json::json!({ "error": { "code": "bad_request", "message": msg } }))
}

// ---- bearer middleware validator ----

/// Validate the Bearer token against the sessions table; on success, inject the
/// resolved `AuthedUser` into request extensions for downstream handlers.
pub async fn validator(
    req: ServiceRequest,
    credentials: BearerAuth,
) -> Result<ServiceRequest, (actix_web::Error, ServiceRequest)> {
    let Some(pool) = req.app_data::<web::Data<PgPool>>().map(|p| p.clone()) else {
        return Err((actix_web::error::ErrorInternalServerError("no db pool"), req));
    };
    let row = sqlx::query_as::<_, (Uuid, String, String)>(
        "SELECT u.id, u.username, u.display_name
         FROM sessions s JOIN users u ON u.id = s.user_id
         WHERE s.token = $1 AND s.expires_at > now()",
    )
    .bind(credentials.token())
    .fetch_optional(pool.get_ref())
    .await;

    match row {
        Ok(Some((id, username, display_name))) => {
            req.extensions_mut().insert(AuthedUser { id, username, display_name });
            Ok(req)
        }
        _ => Err((actix_web::error::ErrorUnauthorized("invalid or expired session"), req)),
    }
}

// ---- DTOs ----

#[derive(Deserialize)]
struct LoginBody {
    username: String,
    password: String,
}

#[derive(Serialize)]
struct UserInfo {
    username: String,
    display_name: String,
}

#[derive(Serialize)]
struct AuthResponse {
    token: String,
    user: UserInfo,
}

#[derive(Deserialize)]
struct ChangeBody {
    current_password: String,
    new_password: String,
}

#[derive(Deserialize)]
struct ForgotBody {
    username: String,
    new_password: String,
}

// ---- handlers ----

/// POST /api/login  (public)
#[post("/login")]
async fn login(pool: web::Data<PgPool>, body: web::Json<LoginBody>) -> Result<HttpResponse, AppError> {
    let username = body.username.trim().to_lowercase();
    let row = sqlx::query_as::<_, (Uuid, String, String)>(
        "SELECT id, display_name, password_hash FROM users WHERE username = $1",
    )
    .bind(&username)
    .fetch_optional(pool.get_ref())
    .await?;

    // Same message whether the user is missing or the password is wrong.
    let Some((id, display_name, hash)) = row else {
        return Ok(unauthorized("Invalid username or password"));
    };
    if !verify_password(&body.password, &hash) {
        return Ok(unauthorized("Invalid username or password"));
    }

    let token = new_token();
    sqlx::query("INSERT INTO sessions (token, user_id, expires_at) VALUES ($1, $2, $3)")
        .bind(&token)
        .bind(id)
        .bind(Utc::now() + Duration::days(SESSION_DAYS))
        .execute(pool.get_ref())
        .await?;

    Ok(HttpResponse::Ok().json(AuthResponse { token, user: UserInfo { username, display_name } }))
}

/// GET /api/me  (protected) — used by the frontend to validate a stored token.
#[get("/me")]
async fn me(user: web::ReqData<AuthedUser>) -> Result<HttpResponse, AppError> {
    Ok(HttpResponse::Ok().json(UserInfo {
        username: user.username.clone(),
        display_name: user.display_name.clone(),
    }))
}

/// POST /api/logout  (protected) — revoke the current session.
#[post("/logout")]
async fn logout(pool: web::Data<PgPool>, credentials: BearerAuth) -> Result<HttpResponse, AppError> {
    sqlx::query("DELETE FROM sessions WHERE token = $1")
        .bind(credentials.token())
        .execute(pool.get_ref())
        .await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "ok": true })))
}

/// POST /api/change-password  (protected) — needs the current password.
#[post("/change-password")]
async fn change_password(
    pool: web::Data<PgPool>,
    user: web::ReqData<AuthedUser>,
    body: web::Json<ChangeBody>,
) -> Result<HttpResponse, AppError> {
    if body.new_password.len() < MIN_PASSWORD_LEN {
        return Ok(bad_request("New password must be at least 8 characters."));
    }
    let hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1")
        .bind(user.id)
        .fetch_one(pool.get_ref())
        .await?;
    if !verify_password(&body.current_password, &hash) {
        return Ok(unauthorized("Current password is incorrect."));
    }
    let new_hash = hash_password(&body.new_password).map_err(AppError::internal)?;
    sqlx::query("UPDATE users SET password_hash = $1, updated_at = now() WHERE id = $2")
        .bind(&new_hash)
        .bind(user.id)
        .execute(pool.get_ref())
        .await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "ok": true })))
}

/// POST /api/forgot-password  (public) — resets by username (no email service
/// here). Reasonable for a small trusted internal team; a public deployment
/// would gate this behind an emailed reset link.
#[post("/forgot-password")]
async fn forgot_password(
    pool: web::Data<PgPool>,
    body: web::Json<ForgotBody>,
) -> Result<HttpResponse, AppError> {
    if body.new_password.len() < MIN_PASSWORD_LEN {
        return Ok(bad_request("New password must be at least 8 characters."));
    }
    let username = body.username.trim().to_lowercase();
    let new_hash = hash_password(&body.new_password).map_err(AppError::internal)?;
    let res = sqlx::query("UPDATE users SET password_hash = $1, updated_at = now() WHERE username = $2")
        .bind(&new_hash)
        .bind(&username)
        .execute(pool.get_ref())
        .await?;
    if res.rows_affected() == 0 {
        return Ok(unauthorized("No account exists with that username."));
    }
    // Force re-login everywhere for that user.
    sqlx::query("DELETE FROM sessions WHERE user_id = (SELECT id FROM users WHERE username = $1)")
        .bind(&username)
        .execute(pool.get_ref())
        .await
        .ok();
    Ok(HttpResponse::Ok().json(serde_json::json!({ "ok": true })))
}

/// Public auth routes (no token required).
pub fn public(cfg: &mut web::ServiceConfig) {
    cfg.service(login).service(forgot_password);
}

/// Protected auth routes (require a valid session).
pub fn protected(cfg: &mut web::ServiceConfig) {
    cfg.service(me).service(logout).service(change_password);
}
