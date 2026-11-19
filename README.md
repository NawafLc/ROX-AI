# ROX-AI 🤖🎮

**A Windows app that studies the top Roblox games for you.** It follows the charts every day and lets Claude
**learn each new chart game from YouTube videos**: transcripts, descriptions and chapters of gameplay and guide
videos, plus the game's public store data. It never touches your screen. Everything goes into an **Obsidian vault**
with one detailed **template / blueprint note per game** (core loop, onboarding, systems, progression, UI,
monetization with real prices, retention, content angles and a Roblox Studio build plan), and you can read it right
inside the app.

Two modes, switchable any time on the Research page or in Settings:

| Mode | What it does | Needs |
|---|---|---|
| **Passive** | Daily chart tracking, trends, store prices, badges: just the data | nothing |
| **Active** | All of the above, plus Claude learns every new chart game from YouTube videos and writes the full breakdown | Claude Code (your Claude subscription) |

The app (version 2.1+, rebuilt with Tauri + React) has a sidebar with:

- **Home** (opens first): your games, what each member of your AI team is doing, and a very short summary of today.
- **My Games**: every game your team builds, with screenshots, **Play**, **Feedback** and **Publish** (the building
  parts arrive in the next milestones, see `docs/PLAN.md`).
- **Team**: each agent with its model and live status; the chat with the Lead goes here.
- Under **Knowledge**:
  - **Research**: what ROX-AI is doing right now (with **Stop**), the Passive/Active switch, and every tracked game with search, filters (learned / from videos / not learned), genre and sortable
  columns. Click a game for its page: stats, player graph, the videos it learned from, its full vault note and a
  **Learn now** button.
  - **Market**: today's chart with rank moves, player changes, the hottest games and where players are by genre.
  - **Vault**: browse and read every note inside the app, with working links and callouts; "Open in Obsidian" any time.
- **Settings**: mode, automatic runs, learning amounts, Claude model, chart, things to avoid, updates, folders and
  Remove from this PC.

An **Update ready** card appears in the sidebar when a new version is out: one click installs it. **Check for
updates** always answers ("You're up to date", or why it couldn't check). The window has its own dark title bar.

> AI playtesting on the real Roblox client still exists in the code but is **turned off** for now; video learning
> replaces it.

## Get started

1. Download **`ROX-AI.exe`** from the latest **Release** (right side of the GitHub page).
2. Double-click it. A **setup wizard** walks you through everything:
   - **Location**: keep ROX-AI where it is, or move it to a folder of your choice (recommended if it's in
     Downloads), with optional Desktop / Start-menu shortcuts.
   - **Mode**: Passive (data only) or Active (also learns games from YouTube with Claude).
   - **Connect**: shows what you already have (Claude Code, your Claude account, the video reader, Obsidian, Claude
     Desktop); anything missing sets itself up with one click and a progress bar. "Set up all" does everything.
   - **Done**: run automatically in the background, on or off.

ROX-AI is **portable**: settings, history and your Obsidian vault live in its own folder. No installer, no console
windows. The only things it registers in Windows are the Claude connection and (if "Run automatically" is on) two
scheduled tasks; **Settings → Remove from this PC** undoes both, then just delete the folder.

## What happens automatically

| When | What | Needs Claude? |
|---|---|---|
| Every day (and at login if a day was missed) | Scans **Top Trending** (plus Top Playing Now and Up-and-Coming for trends), refreshes every game note, the day's chart note, genre notes and the dashboard | No |
| Checked hourly in **Active** mode, runs once a day | Claude learns the new or changed chart games (5 per day by default) from 4 YouTube videos each, then once a week writes a **Weekly Market Report** | Yes (runs in the background, never uses your screen) |

The optional **Video reader** (the open-source `yt-dlp`, kept inside ROX-AI's folder) reads YouTube captions when the
public page doesn't show them. Without it Claude still uses titles, descriptions, chapters and web search.

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
- *"Learn Throw Food For Animals from YouTube and fill its template."* (or use the **learn_game** prompt)
- *"Run the weekly cycle."* / *"Write a market report."*
- *"Which games have the best clone potential?"*

## Safety & fail-safes

- **You are always in charge:** touching your **mouse or keyboard** during a playtest, pressing **Ctrl+Shift+Q**, or
  the **Stop** button in the ROX-AI app immediately releases all keys and closes Roblox.
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

## Settings

Most settings are on the app's **Settings** page. All of them live in `data\settings.json` in the app folder (or
`roxai set <key> <value>`):

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
| `vault_path` | (empty = `ROX-AI Vault` in the app folder) | where the vault lives |
| `auto_run` | true | daily scan + learning in the background |
| `mode` | active | `passive` = data only · `active` = Claude learns games from videos |
| `learn_per_run` | 5 | new games learned per day |
| `videos_per_game` | 4 | YouTube videos read per game |
| `reanalyze_after_days` | 30 | re-learn a game after this long |
| `playtesting_enabled` | false | AI playtesting on the Roblox client (off) |
| `avoid` | (empty) | genres or themes your AI team should never make |

## Command line (optional)

`roxai.exe` (next to `ROX-AI.exe`) does everything the app does, for scripting:
```
roxai scan | autopilot --force | learn <universeId> | queue | doctor | stop | update | remove | selftest
```

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
control · `autopilot.rs` runner · `setup.rs` Windows integration · `ui_api.rs` the one API the interface uses ·
`store.rs` crash-safe history (locked JSON files). The interface is in `ui/` (React + Tailwind) and the desktop
shell in `app/` (Tauri 2, uses Windows' built-in WebView2). Two binaries: `roxai.exe` (console/CLI/MCP) and
`ROX-AI.exe` (the app; with arguments it behaves exactly like `roxai.exe`, so the single file is enough).

Build it yourself: `cd ui && npm ci && npm run build`, then `cargo build --release --workspace`. To preview the
interface in a browser: `roxai ui-server --dist ui/dist` and open http://127.0.0.1:7777. CI runs all tests, a live Roblox API scan, the Win32 self-test, the full
portable first start, an MCP handshake and removal on Windows, then publishes the release ZIP.

Remove: in the app, **Settings → Remove from this PC**, then delete the folder.
