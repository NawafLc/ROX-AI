"""Logging: rotating file in the app folder. Never prints to stdout (stdout is the MCP channel)."""
from __future__ import annotations

import logging
import logging.handlers
import sys

from .config import logs_dir

_configured = False


def get(name: str = "roxai") -> logging.Logger:
    global _configured
    if not _configured:
        root = logging.getLogger("roxai")
        root.setLevel(logging.INFO)
        fh = logging.handlers.RotatingFileHandler(
            logs_dir() / "roxai.log", maxBytes=2_000_000, backupCount=5, encoding="utf-8"
        )
        fh.setFormatter(logging.Formatter("%(asctime)s %(levelname)s [%(name)s] %(message)s"))
        root.addHandler(fh)
        # stderr is safe for MCP stdio servers and shows up in Claude's MCP logs.
        sh = logging.StreamHandler(sys.stderr)
        sh.setLevel(logging.WARNING)
        sh.setFormatter(logging.Formatter("roxai %(levelname)s: %(message)s"))
        root.addHandler(sh)
        for noisy in ("httpx", "httpcore", "mcp", "mcp.server.lowlevel.server"):
            logging.getLogger(noisy).setLevel(logging.WARNING)
        _configured = True
    return logging.getLogger(name if name.startswith("roxai") else f"roxai.{name}")
