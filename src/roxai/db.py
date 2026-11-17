"""SQLite history store: chart snapshots (for trends), games, playtests, runs."""
from __future__ import annotations

import json
import sqlite3
import threading
from contextlib import contextmanager
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterator

from .config import db_path

SCHEMA = """
CREATE TABLE IF NOT EXISTS games (
  universe_id INTEGER PRIMARY KEY,
  place_id INTEGER,
  name TEXT,
  creator TEXT,
  genre TEXT,
  subgenre TEXT,
  first_seen TEXT,
  last_seen TEXT,
  best_rank INTEGER,
  roblox_created TEXT,
  roblox_updated TEXT,
  note_path TEXT,
  last_playtest TEXT,
  playtest_count INTEGER DEFAULT 0,
  playtest_fail_count INTEGER DEFAULT 0,
  details_json TEXT
);
CREATE TABLE IF NOT EXISTS snapshots (
  ts TEXT NOT NULL,
  day TEXT NOT NULL,
  sort_id TEXT NOT NULL,
  rank INTEGER,
  universe_id INTEGER NOT NULL,
  playing INTEGER,
  visits INTEGER,
  favorites INTEGER,
  up_votes INTEGER,
  down_votes INTEGER,
  PRIMARY KEY (day, sort_id, universe_id)
);
CREATE INDEX IF NOT EXISTS snap_uni ON snapshots(universe_id, day);
CREATE TABLE IF NOT EXISTS playtests (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  universe_id INTEGER,
  started TEXT,
  finished TEXT,
  status TEXT,
  note_path TEXT,
  summary TEXT
);
CREATE TABLE IF NOT EXISTS runs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  kind TEXT,
  started TEXT,
  finished TEXT,
  status TEXT,
  detail TEXT
);
CREATE TABLE IF NOT EXISTS kv (k TEXT PRIMARY KEY, v TEXT);
"""

_lock = threading.RLock()


def now_iso() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat()


def today() -> str:
    return datetime.now().strftime("%Y-%m-%d")


class DB:
    def __init__(self, path: Path | None = None):
        self.path = path or db_path()
        self.conn = sqlite3.connect(self.path, check_same_thread=False, timeout=30)
        self.conn.row_factory = sqlite3.Row
        self.conn.execute("PRAGMA journal_mode=WAL")
        self.conn.executescript(SCHEMA)
        self.conn.commit()

    @contextmanager
    def tx(self) -> Iterator[sqlite3.Connection]:
        with _lock:
            try:
                yield self.conn
                self.conn.commit()
            except Exception:
                self.conn.rollback()
                raise

    def q(self, sql: str, args: tuple = ()) -> list[sqlite3.Row]:
        with _lock:
            return list(self.conn.execute(sql, args))

    def one(self, sql: str, args: tuple = ()) -> sqlite3.Row | None:
        rows = self.q(sql, args)
        return rows[0] if rows else None

    # ---- kv
    def get(self, k: str, default: Any = None) -> Any:
        r = self.one("SELECT v FROM kv WHERE k=?", (k,))
        return json.loads(r["v"]) if r else default

    def set(self, k: str, v: Any) -> None:
        with self.tx() as c:
            c.execute("INSERT INTO kv(k,v) VALUES(?,?) ON CONFLICT(k) DO UPDATE SET v=excluded.v", (k, json.dumps(v)))

    # ---- games
    def game(self, universe_id: int) -> sqlite3.Row | None:
        return self.one("SELECT * FROM games WHERE universe_id=?", (universe_id,))

    def upsert_game(self, universe_id: int, **cols: Any) -> None:
        existing = self.game(universe_id)
        with self.tx() as c:
            if existing is None:
                cols.setdefault("first_seen", today())
                keys = ["universe_id", *cols.keys()]
                c.execute(f"INSERT INTO games({','.join(keys)}) VALUES({','.join('?' * len(keys))})",
                          (universe_id, *cols.values()))
            elif cols:
                sets = ",".join(f"{k}=?" for k in cols)
                c.execute(f"UPDATE games SET {sets} WHERE universe_id=?", (*cols.values(), universe_id))

    def add_snapshot(self, day: str, sort_id: str, rank: int, universe_id: int, playing: int, visits: int,
                     favorites: int, up: int, down: int) -> None:
        with self.tx() as c:
            c.execute("""INSERT INTO snapshots(ts,day,sort_id,rank,universe_id,playing,visits,favorites,up_votes,down_votes)
                         VALUES(?,?,?,?,?,?,?,?,?,?)
                         ON CONFLICT(day,sort_id,universe_id) DO UPDATE SET ts=excluded.ts, rank=excluded.rank,
                         playing=excluded.playing, visits=excluded.visits, favorites=excluded.favorites,
                         up_votes=excluded.up_votes, down_votes=excluded.down_votes""",
                      (now_iso(), day, sort_id, rank, universe_id, playing, visits, favorites, up, down))

    def history(self, universe_id: int, sort_id: str, days: int = 60) -> list[sqlite3.Row]:
        return self.q("""SELECT * FROM snapshots WHERE universe_id=? AND sort_id=?
                         ORDER BY day DESC LIMIT ?""", (universe_id, sort_id, days))[::-1]

    def chart_on(self, day: str, sort_id: str) -> list[sqlite3.Row]:
        return self.q("SELECT * FROM snapshots WHERE day=? AND sort_id=? ORDER BY rank", (day, sort_id))

    def previous_day(self, day: str, sort_id: str) -> str | None:
        r = self.one("SELECT MAX(day) d FROM snapshots WHERE day<? AND sort_id=?", (day, sort_id))
        return r["d"] if r and r["d"] else None

    # ---- runs
    def run_start(self, kind: str) -> int:
        with self.tx() as c:
            cur = c.execute("INSERT INTO runs(kind,started,status) VALUES(?,?,?)", (kind, now_iso(), "running"))
            return int(cur.lastrowid)

    def run_end(self, run_id: int, status: str, detail: str = "") -> None:
        with self.tx() as c:
            c.execute("UPDATE runs SET finished=?, status=?, detail=? WHERE id=?", (now_iso(), status, detail[:4000], run_id))

    def last_run(self, kind: str, status: str | None = None) -> sqlite3.Row | None:
        if status:
            return self.one("SELECT * FROM runs WHERE kind=? AND status=? ORDER BY id DESC LIMIT 1", (kind, status))
        return self.one("SELECT * FROM runs WHERE kind=? ORDER BY id DESC LIMIT 1", (kind,))

    # ---- playtests
    def playtest_start(self, universe_id: int) -> int:
        with self.tx() as c:
            cur = c.execute("INSERT INTO playtests(universe_id,started,status) VALUES(?,?,?)",
                            (universe_id, now_iso(), "running"))
            return int(cur.lastrowid)

    def playtest_end(self, pid: int, status: str, note_path: str = "", summary: str = "") -> None:
        with self.tx() as c:
            c.execute("UPDATE playtests SET finished=?, status=?, note_path=?, summary=? WHERE id=?",
                      (now_iso(), status, note_path, summary[:2000], pid))


_db: DB | None = None


def get_db() -> DB:
    global _db
    with _lock:
        if _db is None:
            _db = DB()
        return _db
