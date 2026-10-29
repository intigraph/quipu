#!/usr/bin/env python3
"""Check that daily proposal generation cannot publish or follow every push.

# arming: ci; the existing release-contract step runs these controls.
"""

import copy
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import textwrap
import tomllib
import yaml


def check(publication, proposal):
    assert set(proposal["on"]) == {"schedule", "workflow_dispatch"}, "proposals must not chase pushes"
    assert not proposal["on"]["workflow_dispatch"], "a manual proposal takes no inputs"
    assert proposal["on"]["schedule"] == [{"cron": "0 7 * * *"}]
    assert proposal["concurrency"]["cancel-in-progress"] is False
    jobs = proposal["jobs"]
    waiter = jobs["ci-correctness"]
    assert waiter["if"] == (
        "github.event_name == 'schedule' || github.event_name == 'workflow_dispatch'"
    )
    assert waiter["outputs"]["ready"] == "${{ steps.correctness.outputs.ready }}"
    gate = next(s for s in waiter["steps"] if s.get("id") == "correctness")
    assert gate["run"] == 'python3 scripts/wait_release_correctness.py "$GITHUB_SHA"'
    assert "if" not in gate and not gate.get("continue-on-error", False)
    job = jobs["release-pr"]
    assert job["needs"] == "ci-correctness"
    assert job["if"] == (
        "(github.event_name == 'schedule' || github.event_name == 'workflow_dispatch')"
        " && needs.ci-correctness.outputs.ready == 'true'"
    )
    assert set(jobs) == {"ci-correctness", "release-pr"}
    actions = [s for s in job["steps"] if "release-plz-action@" in s.get("uses", "")]
    assert len(actions) == 1 and actions[0]["with"]["command"] == "release-pr"
    assert actions[0]["env"]["GITHUB_TOKEN"] == "${{ secrets.RELEASE_PLZ_TOKEN }}"
    assert "CARGO_REGISTRY_TOKEN" not in str(proposal), "proposal lane cannot publish"
    snapshot = next(s for s in job["steps"] if s.get("name") == "Require the measured main snapshot")
    assert 'test "$CURRENT" = "$GITHUB_SHA"' in snapshot["run"]
    assert job["steps"].index(snapshot) < job["steps"].index(actions[0])
    publisher = publication["jobs"]["release-plz"]
    publish_actions = [s for s in publisher["steps"] if "release-plz-action@" in s.get("uses", "")]
    assert len(publish_actions) == 1
    assert publish_actions[0]["with"]["command"] == "release"
    assert publish_actions[0]["id"] == "release"
    assert publish_actions[0]["env"]["GITHUB_TOKEN"] == "${{ secrets.GITHUB_TOKEN }}"
    assert "RELEASE_PLZ_TOKEN" not in str(publisher)


def binding_control(command, *, cargo_mode="sync", no_release=False):
    """Real private git remote and sync script; Cargo lock writes are simulated.

    This exercises proposal commits, not Cargo resolution or wheel builds.
    Nothing contacts a forge, changes a real release or uses a live credential.
    """
    with tempfile.TemporaryDirectory(prefix="release-binding-") as folder:
        work = Path(folder)
        repo, remote, bins = work / "repo", work / "origin.git", work / "bin"
        for directory in (repo, bins, work / "home", repo / "scripts", repo / "bindings/python"):
            directory.mkdir(parents=True, exist_ok=True)
        env = dict(os.environ, HOME=str(work / "home"), GIT_CONFIG_GLOBAL=os.devnull,
                   GIT_CONFIG_NOSYSTEM="1", PATH=str(bins) + os.pathsep + os.environ["PATH"],
                   FIXTURE_CARGO_MODE=cargo_mode, FIXTURE_NO_RELEASE=str(int(no_release)))
        for name in ("GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE"):
            env.pop(name, None)

        def git(*args, bare=False):
            prefix = ["git", "--git-dir", str(remote)] if bare else ["git", "-C", str(repo)]
            return subprocess.check_output(prefix + list(args), env=env, text=True,
                                           stderr=subprocess.PIPE).strip()

        git("init", "--bare", str(remote))
        git("init", "--initial-branch=main")
        git("config", "user.name", "Release fixture")
        git("config", "user.email", "fixture@example.test")
        git("remote", "add", "origin", str(remote))
        (repo / "Cargo.toml").write_text('[package]\nname = "quipu-ai"\nversion = "0.1.0"\n')
        (repo / "bindings/python/Cargo.toml").write_text(
            '[package]\nname = "quipu-kg"\nversion = "0.1.0"\n')
        (repo / "bindings/python/Cargo.lock").write_text(
            'version = 4\n[[package]]\nname = "quipu-ai"\nversion = "0.1.0"\n'
            '[[package]]\nname = "quipu-kg"\nversion = "0.1.0"\n')
        (repo / "CHANGELOG.md").write_text("Already correct.\n")
        shutil.copyfile("scripts/sync-python-binding.py", repo / "scripts/sync-python-binding.py")
        for name in ("fix-changelog.sh", "verify-changelog.sh"):
            path = repo / "scripts" / name
            path.write_text("#!/bin/sh\nexit 0\n")
            path.chmod(0o700)
        (bins / "gh").write_text(
            '#!/bin/sh\n[ "$1 $2" = "pr list" ] || exit 9\n'
            '[ "$FIXTURE_NO_RELEASE" = 1 ] || echo release-plz-fixture\n')
        (bins / "gh").chmod(0o700)
        (bins / "cargo").write_text(textwrap.dedent("""\
            #!/usr/bin/env python3
            import os, pathlib, sys, tomllib
            assert sys.argv[1:5] == ["update", "-p", "quipu-ai", "--manifest-path"]
            mode = os.environ["FIXTURE_CARGO_MODE"]
            if mode == "fail":
                sys.exit(9)
            if mode == "sync":
                manifest = pathlib.Path(sys.argv[5])
                version = tomllib.loads(manifest.read_text())["package"]["version"]
                manifest.with_name("Cargo.lock").write_text(
                    'version = 4\\n[[package]]\\nname = "quipu-ai"\\nversion = "' + version
                    + '"\\n[[package]]\\nname = "quipu-kg"\\nversion = "' + version + '"\\n')
            """))
        (bins / "cargo").chmod(0o700)
        git("add", ".")
        git("commit", "-qm", "fixture initial")
        git("checkout", "-qb", "release-plz-fixture")
        (repo / "Cargo.toml").write_text('[package]\nname = "quipu-ai"\nversion = "0.1.1"\n')
        git("add", "Cargo.toml")
        git("commit", "-qm", "fixture release bump")
        git("push", "-q", "origin", "main", "release-plz-fixture")
        git("checkout", "-q", "main")
        before = git("rev-parse", "refs/heads/release-plz-fixture", bare=True)
        result = subprocess.run(["bash", "-c", command], cwd=repo, env=env,
                                capture_output=True, text=True, timeout=30)
        after = git("rev-parse", "refs/heads/release-plz-fixture", bare=True)
        versions = []
        for path in ("Cargo.toml", "bindings/python/Cargo.toml"):
            versions.append(tomllib.loads(git("show", "release-plz-fixture:" + path, bare=True))
                            ["package"]["version"])
        locked = tomllib.loads(git("show", "release-plz-fixture:bindings/python/Cargo.lock", bare=True))
        versions.append(next(p["version"] for p in locked["package"] if p["name"] == "quipu-ai"))
        if result.returncode or no_release:
            assert after == before, "failed sync or absent proposal must not push"
        elif versions == ["0.1.1"] * 3:
            assert after != before, "binding-only changes must create a correction commit"
            changed = set(git("diff-tree", "--no-commit-id", "--name-only", "-r", after,
                              bare=True).splitlines())
            assert changed == {"bindings/python/Cargo.toml", "bindings/python/Cargo.lock"}
            again = subprocess.run(["bash", "-c", command], cwd=repo, env=env,
                                   capture_output=True, text=True, timeout=30)
            assert again.returncode == 0, again.stderr
            assert git("rev-parse", "refs/heads/release-plz-fixture", bare=True) == after
        return result.returncode, versions


def check_binding_correction(proposal):
    step = next(s for s in proposal["jobs"]["release-pr"]["steps"]
                if "./scripts/fix-changelog.sh" in s.get("run", ""))
    command = step["run"]
    rc, versions = binding_control(command)
    assert rc == 0 and versions == ["0.1.1"] * 3, (rc, versions)
    for mode in ("fail", "no-write"):
        rc, versions = binding_control(command, cargo_mode=mode)
        assert rc != 0 and versions == ["0.1.1", "0.1.0", "0.1.0"]
    assert binding_control(command, no_release=True)[0] == 0
    # Discrimination: the old no-sync and changelog-only-diff flows silently
    # leave the remote binding stale even though the shell exits successfully.
    without_sync = "\n".join(line for line in command.splitlines()
                             if "python3 scripts/sync-python-binding.py" not in line)
    changelog_only = command.replace(
        "git diff --quiet -- CHANGELOG.md bindings/python/Cargo.toml bindings/python/Cargo.lock",
        "git diff --quiet -- CHANGELOG.md")
    for broken in (without_sync, changelog_only):
        rc, versions = binding_control(broken)
        assert rc == 0 and versions != ["0.1.1"] * 3, (rc, versions)
    print("binding correction: binding-only commit, repeat no-op, absent PR, failed/incomplete sync,"
          " and two stale-remote controls pass")


def main():
    # SafeLoader treats the event key `on` as a YAML 1.1 boolean.
    # Preserve semantic booleans while restoring that GitHub key.
    def load(path):
        result = yaml.safe_load(Path(path).read_text())
        result["on"] = result.pop(True, result.get("on"))
        return result

    publication = load(".github/workflows/release.yml")
    proposal = load(".github/workflows/release-pr-daily.yml")
    check(publication, proposal)
    for sabotage in ("push", "pull_request", "unready", "publish", "credential", "snapshot"):
        bad = copy.deepcopy(proposal)
        job = bad["jobs"]["release-pr"]
        action = next(s for s in job["steps"] if "release-plz-action@" in s.get("uses", ""))
        if sabotage == "push":
            bad["on"]["push"] = {"branches": ["main"]}
        elif sabotage == "pull_request":
            bad["on"]["pull_request"] = {}
        elif sabotage == "unready":
            job["if"] = "github.event_name == 'schedule' || github.event_name == 'workflow_dispatch'"
        elif sabotage == "publish":
            action["with"]["command"] = "release"
        elif sabotage == "credential":
            action["env"]["GITHUB_TOKEN"] = "${{ secrets.GITHUB_TOKEN }}"
        else:
            job["steps"] = [s for s in job["steps"] if s.get("name") != "Require the measured main snapshot"]
        try:
            check(publication, bad)
        except (AssertionError, StopIteration):
            print("PASS: refused", sabotage)
        else:
            raise AssertionError("unsafe scheduling mutation survived: " + sabotage)
    print("release scheduling: current positive and six unsafe-event controls pass")
    check_binding_correction(proposal)


if __name__ == "__main__":
    main()
