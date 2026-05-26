"""Replayable Python SDK.

Public surface kept intentionally small at v0.0.1.
See docs/ARCHITECTURE.md and docs/adr/0001-canonical-trace-schema.md.
"""

from replayable.trace import AgentTrace

__version__ = "0.0.1"
__all__ = ["AgentTrace", "__version__"]
