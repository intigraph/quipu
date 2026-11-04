"""Typed errors raised by the local Quipu binding.

Every error carries the facade's fields: ``code`` (stable classification),
``operation``, ``commit_state`` (``not_started``, ``committed`` or
``unknown``), ``retryable`` (always ``False``: never retry a write
automatically) and ``details`` (native validation/governance data or None).
An exception never implies the write did not commit unless ``commit_state``
is ``not_started``.
"""

from __future__ import annotations

from typing import Any


class QuipuError(Exception):
    """Base class for every binding and engine error."""

    def __init__(
        self,
        message: str,
        *,
        code: str = "engine_error",
        operation: str = "",
        commit_state: str = "unknown",
        retryable: bool = False,
        details: Any = None,
    ) -> None:
        super().__init__(message)
        self.message = message
        self.code = code
        self.operation = operation
        self.commit_state = commit_state
        self.retryable = retryable
        self.details = details

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}(code={self.code!r}, operation={self.operation!r}, "
            f"commit_state={self.commit_state!r}, message={self.message!r})"
        )


class UnsupportedCapabilityError(QuipuError):
    """The operation is not available in this binding or build."""


class SchemaNotAdoptedError(QuipuError):
    """Writes need an explicitly adopted class vocabulary (load shapes first)."""


class ValidationError(QuipuError):
    """The SHACL write gate refused the data. ``details`` holds the report."""


class PolicyDeniedError(QuipuError):
    """Governance refused the write."""


class InvalidInputError(QuipuError):
    """The input has no exact JSON form or is malformed JSON."""


class ReadOnlyReplicaError(QuipuError):
    """The store is a replica: only its source's feed writes to it."""


class ThreadAffinityError(QuipuError):
    """The handle was used from a thread other than the one that opened it."""


class BusyError(QuipuError):
    """The handle is already inside a call (no reentrant calls)."""


class StoreClosedError(QuipuError):
    """The store was closed."""


_BY_CODE = {
    "unsupported_capability": UnsupportedCapabilityError,
    "schema_not_adopted": SchemaNotAdoptedError,
    "invalid_json": InvalidInputError,
    "invalid_input": InvalidInputError,
    "read_only_replica": ReadOnlyReplicaError,
    "thread_affinity": ThreadAffinityError,
    "busy": BusyError,
    "store_closed": StoreClosedError,
}

_BY_DETAIL_KIND = {
    "validation_failed": ValidationError,
    "policy_denied": PolicyDeniedError,
}


def _from_native(error: dict) -> QuipuError:
    """Build the typed exception for a facade error dict (called from Rust)."""
    details = error.get("details")
    kind = details.get("kind") if isinstance(details, dict) else None
    cls = _BY_CODE.get(error.get("code", ""), None) or _BY_DETAIL_KIND.get(kind, QuipuError)
    return cls(
        error.get("message", ""),
        code=error.get("code", "engine_error"),
        operation=error.get("operation", ""),
        commit_state=error.get("commit_state", "unknown"),
        retryable=bool(error.get("retryable", False)),
        details=details,
    )
