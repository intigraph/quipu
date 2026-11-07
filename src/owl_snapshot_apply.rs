//! Live publication and receipt checks for the private OWL snapshot.
use super::*;

impl Snapshot {
    /// Compatibility helper for callers that already own an isolated live Store.
    /// Servers must use prepare/apply/reconcile so private work is off-lock.
    pub fn finish(&self, live: &mut Store) -> Result<()> {
        let prepared = self.prepare_finish()?;
        self.reconcile_finish(prepared.apply(live))
    }

    /// Compatibility helper; server callers split preparation and acknowledgement.
    pub fn apply_batch(&mut self, live: &mut Store) -> Result<usize> {
        let Some(page) = self.prepare_page()? else {
            return Ok(0);
        };
        self.reconcile_page(page.apply(live))
    }

    pub(super) fn validate(&self, live: &Store) -> Result<()> {
        let job = pages::load(&self.scratch)?;
        if matches!(job.phase.as_str(), "FAILED" | "INVALIDATED") {
            return Err(Error::InvalidValue(format!(
                "OWL job {}: {}",
                job.phase,
                job.failure.unwrap_or_default()
            )));
        }
        let result = self.validate_inner(live);
        if let Err(Error::InvalidValue(reason)) = &result
            && let Ok(mut job) = pages::load(&self.scratch)
        {
            job.phase = "INVALIDATED".into();
            job.failure = Some(reason.clone());
            let _ = pages::save(&self.scratch, &job);
        }
        result
    }

    fn validate_inner(&self, live: &Store) -> Result<()> {
        super::prepare::validate_premises(&self.identity, &self.ontologies, self.premise_head, live)
    }
}
