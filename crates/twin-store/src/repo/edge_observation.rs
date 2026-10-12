use std::collections::HashMap;

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

    pub fn list_observations_for_edges(
        &self,
        edge_ids: &[&str],
    ) -> Result<HashMap<String, Vec<(String, String)>>, StoreError> {
        let mut links: HashMap<String, Vec<(String, String)>> = HashMap::new();
        for chunk in edge_ids.chunks(500) {
            let placeholders = vec!["?"; chunk.len()].join(",");
            let sql = format!("SELECT edge_id, observation_id, role FROM edge_observations WHERE edge_id IN ({placeholders}) ORDER BY edge_id, observation_id, role");
            let mut statement = self
                .conn
                .prepare(&sql)
                .map_err(|source| StoreError::Query { source })?;
            let rows = statement
                .query_map(rusqlite::params_from_iter(chunk.iter()), |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })
                .map_err(|source| StoreError::Query { source })?;
            for row in rows {
                let (edge, observation, role) =
                    row.map_err(|source| StoreError::Query { source })?;
                links.entry(edge).or_default().push((observation, role));
            }
        }
        for observations in links.values_mut() {
            observations.sort();
            observations.dedup();
        }
        Ok(links)
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
