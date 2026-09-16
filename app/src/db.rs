//! The signature store. One table, no migration framework: the schema is
//! created if missing and that is the whole of it.

use std::path::Path;

use rusqlite::{Connection, Result};

/// A signature as the page needs it. Everything else stays in the database.
pub struct Signature {
    pub ordinal: i64,
    pub login: String,
}

/// `ordinal` is AUTOINCREMENT rather than a plain INTEGER PRIMARY KEY on
/// purpose. Without it SQLite reuses the highest rowid after a delete, so the
/// next person to sign would silently inherit a departed signer's number. The
/// numbers are the one thing we promised never changes, so the gaps stay.
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
";

/// Leandro and Cadu sign in the body of the manifesto, not in the list below
/// it. Seeding them as rows 1 and 2 keeps the count honest without a magic
/// `+ 2` anywhere, and leaves the first real signer at #3.
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

/// Write a consistent snapshot to `dest`.
///
/// `VACUUM INTO` runs inside a read transaction and writes a single compact
/// file with the WAL already folded in, so the result is safe to copy away
/// while the site is serving. It refuses to overwrite an existing file, which
/// is the behaviour we want from a command someone runs by hand.
pub fn backup(src: &Path, dest: &Path) -> Result<()> {
    let conn = Connection::open(src)?;
    conn.execute("VACUUM INTO ?1", [dest.to_string_lossy().as_ref()])?;
    Ok(())
}
