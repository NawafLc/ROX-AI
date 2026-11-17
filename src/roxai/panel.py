"""ROX-AI control panel (desktop app). Pure tkinter — no extra installs.

Everything runs automatically; this window is only for looking and for the big STOP button.
Actions launch detached background processes, so closing the panel never interrupts a run.
"""
from __future__ import annotations

import json
import os
import subprocess
import sys
import threading
import tkinter as tk
from tkinter import ttk

from .config import IS_WINDOWS, app_dir, logs_dir, settings, state_dir

BG, FG, ACC, MUTED, RED, CARD = "#12141f", "#e8ecf4", "#00e0b0", "#8a93a8", "#ff4d5e", "#1b1f2e"
CREATE_NO_WINDOW = 0x08000000
DETACHED = 0x00000008


def bg_cli(*args: str) -> None:
    from .setup import pythonw_exe
    flags = (CREATE_NO_WINDOW | DETACHED) if IS_WINDOWS else 0
    subprocess.Popen([pythonw_exe(), "-m", "roxai.cli", *args], cwd=str(app_dir()), creationflags=flags,
                     stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, close_fds=True)


def open_path(p: str) -> None:
    if IS_WINDOWS:
        os.startfile(p)  # type: ignore[attr-defined]
    else:
        subprocess.Popen(["xdg-open", p])


class Panel:
    def __init__(self) -> None:
        self.root = tk.Tk()
        self.root.title("ROX-AI")
        self.root.geometry("760x640")
        self.root.minsize(640, 520)
        self.root.configure(bg=BG)
        try:
            from .setup import make_icon
            self.root.iconbitmap(str(make_icon()))
        except Exception:  # noqa: BLE001
            pass
        style = ttk.Style()
        style.theme_use("clam")
        style.configure("TButton", background=CARD, foreground=FG, borderwidth=0, padding=10, font=("Segoe UI", 10))
        style.map("TButton", background=[("active", "#262b40")])
        style.configure("Stop.TButton", background=RED, foreground="white", font=("Segoe UI", 11, "bold"))
        style.map("Stop.TButton", background=[("active", "#ff6b79")])
        style.configure("Acc.TButton", background=ACC, foreground="#06110e", font=("Segoe UI", 10, "bold"))
        style.map("Acc.TButton", background=[("active", "#4dffd3")])

        head = tk.Frame(self.root, bg=BG)
        head.pack(fill="x", padx=18, pady=(16, 6))
        tk.Label(head, text="ROX-AI", fg=ACC, bg=BG, font=("Segoe UI", 22, "bold")).pack(side="left")
        tk.Label(head, text="  Roblox chart analyst · AI playtester · Obsidian vault", fg=MUTED, bg=BG,
                 font=("Segoe UI", 10)).pack(side="left", pady=(8, 0))

        self.session_lbl = tk.Label(self.root, text="", fg=FG, bg=CARD, font=("Segoe UI", 11), anchor="w",
                                    justify="left", padx=12, pady=10)
        self.session_lbl.pack(fill="x", padx=18, pady=6)

        btns = tk.Frame(self.root, bg=BG)
        btns.pack(fill="x", padx=18, pady=6)
        s = settings()
        rows = [
            [("📖 Open Vault", "Acc.TButton", lambda: bg_cli("open-vault")),
             ("📊 Scan charts now", "TButton", lambda: self.fire("scan", "Chart scan started (1-3 min).")),
             ("🕹️ Run playtests now", "TButton", lambda: self.fire("autopilot --force", "Playtest run started — hands off the PC."))],
            [(f"⛔ STOP  ({s.kill_hotkey.upper()})", "Stop.TButton", self.stop),
             ("✅ I fixed the login", "TButton", lambda: self.fire("unblock", "Blocker cleared.")),
             ("🔧 Repair setup", "TButton", lambda: self.fire("setup", "Re-running setup…"))],
            [("📁 Logs", "TButton", lambda: open_path(str(logs_dir()))),
             ("⚙️ Settings", "TButton", lambda: open_path(str(app_dir() / "settings.json"))),
             ("🔄 Refresh", "TButton", self.refresh)],
        ]
        for r, row in enumerate(rows):
            for c, (txt, sty, cmd) in enumerate(row):
                ttk.Button(btns, text=txt, style=sty, command=cmd).grid(row=r, column=c, sticky="ew", padx=4, pady=4)
        for c in range(3):
            btns.columnconfigure(c, weight=1)

        self.msg = tk.Label(self.root, text="", fg=ACC, bg=BG, font=("Segoe UI", 10), anchor="w")
        self.msg.pack(fill="x", padx=22)

        tk.Label(self.root, text="System health", fg=MUTED, bg=BG, font=("Segoe UI", 10, "bold"), anchor="w").pack(
            fill="x", padx=22, pady=(10, 0))
        self.health = tk.Text(self.root, bg=CARD, fg=FG, relief="flat", font=("Consolas", 10), height=14, wrap="word",
                              padx=10, pady=8)
        self.health.pack(fill="both", expand=True, padx=18, pady=(4, 6))
        tk.Label(self.root, text="Next playtest queue", fg=MUTED, bg=BG, font=("Segoe UI", 10, "bold"), anchor="w").pack(
            fill="x", padx=22)
        self.queue = tk.Label(self.root, text="", fg=FG, bg=CARD, font=("Segoe UI", 10), anchor="w", justify="left",
                              padx=12, pady=8)
        self.queue.pack(fill="x", padx=18, pady=(4, 16))

        self.refresh()
        self.tick()

    def fire(self, cmd: str, msg: str) -> None:
        bg_cli(*cmd.split())
        self.msg.config(text=msg)
        self.root.after(4000, self.refresh)

    def stop(self) -> None:
        from .session import request_stop
        request_stop("panel STOP button")
        bg_cli("stop")
        self.msg.config(text="⛔ Stop sent — Roblox closed, AI halted.", fg=RED)

    def tick(self) -> None:
        try:
            p = state_dir() / "session.json"
            if p.exists():
                d = json.loads(p.read_text(encoding="utf-8"))
                el = int(d.get("elapsed", 0))
                txt = (f"🎮 PLAYTESTING: {d.get('name')}   ⏱ {el // 60}:{el % 60:02d} / {int(d.get('budget_s', 0)) // 60} min\n"
                       f"Hands off the mouse & keyboard — touching them (or {settings().kill_hotkey.upper()}) stops the AI.")
                self.session_lbl.config(text=txt, fg=ACC)
            else:
                from .db import get_db
                n = get_db().get("last_notice")
                txt = "💤 Idle — everything runs automatically (daily scan, weekly playtests when your PC is idle)."
                if n:
                    txt += f"\nLast: {n.get('title')} — {n.get('text')}"
                self.session_lbl.config(text=txt, fg=FG)
        except Exception as e:  # noqa: BLE001
            self.session_lbl.config(text=f"status error: {e}")
        self.root.after(1500, self.tick)

    def refresh(self) -> None:
        def work():
            try:
                from .doctor import report_text
                from .scan import playtest_queue
                h = report_text(network=False)
                q = playtest_queue()
                qt = "\n".join(f"{i + 1}. {x['title']}  (#{x['rank']}, {x['reason']})" for i, x in enumerate(q)) or "Nothing queued."
            except Exception as e:  # noqa: BLE001
                h, qt = f"error: {e}", ""
            self.root.after(0, lambda: self._show(h, qt))
        threading.Thread(target=work, daemon=True).start()

    def _show(self, h: str, q: str) -> None:
        self.health.config(state="normal")
        self.health.delete("1.0", "end")
        self.health.insert("end", h)
        self.health.config(state="disabled")
        self.queue.config(text=q)

    def run(self) -> None:
        self.root.mainloop()


def main() -> None:
    if IS_WINDOWS:
        try:
            import ctypes
            ctypes.windll.shcore.SetProcessDpiAwareness(1)
        except Exception:  # noqa: BLE001
            pass
    Panel().run()


if __name__ == "__main__":
    main()
