# ROX-AI 🤖🎮

**A Windows desktop app that lets Claude study the top Roblox games for you**: it follows the charts every day,
**actually plays** the trending games on an alt account (it sees the screen and uses the keyboard and mouse), and writes
a detailed **Obsidian vault** with one **template / blueprint note per game**: core loop, onboarding, systems,
progression, UI, monetization (with real prices), retention, content angles and a Roblox Studio build plan.

Claude is plugged in as an **MCP server**, so it runs on your existing Claude subscription. No API key needed.
ROX-AI is written in **Rust**: one small, fast, self-contained `roxai.exe` (no Python or other runtime needed).

---

## Install (one time, ~10 minutes, almost all automatic)

1. Download **`ROX-AI-windows-x64.zip`** from the latest **Release** (right side of the GitHub page) and unzip it anywhere.
   (Downloading the source ZIP also works: the installer then fetches the latest release, or builds it for you.)
2. Double-click **`INSTALL.bat`**.
3. It installs and sets up everything by itself: ROX-AI, Obsidian, Roblox, Claude Desktop, Claude Code,
   the vault, the Claude connection, the schedule, a desktop shortcut, and the first chart scan.
4. At the end it asks you for the only two things it can't do for you:
   - **Roblox:** log in with an **alt account** (never your main) and press Play on any game once.
   - **Claude:** log in when the `claude` window opens (then type `/exit`), and sign in to Claude Desktop once.

That's it. Re-running `INSTALL.bat` at any time repairs or updates everything.

## What happens automatically

| When | What | Needs Claude? |
|---|---|---|
| Every day (and at login if a day was missed) | Scans **Top Trending** (plus Top Playing Now and Up-and-Coming for trends), refreshes every game note, writes the day's chart note, genre notes and the dashboard | No |
| Checked every hour; runs **once a week** when your PC has been **idle** for a few minutes | Claude playtests the queue (new games first, then stale or updated ones; 8 per run by default), fills each game's template, then writes a **Weekly Market Report** | Yes (Claude Code, your subscription) |

You'll get a Windows notification when a weekly run finishes, or if it ever needs you (for example, the Roblox login expired).

## The vault

```
ROX-AI Vault/
├── 00 Dashboard.md        today's chart, risers, new entries, queue, live Dataview tables
├── 01 Charts/             one note per day (rank changes, new entries, drop-offs)
├── 02 Games/              ONE TEMPLATE NOTE PER GAME  ← the main output
├── 03 Playtests/          every AI play session: timeline, screenshots, report
├── 04 Genres/             genre overviews and patterns
├── 05 Insights/           weekly market reports and cross-game analysis
└── Attachments/           icons and screenshots
```

Every game note has the same sections:
📊 Snapshot · 📈 Chart Trend · 🎯 Summary & Why It's Trending · 🔁 Core Loop · 🚪 First 5 Minutes · ⚙️ Systems ·
📶 Progression & Economy · 🖥️ UI/UX · 👥 Social · 🧲 Retention · 💰 Monetization + 📊 Store Data (every pass and product
with its price) · 🎨 Art/Feel · 🎬 Content Angles · 🛠️ Build Blueprint (Roblox Studio) · 🚀 How To Beat It ·
🧾 Scores · 📊 Description · 📊 Badges · 📊 Playtest History · ✍️ **My Notes** (yours, never overwritten).

📊 = refreshed daily from Roblox data · 🤖 = written by Claude · ✍️ = yours.

## Using it from Claude Desktop

The `roxai` tools show up in Claude Desktop. Just ask:

- *"What's trending on Roblox right now?"*
- *"Playtest Throw Food For Animals and fill its template."* (or use the **playtest_game** prompt)
- *"Run the weekly cycle."* / *"Write a market report."*
- *"Which games have the best clone potential?"*

## Safety & fail-safes

- **You are always in charge:** touching your **mouse or keyboard** during a playtest, pressing **Ctrl+Shift+Q**, or
  the **STOP** button in the ROX-AI panel immediately releases all keys and closes Roblox.
- Playtests only start when the PC is **unlocked and idle**. ROX-AI never takes over while you're using it.
- Hard **time budget** per game (12 minutes by default) and a watchdog that closes Roblox if anything hangs, crashes or
  overruns.
- Claude is told **never to spend Robux, chat, trade, or accept requests**. Opening chat is blocked. Alt+F4 and Windows
  keys are blocked.
- Login, ban or verification screens **pause** the autopilot and notify you, instead of retrying forever.
- Every note is written **as it happens** (crash-safe). Interrupted sessions are detected and retried later.
  A game that fails 3 times is skipped for 2 weeks.
- One scan or autopilot at a time (lock files that recover by themselves after a crash). Scheduled tasks retry on failure
  and catch up after the PC was off.
- **Use an alt account.** Automated play is against Roblox's Terms of Use and can get an account banned.

## Control panel

Desktop shortcut **ROX-AI** opens the panel, with live status, system health, the queue and buttons:
Open Vault · Scan now · Run playtests now · STOP · I fixed the login · Repair setup · Logs · Settings.

## Settings

`%LOCALAPPDATA%\ROX-AI\settings.json` (or `roxai set <key> <value>`):

| Key | Default | Meaning |
|---|---|---|
| `chart_limit` | 25 | games per chart scan |
| `sort_id` | top-trending | main chart (`top-playing-now`, `up-and-coming`, …) |
| `playtest_minutes` | 12 | in-game time per playtest |
| `playtests_per_run` | 8 | max games per weekly run (protects your Claude usage limits) |
| `replay_after_days` | 28 | re-playtest a game after this long |
| `playtest_interval_days` | 7 | how often the autopilot runs |
| `idle_required_minutes` | 3 | PC must be idle this long before playtests start |
| `kill_hotkey` | ctrl+shift+q | emergency stop |
| `claude_model` | (your default) | model the autopilot uses |
| `vault_path` | Documents\ROX-AI Vault | where the vault lives |

## Command line

```
roxai scan              # chart scan now
roxai autopilot --force # weekly playtest run now
roxai playtest <universeId>
roxai queue | doctor | stop | unblock | open-vault | settings | set <k> <v>
```
(Installed at `%LOCALAPPDATA%\ROX-AI\bin\roxai.exe` and added to your PATH. `roxai selftest` checks the Windows input/capture/scheduler plumbing.)

## How it works

```
Task Scheduler ─┬─ daily ──► roxai scan ──► Roblox web APIs ──► SQLite history ──► Obsidian vault
                └─ hourly ─► roxai autopilot (if due + PC idle)
                               └─► claude -p (Claude Code, your subscription)
                                     └─► roxai MCP server ──► Roblox Player (launch / screenshot / SendInput)
                                                          └─► Obsidian vault (notes, screenshots)
Claude Desktop ─────────────────────► same roxai MCP server (manual use)
```

Source (`src/`): `scan.rs` data pipeline · `vault.rs` templates · `mcp.rs` MCP server (JSON-RPC over stdio) ·
`session.rs` playtest fail-safes · `win/` Win32 input (SendInput), window control, GDI capture, input hooks, Roblox process
control · `autopilot.rs` weekly runner · `setup.rs` Windows integration · `panel.rs` desktop app (egui) · `store.rs`
crash-safe history (locked JSON files). Two binaries: `roxai.exe` (console/CLI/MCP) and `roxaiw.exe` (same program without
a console window, used by scheduled tasks and the panel).

Build it yourself: `cargo build --release`. CI runs all tests, a live Roblox API scan, the Win32 self-test, the full
installer and an MCP handshake on Windows, then publishes the release ZIP.

Uninstall: `UNINSTALL.bat` (your vault and history are kept).
