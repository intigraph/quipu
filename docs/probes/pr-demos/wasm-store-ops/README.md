# Governed WASM writes and history demonstration

The [recorded output](output.log) compares real compiled Node WASM bundles using a synthetic native-produced pack. Both bundles first return two Widget rows. The base has no `shapes`, `knot`, or `entityHistory` methods. The candidate loads a shape, accepts a conforming write, refuses a missing required property without visible facts or an edit-log entry, and retains assertion/retraction history after replacing a value.

Base code is `a7510ca8ea23605b395f164b8bd56dd3de7c26ac` plus only the nested Cargo.lock dependency-list repair, compiled as `0f6f5995e435bf562e8e89f5f9c933c8a56453ac`. Candidate and native fixture producer were built from clean `9a641201dfd51dc5006e23aaf50635ace19774b9`. Later recording/documentation commits do not change those compiled sources. This is the first portable API slice; it does not demonstrate full Creel integration, general OPFS durability, or Python bindings.

Replay the actual recording with util-linux:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

To reproduce, build each revision from `wasm/explorer` with `cargo build --locked --release`, then generate Node glue with the matching `wasm-bindgen --target nodejs`. Build the native `quipu` fixture producer with `owl,reactive-reasoner` features and set `QUIPU_BIN` to that binary. Point `BASE_BUNDLE` and `CANDIDATE_BUNDLE` to their respective generated Node bundle directories, then run `bash run.sh`. Use separate target directories for the two builds.
