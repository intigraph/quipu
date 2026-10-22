//! Test-only seams at the import's phase boundaries (aegis-oa5).

/// A phase boundary; a deadline check follows each, inside the rollback.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Boundary {
    /// Resolution (and its writer-side re-check) has finished.
    Resolved,
    /// Staging and the review record are written; the savepoint is open.
    Staged,
    /// The promotion is written; the savepoint is open.
    Promoted,
}
/// A boundary and the function to run when it is reached.
pub(crate) type Hook = Option<(Boundary, fn())>;
thread_local! {
    static HOOK: std::cell::Cell<Hook> = const { std::cell::Cell::new(None) };
}
/// Run `hook` when an import or promotion reaches `boundary`.
pub(crate) fn set(hook: Hook) {
    HOOK.with(|slot| slot.set(hook));
}
pub(crate) fn at(boundary: Boundary) {
    if let Some((at, hook)) = HOOK.with(std::cell::Cell::get)
        && at == boundary
    {
        hook();
    }
}
