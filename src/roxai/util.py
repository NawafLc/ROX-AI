"""Small shared helpers: single-instance locks, time helpers."""
from __future__ import annotations

import os
import time
from contextlib import contextmanager
from datetime import datetime, timezone
from typing import Iterator

import psutil

from .config import state_dir


class AlreadyRunning(RuntimeError):
    pass


@contextmanager
def single_instance(name: str, stale_after_s: float = 6 * 3600) -> Iterator[None]:
    """PID lock file. A crashed process never blocks future runs: dead/stale locks are taken over."""
    lock = state_dir() / f"{name}.lock"
    if lock.exists():
        try:
            pid_s, ts_s = lock.read_text().split(",")
            pid, ts = int(pid_s), float(ts_s)
            alive = psutil.pid_exists(pid) and pid != os.getpid()
            if alive and time.time() - ts < stale_after_s:
                raise AlreadyRunning(f"{name} already running (pid {pid})")
        except (ValueError, OSError):
            pass
    lock.write_text(f"{os.getpid()},{time.time()}")
    try:
        yield
    finally:
        try:
            if lock.read_text().startswith(f"{os.getpid()},"):
                lock.unlink()
        except OSError:
            pass


def is_locked(name: str) -> bool:
    lock = state_dir() / f"{name}.lock"
    if not lock.exists():
        return False
    try:
        pid = int(lock.read_text().split(",")[0])
        return psutil.pid_exists(pid)
    except (ValueError, OSError):
        return False


def iso_to_dt(s: str | None) -> datetime | None:
    if not s:
        return None
    try:
        dt = datetime.fromisoformat(s.replace("Z", "+00:00"))
        return dt if dt.tzinfo else dt.replace(tzinfo=timezone.utc)
    except ValueError:
        try:
            return datetime.strptime(s[:10], "%Y-%m-%d").replace(tzinfo=timezone.utc)
        except ValueError:
            return None


def days_ago(s: str | None) -> float | None:
    dt = iso_to_dt(s)
    if not dt:
        return None
    return (datetime.now(timezone.utc) - dt).total_seconds() / 86400
