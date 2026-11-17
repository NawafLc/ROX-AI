"""Windows smoke test (run in CI on windows-latest): exercises the real Win32 code paths.

Exit code 0 = all hard checks passed. Soft checks (things a headless CI desktop may not allow)
are reported as WARN.
"""
from __future__ import annotations

import ctypes
import json
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))
os.environ.setdefault("ROXAI_HOME", tempfile.mkdtemp(prefix="roxai-ci-"))

fails: list[str] = []


def check(name, fn, hard=True):
    try:
        r = fn()
        print(f"PASS {name}: {r}")
    except Exception as e:  # noqa: BLE001
        print(f"{'FAIL' if hard else 'WARN'} {name}: {type(e).__name__}: {e}")
        if hard:
            fails.append(name)


from roxai.winctl import capture as C, input as IN, roblox as RB, window as W  # noqa: E402
from roxai.winctl.humanmon import HumanInputMonitor  # noqa: E402

W.make_dpi_aware()
check("INPUT struct size (x64=40)", lambda: ctypes.sizeof(IN.INPUT) if ctypes.sizeof(IN.INPUT) == (40 if sys.maxsize > 2**32 else 28) else (_ for _ in ()).throw(AssertionError(ctypes.sizeof(IN.INPUT))))
check("screen size", W.screen_size)
check("idle seconds", W.idle_seconds)
check("is_locked", W.is_locked)
check("foreground hwnd", W.foreground, hard=False)
check("roblox not running", lambda: RB.is_running() is False or "running")
check("roblox find_window", lambda: RB.find_window())
check("roblox installed?", RB.is_installed, hard=False)
check("hotkey poll", lambda: W.hotkey_down("ctrl+shift+q"))
check("forbidden key blocked", lambda: (_ for _ in ()).throw(AssertionError("not blocked")) if not _blocked() else "blocked")


def _blocked():
    try:
        IN._key_input("alt", False)
        return False
    except IN.InputError:
        return True


def human_monitor():
    m = HumanInputMonitor()
    m.start()
    time.sleep(1.0)
    ok = m.ok
    m.stop()
    return f"hooks installed={ok}"


check("human input monitor", human_monitor, hard=False)


def capture_screen():
    w, h = W.screen_size()
    r = W.Rect(0, 0, min(w, 1600), min(h, 900))
    shot = C.grab(r, 800, 60, grid=True)
    out = Path(os.environ.get("RUNNER_TEMP", tempfile.gettempdir())) / "roxai-capture.jpg"
    out.write_bytes(shot.jpeg)
    return f"{shot.width}x{shot.height} scale={shot.scale:.2f} bright={shot.mean_brightness:.0f} bytes={len(shot.jpeg)}"


check("screen capture", capture_screen, hard=False)


def notepad_typing():
    """Real SendInput test: type into Notepad, read the text back through WM_GETTEXT."""
    p = subprocess.Popen(["notepad.exe"])
    try:
        hwnd = None
        for _ in range(50):
            time.sleep(0.2)
            wins = W.find_windows_for_pids({p.pid})
            if wins:
                hwnd = wins[0]
                break
        if not hwnd:
            # Windows 11 notepad is a store app with another pid — search by title
            raise RuntimeError("notepad window not found")
        W.focus(hwnd)
        time.sleep(0.5)
        IN.type_unicode("rox ai 123")
        IN.tap("space")
        for k in "wasd":
            IN.tap(k)
        time.sleep(0.5)
        user32 = ctypes.windll.user32
        edit = user32.FindWindowExW(hwnd, None, "Edit", None) or user32.FindWindowExW(hwnd, None, "RichEditD2DPT", None)
        buf = ctypes.create_unicode_buffer(256)
        user32.SendMessageW(edit, 0x000D, 256, buf)  # WM_GETTEXT
        txt = buf.value
        IN.release_all()
        if "rox ai 123" not in txt:
            raise AssertionError(f"typed text not found, got {txt!r}")
        if "wasd" not in txt:
            raise AssertionError(f"scan-code keys not received, got {txt!r}")
        return repr(txt)
    finally:
        p.kill()


check("SendInput typing into Notepad", notepad_typing, hard=False)


def schtask_roundtrip():
    from roxai import setup as S
    r = S._create_task("CI Test Task", "ci", "03:33", "-m roxai.cli doctor", "PT5M", "PT1M")
    st = S.task_status("CI Test Task")
    S._run(["schtasks", "/Delete", "/TN", r"ROX-AI\CI Test Task", "/F"])
    if r != "ok":
        raise RuntimeError(r)
    return st


check("Task Scheduler XML create/query/delete", schtask_roundtrip, hard=False)


def claude_desktop_cfg():
    from roxai import setup as S
    tmp = Path(tempfile.mkdtemp())
    os.environ["APPDATA"] = str(tmp)
    (tmp / "Claude").mkdir()
    (tmp / "Claude" / "claude_desktop_config.json").write_text(json.dumps({"mcpServers": {"other": {"command": "x"}}}))
    done = S.register_claude_desktop()
    cfg = json.loads((tmp / "Claude" / "claude_desktop_config.json").read_text())
    assert "other" in cfg["mcpServers"] and "roxai" in cfg["mcpServers"], cfg
    return done


check("Claude Desktop config merge", claude_desktop_cfg)
check("icon generation", lambda: __import__("roxai.setup", fromlist=["x"]).make_icon())

print()
print("HARD FAILURES:", fails or "none")
sys.exit(1 if fails else 0)
