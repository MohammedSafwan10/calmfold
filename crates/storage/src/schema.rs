use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};

use crate::StorageResult;

/// Ordered list of migrations. Append only; never edit or reorder entries.
const MIGRATIONS: &[M<'static>] = &[M::up(include_str!("../migrations/0001_init.sql"))];

pub(crate) fn migrate(conn: &mut Connection) -> StorageResult<()> {
    Migrations::from_slice(MIGRATIONS).to_latest(conn)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_valid() {
        Migrations::from_slice(MIGRATIONS)
            .validate()
            .expect("migrations validate");
    }
}
