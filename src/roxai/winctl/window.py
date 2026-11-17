"""Win32 window helpers: find/focus/resize the Roblox window, idle & lock detection, hotkey polling."""
from __future__ import annotations

import ctypes
import sys
import time
from dataclasses import dataclass

IS_WIN = sys.platform == "win32"

if IS_WIN:
    from ctypes import wintypes

    user32 = ctypes.WinDLL("user32", use_last_error=True)
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)

    WNDENUMPROC = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    user32.EnumWindows.argtypes = [WNDENUMPROC, wintypes.LPARAM]
    user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
    user32.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
    user32.GetClientRect.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.RECT)]
    user32.ClientToScreen.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.POINT)]
    user32.IsHungAppWindow.argtypes = [wintypes.HWND]
    user32.GetForegroundWindow.restype = wintypes.HWND
    user32.OpenInputDesktop.restype = wintypes.HANDLE
    user32.SwitchDesktop.argtypes = [wintypes.HANDLE]
    user32.CloseDesktop.argtypes = [wintypes.HANDLE]
    user32.GetWindowLongW.restype = ctypes.c_long
    user32.SetWindowPos.argtypes = [wintypes.HWND, wintypes.HWND, ctypes.c_int, ctypes.c_int, ctypes.c_int,
                                    ctypes.c_int, wintypes.UINT]

    class LASTINPUTINFO(ctypes.Structure):
        _fields_ = [("cbSize", wintypes.UINT), ("dwTime", wintypes.DWORD)]


_dpi_done = False


def make_dpi_aware() -> None:
    """Use physical pixels everywhere so screenshots and clicks line up on scaled displays."""
    global _dpi_done
    if _dpi_done or not IS_WIN:
        return
    try:
        user32.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))  # PER_MONITOR_AWARE_V2
    except Exception:  # noqa: BLE001
        try:
            ctypes.windll.shcore.SetProcessDpiAwareness(2)
        except Exception:  # noqa: BLE001
            try:
                user32.SetProcessDPIAware()
            except Exception:  # noqa: BLE001
                pass
    _dpi_done = True


@dataclass
class Rect:
    left: int
    top: int
    width: int
    height: int

    @property
    def right(self) -> int:
        return self.left + self.width

    @property
    def bottom(self) -> int:
        return self.top + self.height


def window_pid(hwnd) -> int:
    pid = wintypes.DWORD()
    user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
    return pid.value


def window_title(hwnd) -> str:
    buf = ctypes.create_unicode_buffer(512)
    user32.GetWindowTextW(hwnd, buf, 512)
    return buf.value


def find_windows_for_pids(pids: set[int]) -> list[int]:
    found: list[int] = []

    def cb(hwnd, _lp):
        if user32.IsWindowVisible(hwnd) and window_pid(hwnd) in pids:
            found.append(hwnd)
        return True

    user32.EnumWindows(WNDENUMPROC(cb), 0)
    return found


def client_rect(hwnd) -> Rect | None:
    r = wintypes.RECT()
    if not user32.GetClientRect(hwnd, ctypes.byref(r)):
        return None
    pt = wintypes.POINT(0, 0)
    user32.ClientToScreen(hwnd, ctypes.byref(pt))
    return Rect(pt.x, pt.y, r.right - r.left, r.bottom - r.top)


def is_minimized(hwnd) -> bool:
    return bool(user32.IsIconic(hwnd))


def is_hung(hwnd) -> bool:
    try:
        return bool(user32.IsHungAppWindow(hwnd))
    except Exception:  # noqa: BLE001
        return False


def foreground() -> int:
    return user32.GetForegroundWindow() or 0


def focus(hwnd) -> bool:
    """Bring a window to the front reliably (works around Windows' foreground-lock rules)."""
    SW_RESTORE, SW_SHOW = 9, 5
    if user32.IsIconic(hwnd):
        user32.ShowWindow(hwnd, SW_RESTORE)
    else:
        user32.ShowWindow(hwnd, SW_SHOW)
    if foreground() == hwnd:
        return True
    fg = foreground()
    cur_thread = kernel32.GetCurrentThreadId()
    fg_thread = user32.GetWindowThreadProcessId(fg, None) if fg else 0
    tgt_thread = user32.GetWindowThreadProcessId(hwnd, None)
    attached = []
    try:
        for t in {fg_thread, tgt_thread}:
            if t and t != cur_thread and user32.AttachThreadInput(cur_thread, t, True):
                attached.append(t)
        user32.BringWindowToTop(hwnd)
        user32.SetForegroundWindow(hwnd)
        user32.SetFocus(hwnd)
    finally:
        for t in attached:
            user32.AttachThreadInput(cur_thread, t, False)
    time.sleep(0.15)
    if foreground() != hwnd:
        # last resort: a harmless synthetic key event unlocks SetForegroundWindow
        VK_MENU, KEYUP = 0x12, 0x2
        user32.keybd_event(VK_MENU, 0, 0, 0)
        user32.keybd_event(VK_MENU, 0, KEYUP, 0)
        user32.SetForegroundWindow(hwnd)
        time.sleep(0.15)
    return foreground() == hwnd


def is_fullscreen(hwnd) -> bool:
    r = client_rect(hwnd)
    if not r:
        return False
    w = user32.GetSystemMetrics(0)
    h = user32.GetSystemMetrics(1)
    return r.width >= w and r.height >= h


def resize_client(hwnd, width: int, height: int) -> None:
    """Size the window so its *client* area is width x height, and center it on the primary screen."""
    GWL_STYLE, GWL_EXSTYLE = -16, -20
    style = user32.GetWindowLongW(hwnd, GWL_STYLE)
    ex = user32.GetWindowLongW(hwnd, GWL_EXSTYLE)
    sw, sh = user32.GetSystemMetrics(0), user32.GetSystemMetrics(1)
    width, height = min(width, sw - 40), min(height, sh - 80)
    r = wintypes.RECT(0, 0, width, height)
    user32.AdjustWindowRectEx(ctypes.byref(r), style, False, ex)
    w, h = r.right - r.left, r.bottom - r.top
    x, y = max(0, (sw - w) // 2), max(0, (sh - h) // 2)
    SWP_NOZORDER, SWP_SHOWWINDOW = 0x4, 0x40
    user32.SetWindowPos(hwnd, 0, x, y, w, h, SWP_NOZORDER | SWP_SHOWWINDOW)


def idle_seconds() -> float:
    """Seconds since the last keyboard/mouse input on this PC (any source)."""
    if not IS_WIN:
        return 1e9
    lii = LASTINPUTINFO()
    lii.cbSize = ctypes.sizeof(LASTINPUTINFO)
    if not user32.GetLastInputInfo(ctypes.byref(lii)):
        return 0.0
    return (kernel32.GetTickCount() - lii.dwTime) / 1000.0


def is_locked() -> bool:
    """True when the workstation is locked / on the secure desktop (input can't be injected)."""
    if not IS_WIN:
        return False
    DESKTOP_SWITCHDESKTOP = 0x0100
    h = user32.OpenInputDesktop(0, False, DESKTOP_SWITCHDESKTOP)
    if not h:
        return True
    try:
        return not user32.SwitchDesktop(h)
    finally:
        user32.CloseDesktop(h)


VK = {"ctrl": 0x11, "shift": 0x10, "alt": 0x12, "esc": 0x1B, "f12": 0x7B, "pause": 0x13, "q": 0x51, "x": 0x58,
      "k": 0x4B, "s": 0x53, "end": 0x23}


def hotkey_down(combo: str) -> bool:
    """Poll a hotkey like 'ctrl+shift+q' (physical keyboard state)."""
    if not IS_WIN:
        return False
    for part in combo.lower().split("+"):
        vk = VK.get(part.strip())
        if vk is None:
            if len(part) == 1:
                vk = ord(part.upper())
            else:
                return False
        if not (user32.GetAsyncKeyState(vk) & 0x8000):
            return False
    return True


def screen_size() -> tuple[int, int]:
    if not IS_WIN:
        return (1920, 1080)
    return user32.GetSystemMetrics(0), user32.GetSystemMetrics(1)
