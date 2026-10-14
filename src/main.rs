use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::json; // FIX 1: removed `Value::String` (it shadowed std's String)

#[derive(Debug, Clone, Serialize)]
struct Task {
    id: u64,
    title: String,
    description: Option<String>,
    completed: bool,
}

#[derive(Debug, Deserialize)]
struct CreateTask {
    title: String,
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UpdateTask {
    title: String,
    description: Option<String>,
    completed: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ListFilter {
    completed: Option<bool>,
}

#[derive(Default)]
struct Store {
    tasks: HashMap<u64, Task>,
    next_id: u64,
}

type AppState = Arc<RwLock<Store>>;

enum AppError {
    NotFound(u64),
    Validation(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AppError::NotFound(id) => (StatusCode::NOT_FOUND, format!("task {} not found", id)),
            AppError::Validation(msg) => (StatusCode::UNPROCESSABLE_ENTITY, msg),
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}

fn validate_title(title: &str) -> Result<(), AppError> {
    if title.trim().is_empty() {
        Err(AppError::Validation("title must not be empty".into()))
    } else {
        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: bool,
    message: String,
}

async fn health_route() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: true,
        message: "Server is running".to_string(),
    })
}

async fn add_task(
    State(state): State<AppState>,
    Json(payload): Json<CreateTask>,
) -> Result<(StatusCode, Json<Task>), AppError> {
    validate_title(&payload.title)?;

    let mut store = state.write().await;
    store.next_id += 1;

    let task = Task {
        id: store.next_id,
        title: payload.title.trim().to_string(),
        description: payload.description,
        completed: false,
    };

    store.tasks.insert(task.id, task.clone());

    Ok((StatusCode::CREATED, Json(task)))
}

async fn list_tasks(
    State(state): State<AppState>,
    Query(filter): Query<ListFilter>,
) -> Json<Vec<Task>> {
    let store = state.read().await;

    let mut tasks: Vec<Task> = store
        .tasks
        .values()
        .filter(|t| filter.completed.map_or(true, |c| t.completed == c))
        .cloned()
        .collect();

    tasks.sort_by_key(|t| t.id);

    Json(tasks)
}

async fn get_task(
    State(state): State<AppState>,
    Path(id): Path<u64>,
) -> Result<Json<Task>, AppError> {
    let store = state.read().await;
    store
        .tasks
        .get(&id)
        .cloned()
        .map(Json)
        .ok_or(AppError::NotFound(id))
}

async fn update_task(
    State(state): State<AppState>,
    Path(id): Path<u64>,
    Json(payload): Json<UpdateTask>,
) -> Result<Json<Task>, AppError> {
    validate_title(&payload.title)?;

    let mut store = state.write().await;
    let task = store.tasks.get_mut(&id).ok_or(AppError::NotFound(id))?;

    task.title = payload.title.trim().to_string();

    if let Some(description) = payload.description {
        task.description = Some(description);
    }
    if let Some(completed) = payload.completed {
        task.completed = completed;
    }

    Ok(Json(task.clone()))
}

// FIX 2: needs `Serialize`, otherwise `Json<SuccessMessage>` can't be returned
#[derive(Debug, Serialize)]
struct SuccessMessage {
    message: String,
}

async fn delete_task(
    State(state): State<AppState>,
    Path(id): Path<u64>,
) -> Result<(StatusCode, Json<SuccessMessage>), AppError> {
    let mut store = state.write().await;

    // FIX 3: the old `.map(|_| NO_CONTENT)` result was thrown away; just check removal
    store.tasks.remove(&id).ok_or(AppError::NotFound(id))?;

    Ok((
        StatusCode::OK,
        Json(SuccessMessage {
            // FIX 4: field is a String, so convert the &str literal
            message: "Deleted Successfully".to_string(),
        }),
    ))
}

#[tokio::main]
async fn main() {
    let port = 3000;
    let state: AppState = Arc::new(RwLock::new(Store::default()));

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        // FIX 5: `expect` doesn't format `{}`; use unwrap_or_else + panic! to interpolate
        .unwrap_or_else(|e| panic!("Unable to bind to port {port}: {e}"));

    println!("Listening on http://{}", listener.local_addr().unwrap());

    axum::serve(listener, app(state))
        .await
        .expect("Error in serving from axum");
}

fn app(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health_route))
        .route("/api/task", get(list_tasks).post(add_task))
        .route(
            "/api/task/{id}",
            get(get_task).put(update_task).delete(delete_task),
        )
        .with_state(state)
}