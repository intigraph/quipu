// Native phases describe actual work; elapsed time is never a percentage.
const labels = {
  verify_default: "Checking the published pack…",
  restore_default: "Opening the validated graph…",
  unpack: "Opening the downloaded archive…",
  open_store: "Creating the local graph store…",
  adopt_shapes: "Adopting the bundled shapes…",
  verify_import: "Verifying and importing the graph…",
  promote: "Promoting the verified graph…",
  complete: "Preparing the graph views…",
};
export function renderLoadProgress({ phase, elapsed_ms: elapsed }) {
  if (!labels[phase] || !Number.isFinite(elapsed) || elapsed < 0) return;
  const text = `${labels[phase]} ${(elapsed / 1000).toFixed(1)}s elapsed`;
  const detail = document.querySelector("#loading-detail");
  if (detail) detail.textContent = text;
  const status = document.querySelector("#status");
  if (status) status.textContent = text;
}
