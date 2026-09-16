//! Just Prompt, Motherfucker.
//!
//! The whole website: one binary, one SQLite file, no JavaScript. The
//! manifesto is compiled in; see `render`.

mod db;
mod render;

use std::error::Error;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::{
    Router,
    extract::State,
    http::{StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use clap::{Parser, Subcommand};
use rusqlite::Connection;
use tokio::net::TcpListener;
use tokio::signal;

#[derive(Parser)]
#[command(name = "jpmf", version, about = "Just Prompt, Motherfucker")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Serve the site. This is the default when no subcommand is given.
    Serve,
    /// Write a consistent snapshot of the database to DEST.
    ///
    /// Safe to run while the site is serving: the snapshot is taken in a read
    /// transaction with the write-ahead log folded in. DEST must not exist.
    Backup {
        /// Where to write the snapshot, e.g. ~/jpmf-2026-09-16.db
        dest: PathBuf,
    },
}

struct Config {
    bind: String,
    db: PathBuf,
    base_url: String,
}

impl Config {
    /// Defaults are the local development values, so `cargo run` just works.
    /// The deploy overrides all three through the systemd EnvironmentFile.
    fn from_env() -> Self {
        let var =
            |key: &str, default: &str| std::env::var(key).unwrap_or_else(|_| default.to_string());
        Self {
            bind: var("JPMF_BIND", "127.0.0.1:8100"),
            db: PathBuf::from(var("JPMF_DB", "jpmf.db")),
            base_url: var("JPMF_BASE_URL", "http://localhost:8100"),
        }
    }
}

#[derive(Clone)]
struct App {
    db: Arc<Mutex<Connection>>,
    base_url: Arc<str>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    let cfg = Config::from_env();

    match cli.command.unwrap_or(Command::Serve) {
        // Only `serve` needs an async runtime; `backup` is a few milliseconds
        // of synchronous SQLite and has no business starting one.
        Command::Serve => tokio::runtime::Runtime::new()?.block_on(serve(cfg)),
        Command::Backup { dest } => {
            // Without this guard SQLite would happily open a missing database,
            // create it empty, and hand back a perfectly valid snapshot of
            // nothing — the worst possible outcome for a backup command.
            if !cfg.db.exists() {
                return Err(format!(
                    "no database at {} (set JPMF_DB, or run this from the directory holding it)",
                    cfg.db.display()
                )
                .into());
            }
            if dest.exists() {
                return Err(format!("{} already exists", dest.display()).into());
            }
            db::backup(&cfg.db, &dest)?;
            println!("snapshot written to {}", dest.display());
            Ok(())
        }
    }
}

async fn serve(cfg: Config) -> Result<(), Box<dyn Error>> {
    let conn = db::open(&cfg.db, render::manifesto_sha())?;
    let app = App {
        db: Arc::new(Mutex::new(conn)),
        base_url: cfg.base_url.as_str().into(),
    };

    let router = Router::new()
        .route("/", get(index))
        .route("/sign", post(sign))
        .route("/healthz", get(healthz))
        .route("/robots.txt", get(robots))
        .with_state(app);

    let listener = TcpListener::bind(&cfg.bind).await?;
    println!(
        "jpmf listening on http://{}  (db {}, manifesto {})",
        cfg.bind,
        cfg.db.display(),
        &render::manifesto_sha()[..12],
    );

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}

/// The manifesto, then everyone who signed it.
///
/// Rendered from SQLite on every request. At this scale that is well under a
/// millisecond, and it means there is no cache to invalidate when someone
/// signs — they refresh and they are simply there.
async fn index(State(app): State<App>) -> Response {
    let rendered = (|| -> rusqlite::Result<String> {
        // Short critical section, and nothing is awaited while the lock is
        // held, so a std Mutex is the right tool here.
        let conn = app
            .db
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let count = db::count(&conn)?;
        let signers = db::signatures(&conn)?;
        Ok(render::page(&app.base_url, count, &signers))
    })();

    match rendered {
        Ok(html) => Html(html).into_response(),
        Err(err) => {
            eprintln!("database error while rendering the page: {err}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html(render::notice(
                    &app.base_url,
                    "Something broke",
                    "The manifesto is fine. The database is not. Try again in a moment.",
                )),
            )
                .into_response()
        }
    }
}

async fn sign(State(app): State<App>) -> Response {
    (
        StatusCode::NOT_IMPLEMENTED,
        Html(render::notice(
            &app.base_url,
            "Not wired up yet",
            "Signing goes through GitHub, and that part has not been built. Soon.",
        )),
    )
        .into_response()
}

/// The deploy gates on this before it calls a release healthy.
async fn healthz() -> &'static str {
    "ok"
}

async fn robots() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        "User-agent: *\nAllow: /\n",
    )
}

/// systemd sends SIGTERM on restart; without handling it the unit waits out
/// the stop timeout on every deploy before being killed.
async fn shutdown() {
    let ctrl_c = async {
        let _ = signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        match signal::unix::signal(signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
