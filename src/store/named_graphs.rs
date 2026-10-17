//! Default named-graph membership keeps registered inference opt-in.
use super::Store;
use crate::error::Result;
use rusqlite::params;

impl Store {
    /// Every ordinary named graph's id, excluding ROOT and the meta-graph.
    ///
    /// Registered companion inferred graphs are deliberately excluded: registry
    /// coverage must not make engine-derived facts implicit query members.
    /// Explicit GRAPH/FROM NAMED access still opts into those companions.
    pub fn all_named_graph_ids(&self) -> Result<Vec<i64>> {
        let meta_g = self.meta_graph_id()?;
        let mut stmt = self.conn.prepare(&format!(
            "SELECT g FROM graphs WHERE g <> 0 AND g <> ?1 AND {}",
            self.inferred_graph_exclusion_sql()
        ))?;
        Ok(stmt
            .query_map(params![meta_g], |r| r.get::<_, i64>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// Match the reserved suffix exactly as `is_inferred_graph_iri` does.
    /// Probe registry rows, not every term or fact, including attached stores'
    /// disjoint term spaces. Registration and implicit query membership differ.
    pub(crate) fn inferred_graph_exclusion_sql(&self) -> String {
        let scopes =
            std::iter::once("main").chain(self.attachments().iter().map(|a| a.alias.as_str()));
        let ids: Vec<_> = scopes
            .map(|scope| {
                format!(
                    "SELECT r.g FROM {scope}.graphs AS r JOIN {scope}.terms AS t ON t.id=r.g \
             WHERE t.iri GLOB '*#inferred' OR (instr(t.iri, '#') > 0 AND t.iri GLOB '*-inferred')"
                )
            })
            .collect();
        format!("g NOT IN ({})", ids.join(" UNION ALL "))
    }
}
