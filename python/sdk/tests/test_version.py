"""Smoke tests for the public SDK surface."""

import replayable


def test_version_is_set() -> None:
    assert replayable.__version__
    assert replayable.__version__.startswith("0.")


def test_agent_trace_constructible() -> None:
    t = replayable.AgentTrace(trace_id="abc-123")
    assert t.trace_id == "abc-123"
    assert t.framework == "unknown"
    assert t.metadata == {}
