# Isolated fixture only. Cargo uses the caller's normal target policy.
[working-directory: "../../.."]
demo:
    cargo run --quiet --example write_provenance
