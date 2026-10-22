# Candidate renderer: absent directory to owned markdown

Run `python3 docs/probes/pr-demos/render/reproduce.py /path/to/candidate/quipu`.
The actual candidate CLI creates `report.md`, preserves file and manifest bytes,
inodes and modification times on a no-op, then refuses an unknown file while
preserving it. `transcript.json` records the observed output. The probe uses a
private temporary local store; it invokes no running installation or deployment.
