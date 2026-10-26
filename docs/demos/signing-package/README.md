# Shared package route demonstration

Before this extension, the publisher has only a root `quipu-ai` route. This
candidate preserves that default, refuses an implicit shared selection, and
admits an explicit **preparation-only** route. The terminal transcript records
those three actual selector outcomes. It does not invoke Cargo, a registry,
authentication, publication or a service.

Run from the repository root:

```sh
bash docs/demos/signing-package/run-demo.sh
```

See [the recorded output](recording.log) and the
[distribution contract](../../design/signing-distribution.md). Package/registry
comparison controls use synthetic fixtures; they are not distribution proof.
