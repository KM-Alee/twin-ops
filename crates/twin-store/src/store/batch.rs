use rusqlite::Error as RusqliteError;

use crate::error::StoreError;
use crate::store::Store;

fn is_nested_transaction_begin(err: &RusqliteError) -> bool {
    matches!(
        err,
        RusqliteError::SqliteFailure(_, Some(msg))
            if msg.contains("cannot start a transaction within a transaction")
    )
}

enum TxPlan<T> {
    Committed(T),
    Nested,
}

impl Store {
    pub fn with_transaction<F, T>(&mut self, mut f: F) -> Result<T, StoreError>
    where
        F: FnMut(&mut Self) -> Result<T, StoreError>,
    {
        let plan = match self.transaction() {
            Ok(guard) => {
                let out = f(guard.store);
                let value = out?;
                guard.commit()?;
                TxPlan::Committed(value)
            }
            Err(StoreError::TransactionBegin { source })
                if is_nested_transaction_begin(&source) =>
            {
                TxPlan::Nested
            }
            Err(err) => return Err(err),
        };

        match plan {
            TxPlan::Committed(value) => Ok(value),
            TxPlan::Nested => f(self),
        }
    }
}
