# ROX-AI: the plan

An AI Roblox game studio for one person. A team of AI agents picks what to build from the Roblox market, builds and
tests the game without touching the screen, and hands you finished games to play, give feedback on, and publish.
**Your only jobs: play, write feedback, press Publish.**

Decided together, point by point. Personal use for now; the parts that would change for a product
(accounts, shared playbook, payments) stay cleanly separated.

---

## 1. Playbook: what the agents know
- Chapters covering **all genres**: picking ideas · core loops by genre · first 5 minutes · progression & economy ·
  monetization & pricing · retention · UI/UX · social · technical (data, performance, anti-exploit) · launch & live ops ·
  marketing.
- Every rule backed by **evidence** (links to game notes) and **our own numbers** (price benchmarks, pass mixes…),
  with a confidence level (strong / emerging / opinion).
- Sources: our game notes (charts, store data, YouTube breakdowns) + **developer devlogs and postmortems** found
  automatically (YouTube, DevForum) + Roblox's official docs + results of our own launched games.
- English only. Updated automatically after each learning run; you can edit it and your edits win.
- Agents search it and read only the chapters for their job.

## 2. Building without your screen
- **Headless build**: all code and world as files → **Rojo** builds the place file → **Lune** checks scripts and runs
  unit tests locally (invisible).
- **Real tests on Roblox's servers**: upload to a private test place via a free **Open Cloud API key**, run the QA
  agent's tests with the **Luau Execution API**, read logs back.
- **Invisible Studio** only when pictures are needed (screenshots, thumbnails, promo filming, visual QA), on a desktop
  you never see. Try: separate hidden desktop → background virtual desktop → off-screen window.
- Full version history per game. Windows only. Rojo/Lune/Blender/ffmpeg live inside ROX-AI's folder (portable).
- Exact tools get verified on your PC when building starts.

## 3. The agent team: full automation
- Runs on **Claude Code** with sub-agents, on your Claude plan, always in the background.
- **15 agents**: Lead · Market Scout · Game Designer · Economy Designer · Systems Scripter · Gameplay Scripter ·
  UI Builder · World Builder · 3D Artist · VFX Artist · QA Tester · Code Reviewer · Thumbnail Artist ·
  Promo Producer · Live-ops Analyst.
- **No questions**: the Lead decides and logs why.
- **Models per agent** (changeable in the app). Defaults: Opus 5.5 for Lead, Game Designer, Systems/Gameplay
  Scripters, Code Reviewer · Sonnet 5.5 for Economy, UI, World, QA, Market, 3D, VFX, Live-ops · Haiku 5.5 for
  Thumbnail, Promo and small chores.
- Switch: **all agents at once** or **one at a time**.
- You **chat with the Lead only**; everyone else shows a **status card** (model, current task, progress).
- Coordination through files: design doc · task board (owner, status, "done when…") · decisions log.
- **My Games**: each finished game has a screenshot gallery, summary, **Play** (your private test version),
  **Feedback** box (Lead turns it into fix tasks), **Publish** (the only way anything goes public).

## 4. Market, ideas and choosing what to build
- Data: **every home-page chart**, top 100 each, daily history, genre & mechanic trends, revenue *estimates*.
- **Pitches** daily (hook, loop, inspired-by, twist, why now, size, money plan, risks).
- **Idea score 0–100**: demand · competition · timing · money potential · freshness · team fit; learns from
  launched games' real results.
- **The Lead creates all ideas** for now. You set **how many games** to build; they're built **one at a time**.
- **Size**: the Lead decides per idea; **Large is capped off** for now (small and medium only).
- **Avoid list** in settings: type genres/themes/mechanics to skip.
- Rule: copy mechanics, never names, characters, art, models, sounds or copyrighted memes.

## 5. Live ops: after Publish
- Numbers: public stats + Roblox **Analytics API** (retention) + a counts-only **tracking module** the agents add to
  every game (where players leave, what sells).
- Live page per game with charts and a **"Needs attention"** list, most urgent first.
- **Fixes**: always prepared. **Content updates**: automatic only for games with real players (threshold setting);
  skipped for games without traction.
- **Building comes first**; live work waits for the current build.
- Per-game **Live maintenance** switch: off = no Claude use for that game (numbers still shown).
- Fixes and updates appear as **"Update ready"** → you press **Publish**.

## 6. Create: free only
- Models: Roblox parts · invisible **Blender** (portable) · Roblox's own AI 3D generation if usable.
- Effects in Luau, checked with invisible-Studio screenshots.
- Icon & thumbnails staged in invisible Studio from the real game, finished by ROX-AI, compared with today's top
  thumbnails and redone if weaker.
- Music & sounds from Roblox's free licensed library. **Lead picks the art style** per game.

## 7. Asset library: behind the scenes
- Sources: Roblox official · Creator Store · CC0 packs (Kenney, Quaternius, Poly Pizza, ambientCG, Poly Haven); the
  Lead chooses.
- Community models always go through the **safety filter** (scripts stripped, quality picks, name/look check).
- Everything our agents make is saved back. Tags, previews, licenses tracked; agents search it.

## 8. Promo maker: after launch only
- Invisible-Studio filming (fly-throughs, scripted core loop, big moments) → **ffmpeg** edit: 9:16, beat-synced cuts,
  hook text, captions, end card.
- Styles: hook + gameplay · progression montage · rare drop · POV · explainer · update showcase.
- Voiceover: **Windows built-in voices**. Two versions: free CC0 music, and no music (add a trending sound in-app).
- **Link account** per platform (TikTok, YouTube Shorts, Instagram Reels). Only linked ones get uploads; TikTok and
  YouTube arrive private until their apps are approved (one tap to make public).

## 9. Phone: iPhone web app
- The full app on your phone (games, screenshots, Feedback, Publish, chat with the Lead, agent status).
- Paired once by QR code, encrypted, unpair any time; free secure tunnel; PC must be on.
- Add to Home Screen once (needed for iPhone notifications): "Game ready", "Update ready", "Needs attention".

## 10. Look and feel
- **Very intuitive, good-looking, professional.**
- UI moves to **Tauri** (Rust engine + modern web interface): one interface for PC and iPhone, polished typography,
  animations and layouts, still a single portable exe.
- Main screens: **Studio** (Lead chat + agent status cards + current build), **My Games** (cards, galleries, Play,
  Feedback, Publish), **Market** (trends, pitches, scores), **Vault/Playbook** (reader), **Settings** (models, speed,
  game count, size cap, avoid list, linked accounts, live-maintenance thresholds).

---

## Build order

| # | Milestone | Needs you |
|---|---|---|
| M0 ✅ | New UI foundation (Tauri), design system, move existing screens | Look at screenshots, say what you think |
| M1 ✅ | Playbook v1: chapters, benchmarks, devlog learning, search tool | — |
| M2 ✅ | Headless build pipeline: Rojo, Lune, project template, test place + Luau Execution tests | **Open Cloud API key** (one-time), **access to your PC** for testing |
| M3 ✅ | Agent team v1: Lead + core agents, task board, models, parallel/one-at-a-time, Lead chat; first small game end to end | PC access |
| M4 | Market: all charts, trends, pitches, scoring, avoid list, size cap, game count | — |
| M5 | Invisible Studio: screenshots, visual QA; My Games gallery, Play, Feedback loop, Publish | PC access |
| M6 | Create + Library: Blender, VFX, thumbnails, library with safety filter | — |
| M7 | Live ops: analytics, tracking module, Needs attention, fixes/updates, maintenance switch | — |
| M8 | Promo maker: filming, editing, styles, voices, platform linking | Link accounts (optional) |
| M9 | iPhone app: tunnel, QR pairing, notifications | Scan a QR code |

What already exists and carries over: chart scanning, game notes, YouTube learning, the vault, portable setup,
self-update, the Claude connection.
