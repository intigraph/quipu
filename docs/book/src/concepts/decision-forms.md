# Decision forms

`shapes/brief-decision-forms.ttl` describes unanswered human questions. It is a
separate opt-in profile, `brief-decision-forms`; deployments must ship and load
it explicitly. A source file alone does not establish live vocabulary admission.

A `DecisionForm` declares a title, prompt, positive revision, one source-evidence
IRI and a `formKind`: `approve-reject`, `choose-one`, `ranked-choice`, `free-text`,
`date-window`, `step-confirm` or `acknowledge`. A decision seed can point to the
form through `decisionForm`. Clients should query that exact seed and refuse a
missing or ambiguous pairing.

Choice forms reference at most twelve `DecisionFormOption` entities. Each option
has a value, label, consequence, bounded order and one to four evidence IRIs.
`recommendedOption` identifies a recommendation. Text forms carry minimum and
maximum character counts; date windows carry minimum and maximum dates.

The SHACL profile governs field types, cardinalities, enumerations and individual
bounds. Readers must additionally validate the complete form: recommendation
membership, distinct option values and ranks, kind-specific required inputs,
consistent text/date ranges, and complete bounded query results. Invalid or
unsupported forms must appear as UNKNOWN; a reader must not invent a default.

Changing an entity changes its presentation on the next read. Clients must bind
a draft to the content they displayed and discard or explicitly reconcile it
when that content changes. Evidence links and source text remain untrusted data.

These entities are neither signed `Decision` outcomes nor sealed
`DecisionPresentation` records. Rendering or capturing a draft confers no
execution authority. Submission must use the deployment's signed decision and
offline settlement contract; an unsupported submission remains visibly disabled.
