"""Detects *real* (physical) keyboard/mouse input while the AI is playing.

Uses low-level Windows hooks: events we inject carry the INJECTED flag, so anything
without it came from a human. The moment you touch your mouse or keyboard during a
playtest, ROX-AI lets go of all controls and stops — you are always in charge.
"""
from __future__ import annotations

import ctypes
import sys
import threading
import time

IS_WIN = sys.platform == "win32"


class HumanInputMonitor:
    def __init__(self, mouse_threshold_px: int = 60):
        self.mouse_threshold = mouse_threshold_px
        self.last_human = 0.0
        self.events = 0
        self._moved = 0
        self._last_pt: tuple[int, int] | None = None
        self._thread: threading.Thread | None = None
        self._tid = 0
        self.ok = False

    def start(self) -> None:
        if not IS_WIN or self._thread:
            return
        self._thread = threading.Thread(target=self._run, name="roxai-humanmon", daemon=True)
        self._thread.start()

    def stop(self) -> None:
        if IS_WIN and self._tid:
            WM_QUIT = 0x0012
            ctypes.windll.user32.PostThreadMessageW(self._tid, WM_QUIT, 0, 0)
        self._thread = None

    def human_since(self, t: float) -> bool:
        return self.last_human > t

    def _run(self) -> None:  # pragma: no cover - Windows only
        from ctypes import wintypes

        user32 = ctypes.WinDLL("user32", use_last_error=True)
        kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        self._tid = kernel32.GetCurrentThreadId()

        WH_KEYBOARD_LL, WH_MOUSE_LL = 13, 14
        LLKHF_INJECTED, LLMHF_INJECTED = 0x10, 0x01
        WM_MOUSEMOVE = 0x0200

        class KBDLLHOOKSTRUCT(ctypes.Structure):
            _fields_ = [("vkCode", wintypes.DWORD), ("scanCode", wintypes.DWORD), ("flags", wintypes.DWORD),
                        ("time", wintypes.DWORD), ("dwExtraInfo", ctypes.c_size_t)]

        class MSLLHOOKSTRUCT(ctypes.Structure):
            _fields_ = [("pt", wintypes.POINT), ("mouseData", wintypes.DWORD), ("flags", wintypes.DWORD),
                        ("time", wintypes.DWORD), ("dwExtraInfo", ctypes.c_size_t)]

        LRESULT = ctypes.c_ssize_t
        HOOKPROC = ctypes.WINFUNCTYPE(LRESULT, ctypes.c_int, wintypes.WPARAM, wintypes.LPARAM)
        user32.CallNextHookEx.argtypes = [wintypes.HHOOK, ctypes.c_int, wintypes.WPARAM, wintypes.LPARAM]
        user32.CallNextHookEx.restype = LRESULT
        user32.SetWindowsHookExW.argtypes = [ctypes.c_int, HOOKPROC, wintypes.HINSTANCE, wintypes.DWORD]
        user32.SetWindowsHookExW.restype = wintypes.HHOOK

        def kb(n, wp, lp):
            if n == 0:
                k = ctypes.cast(lp, ctypes.POINTER(KBDLLHOOKSTRUCT)).contents
                if not (k.flags & LLKHF_INJECTED):
                    self.last_human = time.monotonic()
                    self.events += 1
            return user32.CallNextHookEx(None, n, wp, lp)

        def ms(n, wp, lp):
            if n == 0:
                m = ctypes.cast(lp, ctypes.POINTER(MSLLHOOKSTRUCT)).contents
                if not (m.flags & LLMHF_INJECTED):
                    if wp == WM_MOUSEMOVE:
                        pt = (m.pt.x, m.pt.y)
                        if self._last_pt:
                            self._moved += abs(pt[0] - self._last_pt[0]) + abs(pt[1] - self._last_pt[1])
                        self._last_pt = pt
                        if self._moved >= self.mouse_threshold:
                            self._moved = 0
                            self.last_human = time.monotonic()
                            self.events += 1
                    else:  # clicks / wheel
                        self.last_human = time.monotonic()
                        self.events += 1
                else:
                    self._last_pt = None
            return user32.CallNextHookEx(None, n, wp, lp)

        self._kb_proc, self._ms_proc = HOOKPROC(kb), HOOKPROC(ms)  # keep refs alive
        hk = user32.SetWindowsHookExW(WH_KEYBOARD_LL, self._kb_proc, None, 0)
        hm = user32.SetWindowsHookExW(WH_MOUSE_LL, self._ms_proc, None, 0)
        self.ok = bool(hk and hm)
        msg = wintypes.MSG()
        try:
            while user32.GetMessageW(ctypes.byref(msg), None, 0, 0) > 0:
                user32.TranslateMessage(ctypes.byref(msg))
                user32.DispatchMessageW(ctypes.byref(msg))
        finally:
            if hk:
                user32.UnhookWindowsHookEx(hk)
            if hm:
                user32.UnhookWindowsHookEx(hm)
