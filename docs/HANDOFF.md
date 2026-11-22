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
  - `pipeline.rs` (M2) headless builds: Rojo 7.7.1 + Lune 0.11.1 (pinned) downloaded into `data\tools\`; game
    projects in `data\games\<slug>\` from the template in `src/template/` (Rojo project: Shared/Server/Client +
    `tests/*.spec.luau`). `run()` = Rojo build → Lune compiles every script + runs specs (`src/luau/check.luau`) →
    upload as a **Saved** version to the test place → same specs on Roblox via Luau Execution (`src/luau/remote.luau`).
    Shared test harness `src/luau/harness.luau`. Report in `<game>\.roxai\last_build.json` + store `pipeline_last`.
  - `cloud.rs` (M2) Open Cloud: `connect()` checks key + place with a real Luau run; key saved DPAPI-encrypted in
    `data\secrets\roblox.key` (never in settings, logs or tool output); `upload_place()`, `run_luau()`.
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
- ✅ M0 new UI (Tauri + React). ✅ M1 Playbook. ✅ M2 headless build pipeline — **verified live on the owner's PC**:
  build → script checks + unit tests (Lune) → upload as Saved version → 5/5 specs passed on Roblox's servers via Luau
  Execution. Test place "ROX-AI Test" (alt account) is connected; key is DPAPI-encrypted in `data\secrets\`.
  Settings → Game building has the connection card and **Test pipeline**. CLI: `roxai pipeline tools|new|build
  [--remote]|selftest`, `roxai roblox connect --place <id>` (key on stdin)`|status|disconnect`. CI runs
  `pipeline selftest` on Linux and Windows.
- Roblox refuses uploads (HTTP 409 "server is busy") while the test place is open in **Studio**; uploads retry for a
  few minutes and then tell the owner to close Studio. Agents never need Studio open.
- Dev builds (exe under `target\`) skip Windows registration in `setup::ensure`, so interface previews never take
  over the real app's schedule or Claude connections (`ROXAI_DEV_REGISTER=1` forces it).
- **M3 agent team: done.** First game, *Skip a Stone!*, built end to end on 2026-11-21 (46 tasks; 359 local +
  427 Roblox-server tests green; Lead verdict "ready"). The owner tests it before the team starts another game
  (team switched off). Still owner-side: create the 9 passes / 30 products on the published place and paste
  the ids (`monetization-ids.md` in the game folder; ids stay 0 until then, which sells nothing).
- **Play button (start of M5):** My Games → Play builds the game and uploads it as the *Published* version of the
  owner's private test place (`cloud::upload_playable`, `pipeline::play_upload`, UI command `game_play`), then opens
  `roblox://experiences/start?placeId=…`. The experience stays private, so only its owner account can join. Tests
  keep using *Saved* versions. Publish (a real public experience) is still to do.
- **Visual checks (M5, without Studio or the owner's screen):** `project_render` gives agents pictures of the 3D
  world. `luau/scene.luau` loads the built place in Lune, runs an optional `setup` snippet (the game's world builder),
  and exports every visible part (Lune keeps `part.Position`/`Orientation` writes as plain values, so the export
  applies them like Roblox would). `render.rs` draws them with a small software rasterizer: flat shading, sun, sky,
  haze, 2× supersampling, camera pushed in front of obstacles like Roblox's. Views: spawn, overview, top, custom
  `{eye, look_at}`; findings such as a buried spawn. The setup is remembered in `.roxai/render_setup.luau`; each render
  also saves `.roxai/card.jpg`, the picture on the owner's game card (redrawn when a game finishes). World builder,
  QA and the Lead's final review must look at renders. CLI: `roxai pipeline render <dir> --setup "<luau>"`.
  Not covered yet: UI screens, text on signs, terrain, Roblox's real lighting.
  - `team.rs`: roster of 15 (9 active: lead, designer, economy, systems, gameplay, ui, world, qa, reviewer; the rest
    "join later"), default model aliases per agent (`opus`/`sonnet`/`haiku`, setting `agent_models` overrides).
    Board per game in `<game>\.roxai\studio.json` (locked store): game info, tasks (kind design/plan/work/fix/qa/
    review/final/feedback, owner, done_when, deps, attempts ≤3), decisions, activity, owner feedback, verdict.
    Orchestrator `team::run` (lock `studio`): Lead ideates → `game_create` → Designer writes design.md → Lead plans
    tasks → specialists (one at a time, or one task per owner in parallel up to `team_max_parallel`) → local build
    after each batch (failure = fix task; 6 failed fix rounds or `max_agent_runs_per_game` = stopped) → QA + code
    review → Lead's final review → `game_verdict` → final test on Roblox's servers → ready + toast. Owner feedback
    adds a Lead "feedback" task and resumes building. Usage limit = break until the reset time Claude names (+2 min; 15 min if it names none) (`team_paused_until`).
  - `team_tools.rs`: MCP tools only present in agent sessions (env `ROXAI_AGENT`, `ROXAI_GAME`), sandboxed to the
    game folder (`safe_path`: no absolute/`..`/hidden/`build`): board, project_files/read/write/edit/delete/search,
    project_build, project_test, task_add/update, decision; Lead also game_create, game_verdict, game_feedback,
    team_settings. Agents get no built-in file or shell tools, so they can never reach the Roblox key.
  - Lead chat: `chat_send` → `roxai team chat` (lock `lead-chat`) replies as the Lead. CLI: `roxai team run
    [--if-needed]|chat|say "<text>"|status|feedback <game> "<text>"`. The hourly scheduled check wakes the team;
    learning skips while the team builds (building comes first).
  - App: Team page (Start building / Pause, game in progress with live tasks + activity, Lead chat, games to build,
    size cap small/medium (large off), pace one-at-a-time/all-at-once, member cards with model picker), My Games
    cards with the Lead's summary and a working Feedback box (Play/Publish arrive in M5).
- **Pivot (2026-11-21): game building paused, research first.** The owner played *Skip a Stone!* and judged it
  not good enough: no style, unclear core loop, no level design, no models. The team stays off until the knowledge
  is much deeper. Research moves into its own app.
- **ROX Research (`ROX-Research.exe`, crates `research/` + `research-ui/`).** The owner films Roblox games themself:
  1. They pick a game (name, link or place id).
  2. Claude, with WebSearch, writes a shot list: segments, each with steps, "must show" points and minutes
     (`core::make_plan`).
  3. A guide window floats over Roblox (`overlay.html`, always on top, compact mode). Windows hides it from capture
     with `WDA_EXCLUDEFROMCAPTURE`, so it never appears in a video. It shows what to do, a timer, Start/Finish/Skip
     and a notes box per segment.
  4. Each segment is its own `segNN.mp4`:
     - video: only the Roblox window (ffmpeg `gfxcapture hwnd=`; NVENC→AMF→QSV→MF→x264);
     - sound: only the game's audio (WASAPI process loopback);
     - the two are joined with the lead-in trimmed.
     ffmpeg is downloaded into `ROX-Research Data\tools` on first start.
  5. The owner writes final notes and clicks **Push**:
     - contact sheets per segment;
     - Claude reads every frame with the owner's notes (`analysis/segNN.md`);
     - a synthesis pass writes `ROX Research Vault\Teardowns\<Game>.md`. It places 12 to 24 `SCREENSHOT: segNN @ m:ss`
       lines. Each one becomes a full-size HD frame cut from that video into `Teardowns\attachments\<session>`;
     - then the segment videos and contact sheets are deleted (`videos_deleted`); the analysis and screenshots stay.

  Recording is HD: the window is scaled to fit 1920×1080 at 6 Mbps. Shot lists have 8 to 12 segments of 1 to 3 minutes.
  A segment with no picture (for example, a minimized window) is refused, and the segment goes back to ready.
  Tabs: Study a game, Sessions, **Studied** (every game with a finished analysis, by name, opening the full
  analysis), Playbook.

  Folders sit next to the exe: `ROX-Research Data` and `ROX Research Vault`. Override them with `ROXRESEARCH_HOME` /
  `ROXRESEARCH_VAULT`. **The vault's Playbook is new and deliberately not linked to ROX-AI's old Playbook.** It
  starts empty and grows from teardowns. Turning teardowns into Playbook pages is the next step.

  CLI for checks: `ROX-Research.exe <command> '<json>'` (status, search, session_new, rec_test, push…).

  Test switches:
  - `ROXRESEARCH_TEST_EXE` records another app instead of Roblox.
  - `ROXRESEARCH_ROUTE` / `ROXRESEARCH_OVERLAY` open a page or the guide without clicks.
  - `ROXRESEARCH_GUIDE_VISIBLE` lets checks capture the guide.
  - The UI runs in a browser with sample data: `npm run dev` in `research-ui` with `VITE_MOCK` set to a `session.json` URL (dev only).

  Not yet verified on the owner's PC: recording a real Roblox window and its sound (needs them in a game).
- Then M4 Market ideas, M5 invisible Studio + My Games Play/Feedback/Publish, M6 Create + Library, M7 Live ops,
  M8 Promo, M9 iPhone.

## Running the team for real (learned on the first game)
- Claude usage limits: agent runs that hit one pause the team (`team_paused_until`, until the "resets 5:40pm" time Claude gives +2 min, else 15 min); the `team run` process
  waits it out itself (PC kept awake by `util::KeepAwake`) and doesn't use up task attempts. The hourly Learning
  task repeats all day (P1D) as a backup; `setup::ensure` re-creates tasks when the `sched2` suffix changes.
- Sign-in: the shared Claude Code login kept expiring between runs (other Claude apps refresh it). Team page →
  "Keep the team signed in": `claude setup-token` in a console, owner pastes the token, saved DPAPI-encrypted in
  `data\secrets\claude.token`, passed to every session as `CLAUDE_CODE_OAUTH_TOKEN`.
- Token economy: agent sessions get no built-in tools (`--tools ""`; the Lead and learning runs keep only
  WebSearch/WebFetch), MCP tools load up front (`ENABLE_TOOL_SEARCH=false`). `board` is compact (finished tasks one
  line, others' text cut), `project_read` ≤ 400 lines, `project_outline` (file skeleton with line numbers),
  `project_search` ≤ 60 hits with `path` filter. The rulebook has a "Work economy" section. Thinking effort per
  task (`team::effort_for`): high for design/plan/fix/review/final/feedback/ideas, medium for normal building
  (setting `agent_effort` overrides per agent). Thinking was ~95% of an agent's output and stays in its context. On the first game the
  board was 73k chars and file reads were 59% of all tool output; the agents' own context re-reads dominate usage.
- Progress: `team::estimate` = average real task time (from the activity log) × open tasks + QA/review/final, shown
  as a bar with % and "about X left · done around HH:MM" (Team page, My Games, `roxai team status`, Lead chat).

## Known notes
- Windows title bar is custom (frameless); confirm on their screen that dragging/maximize/close and the border look right.
- Game icons and some YouTube pages were blocked in the cloud sandbox; they should work on the PC — verify.
- Playtesting code (Win32 input/capture) still exists but is disabled; don't use the owner's screen.
