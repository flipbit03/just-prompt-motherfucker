//! Just Prompt, Motherfucker.

mod cookies;
mod db;
mod github;
mod render;

use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::{
    Form, Router,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use clap::{Parser, Subcommand};
use cookies::Intent;
use rusqlite::Connection;
use serde::Deserialize;
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
    client_id: Option<String>,
    client_secret: Option<String>,
}

impl Config {
    /// Defaults are the local development values, so `cargo run` just works.
    /// The deploy overrides them through the systemd EnvironmentFile.
    fn from_env() -> Self {
        let var =
            |key: &str, default: &str| std::env::var(key).unwrap_or_else(|_| default.to_string());
        Self {
            bind: var("JPMF_BIND", "127.0.0.1:8100"),
            db: PathBuf::from(var("JPMF_DB", "jpmf.db")),
            base_url: var("JPMF_BASE_URL", "http://localhost:8100"),
            client_id: std::env::var("JPMF_CLIENT_ID")
                .ok()
                .filter(|s| !s.is_empty()),
            client_secret: std::env::var("JPMF_CLIENT_SECRET")
                .ok()
                .filter(|s| !s.is_empty()),
        }
    }
}

#[derive(Clone)]
struct App {
    db: Arc<Mutex<Connection>>,
    base_url: Arc<str>,
    /// `None` without GitHub credentials: the site serves, signing 503s.
    oauth: Option<github::Oauth>,
    /// Secure cookies only over https, or the browser drops them in dev.
    secure: bool,
    /// Refreshed on a timer; negative until the first successful fetch.
    star_count: Arc<AtomicI64>,
}

impl App {
    fn stars(&self) -> Option<u64> {
        let n = self.star_count.load(Ordering::Relaxed);
        (n >= 0).then_some(n as u64)
    }
}

/// GitHub allows 60 unauthenticated calls an hour per IP. Six.
const STAR_REFRESH: Duration = Duration::from_secs(600);

/// Poll the star count in the background so no request ever waits on GitHub.
/// A private repository answers 404 here, which is a warning and nothing more:
/// the footer simply shows no number.
fn watch_stars(star_count: Arc<AtomicI64>) {
    tokio::spawn(async move {
        let http = match github::client() {
            Ok(http) => http,
            Err(err) => {
                eprintln!("no http client for the star count: {err}");
                return;
            }
        };
        loop {
            match github::stars(&http, render::REPO).await {
                Ok(n) => star_count.store(n as i64, Ordering::Relaxed),
                Err(err) => eprintln!("star count unavailable: {err}"),
            }
            tokio::time::sleep(STAR_REFRESH).await;
        }
    });
}

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let cli = Cli::parse();
    let cfg = Config::from_env();

    match cli.command.unwrap_or(Command::Serve) {
        // Only `serve` needs the async runtime.
        Command::Serve => tokio::runtime::Runtime::new()?.block_on(serve(cfg)),
        Command::Backup { dest } => {
            // SQLite would create a missing file and snapshot an empty database.
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

async fn serve(cfg: Config) -> Result<(), Box<dyn Error + Send + Sync>> {
    let conn = db::open(&cfg.db)?;

    let oauth = match (&cfg.client_id, &cfg.client_secret) {
        (Some(id), Some(secret)) => Some(github::Oauth::new(
            id.clone(),
            secret.clone(),
            &cfg.base_url,
        )?),
        _ => {
            eprintln!("warning: JPMF_CLIENT_ID / JPMF_CLIENT_SECRET not set — signing is disabled");
            None
        }
    };

    let app = App {
        db: Arc::new(Mutex::new(conn)),
        base_url: cfg.base_url.as_str().into(),
        oauth,
        secure: cfg.base_url.starts_with("https://"),
        star_count: Arc::new(AtomicI64::new(-1)),
    };
    watch_stars(app.star_count.clone());

    let router = Router::new()
        .route("/", get(index))
        .route("/sign", post(start_sign))
        .route("/unsign", post(start_unsign))
        .route("/unsign/confirm", post(confirm_unsign))
        .route("/auth/callback", get(callback))
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

// ---------------------------------------------------------------- rendering

fn oops(app: &App, status: StatusCode, heading: &str, body: &str) -> Response {
    (
        status,
        Html(render::notice(&app.base_url, app.stars(), heading, body)),
    )
        .into_response()
}

/// The manifesto, then everyone who signed it. Rendered per request so there
/// is no cache to invalidate when someone signs.
async fn index(State(app): State<App>, headers: HeaderMap) -> Response {
    // Reuse the existing cookie so a form open in another tab still submits.
    let csrf = cookies::get(&headers, cookies::CSRF).unwrap_or_else(cookies::token);
    let stars = app.stars();

    let rendered = (|| -> rusqlite::Result<String> {
        // Nothing is awaited while the lock is held.
        let conn = app
            .db
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Ok(render::page(&app.base_url, &csrf, stars, &db::roll(&conn)?))
    })();

    match rendered {
        Ok(html) => (
            [(
                header::SET_COOKIE,
                cookies::set(cookies::CSRF, &csrf, cookies::CSRF_MAX_AGE, app.secure),
            )],
            Html(html),
        )
            .into_response(),
        Err(err) => {
            eprintln!("database error while rendering the page: {err}");
            oops(
                &app,
                StatusCode::INTERNAL_SERVER_ERROR,
                "Something broke",
                "The manifesto is fine. The database is not. Try again in a moment.",
            )
        }
    }
}

// ------------------------------------------------------------ starting a flow

#[derive(Deserialize)]
struct CsrfForm {
    csrf: String,
}

/// Send someone to GitHub to prove who they are.
///
/// The CSRF check guards the way *in*. The state cookie only guards the
/// callback, and we set that cookie ourselves, so without this a cross-site
/// POST could start a flow: GitHub skips consent for anyone already
/// authorised, and they would be signed or unsigned silently.
fn begin(app: &App, headers: &HeaderMap, submitted: &str, intent: Intent) -> Response {
    let expected = cookies::get(headers, cookies::CSRF);
    if expected.as_deref() != Some(submitted) || submitted.is_empty() {
        return oops(
            app,
            StatusCode::BAD_REQUEST,
            "That request did not come from the page",
            "Go back to the manifesto and press the button there.",
        );
    }

    let Some(oauth) = app.oauth.as_ref() else {
        return oops(
            app,
            StatusCode::SERVICE_UNAVAILABLE,
            "Signing is not configured",
            "This copy of the site is running without GitHub credentials.",
        );
    };

    let state = cookies::state_value(intent);
    let url = oauth.authorize_url(&state);
    (
        StatusCode::SEE_OTHER,
        [
            (header::LOCATION, url),
            (
                header::SET_COOKIE,
                cookies::set(cookies::STATE, &state, cookies::STATE_MAX_AGE, app.secure),
            ),
        ],
    )
        .into_response()
}

async fn start_sign(
    State(app): State<App>,
    headers: HeaderMap,
    Form(form): Form<CsrfForm>,
) -> Response {
    begin(&app, &headers, &form.csrf, Intent::Sign)
}

async fn start_unsign(
    State(app): State<App>,
    headers: HeaderMap,
    Form(form): Form<CsrfForm>,
) -> Response {
    begin(&app, &headers, &form.csrf, Intent::Unsign)
}

// ---------------------------------------------------------------- the callback

#[derive(Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

async fn callback(
    State(app): State<App>,
    headers: HeaderMap,
    Query(q): Query<CallbackQuery>,
) -> Response {
    let clear = cookies::clear(cookies::STATE, app.secure);

    // Cancelling on GitHub's consent screen is not an error.
    if q.error.is_some() {
        return (
            StatusCode::SEE_OTHER,
            [
                (header::LOCATION, "/".to_string()),
                (header::SET_COOKIE, clear),
            ],
        )
            .into_response();
    }

    let (Some(code), Some(state)) = (q.code, q.state) else {
        return oops(
            &app,
            StatusCode::BAD_REQUEST,
            "That link is incomplete",
            "Start again from the manifesto.",
        );
    };

    // Compared whole: a forged query string cannot match a cookie an attacker
    // was never able to set.
    if cookies::get(&headers, cookies::STATE).as_deref() != Some(state.as_str()) {
        return oops(
            &app,
            StatusCode::BAD_REQUEST,
            "That took too long",
            "Your sign-in expired or came back to the wrong place. Try again.",
        );
    }

    let Some(intent) = cookies::state_intent(&state) else {
        return oops(
            &app,
            StatusCode::BAD_REQUEST,
            "That link is malformed",
            "Start again from the manifesto.",
        );
    };

    let Some(oauth) = app.oauth.as_ref() else {
        return oops(
            &app,
            StatusCode::SERVICE_UNAVAILABLE,
            "Signing is not configured",
            "This copy of the site is running without GitHub credentials.",
        );
    };

    // Both network calls finish before any lock is taken.
    let user = match oauth.exchange(&code).await {
        Ok(token) => match oauth.user(&token).await {
            Ok(user) => user,
            Err(err) => {
                eprintln!("github /user failed: {err}");
                return oops(
                    &app,
                    StatusCode::BAD_GATEWAY,
                    "GitHub is having a moment",
                    "It would not tell us who you are. Try again shortly.",
                );
            }
        },
        Err(err) => {
            eprintln!("token exchange failed: {err}");
            return oops(
                &app,
                StatusCode::BAD_REQUEST,
                "That took too long",
                "GitHub would not accept the sign-in. Go back and try again.",
            );
        } // The token drops here. It is never stored.
    };

    // Founders are not rows. There is nothing to insert and nothing to delete,
    // so both intents stop here for them.
    if db::is_founder(user.id) {
        println!("founder tried to {}: {}", intent.as_str(), user.login);
        let heading = match intent {
            Intent::Sign => "You are already in it!",
            Intent::Unsign => "You cannot be removed!",
        };
        return (
            [(header::SET_COOKIE, clear)],
            Html(render::notice(
                &app.base_url,
                app.stars(),
                heading,
                "You are named in the manifesto itself.",
            )),
        )
            .into_response();
    }

    match intent {
        Intent::Sign => {
            let placed = (|| -> rusqlite::Result<Option<usize>> {
                let conn = app
                    .db
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                db::sign(&conn, user.id, &user.login, render::manifesto_sha())?;
                // Their number is a position, so it has to be read back off the
                // roll rather than returned by the insert.
                Ok(db::roll(&conn)?
                    .iter()
                    .position(|s| s.github_id == user.id)
                    .map(render::rank))
            })();

            match placed {
                Ok(position) => {
                    let n = position.unwrap_or_default();
                    println!("signed: {} (#{n})", user.login);
                    (
                        StatusCode::SEE_OTHER,
                        [
                            // Anchor on their own line, so they land on it.
                            (header::LOCATION, format!("/#s{n}")),
                            (header::SET_COOKIE, clear),
                        ],
                    )
                        .into_response()
                }
                Err(err) => {
                    eprintln!("could not record signature for {}: {err}", user.login);
                    oops(
                        &app,
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "We could not write that down",
                        "GitHub vouched for you but the database would not take it.",
                    )
                }
            }
        }

        Intent::Unsign => {
            let found = (|| -> rusqlite::Result<Option<(String, usize, String)>> {
                let conn = app
                    .db
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let roll = db::roll(&conn)?;
                let Some(position) = roll.iter().position(|s| s.github_id == user.id) else {
                    return Ok(None);
                };
                let token = cookies::token();
                db::pending_unsign_create(&conn, &token, user.id, &roll[position].signed_at)?;
                Ok(Some((
                    roll[position].login.clone(),
                    render::rank(position),
                    token,
                )))
            })();

            match found {
                Ok(Some((login, n, token))) => (
                    [(header::SET_COOKIE, clear)],
                    Html(render::confirm_unsign(
                        &app.base_url,
                        app.stars(),
                        &login,
                        n as i64,
                        &token,
                    )),
                )
                    .into_response(),
                Ok(None) => oops(
                    &app,
                    StatusCode::NOT_FOUND,
                    "You have not signed it",
                    "There is nothing to remove. You are welcome to sign, though.",
                ),
                Err(err) => {
                    eprintln!("could not prepare removal for {}: {err}", user.login);
                    oops(
                        &app,
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Something broke",
                        "Try again in a moment.",
                    )
                }
            }
        }
    }
}

#[derive(Deserialize)]
struct TokenForm {
    token: String,
}

async fn confirm_unsign(State(app): State<App>, Form(form): Form<TokenForm>) -> Response {
    let outcome = {
        let conn = app
            .db
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match db::pending_unsign_take(&conn, &form.token) {
            Ok(Some((github_id, signed_at))) => db::unsign(&conn, github_id, &signed_at).map(Some),
            Ok(None) => Ok(None),
            Err(err) => Err(err),
        }
    };

    match outcome {
        Ok(Some(Some(login))) => {
            println!("removed: {login}");
            (StatusCode::SEE_OTHER, [(header::LOCATION, "/")]).into_response()
        }
        // The token was valid but pinned to a signature that is no longer
        // there. Saying so beats redirecting as though it had worked.
        Ok(Some(None)) => oops(
            &app,
            StatusCode::NOT_FOUND,
            "Nothing to remove",
            "That confirmation was for a signature that is no longer there.",
        ),
        Ok(None) => oops(
            &app,
            StatusCode::BAD_REQUEST,
            "That confirmation expired",
            "Removal links last five minutes and work once. Start again if you still want to.",
        ),
        Err(err) => {
            eprintln!("could not remove signature: {err}");
            oops(
                &app,
                StatusCode::INTERNAL_SERVER_ERROR,
                "Something broke",
                "Your signature is still there. Try again in a moment.",
            )
        }
    }
}

// -------------------------------------------------------------------- plumbing

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
