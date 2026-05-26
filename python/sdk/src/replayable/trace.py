"""Canonical AgentTrace data model.

v0.0.1 stub. Full schema defined in docs/adr/0001-canonical-trace-schema.md;
fields will expand to match it in subsequent feature branches.
"""

from dataclasses import dataclass, field
from typing import Any


@dataclass
class AgentTrace:
    """A single agent trace."""

    trace_id: str
    framework: str = "unknown"
    metadata: dict[str, Any] = field(default_factory=dict)
