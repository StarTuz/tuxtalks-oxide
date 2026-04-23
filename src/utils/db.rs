use crate::Result;
use rusqlite::{Connection, Params};

/// A simple robust connection wrapper for local `SQLite` databases.
pub struct DbConnection {
    path: String,
}

impl DbConnection {
    #[must_use]
    pub fn new(path: &str) -> Self {
        Self {
            path: path.to_string(),
        }
    }

    /// Executes a query and returns a connection for the duration of the closure.
    /// This ensures connections are not held open longer than necessary for local DBs.
    ///
    /// # Errors
    /// Returns [`crate::PlayerError::Database`] if the `SQLite` connection cannot
    /// be opened or if the closure returns an error.
    pub fn with_conn<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> std::result::Result<T, rusqlite::Error>,
    {
        let conn = Connection::open(&self.path)
            .map_err(|e| crate::PlayerError::Database(e.to_string()))?;

        f(&conn).map_err(|e| crate::PlayerError::Database(e.to_string()))
    }

    /// Helper for simple SELECT queries that return a list of items.
    ///
    /// # Errors
    /// Returns [`crate::PlayerError::Database`] if the `SQLite` connection
    /// cannot be opened, the statement fails to prepare, or any row fails
    /// to decode.
    pub fn query_list<T, P, F>(&self, sql: &str, params: P, row_f: F) -> Result<Vec<T>>
    where
        P: Params,
        F: FnMut(&rusqlite::Row) -> std::result::Result<T, rusqlite::Error>,
    {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(sql)?;
            let rows = stmt.query_map(params, row_f)?;
            let mut results = Vec::new();
            for row in rows {
                results.append(&mut vec![row?]);
            }
            Ok(results)
        })
    }
}
