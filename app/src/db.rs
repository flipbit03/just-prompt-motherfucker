//! The signature store. One table, no migration framework: the schema is
//! created if missing and that is the whole of it.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension as _, Result};

/// A signature as the page needs it. Everything else stays in the database.
pub struct Signature {
    pub ordinal: i64,
    pub login: String,
}

/// AUTOINCREMENT, not a plain INTEGER PRIMARY KEY: without it SQLite reuses
/// the highest rowid after a delete and the next signer inherits a departed
/// one's number. Gaps are correct.
const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS signatures (
    ordinal       INTEGER PRIMARY KEY AUTOINCREMENT,
    github_id     INTEGER NOT NULL UNIQUE,
    login         TEXT    NOT NULL,
    manifesto_sha TEXT    NOT NULL,
    in_body       INTEGER NOT NULL DEFAULT 0,
    hidden_at     TEXT,
    created_at    TEXT    NOT NULL DEFAULT (datetime('now'))
);

-- With no sessions, this carries a proven identity from the OAuth callback to
-- the confirm button, for five minutes.
CREATE TABLE IF NOT EXISTS pending_unsign (
    token      TEXT    PRIMARY KEY,
    github_id  INTEGER NOT NULL,
    expires_at TEXT    NOT NULL
);
";

/// Seeded as rows 1 and 2 because they sign in the body, not the list. Keeps
/// the count honest with no offset, and leaves the first signer at #3.
const FOUNDERS: [(i64, &str); 2] = [(385_640, "leandronsp"), (5_620_032, "flipbit03")];

pub fn open(path: &Path, manifesto_sha: &str) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous  = NORMAL;
         PRAGMA busy_timeout = 5000;
         PRAGMA foreign_keys = ON;",
    )?;
    conn.execute_batch(SCHEMA)?;

    for (ordinal, (github_id, login)) in FOUNDERS.iter().enumerate() {
        conn.execute(
            "INSERT OR IGNORE INTO signatures
                 (ordinal, github_id, login, manifesto_sha, in_body)
             VALUES (?1, ?2, ?3, ?4, 1)",
            rusqlite::params![(ordinal + 1) as i64, github_id, login, manifesto_sha],
        )?;
    }

    Ok(conn)
}

/// Everyone who has signed, founders included. This is the number on the page.
pub fn count(conn: &Connection) -> Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM signatures WHERE hidden_at IS NULL",
        [],
        |row| row.get(0),
    )
}

/// Everyone who appears under "Also signed:" — which excludes the two who are
/// already named in the manifesto itself.
pub fn signatures(conn: &Connection) -> Result<Vec<Signature>> {
    let mut stmt = conn.prepare(
        "SELECT ordinal, login
           FROM signatures
          WHERE in_body = 0 AND hidden_at IS NULL
          ORDER BY ordinal",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(Signature {
            ordinal: row.get(0)?,
            login: row.get(1)?,
        })
    })?;
    rows.collect()
}

/// `VACUUM INTO` runs in a read transaction and writes one compact file with
/// the WAL folded in, so this is safe while the site is serving. It refuses to
/// overwrite.
pub fn backup(src: &Path, dest: &Path) -> Result<()> {
    let conn = Connection::open(src)?;
    conn.execute("VACUUM INTO ?1", [dest.to_string_lossy().as_ref()])?;
    Ok(())
}

/// Record a signature, or refresh a known signer's handle. Returns their
/// ordinal. The narrow DO UPDATE keeps repeat signing from issuing a new
/// number or un-hiding a moderated row.
pub fn sign(conn: &Connection, github_id: i64, login: &str, manifesto_sha: &str) -> Result<i64> {
    conn.query_row(
        "INSERT INTO signatures (github_id, login, manifesto_sha)
              VALUES (?1, ?2, ?3)
         ON CONFLICT(github_id) DO UPDATE SET login = excluded.login
           RETURNING ordinal",
        rusqlite::params![github_id, login, manifesto_sha],
        |row| row.get(0),
    )
}

/// Used by the confirmation page to name the number about to be given up.
pub fn find(conn: &Connection, github_id: i64) -> Result<Option<Signature>> {
    conn.query_row(
        "SELECT ordinal, login FROM signatures WHERE github_id = ?1",
        [github_id],
        |row| {
            Ok(Signature {
                ordinal: row.get(0)?,
                login: row.get(1)?,
            })
        },
    )
    .optional()
}

/// A real delete: they asked for their data gone. `hidden_at` is for
/// moderation, where the record should survive.
pub fn unsign(conn: &Connection, github_id: i64) -> Result<bool> {
    let removed = conn.execute("DELETE FROM signatures WHERE github_id = ?1", [github_id])?;
    Ok(removed > 0)
}

pub fn pending_unsign_create(conn: &Connection, token: &str, github_id: i64) -> Result<()> {
    // Nothing else cleans this table, so every insert sweeps it.
    conn.execute(
        "DELETE FROM pending_unsign WHERE expires_at < datetime('now')",
        [],
    )?;
    conn.execute(
        "INSERT INTO pending_unsign (token, github_id, expires_at)
         VALUES (?1, ?2, datetime('now', '+5 minutes'))",
        rusqlite::params![token, github_id],
    )?;
    Ok(())
}

/// Consume a pending confirmation. Single use: a valid row is deleted by the
/// same statement that reads it, so a token cannot be replayed. An expired one
/// matches nothing here and is left for the sweep in `pending_unsign_create`.
pub fn pending_unsign_take(conn: &Connection, token: &str) -> Result<Option<i64>> {
    conn.query_row(
        "DELETE FROM pending_unsign
               WHERE token = ?1 AND expires_at >= datetime('now')
           RETURNING github_id",
        [token],
        |row| row.get(0),
    )
    .optional()
}
