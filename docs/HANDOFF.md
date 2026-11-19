# ROX-AI — handoff for a new Claude session

Read this first, then `docs/PLAN.md` (the agreed plan) and `README.md`. You are continuing work that was done in a
cloud session. The owner now works on their own **Windows PC** with you.

## The owner and how to work with them
- Wants **full automation** and to be kept **out of the loop**. Do the whole job, never half. If something blocks you,
  find another way before asking. Only ask them for things only they can do: **sign-ins, API keys, approvals**.
- Their only jobs in the finished product: **play** a finished game, **write feedback**, press **Publish**.
- Wants the UI/UX **very intuitive, good-looking and professional**. Show screenshots of UI changes.
- Personal use only for now (not a product yet).
- Do not put model names in commits or code. Keep their personal info private.

## What ROX-AI is
A **portable Windows app** (no installer; everything lives in its own folder: `data\`, `ROX-AI Vault\`, `tools\`)
that is becoming an **AI Roblox game studio**, like roautomation.dev, with all of its features: a team of AI agents
studies the Roblox market, builds games **without using the owner's screen**, tests them, and hands over finished games.

Repo: `NawafLc/ROX-AI`, working branch `claude/clever-edison-vldouv`. Every push builds, tests and **publishes a
GitHub release** (`v2.1.<run>`, files `ROX-AI.exe` + `ROX-AI-windows-x64.zip`). The app's update button installs
the latest release. Latest at handoff: **v2.1.13** (M0 + M1 done).

## Architecture
- **Rust crate `roxai`** (`src/`) = the engine + `roxai.exe` CLI + MCP server.
  - `scan.rs` daily chart scan (Roblox web APIs) → `store.rs` (crash-safe locked JSON) → `vault.rs` (Obsidian vault,
    blocks `<!-- roxai:auto|ai:key -->`; "✍️ My Notes" never overwritten).
  - `youtube.rs` YouTube search/transcripts (optional yt-dlp "video reader" in `tools\`). `autopilot.rs` runs Claude
    Code headless: `claude -p … --mcp-config … --strict-mcp-config --allowedTools mcp__roxai,WebSearch,WebFetch`.
  - `mcp.rs` MCP tools (charts, game_info, vault_*, youtube_*, learn_*, playbook_*). Playtesting tools exist but
    are **disabled** (`playtesting_enabled=false`): learning is from YouTube + data, never the screen.
  - `playbook.rs` (M1) 11 chapters in `06 Playbook`: 📊 our numbers (computed), 🤖 rules (strong/emerging/opinion
    with evidence), 📚 sources, ✍️ My rules (owner's, always win). Most outdated chapter is rewritten after each
    learning run. Prompt: `src/prompts/playbook.md`. `AGENT_CHAPTERS` maps agents → chapters.
  - `ui_api.rs` the ONE API the interface uses (`call(cmd, json)`): status, home, games, game, chart, playbook,
    vault_tree, note, settings_get/set, start{scan|learn|learn_game|playbook|stop}, update_check/run, deps, …
    Also `roxai ui-server --dist ui/dist` serves the UI + API over HTTP on 127.0.0.1:7777 (previews; later iPhone).
  - `setup.rs`/`deps.rs`/`relocate.rs`/`update.rs`: portable setup, Task Scheduler tasks (daily scan + learning),
    dependency installs (Claude Code, sign-in, video reader, Obsidian, Claude Desktop), move app, self-update.
- **Tauri 2 desktop app** (`app/`, exe `ROX-AI.exe`): frameless window, own dark title bar, dark DWM border.
  With arguments it behaves exactly like `roxai.exe` (single exe is enough).
- **Interface** (`ui/`): React 19 + Vite + Tailwind 4 + lucide-react. Pages: Home (your games, team status, today),
  My Games, Team, Knowledge → Playbook / Research / Market / Vault, Settings, Setup wizard. Hash routing.
  Design tokens in `ui/src/styles.css` (dark: bg #0b0d14, brand #00d6a8).
- Build: `cd ui && npm ci && npm run build`, then `cargo build --release --workspace`. Tests: `cargo test -p roxai`.
  CI: `.github/workflows/ci.yml` (Linux tests + live scan; Windows build, Win32 self-test, portable first start, MCP
  handshake, single-exe test, window-opens test, then release).

## Decisions already made (do not re-ask)
1. **Playbook**: all genres, English only, Claude finds devlogs itself. ✅ built.
2. **Building without the screen**: headless — all code/world as files → **Rojo** builds the place → **Lune** runs
   script checks/unit tests locally → upload to a private **test place** with a free **Open Cloud API key** → run tests
   on Roblox servers with the **Luau Execution API**, read logs. **Invisible desktop** (hidden Win32 desktop →
   virtual desktop → off-screen window) only for screenshots/thumbnails/promo filming. Rojo/Lune/etc. live inside
   ROX-AI's folder. Studio is installed and signed in. Windows only. Owner does not want to watch agents work.
3. **Agents**: full automation, no questions (the Lead decides and logs why). Finished games appear in My Games with
   screenshots, **Play** (private test), **Feedback** (team fixes it), **Publish** (only owner action). Per-agent
   model choice in the app (Opus 5.5 only for important agents, cheaper models for others). Option: all agents at
   once vs one at a time. Owner talks only to the Lead; others just show status.
4. **Market**: the Lead comes up with ideas; owner picks how many games; built **one at a time**; size decided by the
   Lead but **no very big games** for now; **avoid list** in Settings (`avoid` setting exists).
5. **Live ops**: team **prepares fixes only**; content updates automatically only for games that have players; skip
   games doing poorly; **building has priority**; per-game switch to stop live maintenance (saves Claude usage).
6. **Create**: free tools only; the Lead picks the art style.
7. **Library**: behind the scenes; Creator Store + official + CC0 assets, with a safety filter (the Lead's call).
8. **Promo**: Windows built-in voices; owner links TikTok / YouTube Shorts / Instagram when they want; after launch only.
9. **Phone**: iPhone web app (PWA) via the local server.
10. Safety: use an **alt** Roblox account only, never spend Robux, never chat/trade, nothing public without the
    owner's Publish click, the Roblox key stays only on the PC and agents can't read it.

## Where we are / what's next (see PLAN.md build order)
- ✅ M0 new UI (Tauri + React). ✅ M1 Playbook.
- **Next: M2 headless build pipeline** — needs the owner once: (1) a free Open Cloud API key (walk them through
  create.roblox.com → Creator Dashboard → Open Cloud → API keys; permissions for place publishing to a test
  universe and Luau execution; store it only in ROX-AI's data folder, never in the repo or logs) and (2) a private
  test experience/place. On the PC: install Rojo + Lune into ROX-AI's `tools\` (portable), make a project template,
  build a place file, upload to the test place, run a Luau Execution test and read the result. Add this to the app
  (Settings → Roblox connection, deps list) and CI where possible.
- Then M3 agent team (Lead + core agents, task board, models per agent, Lead chat), M4 Market ideas, M5 invisible
  Studio + My Games Play/Feedback/Publish, M6 Create + Library, M7 Live ops, M8 Promo, M9 iPhone.
- Option the owner was offered: build M4 (needs nothing from them) before M2.

## Known notes
- Windows title bar is custom (frameless); confirm on their screen that dragging/maximize/close and the border look right.
- Game icons and some YouTube pages were blocked in the cloud sandbox; they should work on the PC — verify.
- Playtesting code (Win32 input/capture) still exists but is disabled; don't use the owner's screen.
