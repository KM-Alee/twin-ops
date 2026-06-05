use rusqlite::params;

use crate::error::StoreError;
use crate::store::Store;

impl Store {
    pub fn link_edge_observation(
        &mut self,
        edge_id: &str,
        observation_id: &str,
        role: &str,
    ) -> Result<(), StoreError> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO edge_observations (edge_id, observation_id, role)
                 VALUES (?1, ?2, ?3)",
                params![edge_id, observation_id, role],
            )
            .map_err(|source| StoreError::Insert { source })?;
        Ok(())
    }

    pub fn list_observations_for_edge(
        &self,
        edge_id: &str,
    ) -> Result<Vec<(String, String)>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT observation_id, role FROM edge_observations
                 WHERE edge_id = ?1 ORDER BY observation_id",
            )
            .map_err(|source| StoreError::Query { source })?;

        let rows = stmt
            .query_map(params![edge_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|source| StoreError::Query { source })?;

        rows.map(|row| row.map_err(|source| StoreError::Query { source }))
            .collect()
    }
}
