use crate::config::RunConfig;
use crate::engine::Engine;
use crate::metrics::{Metrics, Snapshot};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Serialize;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::broadcast;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;
use tracing::info;

#[derive(Clone)]
pub struct AppState {
    pub metrics: Arc<Metrics>,
    pub engine: Arc<Engine>,
    pub tx: broadcast::Sender<Snapshot>,
    pub web_dir: PathBuf,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Health {
    ok: bool,
    version: &'static str,
    busy: bool,
}

pub async fn serve(bind: SocketAddr, web_dir: PathBuf) -> anyhow::Result<()> {
    let metrics = Arc::new(Metrics::default());
    let engine = Arc::new(Engine::new(metrics.clone()));
    let (tx, _) = broadcast::channel::<Snapshot>(256);

    let state = AppState {
        metrics: metrics.clone(),
        engine: engine.clone(),
        tx: tx.clone(),
        web_dir: web_dir.clone(),
    };

    // live ticker for WS clients
    let tick_metrics = metrics.clone();
    let tick_tx = tx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(250));
        loop {
            interval.tick().await;
            let snap = tick_metrics.snapshot();
            let _ = tick_tx.send(snap);
        }
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let api = Router::new()
        .route("/api/health", get(health))
        .route("/api/metrics", get(get_metrics))
        .route("/api/report", get(get_report))
        .route("/api/run", post(start_run))
        .route("/api/stop", post(stop_run))
        .route("/ws", get(ws_handler));

    let static_files = ServeDir::new(&web_dir).append_index_html_on_directories(true);

    let app = Router::new()
        .merge(api)
        .route("/", get(index_html))
        .fallback_service(static_files)
        .layer(cors)
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store"),
        ))
        .with_state(state);

    info!(%bind, web = %web_dir.display(), "loadforge dashboard listening");
    let listener = tokio::net::TcpListener::bind(bind).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health(State(state): State<AppState>) -> Json<Health> {
    Json(Health {
        ok: true,
        version: env!("CARGO_PKG_VERSION"),
        busy: state.engine.is_busy(),
    })
}

async fn get_metrics(State(state): State<AppState>) -> Json<Snapshot> {
    Json(state.metrics.snapshot())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    summary: Snapshot,
    grade: &'static str,
    notes: Vec<&'static str>,
}

async fn get_report(State(state): State<AppState>) -> Json<Report> {
    let summary = state.metrics.snapshot();
    let err_rate = if summary.total == 0 {
        0.0
    } else {
        summary.err as f64 / summary.total as f64
    };
    let grade = if summary.total == 0 {
        "idle"
    } else if err_rate < 0.01 && summary.p95_ms < 300.0 {
        "A"
    } else if err_rate < 0.05 && summary.p95_ms < 800.0 {
        "B"
    } else if err_rate < 0.15 {
        "C"
    } else {
        "D"
    };
    let mut notes = Vec::new();
    if summary.total == 0 {
        notes.push("No samples yet — ignite a run.");
    } else {
        if err_rate >= 0.05 {
            notes.push("Error rate is elevated; inspect 4xx/5xx mix.");
        }
        if summary.p95_ms >= 500.0 {
            notes.push("p95 latency is high under current concurrency.");
        }
        if summary.rps > 0.0 {
            notes.push("Throughput captured in summary.rps.");
        }
    }
    Json(Report {
        summary,
        grade,
        notes,
    })
}

async fn start_run(
    State(state): State<AppState>,
    Json(config): Json<RunConfig>,
) -> Response {
    if state.engine.is_busy() {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": "run already in progress" })),
        )
            .into_response();
    }

    let engine = state.engine.clone();
    tokio::spawn(async move {
        if let Err(e) = engine.run(config).await {
            tracing::error!(error = %e, "run failed");
        }
    });

    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "ok": true, "message": "forge started" })),
    )
        .into_response()
}

async fn stop_run(State(state): State<AppState>) -> Json<serde_json::Value> {
    state.engine.stop();
    Json(serde_json::json!({ "ok": true, "message": "stop signaled" }))
}

async fn index_html(State(state): State<AppState>) -> Response {
    let path = state.web_dir.join("index.html");
    match tokio::fs::read_to_string(path).await {
        Ok(html) => Html(html).into_response(),
        Err(_) => (
            StatusCode::NOT_FOUND,
            "web/index.html missing — run from the loadforge repo root",
        )
            .into_response(),
    }
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| client_ws(socket, state))
}

async fn client_ws(mut socket: WebSocket, state: AppState) {
    let mut rx = state.tx.subscribe();
    // push immediate snapshot
    let first = state.metrics.snapshot();
    if socket
        .send(Message::Text(serde_json::to_string(&first).unwrap_or_default()))
        .await
        .is_err()
    {
        return;
    }

    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Ok(snap) => {
                        let text = serde_json::to_string(&snap).unwrap_or_default();
                        if socket.send(Message::Text(text)).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(p))) => {
                        let _ = socket.send(Message::Pong(p)).await;
                    }
                    _ => {}
                }
            }
        }
    }
}
