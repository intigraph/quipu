# Name the pendant while it loads

Open Explore and watch the loading panel while the engine starts. Before this
change, the panel identifies the loading stage but not the pendant. Afterward it
also shows `repository.pendant.tar.gz`; selected URLs use their last path component
and selected files keep their exact name through failures and retry.

- [Before recording](before.webm): base `ed9797c2626b0c1ede4ff5fb38ccfb55e7c4afc6`.
- [After recording](after.webm): the loader source and harness in this change.

These are five-second browser recordings of isolated source fixtures, resized to
800 by 600, with their original timing. They are not recordings of the deployed
site. Both use the same released engine and repository pendant, a 1.6-second
engine response delay, and a streamed pendant. This is controlled slow delivery,
not the DevTools Slow 4G preset. The clips deliberately stop before import ends;
the acceptance suite separately checks real import and a positive graph query.

Stage the browser assets with `just explorer release`, then run:

```sh
OUT_DIR=/tmp/explore-name-after just explorer-loading
```

To check the regression, run the changed harness with `SITE_DIR` pointing to an
asset-staged checkout of the base revision. It captures `engine.png`, then fails
waiting for the absent `#loading-source`. The changed page must pass the same
assertion. `BASELINE=1` is for the older page without any loading panel and is not
the baseline for this follow-up.

The retained clips live beside this caption in the existing demo directory;
the full generated recordings and test output remain build artifacts. Download
the WebM files if the repository viewer does not provide playback.
