//! SQL schema constants and migration setup for the SQLite backend.
//!
//! Uses `rusqlite_migration` to manage schema migrations via SQLite's
//! `user_version` pragma. Migrations are embedded at compile time via
//! `include_str!`.

use rusqlite::Connection;
use rusqlite_migration::{Migrations, M};

use crate::error::StorageError;

/// All schema migrations, applied in order via `user_version` tracking.
fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(include_str!("migrations/001_initial_schema.sql")),
        // Future migrations added here as new M::up(...) entries.
    ])
}

/// Opens (or creates) a SQLite database at `path` with WAL mode, foreign keys,
/// and all pending migrations applied.
pub fn open_database(path: &str) -> Result<Connection, StorageError> {
    let mut conn = Connection::open(path)?;
    configure_and_migrate(&mut conn)?;
    Ok(conn)
}

/// Opens an in-memory SQLite database with WAL mode (no-op for in-memory),
/// foreign keys, and all pending migrations applied.
pub fn open_in_memory() -> Result<Connection, StorageError> {
    let mut conn = Connection::open_in_memory()?;
    configure_and_migrate(&mut conn)?;
    Ok(conn)
}

/// Configures pragmas and applies pending migrations.
fn configure_and_migrate(conn: &mut Connection) -> Result<(), StorageError> {
    // Enable WAL mode for concurrent reads + single writer performance.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    // NORMAL synchronous is safe with WAL mode and provides better performance.
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    // Enable foreign key enforcement (off by default in SQLite).
    conn.pragma_update(None, "foreign_keys", "ON")?;

    // Apply pending migrations.
    migrations()
        .to_latest(conn)
        .map_err(|e| StorageError::Migration(e.to_string()))?;

    Ok(())
}
