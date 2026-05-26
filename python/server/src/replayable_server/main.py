"""FastAPI application entry point.

v0.0.1 stub: /healthz only. Real endpoints land in feature branches.
"""

from fastapi import FastAPI

from replayable_server import __version__

app = FastAPI(title="Replayable API", version=__version__)


@app.get("/healthz")
def healthz() -> dict[str, str]:
    """Liveness probe. Returns the running server version."""
    return {"status": "ok", "version": __version__}
