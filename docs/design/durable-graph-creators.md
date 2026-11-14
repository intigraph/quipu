# Durable graph creators

Creation provenance must outlive the delivery event log. Previously the actor
was read from the latest `graph.created` event. Normal retention could delete
that event, leaving a registered graph with its original timestamp but an
unknown actor after restart.

The additive `graph_creators` registry holds the original declared actor keyed
by graph identity. It is written in the same savepoint as registration and its
creation event. Repeated create calls do not replace it. Event retention stays
unchanged: eligible delivery events can still be deleted.

On writable open, migration runs after the source-identity migration and before
server retention starts. It backfills local registered graphs using retained,
strictly validated creation records. Missing records remain unknown, including
actors already lost to retention. A later caller is never used as the creator.
Malformed retained records or an incompatible registry fail the migration
atomically. Read-only opens never migrate.

Metadata reads use the selected source's registry. Old immutable sources without
the new table fall back to their own retained creation events; missing actors
remain unknown. Neither attached nor frozen graphs borrow a mutable main-store
actor. Term-space remapping classifies and remaps the registry's graph key.

The schema is additive. Older binaries can open it but do not use the durable
actor; rolling back code therefore restores the older provenance limitation.
Keep the new table on rollback. No production schema apply, provenance backfill
from callers, or restoration of previously pruned events is part of this source
change. Migration and retention controls must pass before deployment.
