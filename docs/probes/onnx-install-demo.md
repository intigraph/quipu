# Default ONNX runtime demonstration

This is a transcript of retained actual CLI responses, not a terminal recording.
No recording was made during these runs; replay timing is unavailable. The
[JSON transcript](onnx-install-transcript.json) contains the original exit codes
and responses, with temporary fixture paths redacted as `<fixture>`.

The trigger is a configured, existing MiniLM model and tokenizer in an isolated
HOME/store, without `ORT_DYLIB_PATH`. The input contains two labels: a cat resting
on a sunny windowsill and a diesel train arriving at a station. The query is
`a sleeping feline`.

Before, default installation at `4391747653c30a45991e6e19e901b6fd78cf5f8a`
refused both ingestion and semantic search because ONNX was not compiled.
After, the default-feature debug binary at
`60356ba51484894b11bcc6f4f5a00a59cb328738` ingested two facts and ranked the cat
(0.48822) above the train (0.07647). `doctor search` reported an available,
initialized `bundled-dynamic-cpu` runtime with the checksum-addressed library
path, a ready embedding provider and two vector rows. The context admission
floor remained unmet; this is a ranking demonstration, not context admission.

To repeat, build the named revision with default features, create a disposable
working directory and isolated HOME/XDG paths, and configure an existing model
and tokenizer in `.bobbin/config.toml` (`[quipu.embedding]`, `model_path`,
`tokenizer_path`, `dimension = 384`). Select `[quipu.search] mode = "semantic"`
and a private `[quipu] store_path` and `base_ns`. With no runtime override, run:

```sh
quipu knot facts.ttl
quipu search 'a sleeping feline' --mode semantic --limit 2
quipu doctor search
```

Use this input in `facts.ttl`:

```turtle
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
<urn:fixture:cat> rdfs:label "A cat rests on the sunny windowsill" .
<urn:fixture:train> rdfs:label "The diesel train arrives at the station" .
```

Both probes passed their HOME guards. They used different executable build
recipes, so this does not compare performance or establish a fresh Cargo install
at the final revision. It does not prove model acquisition, automatic GPU
provisioning, a published release or any installed service change. The small
transcript is retained in this repository alongside its caption for review.
