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

pub struct Signed {
    pub ordinal: i64,
    /// Named in the manifesto itself, so absent from the list below it.
    pub in_body: bool,
}

/// Record a signature, or refresh a known signer's handle, returning their
/// ordinal either way.
///
/// Looks before inserting rather than using ON CONFLICT: an upsert allocates
/// an AUTOINCREMENT value before it detects the conflict, so every repeat
/// click would push the next new signer's number up by one.
pub fn sign(conn: &Connection, github_id: i64, login: &str, manifesto_sha: &str) -> Result<Signed> {
    let tx = conn.unchecked_transaction()?;

    let existing = tx
        .query_row(
            "SELECT ordinal, in_body FROM signatures WHERE github_id = ?1",
            [github_id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)? != 0)),
        )
        .optional()?;

    let signed = match existing {
        Some((ordinal, in_body)) => {
            // Only the handle. Not the ordinal, and not hidden_at.
            tx.execute(
                "UPDATE signatures SET login = ?2 WHERE github_id = ?1",
                rusqlite::params![github_id, login],
            )?;
            Signed { ordinal, in_body }
        }
        None => {
            let ordinal = tx.query_row(
                "INSERT INTO signatures (github_id, login, manifesto_sha)
                      VALUES (?1, ?2, ?3)
                   RETURNING ordinal",
                rusqlite::params![github_id, login, manifesto_sha],
                |row| row.get(0),
            )?;
            Signed {
                ordinal,
                in_body: false,
            }
        }
    };

    tx.commit()?;
    Ok(signed)
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
///
/// Returns the row that went, so the removal can be logged — otherwise nothing
/// anywhere records that a signature ever existed.
pub fn unsign(conn: &Connection, github_id: i64) -> Result<Option<Signature>> {
    conn.query_row(
        "DELETE FROM signatures WHERE github_id = ?1 RETURNING ordinal, login",
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

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> Connection {
        open(Path::new(":memory:"), "testsha").unwrap()
    }

    fn seq(conn: &Connection) -> i64 {
        conn.query_row(
            "SELECT seq FROM sqlite_sequence WHERE name = 'signatures'",
            [],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn founders_are_seeded_and_hidden_from_the_list() {
        let conn = fresh();
        assert_eq!(count(&conn).unwrap(), 2);
        assert!(signatures(&conn).unwrap().is_empty());
        assert_eq!(seq(&conn), 2, "first real signer should be #3");
    }

    #[test]
    fn repeat_signing_keeps_the_number_and_burns_none() {
        let conn = fresh();
        let first = sign(&conn, 99, "someone", "sha").unwrap();
        assert_eq!(first.ordinal, 3);
        assert!(!first.in_body);

        for _ in 0..5 {
            let again = sign(&conn, 99, "renamed", "sha").unwrap();
            assert_eq!(again.ordinal, 3, "repeat signing must not reissue");
        }
        assert_eq!(seq(&conn), 3, "repeat signing must not consume ordinals");

        let next = sign(&conn, 100, "next", "sha").unwrap();
        assert_eq!(next.ordinal, 4, "the next signer gets the next number");
        assert_eq!(signatures(&conn).unwrap().len(), 2);
    }

    #[test]
    fn signing_as_a_founder_reports_in_body() {
        let conn = fresh();
        let signed = sign(&conn, 5_620_032, "flipbit03", "sha").unwrap();
        assert_eq!(signed.ordinal, 2);
        assert!(signed.in_body);
        assert!(signatures(&conn).unwrap().is_empty());
        assert_eq!(count(&conn).unwrap(), 2);
    }

    #[test]
    fn repeat_signing_cannot_unhide_a_moderated_row() {
        let conn = fresh();
        sign(&conn, 99, "someone", "sha").unwrap();
        conn.execute(
            "UPDATE signatures SET hidden_at = datetime('now') WHERE github_id = 99",
            [],
        )
        .unwrap();
        sign(&conn, 99, "someone", "sha").unwrap();
        assert!(signatures(&conn).unwrap().is_empty(), "still hidden");
    }

    #[test]
    fn removing_leaves_a_gap_that_is_never_reused() {
        let conn = fresh();
        sign(&conn, 99, "a", "sha").unwrap();
        sign(&conn, 100, "b", "sha").unwrap();
        let gone = unsign(&conn, 100).unwrap().expect("a row went");
        assert_eq!((gone.ordinal, gone.login.as_str()), (4, "b"));
        assert!(unsign(&conn, 100).unwrap().is_none(), "already gone");

        let later = sign(&conn, 101, "c", "sha").unwrap();
        assert_eq!(later.ordinal, 5, "#4 is gone for good");
    }

    #[test]
    fn pending_unsign_is_single_use() {
        let conn = fresh();
        sign(&conn, 99, "a", "sha").unwrap();
        pending_unsign_create(&conn, "tok", 99).unwrap();
        assert_eq!(pending_unsign_take(&conn, "tok").unwrap(), Some(99));
        assert_eq!(pending_unsign_take(&conn, "tok").unwrap(), None);
        assert_eq!(pending_unsign_take(&conn, "never-existed").unwrap(), None);
    }

    #[test]
    fn expired_confirmations_are_refused() {
        let conn = fresh();
        conn.execute(
            "INSERT INTO pending_unsign (token, github_id, expires_at)
             VALUES ('old', 99, datetime('now', '-1 minute'))",
            [],
        )
        .unwrap();
        assert_eq!(pending_unsign_take(&conn, "old").unwrap(), None);
    }
}
