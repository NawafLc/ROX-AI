"""Low-level keyboard/mouse injection for Roblox via Win32 SendInput.

Roblox reads raw/DirectInput, so keys are sent as *scan codes* and camera turns as
*relative* mouse moves — that's what makes them register in-game. Every key we press
is tracked so `release_all()` can guarantee nothing stays stuck after an abort/crash.
"""
from __future__ import annotations

import ctypes
import sys
import threading
import time

IS_WIN = sys.platform == "win32"

# ---- scan codes (set 1)
SCAN = {
    "esc": 0x01, "1": 0x02, "2": 0x03, "3": 0x04, "4": 0x05, "5": 0x06, "6": 0x07, "7": 0x08, "8": 0x09, "9": 0x0A,
    "0": 0x0B, "-": 0x0C, "=": 0x0D, "backspace": 0x0E, "tab": 0x0F, "q": 0x10, "w": 0x11, "e": 0x12, "r": 0x13,
    "t": 0x14, "y": 0x15, "u": 0x16, "i": 0x17, "o": 0x18, "p": 0x19, "[": 0x1A, "]": 0x1B, "enter": 0x1C,
    "ctrl": 0x1D, "a": 0x1E, "s": 0x1F, "d": 0x20, "f": 0x21, "g": 0x22, "h": 0x23, "j": 0x24, "k": 0x25, "l": 0x26,
    ";": 0x27, "'": 0x28, "`": 0x29, "shift": 0x2A, "\\": 0x2B, "z": 0x2C, "x": 0x2D, "c": 0x2E, "v": 0x2F,
    "b": 0x30, "n": 0x31, "m": 0x32, ",": 0x33, ".": 0x34, "/": 0x35, "rshift": 0x36, "alt": 0x38, "space": 0x39,
    "capslock": 0x3A, "f1": 0x3B, "f2": 0x3C, "f3": 0x3D, "f4": 0x3E, "f5": 0x3F, "f6": 0x40, "f7": 0x41,
    "f8": 0x42, "f9": 0x43, "f10": 0x44, "f11": 0x57, "f12": 0x58,
}
EXTENDED = {"up": 0x48, "down": 0x50, "left": 0x4B, "right": 0x4D, "home": 0x47, "end": 0x4F, "pageup": 0x49,
            "pagedown": 0x51, "insert": 0x52, "delete": 0x53}
ALIASES = {"return": "enter", "escape": "esc", "control": "ctrl", "spacebar": "space", " ": "space", "lshift": "shift",
           "jump": "space", "sprint": "shift", "pgup": "pageup", "pgdn": "pagedown", "del": "delete"}
# Keys Claude is never allowed to send (could close the game/OS shortcuts or leave the sandbox)
FORBIDDEN = {"alt", "f4", "win", "lwin", "rwin"}

INPUT_MOUSE, INPUT_KEYBOARD = 0, 1
KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, KEYEVENTF_SCANCODE = 0x1, 0x2, 0x4, 0x8
MOUSEEVENTF_MOVE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP = 0x1, 0x2, 0x4
MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP = 0x8, 0x10
MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP = 0x20, 0x40
MOUSEEVENTF_WHEEL, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_VIRTUALDESK = 0x800, 0x8000, 0x4000

if IS_WIN:
    from ctypes import wintypes

    ULONG_PTR = ctypes.c_size_t

    class MOUSEINPUT(ctypes.Structure):
        _fields_ = [("dx", wintypes.LONG), ("dy", wintypes.LONG), ("mouseData", wintypes.DWORD),
                    ("dwFlags", wintypes.DWORD), ("time", wintypes.DWORD), ("dwExtraInfo", ULONG_PTR)]

    class KEYBDINPUT(ctypes.Structure):
        _fields_ = [("wVk", wintypes.WORD), ("wScan", wintypes.WORD), ("dwFlags", wintypes.DWORD),
                    ("time", wintypes.DWORD), ("dwExtraInfo", ULONG_PTR)]

    class HARDWAREINPUT(ctypes.Structure):
        _fields_ = [("uMsg", wintypes.DWORD), ("wParamL", wintypes.WORD), ("wParamH", wintypes.WORD)]

    class _U(ctypes.Union):
        _fields_ = [("mi", MOUSEINPUT), ("ki", KEYBDINPUT), ("hi", HARDWAREINPUT)]

    class INPUT(ctypes.Structure):
        _anonymous_ = ("u",)
        _fields_ = [("type", wintypes.DWORD), ("u", _U)]

    _user32 = ctypes.WinDLL("user32", use_last_error=True)
    _user32.SendInput.argtypes = (wintypes.UINT, ctypes.POINTER(INPUT), ctypes.c_int)
    _user32.SendInput.restype = wintypes.UINT

# marker in dwExtraInfo so we can tell our own input from the human's
ROXAI_TAG = 0x524F58  # "ROX"

_pressed_keys: set[str] = set()
_pressed_buttons: set[str] = set()
_lock = threading.RLock()


class InputError(RuntimeError):
    pass


def _send(*inputs) -> None:
    if not IS_WIN:
        raise InputError("Input control only works on Windows")
    arr = (INPUT * len(inputs))(*inputs)
    n = _user32.SendInput(len(inputs), arr, ctypes.sizeof(INPUT))
    if n != len(inputs):
        raise InputError(f"SendInput blocked (err {ctypes.get_last_error()}). Is the PC locked or a UAC/admin window focused?")


def norm_key(k: str) -> str:
    k = k.strip().lower()
    return ALIASES.get(k, k)


def _key_input(key: str, up: bool):
    key = norm_key(key)
    if key in FORBIDDEN:
        raise InputError(f"key '{key}' is blocked for safety")
    flags = KEYEVENTF_SCANCODE | (KEYEVENTF_KEYUP if up else 0)
    if key in EXTENDED:
        code = EXTENDED[key]
        flags |= KEYEVENTF_EXTENDEDKEY
    elif key in SCAN:
        code = SCAN[key]
    else:
        raise InputError(f"unknown key '{key}'. Valid: {', '.join(sorted(set(SCAN) | set(EXTENDED)))}")
    i = INPUT(type=INPUT_KEYBOARD)
    i.ki = KEYBDINPUT(0, code, flags, 0, ROXAI_TAG)
    return i


def key_down(key: str) -> None:
    with _lock:
        _send(_key_input(key, False))
        _pressed_keys.add(norm_key(key))


def key_up(key: str) -> None:
    with _lock:
        _send(_key_input(key, True))
        _pressed_keys.discard(norm_key(key))


def tap(key: str, hold: float = 0.06) -> None:
    key_down(key)
    try:
        time.sleep(hold)
    finally:
        key_up(key)


def hold_keys(keys: list[str], seconds: float, check=None) -> None:
    """Hold several keys together (e.g. ['w','shift']) for `seconds`, polling `check()` to allow aborts."""
    seconds = max(0.0, min(seconds, 30.0))
    for k in keys:
        key_down(k)
    try:
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            if check:
                check()
            time.sleep(min(0.05, max(0.0, end - time.monotonic())))
    finally:
        for k in reversed(keys):
            try:
                key_up(k)
            except InputError:
                pass


def type_unicode(text: str, delay: float = 0.012) -> None:
    for ch in text:
        if ch == "\n":
            tap("enter")
            continue
        down = INPUT(type=INPUT_KEYBOARD)
        down.ki = KEYBDINPUT(0, ord(ch), KEYEVENTF_UNICODE, 0, ROXAI_TAG)
        up = INPUT(type=INPUT_KEYBOARD)
        up.ki = KEYBDINPUT(0, ord(ch), KEYEVENTF_UNICODE | KEYEVENTF_KEYUP, 0, ROXAI_TAG)
        _send(down, up)
        time.sleep(delay)


# ---- mouse

def _virtual_screen() -> tuple[int, int, int, int]:
    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN = 76, 77, 78, 79
    g = _user32.GetSystemMetrics
    return g(SM_XVIRTUALSCREEN), g(SM_YVIRTUALSCREEN), g(SM_CXVIRTUALSCREEN), g(SM_CYVIRTUALSCREEN)


def move_abs(x: int, y: int) -> None:
    vx, vy, vw, vh = _virtual_screen()
    nx = int(round((x - vx) * 65535 / max(1, vw - 1)))
    ny = int(round((y - vy) * 65535 / max(1, vh - 1)))
    i = INPUT(type=INPUT_MOUSE)
    i.mi = MOUSEINPUT(nx, ny, 0, MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK, 0, ROXAI_TAG)
    _send(i)


def move_rel(dx: int, dy: int) -> None:
    i = INPUT(type=INPUT_MOUSE)
    i.mi = MOUSEINPUT(int(dx), int(dy), 0, MOUSEEVENTF_MOVE, 0, ROXAI_TAG)
    _send(i)


_BTN = {"left": (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP), "right": (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
        "middle": (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP)}


def button(btn: str, down: bool) -> None:
    if btn not in _BTN:
        raise InputError("button must be left/right/middle")
    i = INPUT(type=INPUT_MOUSE)
    i.mi = MOUSEINPUT(0, 0, 0, _BTN[btn][0 if down else 1], 0, ROXAI_TAG)
    with _lock:
        _send(i)
        (_pressed_buttons.add if down else _pressed_buttons.discard)(btn)


def click_at(x: int, y: int, btn: str = "left", double: bool = False, hold: float = 0.05) -> None:
    move_abs(x, y)
    time.sleep(0.05)
    # Roblox only registers hover/click on GUI after a real move event
    move_rel(1, 0)
    time.sleep(0.03)
    move_rel(-1, 0)
    time.sleep(0.05)
    for n in range(2 if double else 1):
        button(btn, True)
        time.sleep(hold)
        button(btn, False)
        if double and n == 0:
            time.sleep(0.08)


def wheel(clicks: int) -> None:
    i = INPUT(type=INPUT_MOUSE)
    i.mi = MOUSEINPUT(0, 0, ctypes.c_ulong(int(clicks) * 120 & 0xFFFFFFFF).value, MOUSEEVENTF_WHEEL, 0, ROXAI_TAG)
    _send(i)


def drag_camera(dx: int, dy: int, steps: int = 20) -> None:
    """Right-drag = rotate camera in Roblox (works with default camera mode)."""
    button("right", True)
    try:
        sx, sy = dx / steps, dy / steps
        ax = ay = 0.0
        for _ in range(steps):
            ax += sx
            ay += sy
            mx, my = int(ax), int(ay)
            ax -= mx
            ay -= my
            if mx or my:
                move_rel(mx, my)
            time.sleep(0.01)
    finally:
        button("right", False)


def release_all() -> None:
    """Fail-safe: let go of every key/button we might be holding."""
    if not IS_WIN:
        return
    with _lock:
        for k in list(_pressed_keys):
            try:
                _send(_key_input(k, True))
            except Exception:  # noqa: BLE001
                pass
        _pressed_keys.clear()
        for b in list(_pressed_buttons) or ["left", "right"]:
            try:
                i = INPUT(type=INPUT_MOUSE)
                i.mi = MOUSEINPUT(0, 0, 0, _BTN[b][1], 0, ROXAI_TAG)
                _send(i)
            except Exception:  # noqa: BLE001
                pass
        _pressed_buttons.clear()
        # common movement keys, in case of a crash between down/up bookkeeping
        for k in ("w", "a", "s", "d", "space", "shift", "ctrl"):
            try:
                _send(_key_input(k, True))
            except Exception:  # noqa: BLE001
                pass
