# Matched GPU runtime bootstrap

The optional Linux x86_64 GPU runtime uses ONNX Runtime 1.23.2, CUDA 12.8
and cuDNN 9.8, adapted from Bobbin's Apache-2.0 `gpu_runtime` contract.
`src/onnx_runtime/gpu-artifacts.json` retains the seven exact HTTPS wheel URLs
and archive checksums from Bobbin commit `7e5d91b8`.
`gpu-files.json` additionally pins the matched installed library bytes,
including the core, CUDA provider, shared provider and CUDA/cuDNN components.
A local receipt cannot authorize different bytes or a different version.

The packaging interface has three explicit operations:

- `gpu_candidate()` only discovers and verifies an existing bundle. It does
  not fetch, stage, load a library, initialize ORT or claim a CUDA session.
- `acquire_gpu_candidate()` reuses a verified bundle, or acquires the seven
  pinned archives into a locked private staging directory. Call it only after
  device detection and hold/opt-out policy permit GPU. CPU, remote and API
  paths must not call it. Download failure returns an error for the device
  selector to report as a CPU fallback reason.
- `provision_gpu_candidate(source, cache)` stages an existing matched bundle
  offline, rechecking every copied file and retaining redistribution notices.

The device selector passes its selected core to `initialize(selected)` before
creating any ORT session. An explicit `ORT_DYLIB_PATH`, including an invalid
one, takes precedence and bypasses automatic acquisition and loader preparation.
A missing cache yields no candidate; a malformed or corrupted cache is refused
and retained. It is never silently repaired or replaced.

The default private cache is under `XDG_CACHE_HOME/quipu/gpu-runtime`, or
`HOME/.cache/quipu/gpu-runtime`. Existing Bobbin data uses its documented
`BOBBIN_GPU_CACHE` or XDG data path. `QUIPU_GPU_CACHE` selects an explicit source
cache root, with the version directory below it.

The selected GPU bundle is privately staged before loading. The Linux loader
preloads checksum-verified CUDA and cuDNN dependencies by absolute path and
retains their handles for the process lifetime. This avoids assuming that
changing `LD_LIBRARY_PATH` after process startup changes loader search behavior.
The TensorRT provider is retained but never preloaded: TensorRT is not part of
the automatic CUDA contract. ORT initialization still occurs only once, after
selection. Successful library loading alone is not evidence of a CUDA session.

Source controls cover missing/changed libraries, forged receipts, symlinks,
archive checksums, path traversal, duplicate libraries, atomic publication,
cache reuse and offline failure. Native qualification must additionally prove
fresh-cache acquisition, dependency loading and strict CUDA session registration,
CPU fallback with an explicit reason, and unchanged explicit-override behavior.
This source checkpoint does not claim those native controls have run.
