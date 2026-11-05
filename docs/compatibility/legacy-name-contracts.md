# Former-name contracts

Current commands and documentation use Tambo and Yupana. The following is the
per-path allowlist for retained former names. It does not authorize new uses.
Scan all tracked readable files, case-insensitively, for the former installer
name and the former structural tool name as a whole word. This ledger is itself
an explicit compatibility explanation, including the historical paths below.

| Path | Reason |
| --- | --- |
| `CHANGELOG.md` | Published history retains original change titles and commit links. |
| `benchmark/census/wild/README.md` | The measured trace retains its original filename, `hank-pre-edit.jsonl`; the example must address the real artifact. |
| `docs/book/src/getting-started/installation.md` | Persisted plan, intent and state names remain readable across installer upgrades. Renaming these would break resume and uninstall. |
| `scripts/install-stack.sh` | The default persisted plan filename is a compatibility contract; the executable invoked is Tambo. |
| `scripts/test-install-stack.py` | Asserts that persisted filename and installs a deliberately failing former-command stub to prove the current command is used. |
| `scripts/check-readme-badges.py` | Former project names and URLs are rejection patterns, not recommended destinations. |
| `scripts/test_check_readme_badges.py` | Negative control proves the retired installer URL is rejected. |
| `shapes/policies/treesitter.ttl` | Registered verifier identity remains stable. Renaming the human-facing product does not migrate signing authority or registration keys. |
| `src/governance_tests.rs` | Exercises that same registered verifier identity, including incomplete-registration rejection. |
| `src/resolution/tests.rs` | Historical commit-label fixtures test matching existing identifiers; changing them would erase the compatibility case. |
| `tests/fixtures/shapes/camayoc-core-731272ea.ttl` | Immutable upstream shape snapshot; its comments remain byte-identical to the pinned source. |
| `tests/fixtures/shapes/camayoc-core-g51c-candidate.ttl` | Pinned candidate shape snapshot; refresh through its owning fixture workflow rather than rewriting provenance. |

The release workflow's former-name comment is removed by a separate workflow
commit. Full native CI and a public default-branch scan remain required for
acceptance.
