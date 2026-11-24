"""Private receiver decisions and bounded, fixed-field admission diagnostics."""

import hashlib
import json
import os
from pathlib import Path
import stat
import sys
import tempfile

MAX_RESPONSE = 1024 * 1024
MAX_RECEIPT = 2 * MAX_RESPONSE
BLOCKERS = {"shacl_nonconforming", "off_vocabulary", "query_off_vocabulary"}


def _unique(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate import field")
        result[key] = value
    return result


def _count(value):
    return value if type(value) is int and 0 <= value <= 2**63 - 1 else None


def retain(path, receipt):
    """Atomic private inode; refuse redirects or foreign existing evidence."""
    path = Path(path)
    parent = path.parent.lstat()
    if (not stat.S_ISDIR(parent.st_mode) or parent.st_uid != os.geteuid()
            or parent.st_mode & 0o022):
        raise ValueError("receiver decision parent custody unproved")
    try:
        previous = path.lstat()
    except FileNotFoundError:
        previous = None
    if previous and (not stat.S_ISREG(previous.st_mode)
                     or previous.st_uid != os.geteuid()
                     or previous.st_mode & 0o777 != 0o600 or previous.st_nlink != 1):
        raise ValueError("receiver decision existing custody unproved")
    payload = (json.dumps(receipt, ensure_ascii=False, sort_keys=True) + "\n").encode()
    if len(payload) > MAX_RECEIPT:
        raise ValueError("receiver decision receipt exceeds bound")
    fd, temporary = tempfile.mkstemp(prefix=".receiver-import-", dir=path.parent)
    try:
        with os.fdopen(fd, "wb") as stream:
            stream.write(payload)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
        directory = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        Path(temporary).unlink(missing_ok=True)


def decision(raw, receipt, revision):
    """Preserve the old acceptance predicate; never emit arbitrary JSON fields."""
    encoded = raw.encode()
    reasons = []
    value = None
    if len(encoded) > MAX_RESPONSE:
        reasons.append("import_json_oversized")
    else:
        try:
            value = json.loads(raw, object_pairs_hook=_unique)
            if not isinstance(value, dict):
                raise ValueError("not an object")
        except (ValueError, TypeError, RecursionError):
            reasons.append("import_json_invalid")
            value = None
    promotion = value.get("promotion", {}) if value else {}
    triples = value.get("triples", {}) if value else {}
    validation = value.get("validation", {}) if value else {}
    promotion = promotion if isinstance(promotion, dict) else {}
    triples = triples if isinstance(triples, dict) else {}
    validation = validation if isinstance(validation, dict) else {}
    if value is not None:
        if value.get("outcome") != "staged":
            reasons.append("outcome_not_staged")
        if promotion.get("eligible") is not True:
            reasons.append("promotion_ineligible")
        accepted = triples.get("accepted")
        if type(accepted) is not int:
            reasons.append("accepted_not_integer")
        elif accepted <= 3:
            reasons.append("accepted_too_small")
    retained = dict(schema="quipu.receiver-import-decision/v1", step="receiver-import",
                    source_revision=revision, response_bytes=len(encoded),
                    response_sha256=hashlib.sha256(encoded).hexdigest(),
                    reasons=reasons, decision=value)
    retain(receipt, retained)
    blockers = promotion.get("blockers", [])
    blockers = blockers if isinstance(blockers, list) else []
    off_vocab = validation.get("off_vocabulary")
    outcome = value.get("outcome") if value else None
    summary = dict(step="receiver-import", reasons=reasons,
        outcome=outcome if outcome in ("staged", "quarantined", "unchanged") else "other",
        eligible=promotion.get("eligible") if type(promotion.get("eligible")) is bool else None,
        accepted=_count(triples.get("accepted")), quarantined=_count(triples.get("quarantined")),
        blockers=sorted({v for v in blockers if isinstance(v, str) and v in BLOCKERS}),
        blocking=validation.get("blocking") if type(validation.get("blocking")) is bool else None,
        conforms=validation.get("conforms") if type(validation.get("conforms")) is bool else None,
        off_vocabulary_count=len(off_vocab) if isinstance(off_vocab, list) else None)
    print("receiver import decision " + json.dumps(summary, sort_keys=True), file=sys.stderr)
    if reasons:
        raise ValueError("repository receiver admission refused: " + ",".join(reasons))
    return value


def import_receiver(run, binary, stage, receiver, receipt, revision):
    imported = decision(run([binary, "import", stage, "--db", receiver]), receipt, revision)
    run([binary, "import", "promote", imported["share_id"], "--db", receiver])
    return imported
