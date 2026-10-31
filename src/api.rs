use crate::{model::*, service::Service};
use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Path, Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Value, json};
use std::sync::Arc;
use subtle::ConstantTimeEq;

#[derive(Clone)]
pub struct ApiState {
    pub service: Arc<Service>,
    pub token: String,
    pub authority: String,
}
#[derive(Debug)]
pub struct ApiError(pub anyhow::Error);
impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        Self(e)
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let message = format!("{:#}", self.0);
        let status = if message.starts_with("not found:") {
            StatusCode::NOT_FOUND
        } else if message.contains("operation is active") || message.contains("already exists") {
            StatusCode::CONFLICT
        } else {
            StatusCode::BAD_REQUEST
        };
        (status, Json(json!({"error":{"message":message}}))).into_response()
    }
}
async fn guard(State(state): State<ApiState>, request: Request, next: Next) -> Response {
    let supplied = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .unwrap_or("");
    if supplied.len() != state.token.len()
        || !bool::from(supplied.as_bytes().ct_eq(state.token.as_bytes()))
    {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":{"message":"authentication required"}})),
        )
            .into_response();
    }
    if request.headers().contains_key(header::ORIGIN)
        || request
            .headers()
            .get(header::HOST)
            .and_then(|h| h.to_str().ok())
            != Some(state.authority.as_str())
    {
        return (StatusCode::FORBIDDEN,Json(json!({"error":{"message":"browser origins and unexpected Host headers are forbidden; use the native client"}}))).into_response();
    }
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
}
pub fn router(state: ApiState) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/doctor", get(doctor))
        .route("/v1/apps", get(apps).post(register))
        .route("/v1/apps/{name}", get(app))
        .route("/v1/apps/{name}/actions", post(action))
        .route("/v1/apps/{name}/releases", get(releases))
        .route("/v1/apps/{name}/backups", get(backups))
        .route("/v1/apps/{name}/logs", get(logs))
        .route("/v1/jobs", get(jobs))
        .route("/v1/jobs/{id}", get(job))
        .route("/v1/jobs/{id}/log", get(job_log))
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .with_state(state)
}
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| ApiError(e.into()))?
        .map_err(ApiError)
}
async fn health() -> Json<Value> {
    Json(json!({"version":env!("CARGO_PKG_VERSION"),"api":1}))
}
async fn doctor(State(s): State<ApiState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(blocking(move || Ok(s.service.engine.doctor())).await?))
}
async fn apps(State(s): State<ApiState>) -> Result<Json<Vec<Application>>, ApiError> {
    Ok(Json(s.service.store.apps()?))
}
async fn register(
    State(s): State<ApiState>,
    Json(spec): Json<AppSpec>,
) -> Result<(StatusCode, Json<Application>), ApiError> {
    Ok((
        StatusCode::CREATED,
        Json(blocking(move || s.service.register(spec)).await?),
    ))
}
async fn app(State(s): State<ApiState>, Path(name): Path<String>) -> Result<Json<Value>, ApiError> {
    let a = s.service.store.app(&name)?;
    Ok(Json(
        blocking(move || {
            let observed = match s.service.engine.state(&a.machine) {
                Ok(v) => v,
                Err(e) => json!({"error":e.to_string()}),
            };
            Ok(json!({"application":a,"observed":observed}))
        })
        .await?,
    ))
}
async fn action(
    State(s): State<ApiState>,
    Path(name): Path<String>,
    Json(a): Json<Action>,
) -> Result<(StatusCode, Json<Job>), ApiError> {
    Ok((StatusCode::ACCEPTED, Json(s.service.enqueue(&name, a)?)))
}
async fn releases(
    State(s): State<ApiState>,
    Path(name): Path<String>,
) -> Result<Json<Vec<Release>>, ApiError> {
    s.service.store.app(&name)?;
    Ok(Json(s.service.store.releases(&name)?))
}
async fn backups(
    State(s): State<ApiState>,
    Path(name): Path<String>,
) -> Result<Json<Vec<Value>>, ApiError> {
    s.service.store.app(&name)?;
    Ok(Json(blocking(move || s.service.backups(&name)).await?))
}
async fn jobs(State(s): State<ApiState>) -> Result<Json<Vec<Job>>, ApiError> {
    Ok(Json(s.service.store.jobs()?))
}
async fn job(State(s): State<ApiState>, Path(id): Path<String>) -> Result<Json<Job>, ApiError> {
    Ok(Json(s.service.store.job(&id)?))
}
async fn job_log(State(s): State<ApiState>, Path(id): Path<String>) -> Result<Response, ApiError> {
    s.service.store.job(&id)?;
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Body::from(s.service.job_log(&id)?))
        .unwrap())
}
async fn logs(State(s): State<ApiState>, Path(name): Path<String>) -> Result<Response, ApiError> {
    let a = s.service.store.app(&name)?;
    let text = blocking(move || {
        s.service.engine.machine(
            "logs",
            &a.machine,
            &["--tail".into(), "200".into()],
            30,
            None,
        )
    })
    .await?;
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Body::from(text))
        .unwrap())
}
