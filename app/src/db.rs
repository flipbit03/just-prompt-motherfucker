//! The signature store. One table, no migration framework: the schema is
//! created if missing and that is the whole of it.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension as _, Result};

/// Named in the manifesto itself. They are not rows — there is nothing to
/// insert and nothing to delete, so a founder cannot be revoked. They hold the
/// first display positions, and the list below the manifesto starts after them.
pub const FOUNDERS: &[(i64, &str)] = &[(385_640, "leandronsp"), (5_620_032, "flipbit03")];

pub fn is_founder(github_id: i64) -> bool {
    FOUNDERS.iter().any(|(id, _)| *id == github_id)
}

/// A signature. The number shown on the page is this row's position in
/// `roll`, worked out at render time, so nothing here stores a display number.
pub struct Signatory {
    pub github_id: i64,
    pub login: String,
}

/// `github_id` is the primary key: it is the identity that matters, it never
/// changes under a rename, and using it directly removes the need for any
/// surrogate id. `signed_at` carries milliseconds because it is the sort key.
const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS signatures (
    github_id     INTEGER PRIMARY KEY,
    login         TEXT NOT NULL,
    manifesto_sha TEXT NOT NULL,
    hidden_at     TEXT,
    signed_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%f', 'now'))
);

-- With no sessions, this carries a proven identity from the OAuth callback to
-- the confirm button, for five minutes.
CREATE TABLE IF NOT EXISTS pending_unsign (
    token      TEXT    PRIMARY KEY,
    github_id  INTEGER NOT NULL,
    expires_at TEXT    NOT NULL
);
";

pub fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous  = NORMAL;
         PRAGMA busy_timeout = 5000;
         PRAGMA foreign_keys = ON;",
    )?;
    conn.execute_batch(SCHEMA)?;
    Ok(conn)
}

/// Everyone who has signed, oldest first. Hidden rows are left out entirely so
/// that moderation does not leave a hole in the numbering.
///
/// `github_id` breaks ties, so two signatures in the same millisecond still
/// order deterministically rather than swapping places between requests.
pub fn roll(conn: &Connection) -> Result<Vec<Signatory>> {
    let mut stmt = conn.prepare(
        "SELECT github_id, login
           FROM signatures
          WHERE hidden_at IS NULL
          ORDER BY signed_at, github_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(Signatory {
            github_id: row.get(0)?,
            login: row.get(1)?,
        })
    })?;
    rows.collect()
}

/// Record a signature, or refresh a known signer's handle.
///
/// Only the handle: `signed_at` is left alone, so signing again does not move
/// someone to the end of the list, and `hidden_at` is left alone, so it cannot
/// un-hide a moderated row.
pub fn sign(conn: &Connection, github_id: i64, login: &str, manifesto_sha: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO signatures (github_id, login, manifesto_sha)
              VALUES (?1, ?2, ?3)
         ON CONFLICT(github_id) DO UPDATE SET login = excluded.login",
        rusqlite::params![github_id, login, manifesto_sha],
    )?;
    Ok(())
}

/// A real delete: they asked for their data gone. `hidden_at` is for
/// moderation, where the record should survive.
///
/// Returns the handle that went, so the removal reaches the log — nothing else
/// records that a signature ever existed.
pub fn unsign(conn: &Connection, github_id: i64) -> Result<Option<String>> {
    conn.query_row(
        "DELETE FROM signatures WHERE github_id = ?1 RETURNING login",
        [github_id],
        |row| row.get(0),
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

/// `VACUUM INTO` runs in a read transaction and writes one compact file with
/// the WAL folded in, so this is safe while the site is serving. It refuses to
/// overwrite.
pub fn backup(src: &Path, dest: &Path) -> Result<()> {
    let conn = Connection::open(src)?;
    conn.execute("VACUUM INTO ?1", [dest.to_string_lossy().as_ref()])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> Connection {
        open(Path::new(":memory:")).unwrap()
    }

    fn logins(conn: &Connection) -> Vec<String> {
        roll(conn).unwrap().into_iter().map(|s| s.login).collect()
    }

    /// Signatures land in the same millisecond in a test, so the tie-break has
    /// to do the work. Force distinct times where order is what is being tested.
    fn sign_at(conn: &Connection, github_id: i64, login: &str, at: &str) {
        sign(conn, github_id, login, "sha").unwrap();
        conn.execute(
            "UPDATE signatures SET signed_at = ?2 WHERE github_id = ?1",
            rusqlite::params![github_id, at],
        )
        .unwrap();
    }

    #[test]
    fn a_new_database_holds_no_founders() {
        let conn = fresh();
        assert!(roll(&conn).unwrap().is_empty());
        assert!(is_founder(385_640));
        assert!(is_founder(5_620_032));
        assert!(!is_founder(99));
    }

    #[test]
    fn the_roll_is_oldest_first() {
        let conn = fresh();
        sign_at(&conn, 30, "third", "2026-03-01 00:00:00.000");
        sign_at(&conn, 10, "first", "2026-01-01 00:00:00.000");
        sign_at(&conn, 20, "second", "2026-02-01 00:00:00.000");
        assert_eq!(logins(&conn), ["first", "second", "third"]);
    }

    #[test]
    fn signing_again_keeps_your_place_and_refreshes_the_handle() {
        let conn = fresh();
        sign_at(&conn, 10, "first", "2026-01-01 00:00:00.000");
        sign_at(&conn, 20, "second", "2026-02-01 00:00:00.000");

        // A rename, long after the fact.
        sign(&conn, 10, "renamed", "sha").unwrap();
        assert_eq!(
            logins(&conn),
            ["renamed", "second"],
            "re-signing must not move anyone to the end"
        );
    }

    #[test]
    fn removing_closes_the_gap_and_re_signing_goes_to_the_end() {
        let conn = fresh();
        sign_at(&conn, 10, "a", "2026-01-01 00:00:00.000");
        sign_at(&conn, 20, "b", "2026-02-01 00:00:00.000");
        sign_at(&conn, 30, "c", "2026-03-01 00:00:00.000");

        assert_eq!(unsign(&conn, 20).unwrap().as_deref(), Some("b"));
        assert_eq!(unsign(&conn, 20).unwrap(), None, "already gone");
        assert_eq!(logins(&conn), ["a", "c"], "the list closes up");

        sign_at(&conn, 20, "b", "2026-04-01 00:00:00.000");
        assert_eq!(logins(&conn), ["a", "c", "b"], "b rejoins at the end");
    }

    #[test]
    fn hidden_rows_leave_no_hole() {
        let conn = fresh();
        sign_at(&conn, 10, "a", "2026-01-01 00:00:00.000");
        sign_at(&conn, 20, "b", "2026-02-01 00:00:00.000");
        sign_at(&conn, 30, "c", "2026-03-01 00:00:00.000");
        conn.execute(
            "UPDATE signatures SET hidden_at = datetime('now') WHERE github_id = 20",
            [],
        )
        .unwrap();

        assert_eq!(logins(&conn), ["a", "c"]);

        // A hidden signer re-signing must not bring themselves back.
        sign(&conn, 20, "b", "sha").unwrap();
        assert_eq!(logins(&conn), ["a", "c"]);
    }

    #[test]
    fn same_millisecond_signatures_order_deterministically() {
        let conn = fresh();
        // Forced to the same instant: without the github_id tie-break these two
        // could swap places between one request and the next.
        sign_at(&conn, 50, "higher", "2026-01-01 00:00:00.000");
        sign_at(&conn, 10, "lower", "2026-01-01 00:00:00.000");
        assert_eq!(logins(&conn), ["lower", "higher"]);
        assert_eq!(logins(&conn), ["lower", "higher"]);
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
