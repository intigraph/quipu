// Input is the two trusted repository workflow snapshots, never a remote script.
const fs = require('fs');
const vm = require('vm');
const snapshots = JSON.parse(fs.readFileSync(0, 'utf8'));
const context = {
  github: {event_name: 'pull_request', event: {pull_request: {draft: false}}},
  needs: {'pr-paths': {outputs: {wasm: 'false'}}},
  always: () => true,
  cancelled: () => false,
  fromJSON: JSON.parse,
  contains: (values, value) => values.includes(value),
};
const moved = new Set([
  'extended-features', 'bundled-runtime', 'query-perf', 'load-test',
  'build-full', 'wasm', 'wasm-explorer', 'generated-docs',
]);
for (const [revision, jobs] of Object.entries(snapshots)) {
  console.log(`${revision}: ordinary core-only PR`);
  let count = 0;
  for (const [id, job] of Object.entries(jobs)) {
    // Actions allows hyphens in property identifiers; JavaScript requires [] for them.
    const expression = (job.if || 'true')
      .replace(/\.([a-zA-Z_][a-zA-Z0-9_-]*-[a-zA-Z0-9_-]+)/g, '["$1"]')
      .replace(/^\$\{\{\s*|\s*\}\}$/g, '');
    const admitted = vm.runInNewContext(expression, context, {timeout: 100});
    if (admitted) {
      const matrix = job.strategy?.matrix;
      count += matrix?.include?.length || matrix?.os?.length || 1;
    }
    if (moved.has(id)) console.log(`  ${id}: ${admitted ? 'RUN' : 'SKIP'}`);
  }
  console.log(`  CI job slots (matrix-expanded; reusable call counted once): ${count}`);
}
