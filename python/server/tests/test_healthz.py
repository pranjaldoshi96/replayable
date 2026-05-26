"""Smoke test for the /healthz endpoint."""

from fastapi.testclient import TestClient

from replayable_server.main import app


def test_healthz_ok() -> None:
    client = TestClient(app)
    response = client.get("/healthz")
    assert response.status_code == 200
    body = response.json()
    assert body["status"] == "ok"
    assert body["version"].startswith("0.")
