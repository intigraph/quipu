# Mechanical merging and scheduled release proposals

## Preparation state

This branch prepares workflow and documentation changes. It does not enable a
queue, alter branch protection, create credentials, or arm another merge bot.
Activate only after the current 0.16.0 release publishes and this tier-3 workflow
change has a complete-green independent review. Keep the existing deployment
health, signed-write smoke and automatic rollback safeguards.

The baseline is a seven-day sample of 112 merged PRs: green-to-merge p75 was
10.9 hours, and 106 had no GitHub review recorded. Measure the same cohort after
activation: target p75 below two hours over seven days, with zero author branch
refreshes caused solely by another PR merging. Source preparation is not that
acceptance result.

## Interim auto-merge

The preparation read found auto-merge already enabled, strict freshness off,
and seven required contexts: Format, Clippy (default), Clippy (shacl),
Test (default), Test (shacl), Build, and Release correctness. Preserve these
contexts and record fresh protection JSON before any later settings change.

After a non-author approves an exact proposal, the normal risk gate must admit
its current content before enabling native squash auto-merge for that head.
Use the expected-head option and re-read the auto-merge state. Pending checks
remain pending. Never enable an unreviewed higher-tier proposal, and never let
the ordinary bot auto-merge a workflow change. Changes anywhere under `.github/`
retain tier 3 and a human-reviewed landing through the existing workflow lane.

Native auto-merge is the merge executor, not an alternative review authority.
Retain vetoes and operator holds. A reviewer changing a verdict or a proposal
changing content invalidates the prior admission. An unchanged patch does not
need another broad source audit merely because main advanced; use the supported
content-bound admission and preserve any refusal.

With strict freshness off, do not add a branch-updater that rebases every green
PR after every merge: that recreates the measured CI and review churn. Refresh
only actual conflicts or an explicit current-base refusal. If a branch-updater
is later installed, restrict it to those cases and measure its own events.

## Record actual GitHub reviews

The current shared account cannot approve PRs authored by that same account.
An attempted approval's self-review refusal is not an approving review, and a
source-verdict comment must not be counted as one by the DORA reader.

Before enabling required GitHub approvals, provision an explicitly authorized
non-author reviewer identity with repository access. Do not switch identities
merely to evade a refusal. Each Codex reviewer records the actual source verdict
on the exact commit with GitHub's review API, and records the resulting review
ID, login, commit ID and state alongside the native content approval. Read both
back. Failed or absent review writes stay UNKNOWN or refused. Preserve the
existing independent source review while this identity prerequisite is open.

The reviewer router must route a fresh green proposal to one eligible non-author
and observe that review receipt. A delivery attempt or dormant cron is not a
successful route. Integrate with the existing router owner; do not create a
second dispatcher. No required-review setting changes occur on this branch.

## Daily release proposals

`release-pr-daily.yml` generates a candidate at 07:00 UTC once per day. The
candidate is based on the scheduled main snapshot, waits for exact-SHA CI
correctness, and refuses a snapshot that main has already superseded. If checks
are pending beyond the existing budget, no proposal is written; the next daily
run is the next automatic attempt. No hourly retry or new dispatch path is added.

The daily job retains release-window coverage, semver policy, the dedicated PR
credential, and the existing changelog correction and verification. Ordinary
pushes do not regenerate its branch. `release.yml` still publishes a reviewed
release merge and builds its artifacts using its original credential, outputs
and exact-SHA guard. The daily proposal job has no registry token and cannot
invoke the publishing command. No release PR, tag or artifact is hand-edited.

This separates candidate cadence from ordinary main traffic; it does not waive
the final release-window review or exact-base admission. The release owner must
review the final candidate and any genuine integration delta. Resolve that
admission in the queue rollout rather than asserting that a stale release window
has been reviewed. Publication remains a separate observed result.

## Organization merge queue

After the repository transfer, re-read owner identity, plan support and queue
availability. The existing CI merge-group trigger must report all required
contexts on the proposed merge result. Save protection settings, stage a queue
with bounded concurrency, and retain the workflow-change exception. Do not
activate it before an observed merge-group run can report the required checks.

Exercise one reviewed low-risk PR through queue admission, merged-result CI,
actual merge, and the normal deployment smoke. Then admit the remaining eligible
PRs. Keep one observed main-tip watch and attribute a real failure before
reverting. Do not count cancelled or missing runs as green.

## Rollback and measurements

Disable only the newly enabled auto-merge admissions if their head/review binding
fails. Restore saved queue/protection settings through the same reviewed path.
Revert this workflow branch to return release-PR generation to pushes. Preserve
published tags and artifacts; runtime rollback uses the existing deployment lane.

Track daily green-to-merge delay, actual GitHub review coverage, branch-refresh
cause, queue failures and deployment smoke/rollback outcomes. The seven-day
acceptance window begins after activation, not at this branch's preparation.

## References

- [GitHub approving-review restrictions](https://docs.github.com/en/pull-requests/how-tos/review-pull-requests/approving-a-pull-request-with-required-reviews)
- [Release-plz command separation](https://release-plz.dev/docs/github/quickstart)
- [Release PR update behavior](https://release-plz.dev/docs/usage/release-pr)
