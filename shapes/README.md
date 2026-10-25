# Deployment classification

`MANIFEST` is versioned JSON: every Turtle file under this directory must have
exactly one `load` or `exclude` entry and a nonempty reason. `load` also requires
the stable registry `name`; changing a name is a schema migration, because old
registry entries persist. CI runs `verify_manifest.py` and its refusal controls.

Add the classification in the same change as the file. An excluded profile is
shipped without being activated. Choosing `load` changes the served schema and
requires the corresponding schema review; classification is not runtime
acceptance. Keep standalone policy inputs explicitly excluded.

Deployment consumers run `python3 shapes/verify_manifest.py --json` from the exact
source revision being installed. This validates complete file coverage and emits
the load list and reasoned exclusions. A missing manifest, stale entry, duplicate,
unsafe path or unclassified file refuses. Consumers must not fall back to their
own copy of the list. Private and externally owned sets remain deployment-owned;
consumers must still check exact live names and loaded bytes after installation.
