"""One AI playtest session, wrapped in fail-safes.

Fail-safes (all independent, any one of them stops the session cleanly):
  * kill hotkey (default Ctrl+Shift+Q) and the panel's STOP button (state/STOP file)
  * touching your physical mouse/keyboard (human input monitor)
  * hard time budget + grace period, then Roblox is force-closed
  * Roblox crash / window gone / window hung > 30 s
  * workstation locked
  * every held key/button is released on any exit path
  * progress (timeline, screenshots) is written to the vault as it happens, so nothing is lost
  * a session left 'running' by a crash is marked 'interrupted' on next start
"""
from __future__ import annotations

import io
import json
import os
import threading
import time
import traceback
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from . import log
from . import vault as V
from .config import settings, state_dir
from .db import get_db, now_iso

L = log.get("session")



class _Paths:
    """Resolved lazily so ROXAI_HOME changes (tests, multiple installs) are honoured."""

    @property
    def stop(self) -> Path:
        return state_dir() / "STOP"

    @property
    def session(self) -> Path:
        return state_dir() / "session.json"


P = _Paths()


class SessionAborted(RuntimeError):
    pass


class NoSession(RuntimeError):
    pass


def request_stop(reason: str = "stop button") -> None:
    P.stop.write_text(reason, encoding="utf-8")


def clear_stop() -> None:
    try:
        P.stop.unlink()
    except FileNotFoundError:
        pass


def stop_requested() -> str | None:
    try:
        return P.stop.read_text(encoding="utf-8") or "stop requested"
    except FileNotFoundError:
        return None


@dataclass
class Session:
    universe_id: int
    place_id: int
    name: str
    budget_s: float
    playtest_id: int
    note: Path
    game_note: Path | None
    started: float = field(default_factory=time.monotonic)
    started_iso: str = field(default_factory=now_iso)
    hwnd: int | None = None
    abort_reason: str | None = None
    ended: bool = False
    actions: int = 0
    shots_saved: int = 0
    last_shot: Any = None
    problems: list[str] = field(default_factory=list)
    hung_since: float | None = None
    human: Any = None

    @property
    def elapsed(self) -> float:
        return time.monotonic() - self.started

    @property
    def remaining(self) -> float:
        return self.budget_s - self.elapsed

    def clock(self) -> str:
        e = int(self.elapsed)
        return f"{e // 60:02d}:{e % 60:02d}"

    def status_line(self) -> str:
        r = self.remaining
        if r <= 0:
            return f"⛔ time budget used ({self.clock()}). Only look/snap/note/playtest_finish allowed now — finish now."
        warn = " ⚠️ under 2 minutes left: wrap up, take final screenshots and call playtest_finish." if r < 120 else ""
        return f"⏱️ {self.clock()} elapsed, {int(r // 60)}m{int(r % 60):02d}s left · {self.actions} actions.{warn}"

    def persist(self) -> None:
        try:
            P.session.write_text(json.dumps({
                "universe_id": self.universe_id, "name": self.name, "playtest_id": self.playtest_id,
                "note": str(self.note), "started": self.started_iso, "budget_s": self.budget_s,
                "elapsed": int(self.elapsed), "pid": os.getpid(), "ended": self.ended, "abort": self.abort_reason,
            }), encoding="utf-8")
        except OSError:
            pass


class Manager:
    """Process-wide singleton owning the current session and its watchdog."""

    def __init__(self) -> None:
        self.s: Session | None = None
        self.lock = threading.RLock()
        self._wd: threading.Thread | None = None
        self._wd_stop = threading.Event()

    # ------------------------------------------------------------- lifecycle
    def recover_stale(self) -> None:
        """A previous process died mid-session: mark it interrupted so the queue retries it."""
        if not P.session.exists():
            return
        try:
            data = json.loads(P.session.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            P.session.unlink(missing_ok=True)
            return
        import psutil
        if data.get("ended") or (data.get("pid") and data["pid"] != os.getpid() and psutil.pid_exists(data["pid"])):
            if data.get("ended"):
                P.session.unlink(missing_ok=True)
            return
        db = get_db()
        pid = data.get("playtest_id")
        if pid:
            db.playtest_end(pid, "interrupted", V.rel(Path(data["note"])) if data.get("note") else "",
                            "process stopped unexpectedly")
            g = db.game(data["universe_id"])
            if g is not None:
                db.upsert_game(data["universe_id"], playtest_fail_count=(g["playtest_fail_count"] or 0) + 1)
        try:
            n = Path(data["note"])
            if n.exists():
                V.set_fm_value(n, "status", "interrupted")
        except Exception:  # noqa: BLE001
            pass
        P.session.unlink(missing_ok=True)
        L.warning("recovered stale session for %s", data.get("name"))

    def start(self, universe_id: int, minutes: int | None = None) -> Session:
        from .winctl import roblox as RB, window as W
        from .winctl.humanmon import HumanInputMonitor

        with self.lock:
            if self.s and not self.s.ended:
                raise RuntimeError(f"A playtest of '{self.s.name}' is already running — call playtest_finish first.")
            self.recover_stale()
            clear_stop()
            st = settings()
            W.make_dpi_aware()
            if W.is_locked():
                raise SessionAborted("The PC is locked — playtests need an unlocked, logged-in desktop.")
            if not RB.is_installed():
                raise SessionAborted("Roblox Player is not installed on this PC. Install it from roblox.com/download and log in with the alt account.")
            db = get_db()
            g = db.game(universe_id)
            if g is None or not g["place_id"]:
                from .scan import refresh_one
                refresh_one(universe_id)
                g = db.game(universe_id)
            if g is None or not g["place_id"]:
                raise ValueError(f"Unknown game {universe_id}")
            minutes = int(minutes or st.playtest_minutes)
            minutes = max(3, min(minutes, 45))
            game_note = (V.vault_root() / g["note_path"]) if g["note_path"] else None
            note = V.playtest_note_path(universe_id, g["name"])
            V.create_playtest_note(note, game_note, g["name"], universe_id, minutes)
            pid = db.playtest_start(universe_id)
            s = Session(universe_id, int(g["place_id"]), g["name"], minutes * 60.0, pid, note, game_note)
            self.s = s
            s.persist()

            try:
                RB.kill_all()
                time.sleep(1.5)
                RB.launch(s.place_id)
                hwnd = RB.wait_for_window(timeout=180, abort_check=self._check_stop_only)
            except Exception as e:
                self._fail(f"launch failed: {e}")
                raise
            if not hwnd:
                self._fail("Roblox window never appeared (not installed, update stuck, or login screen in browser).")
                raise SessionAborted("Roblox did not open within 3 minutes. Playtest cancelled.")
            s.hwnd = hwnd
            try:
                if not W.is_fullscreen(hwnd):
                    W.resize_client(hwnd, st.window_width, st.window_height)
            except Exception:  # noqa: BLE001
                pass
            W.focus(hwnd)
            s.human = HumanInputMonitor()
            s.human.start()
            self._start_watchdog()
            # restart the clock after loading so the budget is real play time
            s.started = time.monotonic()
            self.timeline(f"Joined game (window ready). Budget {minutes} min.")
            return s

    def _check_stop_only(self) -> None:
        r = stop_requested()
        if r:
            raise SessionAborted(f"Stopped: {r}")

    def _fail(self, reason: str) -> None:
        s = self.s
        if not s:
            return
        s.problems.append(reason)
        try:
            V.append_to_block(s.note, "auto", "problems", f"- {reason}")
        except Exception:  # noqa: BLE001
            pass
        self.end("failed", reason)

    def end(self, status: str, summary: str = "") -> Session | None:
        """Idempotent shutdown: release input, close Roblox, update DB and notes."""
        from .winctl import input as IN, roblox as RB

        with self.lock:
            s = self.s
            if not s or s.ended:
                return s
            s.ended = True
            self._wd_stop.set()
            try:
                IN.release_all()
            except Exception:  # noqa: BLE001
                pass
            if s.human:
                s.human.stop()
            try:
                RB.kill_all()
            except Exception:  # noqa: BLE001
                L.error("kill roblox failed: %s", traceback.format_exc())
            db = get_db()
            db.playtest_end(s.playtest_id, status, V.rel(s.note), summary)
            g = db.game(s.universe_id)
            if status in ("completed",):
                db.upsert_game(s.universe_id, last_playtest=now_iso(), playtest_count=(g["playtest_count"] or 0) + 1 if g else 1,
                               playtest_fail_count=0)
            elif g is not None:
                db.upsert_game(s.universe_id, playtest_fail_count=(g["playtest_fail_count"] or 0) + 1)
            try:
                V.set_fm_value(s.note, "status", status)
                V.set_fm_value(s.note, "played_minutes", round(s.elapsed / 60, 1))
                if s.abort_reason:
                    V.append_to_block(s.note, "auto", "problems", f"- Session ended: {s.abort_reason}")
                if s.game_note and s.game_note.exists():
                    pts = [dict(r) for r in db.q("SELECT * FROM playtests WHERE universe_id=? ORDER BY id DESC LIMIT 10",
                                                 (s.universe_id,))]
                    txt = s.game_note.read_text(encoding="utf-8")
                    V.write(s.game_note, V.set_block(txt, "auto", "playtests", V.playtest_history_md(pts)))
            except Exception:  # noqa: BLE001
                L.error("note update failed: %s", traceback.format_exc())
            s.persist()
            P.session.unlink(missing_ok=True)
            L.info("session %s ended: %s", s.name, status)
            return s

    # -------------------------------------------------------------- watchdog
    def _start_watchdog(self) -> None:
        self._wd_stop.clear()
        self._wd = threading.Thread(target=self._watchdog, name="roxai-watchdog", daemon=True)
        self._wd.start()

    def _watchdog(self) -> None:
        from .winctl import input as IN, roblox as RB, window as W

        st = settings()
        last_persist = 0.0
        grace = 5 * 60
        while not self._wd_stop.wait(0.2):
            s = self.s
            if not s or s.ended:
                return
            reason = None
            if W.hotkey_down(st.kill_hotkey):
                reason = f"kill hotkey {st.kill_hotkey} pressed"
            elif stop_requested():
                reason = f"stop requested ({stop_requested()})"
            elif s.human and s.human.human_since(s.started + 2):
                reason = "you used your mouse/keyboard — AI stepped back"
            elif s.remaining < -grace:
                reason = "time budget + grace period exceeded (Claude never finished)"
            elif not RB.is_running():
                reason = "Roblox closed or crashed"
            elif W.is_locked():
                reason = "PC got locked"
            else:
                h = RB.find_window()
                if h and W.is_hung(h):
                    s.hung_since = s.hung_since or time.monotonic()
                    if time.monotonic() - s.hung_since > 30:
                        reason = "Roblox window frozen for 30 s"
                else:
                    s.hung_since = None
            if reason:
                s.abort_reason = reason
                L.warning("watchdog abort: %s", reason)
                try:
                    IN.release_all()
                except Exception:  # noqa: BLE001
                    pass
                status = "timeout" if "budget" in reason else ("stopped" if "hotkey" in reason or "stop" in reason
                                                                or "mouse" in reason else "crashed")
                self.end(status, reason)
                return
            if time.monotonic() - last_persist > 5:
                s.persist()
                last_persist = time.monotonic()

    # -------------------------------------------------------------- actions
    def require(self, allow_after_budget: bool = False) -> Session:
        s = self.s
        if not s:
            raise NoSession("No playtest running. Call playtest_start(universe_id) first.")
        if s.ended or s.abort_reason:
            raise SessionAborted(f"Session ended: {s.abort_reason or 'finished'}. Call playtest_finish to save your report "
                                 "(it still works after an abort).")
        if not allow_after_budget and s.remaining <= 0:
            raise SessionAborted("Time budget used up. Take a final look/snap if needed and call playtest_finish now.")
        return s

    def ensure_focus(self, s: Session) -> None:
        from .winctl import roblox as RB, window as W

        h = RB.find_window() or s.hwnd
        if not h:
            raise SessionAborted("Roblox window not found.")
        s.hwnd = h
        if W.foreground() != h and not W.focus(h):
            raise SessionAborted("Could not focus the Roblox window (another window is on top).")

    def look(self, grid: bool = False):
        from .winctl import capture as C, window as W

        s = self.require(allow_after_budget=True)
        if s.ended:
            raise SessionAborted("Session ended.")
        h = s.hwnd
        if not h:
            raise SessionAborted("No Roblox window.")
        if W.is_minimized(h):
            W.focus(h)
        r = W.client_rect(h)
        if not r or r.width < 100:
            raise SessionAborted("Roblox window has no visible area.")
        st = settings()
        shot = C.grab(r, st.screenshot_width, st.jpeg_quality, grid=grid)
        s.last_shot = shot
        return shot

    def to_screen(self, s: Session, x: float, y: float) -> tuple[int, int]:
        shot = s.last_shot
        if shot is None:
            raise SessionAborted("Call look() first so coordinates refer to a screenshot.")
        if not (0 <= x <= shot.width and 0 <= y <= shot.height):
            raise ValueError(f"({x},{y}) is outside the screenshot ({shot.width}x{shot.height}).")
        return int(shot.rect.left + x * shot.scale), int(shot.rect.top + y * shot.scale)

    def check(self) -> None:
        s = self.s
        if not s or s.ended or s.abort_reason:
            raise SessionAborted(f"Session ended: {(s and s.abort_reason) or 'no session'}")

    def act(self, actions: list[dict]) -> list[str]:
        from .winctl import input as IN

        s = self.require()
        st = settings()
        done: list[str] = []
        with self.lock:
            self.ensure_focus(s)
            try:
                for i, a in enumerate(actions[:25]):
                    self.check()
                    t = (a.get("type") or a.get("do") or "").lower()
                    if t == "key":
                        key = str(a["key"])
                        if key.strip() == "/" and not st.allow_chat:
                            raise ValueError("Opening public chat is disabled (allow_chat=false).")
                        for _ in range(max(1, min(int(a.get("times", 1)), 20))):
                            IN.tap(key, float(a.get("hold", 0.06)))
                            time.sleep(0.08)
                        done.append(f"key {key}")
                    elif t == "keys":  # hold a combination
                        keys = [str(k) for k in a["keys"]]
                        IN.hold_keys(keys, float(a.get("seconds", 0.3)), self.check)
                        done.append(f"hold {'+'.join(keys)}")
                    elif t == "walk":
                        dirs = str(a.get("dir", a.get("direction", "w"))).lower()
                        keys = [k for k in dirs if k in "wasd"] or ["w"]
                        if a.get("sprint"):
                            keys.append("shift")
                        secs = float(a.get("seconds", 1.0))
                        if a.get("jump"):
                            keys.append("space")
                        IN.hold_keys(keys, secs, self.check)
                        done.append(f"walk {dirs} {secs}s")
                    elif t == "jump":
                        for _ in range(max(1, min(int(a.get("times", 1)), 10))):
                            IN.tap("space", 0.1)
                            time.sleep(0.35)
                        done.append("jump")
                    elif t == "click":
                        x, y = self.to_screen(s, float(a["x"]), float(a["y"]))
                        IN.click_at(x, y, a.get("button", "left"), bool(a.get("double")), float(a.get("hold", 0.05)))
                        done.append(f"click {a['x']},{a['y']}")
                    elif t in ("hold_click", "press_hold"):
                        x, y = self.to_screen(s, float(a["x"]), float(a["y"]))
                        IN.click_at(x, y, a.get("button", "left"), False, min(float(a.get("seconds", 1.0)), 15))
                        done.append(f"hold-click {a['x']},{a['y']}")
                    elif t == "move_mouse":
                        x, y = self.to_screen(s, float(a["x"]), float(a["y"]))
                        IN.move_abs(x, y)
                        done.append("move mouse")
                    elif t == "camera":
                        IN.drag_camera(int(a.get("dx", 0)), int(a.get("dy", 0)))
                        done.append(f"camera {a.get('dx', 0)},{a.get('dy', 0)}")
                    elif t == "scroll":
                        IN.wheel(int(a.get("clicks", -3)))
                        done.append(f"scroll {a.get('clicks', -3)}")
                    elif t == "type":
                        text = str(a.get("text", ""))[:200]
                        IN.type_unicode(text)
                        if a.get("enter"):
                            IN.tap("enter")
                        done.append(f"type {len(text)} chars")
                    elif t == "wait":
                        end = time.monotonic() + min(float(a.get("seconds", 1.0)), 30)
                        while time.monotonic() < end:
                            self.check()
                            time.sleep(0.1)
                        done.append(f"wait {a.get('seconds', 1)}s")
                    else:
                        raise ValueError(f"action #{i + 1}: unknown type '{t}'")
                    s.actions += 1
                    time.sleep(float(a.get("pause", 0.12)))
            finally:
                IN.release_all()
        return done

    # --------------------------------------------------------------- notes
    def timeline(self, text: str) -> None:
        s = self.s
        if not s:
            raise NoSession("No playtest running.")
        V.append_to_block(s.note, "auto", "timeline", f"- `{s.clock()}` {text.strip()}")

    def snap(self, caption: str) -> str:
        s = self.s
        if not s:
            raise NoSession("No playtest running.")
        shot = s.last_shot
        if shot is None or not s.ended:
            try:
                shot = self.look()
            except SessionAborted:
                shot = s.last_shot
        if shot is None:
            raise SessionAborted("No screenshot available.")
        d = V.p("attach") / str(s.universe_id)
        d.mkdir(parents=True, exist_ok=True)
        s.shots_saved += 1
        fname = f"{s.note.stem.replace(' ', '_')}_{s.shots_saved:02d}.jpg"
        img = shot.png_full
        if img.width > 1600:
            img = img.resize((1600, int(img.height * 1600 / img.width)))
        buf = io.BytesIO()
        img.save(buf, "JPEG", quality=82)
        (d / fname).write_bytes(buf.getvalue())
        relp = V.rel(d / fname)
        V.append_to_block(s.note, "auto", "shots", f"![[{relp}|640]]\n*{s.clock()} — {caption.strip()}*\n")
        return relp


manager = Manager()
