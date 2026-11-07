# PR wheel platform selection

A portable `src/generic.rs` edit in an isolated Git fixture selects two Linux
wheel builds and eight installed-wheel tests, compared with three builds and
twelve tests in the frozen base workflow. A macOS-specific change and a scheduled
run both retain three builds and twelve tests. The selector adds one Linux job;
this demonstrates effective matrix selection, not hosted queue-drain speed.

Base: `d52acf0315dd36141716674c218c3e6c08e13632`.
Recorded selector source: `b3317a35`; the accompanying demonstration adds no
selector behavior changes. The fixture commits are private, disposable inputs;
no hosted run, production store, account limit or installed service is changed.

The real terminal recording is [output.log](output.log) with
[timing.log](timing.log). Download both and replay with:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

Playback requires util-linux `scriptreplay`; no inline video is promised.
Both small files are retained beside this caption in Git. The output log is
also a readable transcript. To repeat the demonstration from a checkout that
contains the frozen base commit, with Python 3 and PyYAML available:

```sh
python3 docs/demos/wheel-platform-selection/demo.py
python3 scripts/test_python_wheel_platforms.py
```

The second command tests actual Git diffs, removed platform markers, unknown
history, conservative fallbacks, untrusted revision input and producer/consumer
alignment. Scheduled/manual runs retain every platform. PR changes to bindings,
packaging, known platform code or platform-sensitive diff lines also retain
macOS. This source change does not cancel any already queued run or establish
an account-wide capacity diagnosis.
