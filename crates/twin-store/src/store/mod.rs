mod health;
mod migrate;
mod open;

use rusqlite::Connection;

pub struct Store {
    pub(crate) conn: Connection,
}
