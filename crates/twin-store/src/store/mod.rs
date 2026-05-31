mod batch;
mod health;
mod migrate;
mod open;
mod transaction;

use rusqlite::Connection;

pub struct Store {
    pub(crate) conn: Connection,
}
