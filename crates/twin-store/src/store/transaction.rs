use crate::error::StoreError;
use crate::repo::{NodeRow, ObservationRow};
use crate::store::Store;

pub struct Transaction<'a> {
    pub(crate) store: &'a mut Store,
    committed: bool,
}

impl Store {
    pub fn transaction(&mut self) -> Result<Transaction<'_>, StoreError> {
        self.conn
            .execute("BEGIN", [])
            .map_err(|source| StoreError::TransactionBegin { source })?;
        Ok(Transaction {
            store: self,
            committed: false,
        })
    }
}

impl Transaction<'_> {
    pub fn commit(mut self) -> Result<(), StoreError> {
        self.store
            .conn
            .execute("COMMIT", [])
            .map_err(|source| StoreError::TransactionCommit { source })?;
        self.committed = true;
        Ok(())
    }

    pub fn upsert_node(&mut self, node: &NodeRow) -> Result<(), StoreError> {
        self.store.upsert_node(node)
    }

    pub fn insert_observations(&mut self, obs: &[ObservationRow]) -> Result<(), StoreError> {
        self.store.insert_observations(obs)
    }
}

impl Drop for Transaction<'_> {
    fn drop(&mut self) {
        if !self.committed {
            let _ = self.store.conn.execute("ROLLBACK", []);
        }
    }
}
