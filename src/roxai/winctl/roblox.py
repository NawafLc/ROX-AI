"""Launch / find / kill the Roblox Player on Windows."""
from __future__ import annotations

import glob
import os
import subprocess
import sys
import time
from pathlib import Path

import psutil

from .. import log
from . import window as W

L = log.get("roblox")
IS_WIN = sys.platform == "win32"

PLAYER_EXES = {"robloxplayerbeta.exe"}
HELPER_EXES = {"robloxplayerlauncher.exe", "robloxcrashhandler.exe", "robloxplayerinstaller.exe"}
STORE_EXES = {"windows10universal.exe"}  # Microsoft Store build of Roblox


def player_procs() -> list[psutil.Process]:
    out = []
    for p in psutil.process_iter(["name", "exe"]):
        try:
            n = (p.info.get("name") or "").lower()
            if n in PLAYER_EXES:
                out.append(p)
            elif n in STORE_EXES and "roblox" in (p.info.get("exe") or "").lower():
                out.append(p)
        except (psutil.NoSuchProcess, psutil.AccessDenied):
            continue
    return out


def is_running() -> bool:
    return bool(player_procs())


def installer_running() -> bool:
    for p in psutil.process_iter(["name"]):
        try:
            if (p.info.get("name") or "").lower() in {"robloxplayerinstaller.exe", "robloxplayerlauncher.exe"}:
                return True
        except (psutil.NoSuchProcess, psutil.AccessDenied):
            continue
    return False


def player_path() -> str | None:
    if not IS_WIN:
        return None
    candidates = []
    la = os.environ.get("LOCALAPPDATA", "")
    pf86 = os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)")
    pf = os.environ.get("ProgramFiles", r"C:\Program Files")
    for base in (la, pf86, pf):
        candidates += glob.glob(os.path.join(base, "Roblox", "Versions", "*", "RobloxPlayerBeta.exe"))
    if candidates:
        return max(candidates, key=os.path.getmtime)
    return None


def protocol_registered() -> bool:
    if not IS_WIN:
        return False
    import winreg
    for root, key in ((winreg.HKEY_CURRENT_USER, r"Software\Classes\roblox-player"),
                      (winreg.HKEY_CLASSES_ROOT, "roblox-player"),
                      (winreg.HKEY_CURRENT_USER, r"Software\Classes\roblox"),
                      (winreg.HKEY_CLASSES_ROOT, "roblox")):
        try:
            winreg.CloseKey(winreg.OpenKey(root, key))
            return True
        except OSError:
            continue
    return False


def is_installed() -> bool:
    return bool(player_path()) or protocol_registered()


def kill_all(timeout: float = 8) -> int:
    procs = []
    for p in psutil.process_iter(["name", "exe"]):
        try:
            n = (p.info.get("name") or "").lower()
            if n in PLAYER_EXES | HELPER_EXES or (n in STORE_EXES and "roblox" in (p.info.get("exe") or "").lower()):
                procs.append(p)
        except (psutil.NoSuchProcess, psutil.AccessDenied):
            continue
    for p in procs:
        try:
            p.terminate()
        except psutil.Error:
            pass
    gone, alive = psutil.wait_procs(procs, timeout=timeout)
    for p in alive:
        try:
            p.kill()
        except psutil.Error:
            pass
    if procs:
        L.info("closed %d Roblox process(es)", len(procs))
    return len(procs)


def launch(place_id: int) -> None:
    """Join a place using the logged-in Roblox app's own session (no cookies handled by us)."""
    if not IS_WIN:
        raise RuntimeError("Roblox can only be launched on Windows")
    urls = [f"roblox://experiences/start?placeId={place_id}", f"roblox://placeId={place_id}"]
    last: Exception | None = None
    for u in urls:
        try:
            os.startfile(u)  # type: ignore[attr-defined]
            L.info("launched %s", u)
            return
        except OSError as e:
            last = e
    try:
        subprocess.Popen(["cmd", "/c", "start", "", urls[0]], creationflags=0x08000000)
        return
    except OSError as e:
        last = e
    raise RuntimeError(f"Could not launch Roblox ({last}). Is Roblox installed?")


def find_window() -> int | None:
    if not IS_WIN:
        return None
    pids = {p.pid for p in player_procs()}
    if not pids:
        return None
    wins = W.find_windows_for_pids(pids)
    best, best_area = None, 0
    for h in wins:
        r = W.client_rect(h)
        if r and r.width * r.height > best_area and r.width > 200:
            best, best_area = h, r.width * r.height
    return best


def wait_for_window(timeout: float = 180, abort_check=None) -> int | None:
    """Wait for the game window (Roblox may first auto-update, which can take a while)."""
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        if abort_check:
            abort_check()
        h = find_window()
        if h:
            return h
        time.sleep(1.0)
    return None


def version_dir() -> Path | None:
    p = player_path()
    return Path(p).parent if p else None
