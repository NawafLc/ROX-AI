//! ROX Research: guided recording sessions of Roblox games → segment videos with the owner's notes → an AI
//! teardown in the research vault. Everything lives in its own folders beside the exe ("ROX-Research Data" and
//! "ROX Research Vault"); nothing is shared with ROX-AI's data, vault or Playbook.

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

// ------------------------------------------------------------------ places

pub fn app_folder() -> PathBuf {
    roxai::config::app_folder()
}

pub fn home() -> PathBuf {
    if let Ok(h) = std::env::var("ROXRESEARCH_HOME") {
        if !h.trim().is_empty() {
            return PathBuf::from(h);
        }
    }
    app_folder().join("ROX-Research Data")
}

pub fn vault() -> PathBuf {
    if let Ok(v) = std::env::var("ROXRESEARCH_VAULT") {
        if !v.trim().is_empty() {
            return PathBuf::from(v);
        }
    }
    app_folder().join("ROX Research Vault")
}

pub fn ctx() -> roxai::config::Ctx {
    roxai::config::Ctx::at(home())
}

fn sessions_dir() -> PathBuf {
    home().join("sessions")
}

fn tools_dir() -> PathBuf {
    home().join("tools")
}

/// Create the folders and the empty vault the first time.
pub fn startup() {
    let c = ctx();
    roxai::logx::init(&c.logs_dir());
    let v = vault();
    for d in [v.join("Teardowns").join("attachments"), v.join("Playbook"), sessions_dir()] {
        let _ = std::fs::create_dir_all(d);
    }
    let readme = v.join("README.md");
    if !readme.exists() {
        let _ = std::fs::write(
            &readme,
            "# ROX Research Vault\n\nWhat ROX Research has learned about Roblox games, from real play sessions.\n\n\
             - **Teardowns/**: one deep teardown per recorded game (first 5 minutes, core loop, progression, UI, level design, art, monetization).\n\
             - **Playbook/**: rules for making great Roblox games, grown only from the teardowns. It starts empty.\n",
        );
    }
    let pb = v.join("Playbook").join("README.md");
    if !pb.exists() {
        let _ = std::fs::write(&pb, "# Playbook\n\nStarts empty. Every rule here must come from the teardowns, with the games and moments it is based on.\n");
    }
    // a push that was cut off (app closed) can be pushed again
    for mut s in list_sessions() {
        if s.status == "analyzing" {
            s.status = "recorded".into();
            s.error = "The teardown was interrupted. Press Push again.".into();
            let _ = save(&s);
        }
    }
}

// ------------------------------------------------------------------ games

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Game {
    pub universe_id: u64,
    pub place_id: u64,
    pub name: String,
    pub creator: String,
    pub genre: String,
    pub description: String,
    pub playing: u64,
    pub visits: u64,
    pub likes: f64,
    pub icon: String,
    pub created: String,
    pub updated: String,
    pub passes: Vec<(String, u64)>,
    pub products: Vec<(String, u64)>,
}

fn http() -> Result<ureq::Agent> {
    Ok(ureq::AgentBuilder::new()
        .tls_connector(std::sync::Arc::new(native_tls::TlsConnector::new()?))
        .timeout(Duration::from_secs(25))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) ROX-Research/2.0")
        .build())
}

fn get(url: &str) -> Result<Value> {
    Ok(http()?.get(url).call().map_err(|e| anyhow!("Roblox didn't answer ({e})"))?.into_json()?)
}

/// A place id from a Roblox link or a bare number.
fn place_from(q: &str) -> Option<u64> {
    let q = q.trim();
    if let Ok(n) = q.parse::<u64>() {
        return Some(n);
    }
    let i = q.find("/games/")? + 7;
    q[i..].chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().ok()
}

/// Find games by name, or the one game a link points at.
pub fn search(query: &str) -> Result<Value> {
    use roxai::api::RobloxApi;
    let q = query.trim();
    if q.is_empty() {
        return Ok(json!([]));
    }
    let api = roxai::api::HttpApi::new()?;
    let mut uids: Vec<u64> = vec![];
    if let Some(place) = place_from(q) {
        let v = get(&format!("https://apis.roblox.com/universes/v1/places/{place}/universe"))?;
        uids.push(v["universeId"].as_u64().ok_or_else(|| anyhow!("No Roblox game has that link or id."))?);
    } else {
        let sid = format!("{:x}", chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0));
        let url = format!("https://apis.roblox.com/search-api/omni-search?searchQuery={}&sessionId={sid}&pageType=all", enc(q));
        let v = get(&url)?;
        for r in v["searchResults"].as_array().cloned().unwrap_or_default() {
            for c in r["contents"].as_array().cloned().unwrap_or_default() {
                if let Some(u) = c["universeId"].as_u64() {
                    if !uids.contains(&u) {
                        uids.push(u);
                    }
                }
            }
        }
        uids.truncate(12);
    }
    let info = api.games_info(&uids);
    let icons = api.icons(&uids);
    Ok(json!(uids
        .iter()
        .filter_map(|u| {
            let g = info.get(u)?;
            Some(json!({
                "universe_id": u, "place_id": g["rootPlaceId"], "name": g["name"], "creator": g["creator"]["name"],
                "playing": g["playing"], "visits": g["visits"], "genre": g["genre"], "icon": icons.get(u).cloned().unwrap_or_default(),
            }))
        })
        .collect::<Vec<_>>()))
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

pub fn game_details(uid: u64) -> Result<Game> {
    use roxai::api::RobloxApi;
    let api = roxai::api::HttpApi::new()?;
    let d = api.game_details(uid, true);
    if d.name.is_empty() {
        bail!("Couldn't load that game from Roblox. Try again.");
    }
    let icon = api.icons(&[uid]).get(&uid).cloned().unwrap_or_default();
    Ok(Game {
        universe_id: uid,
        place_id: d.place_id,
        name: d.name.clone(),
        creator: d.creator_name.clone(),
        genre: [d.genre.clone(), d.subgenre.clone()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" / "),
        description: d.description.chars().take(3000).collect(),
        playing: d.playing,
        visits: d.visits,
        likes: d.like_ratio(),
        icon,
        created: d.created.clone(),
        updated: d.updated.clone(),
        passes: d.game_passes.iter().map(|p| (p.name.clone(), p.price.unwrap_or(0))).collect(),
        products: d.dev_products.iter().map(|p| (p.name.clone(), p.price.unwrap_or(0))).collect(),
    })
}

// ------------------------------------------------------------------ sessions

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Shot {
    pub title: String,
    pub goal: String,
    pub steps: Vec<String>,
    pub minutes: f64,
    pub look_for: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Segment {
    pub index: usize,
    pub file: String,
    pub seconds: u64,
    pub notes: String,
    pub recorded_at: String,
    pub skipped: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    pub id: String,
    pub created: String,
    pub game: Game,
    /// planning → ready → recording → recorded → analyzing → done (or failed)
    pub status: String,
    pub step: String,
    pub error: String,
    pub plan: Vec<Shot>,
    /// one per shot, filled as they're recorded
    pub segments: Vec<Segment>,
    pub current: usize,
    pub final_notes: String,
    pub teardown: String,
    /// the segment videos were deleted once the analysis was saved (screenshots stay in the vault)
    pub videos_deleted: bool,
}

static LOCK: Mutex<()> = Mutex::new(());

fn dir_of(id: &str) -> PathBuf {
    sessions_dir().join(id)
}

pub fn load(id: &str) -> Result<Session> {
    let p = dir_of(id).join("session.json");
    Ok(serde_json::from_str(&std::fs::read_to_string(&p).with_context(|| format!("no session {id}"))?)?)
}

pub fn save(s: &Session) -> Result<()> {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let d = dir_of(&s.id);
    std::fs::create_dir_all(&d)?;
    roxai::util::atomic_write(&d.join("session.json"), serde_json::to_string_pretty(s)?.as_bytes())?;
    Ok(())
}

/// Load, change, save.
pub fn update(id: &str, f: impl FnOnce(&mut Session)) -> Result<Session> {
    let mut s = load(id)?;
    f(&mut s);
    save(&s)?;
    Ok(s)
}

pub fn list_sessions() -> Vec<Session> {
    let mut v: Vec<Session> = std::fs::read_dir(sessions_dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| load(&e.file_name().to_string_lossy()).ok())
        .collect();
    v.sort_by(|a, b| b.created.cmp(&a.created));
    v
}

fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_matches('-').chars().take(40).collect()
}

/// Start a session for a game: load its details, then the AI writes the shot list (in the background).
pub fn new_session(uid: u64) -> Result<Session> {
    let game = game_details(uid)?;
    let id = format!("{}-{}", slug(&game.name), chrono::Local::now().format("%Y%m%d-%H%M"));
    let s = Session { id: id.clone(), created: roxai::util::now_iso(), game, status: "planning".into(), step: "Writing your shot list".into(), ..Default::default() };
    save(&s)?;
    std::thread::spawn(move || {
        let r = make_plan(&id);
        let _ = update(&id, |s| match r {
            Ok(plan) => {
                s.segments = (0..plan.len()).map(|i| Segment { index: i, ..Default::default() }).collect();
                s.plan = plan;
                s.status = "ready".into();
                s.step.clear();
                s.error.clear();
            }
            Err(e) => {
                roxai::warn!("plan for {id}: {e:#}");
                s.plan = default_plan(&s.game);
                s.segments = (0..s.plan.len()).map(|i| Segment { index: i, ..Default::default() }).collect();
                s.status = "ready".into();
                s.step.clear();
                s.error = "The AI couldn't write a plan for this game, so this is the standard shot list.".into();
            }
        });
    });
    Ok(s)
}

fn game_facts(g: &Game) -> String {
    let money = |v: &[(String, u64)]| if v.is_empty() { "none".to_string() } else { v.iter().map(|(n, p)| format!("{n} (R${p})")).collect::<Vec<_>>().join("; ") };
    format!(
        "Game: {}\nCreator: {}\nGenre: {}\nPlaying now: {}\nVisits: {}\nLike ratio: {:.0}%\nCreated: {}  Updated: {}\nGame passes: {}\nDeveloper products: {}\nDescription:\n{}",
        g.name, g.creator, g.genre, g.playing, g.visits, g.likes * 100.0, g.created, g.updated, money(&g.passes), money(&g.products), g.description
    )
}

const PLAN_PROMPT: &str = r#"You are a senior Roblox game designer directing a gameplay recording session. The owner will play the game below on a FRESH account while you guide them through segments, shown on top of their screen. The recordings (picture and game sound, no voice) plus the owner's typed notes will be studied frame by frame to learn EVERYTHING about how this game is made: the first-time experience, core loop, progression and economy numbers, every UI screen and the UX, level design and map, models, art style and effects, monetization (what is sold, for how much, where and when it is offered) and social features.

Write the shot list for THIS game. Use web search (the game's wiki, its Roblox page) to learn its real areas, menus, currencies and passes, so every step names real things ("open the Pets menu and scroll through every pet", not "open a menu").

Rules:
- 8 to 12 short segments, 1 to 3 minutes each (never more than 3), about 15 to 25 minutes in total. One segment = one focused thing; split big topics into several segments. The first segment is always the very first minutes as a brand new player, untouched.
- Cover: first-time experience, core loop, every UI screen, shop and monetization (open each pass and product so the price is visible; never buy), progression and later goals, the whole map and level design, models/art/effects up close, and social or multiplayer features.
- steps: 3 to 7 short instructions each (max 14 words), in order, specific to this game. Tell them where to pause the camera ("hold still 3 seconds on each screen") because frames are what gets studied.
- look_for: 2 to 4 things the owner should notice and write notes about in that segment (feelings, confusion, what pulls them forward).
- Never ask them to spend Robux, chat, trade or share personal information.

Reply with ONLY this JSON, nothing else:
{"segments":[{"title":"…","goal":"one sentence: what this segment teaches","steps":["…"],"minutes":2,"look_for":["…"]}]}
"#;

fn make_plan(id: &str) -> Result<Vec<Shot>> {
    let s = load(id)?;
    let prompt = format!("{PLAN_PROMPT}\n\n{}", game_facts(&s.game));
    let out = claude(&prompt, &dir_of(id), "WebSearch,WebFetch", 12, "medium", Duration::from_secs(600))?;
    let start = out.find('{').ok_or_else(|| anyhow!("no JSON in the plan"))?;
    let end = out.rfind('}').ok_or_else(|| anyhow!("no JSON in the plan"))?;
    let v: Value = serde_json::from_str(&out[start..=end])?;
    let plan: Vec<Shot> = serde_json::from_value(v["segments"].clone())?;
    if plan.len() < 3 {
        bail!("the plan was too short");
    }
    // segments stay short: 1 to 3 minutes
    Ok(plan.into_iter().map(|mut x| {
        x.minutes = x.minutes.clamp(1.0, 3.0);
        x
    }).collect())
}

/// The shot list when the AI can't make a game-specific one.
pub fn default_plan(_g: &Game) -> Vec<Shot> {
    let shot = |t: &str, goal: &str, steps: &[&str], m: f64, look: &[&str]| Shot {
        title: t.into(),
        goal: goal.into(),
        steps: steps.iter().map(|s| s.to_string()).collect(),
        minutes: m,
        look_for: look.iter().map(|s| s.to_string()).collect(),
    };
    vec![
        shot("First minutes", "What a brand new player sees and does first.", &["Start recording before the game finishes loading", "Play naturally, like a new player", "Read every popup and tutorial; pause 3 seconds on each"], 3.0, &["When you understood what to do", "Anything confusing"]),
        shot("Core loop", "The thing players repeat and what it pays.", &["Play the main activity again and again", "Buy your first upgrade when you can", "Note how much each action earns"], 3.0, &["How it feels after a few rounds", "What makes you want more"]),
        shot("Every screen", "All menus and the UX.", &["Open every button and menu one by one", "Hold still 3 seconds on each screen", "Scroll slowly through long lists"], 3.0, &["Screens that are clear or messy"]),
        shot("Shop and money", "What is sold, for how much, and where it's offered.", &["Open the shop and every pass and product", "Pause on each price", "Never buy anything"], 2.0, &["Where the game asks you to pay"]),
        shot("The map", "Level design and areas.", &["Walk around the whole map slowly", "Look up and into the distance", "Show locked areas and what unlocking costs"], 3.0, &["What guides you where to go"]),
        shot("Art and effects", "Models, materials, lighting, effects up close.", &["Get close to the best looking models", "Trigger effects (hits, rewards, upgrades)", "Turn the camera slowly around them"], 2.0, &["What makes it look good or cheap"]),
        shot("Later goals", "Progression beyond the start.", &["Show the next goals (rebirth, new areas, quests)", "Open any progress or quest screen"], 2.0, &["What keeps players coming back"]),
    ]
}

// ------------------------------------------------------------------ recording

pub fn ffmpeg_path() -> Option<PathBuf> {
    crate::rec::ffmpeg(&tools_dir())
}

pub fn ensure_ffmpeg() -> Result<PathBuf> {
    if let Some(p) = ffmpeg_path() {
        return Ok(p);
    }
    crate::rec::install_ffmpeg(&tools_dir(), &|t, _| roxai::info!("ffmpeg: {t}"))
}

pub fn segment_file(index: usize) -> String {
    format!("seg{:02}.mp4", index + 1)
}

pub fn rec_start(id: &str, index: usize) -> Result<Value> {
    let ff = ffmpeg_path().ok_or_else(|| anyhow!("The video recorder (ffmpeg) is still being set up. Try again in a minute."))?;
    let mut cur = crate::rec::current().lock().unwrap_or_else(|e| e.into_inner());
    if cur.is_some() {
        bail!("Already recording");
    }
    let s = load(id)?;
    if index >= s.plan.len() {
        bail!("No segment {}", index + 1);
    }
    let out = dir_of(id).join(segment_file(index));
    let r = crate::rec::start(&ff, &out)?;
    *cur = Some((id.to_string(), index, r));
    update(id, |s| {
        s.status = "recording".into();
        s.current = index;
    })?;
    Ok(json!({"ok": true}))
}

/// Stop the segment that's recording, save it with its notes, move to the next one.
pub fn rec_stop(id: &str, notes: &str) -> Result<Value> {
    let taken = crate::rec::current().lock().unwrap_or_else(|e| e.into_inner()).take();
    let Some((sid, index, r)) = taken else { bail!("Nothing is recording") };
    if sid != id {
        bail!("A different session is recording");
    }
    let secs = match r.finish() {
        Ok(s) => s,
        Err(e) => {
            // back to ready on the same segment, so it can be recorded again
            let _ = update(id, |s| s.status = "ready".into());
            return Err(e);
        }
    };
    let s = update(id, |s| {
        if let Some(seg) = s.segments.get_mut(index) {
            seg.file = segment_file(index);
            seg.seconds = secs;
            seg.recorded_at = roxai::util::now_iso();
            seg.skipped = false;
            if !notes.trim().is_empty() {
                seg.notes = notes.to_string();
            }
        }
        s.current = (index + 1).min(s.plan.len());
        s.status = if s.current >= s.plan.len() { "recorded".into() } else { "ready".into() };
    })?;
    Ok(json!({"ok": true, "seconds": secs, "done": s.current >= s.plan.len()}))
}

pub fn recording_state() -> Value {
    let mut cur = crate::rec::current().lock().unwrap_or_else(|e| e.into_inner());
    match cur.as_mut() {
        Some((id, index, r)) => json!({"id": id, "index": index, "seconds": r.seconds(), "alive": r.alive()}),
        None => Value::Null,
    }
}

// ------------------------------------------------------------------ push → teardown

pub fn push(id: &str, final_notes: &str) -> Result<Value> {
    let s = update(id, |s| {
        s.final_notes = final_notes.to_string();
        s.status = "analyzing".into();
        s.step = "Getting the videos ready".into();
        s.error.clear();
    })?;
    if !s.segments.iter().any(|g| !g.file.is_empty()) {
        update(id, |s| s.status = "recorded".into())?;
        bail!("Record at least one segment first.");
    }
    let id = id.to_string();
    std::thread::spawn(move || {
        if let Err(e) = teardown(&id) {
            roxai::error!("teardown {id}: {e:#}");
            let _ = update(&id, |s| {
                s.status = "failed".into();
                s.error = format!("{e:#}");
                s.step.clear();
            });
        }
    });
    Ok(json!({"ok": true}))
}

fn step(id: &str, text: &str) {
    let _ = update(id, |s| s.step = text.to_string());
}

/// Contact sheets: 4 frames (2×2) per image, so the AI sees many moments for few images.
fn sheets(ff: &Path, video: &Path, out: &Path, every: f64) -> Result<Vec<String>> {
    let _ = std::fs::remove_dir_all(out);
    std::fs::create_dir_all(out)?;
    let vf = format!("fps=1/{every},scale=784:-2,tile=2x2");
    let pattern = out.join("sheet_%03d.jpg");
    let (code, text) = roxai::util::run_capture(
        &ff.to_string_lossy(),
        &["-hide_banner", "-loglevel", "error", "-y", "-i", &video.to_string_lossy(), "-vf", &vf, "-q:v", "4", &pattern.to_string_lossy()],
        Duration::from_secs(900),
    )?;
    if code != 0 {
        bail!("couldn't read the video: {}", text.lines().last().unwrap_or(""));
    }
    let mut v: Vec<String> = std::fs::read_dir(out)?.flatten().map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.ends_with(".jpg")).collect();
    v.sort();
    Ok(v)
}

fn teardown(id: &str) -> Result<()> {
    let ff = ensure_ffmpeg()?;
    let s = load(id)?;
    let dir = dir_of(id);
    let recorded: Vec<(usize, &Segment)> = s.segments.iter().enumerate().filter(|(_, g)| !g.file.is_empty() && dir.join(&g.file).is_file()).collect();
    let mut analyses = vec![];
    for (n, (i, seg)) in recorded.iter().enumerate() {
        let shot = &s.plan[*i];
        step(id, &format!("Studying segment {} of {}: {}", n + 1, recorded.len(), shot.title));
        // the first minutes matter most: a frame every 2 s; elsewhere every 4–8 s (at most ~30 sheets a segment)
        let every = if *i == 0 { 2.0 } else { ((seg.seconds as f64) / 120.0).clamp(4.0, 8.0) };
        let folder = format!("frames/seg{:02}", i + 1);
        let files = sheets(&ff, &dir.join(&seg.file), &dir.join(&folder), every)?;
        if files.is_empty() {
            continue;
        }
        let list: Vec<String> = files
            .iter()
            .enumerate()
            .map(|(k, f)| {
                let t0 = k as f64 * 4.0 * every;
                let times: Vec<String> = (0..4).map(|j| clock(t0 + j as f64 * every)).collect();
                format!("- {folder}/{f}: frames at {} (top-left, top-right, bottom-left, bottom-right)", times.join(", "))
            })
            .collect();
        let prompt = format!(
            "{SEGMENT_PROMPT}\n\nThis is segment seg{:02}.\n\n## The game\n{}\n\n## Segment {}: {}\nGoal: {}\nWhat the owner was asked to do:\n{}\nWhat to look for: {}\n\n## The owner's notes on this segment\n{}\n\n## Frames ({} contact sheets; read every one, in order, with the Read tool; black tiles at the very end are padding past the end of the video, ignore them)\n{}",
            i + 1,
            game_facts(&s.game),
            i + 1,
            shot.title,
            shot.goal,
            shot.steps.iter().map(|x| format!("- {x}")).collect::<Vec<_>>().join("\n"),
            shot.look_for.join("; "),
            if seg.notes.trim().is_empty() { "(none)" } else { seg.notes.trim() },
            files.len(),
            list.join("\n")
        );
        let text = claude(&prompt, &dir, "Read", files.len() as u32 + 8, "medium", Duration::from_secs(1800))?;
        let path = dir.join("analysis").join(format!("seg{:02}.md", i + 1));
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(&path, &text)?;
        analyses.push(format!("### Segment {}: {}\n{}", i + 1, shot.title, text));
    }
    if analyses.is_empty() {
        bail!("None of the segments could be read.");
    }
    step(id, "Writing the teardown");
    let prompt = format!(
        "{TEARDOWN_PROMPT}\n\n## The game (Roblox data)\n{}\n\n## The owner's final notes\n{}\n\n## Segment studies\n{}",
        game_facts(&s.game),
        if s.final_notes.trim().is_empty() { "(none)" } else { s.final_notes.trim() },
        analyses.join("\n\n")
    );
    let md = claude(&prompt, &dir, "", 3, "high", Duration::from_secs(1800))?;
    step(id, "Saving the screenshots");
    let md = attach_frames(&ff, &s, &dir, &md)?;
    let name: String = s.game.name.chars().filter(|c| !"\\/:*?\"<>|#^[]".contains(*c)).collect();
    let path = vault().join("Teardowns").join(format!("{}.md", name.trim()));
    std::fs::create_dir_all(path.parent().unwrap())?;
    let head = format!(
        "---\ngame: \"{}\"\nuniverse_id: {}\nplace_id: {}\nrecorded: {}\nsession: {}\n---\n\n",
        s.game.name.replace('"', "'"),
        s.game.universe_id,
        s.game.place_id,
        s.created,
        s.id
    );
    std::fs::write(&path, format!("{head}{md}"))?;
    // the analysis and its screenshots are saved: the videos aren't needed any more
    for g in &s.segments {
        if !g.file.is_empty() {
            let _ = std::fs::remove_file(dir.join(&g.file));
        }
    }
    let _ = std::fs::remove_dir_all(dir.join("frames"));
    update(id, |s| {
        s.status = "done".into();
        s.step.clear();
        s.teardown = path.to_string_lossy().into();
        s.videos_deleted = true;
    })?;
    roxai::info!("teardown written: {}", path.display());
    Ok(())
}

/// "1:24" or "84" → seconds.
fn parse_time(t: &str) -> Option<f64> {
    let t = t.trim();
    match t.split_once(':') {
        Some((m, s)) => Some(m.trim().parse::<f64>().ok()? * 60.0 + s.trim().parse::<f64>().ok()?),
        None => t.parse().ok(),
    }
}

fn clock(secs: f64) -> String {
    let s = secs.round() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Lines `SCREENSHOT: segNN @ m:ss | caption` → a full-size frame from that video, saved in the vault and embedded.
fn attach_frames(ff: &Path, s: &Session, dir: &Path, md: &str) -> Result<String> {
    let att = vault().join("Teardowns").join("attachments").join(&s.id);
    let mut out = vec![];
    for line in md.lines() {
        let t = line.trim().trim_start_matches(['-', '*', ' ']);
        let Some(rest) = t.strip_prefix("SCREENSHOT:").or_else(|| t.strip_prefix("KEYFRAME:")) else {
            out.push(line.to_string());
            continue;
        };
        // never leave a raw marker in the note, even when the frame can't be found
        let (what, caption) = rest.split_once('|').unwrap_or((rest, ""));
        let caption = caption.trim();
        let Some((seg, at)) = what.split_once('@') else { continue };
        let Ok(n) = seg.trim().to_lowercase().trim_start_matches("seg").parse::<usize>() else { continue };
        let Some(video) = s.segments.get(n.wrapping_sub(1)).filter(|g| !g.file.is_empty()).map(|g| dir.join(&g.file)) else { continue };
        let Some(secs) = parse_time(at) else { continue };
        std::fs::create_dir_all(&att)?;
        let name = format!("seg{n:02}-{:04}.jpg", secs as u64);
        let dest = att.join(&name);
        if !dest.is_file() {
            let _ = roxai::util::run_capture(
                &ff.to_string_lossy(),
                &["-hide_banner", "-loglevel", "error", "-y", "-ss", &format!("{secs:.2}"), "-i", &video.to_string_lossy(), "-frames:v", "1", "-q:v", "3", &dest.to_string_lossy()],
                Duration::from_secs(60),
            );
        }
        if dest.is_file() {
            out.push(format!("![{caption}](attachments/{}/{name})", s.id));
            if !caption.is_empty() {
                out.push(format!("*{caption}*"));
            }
            out.push(String::new());
        }
    }
    Ok(out.join("\n"))
}

const SEGMENT_PROMPT: &str = r#"You are a senior Roblox game designer studying one recorded segment of a real game, frame by frame, to learn exactly how it is made. Read EVERY contact sheet listed below (each holds 4 frames in time order: top-left, top-right, bottom-left, bottom-right). Then write what you learned, in English, as Markdown.

Be concrete and exact: name what is on screen, quote UI text and numbers, give times (m:ss), sizes and positions on screen, colours, counts and prices. Separate what you SAW from what you INFER (mark inferences "(likely)"). Use the owner's notes: they say what it felt like to play.

Cover whatever this segment shows of:
- What the player does, step by step, with times
- Core loop: action → reward → upgrade, with the numbers seen
- Progression and economy: currencies, costs, rates, multipliers, unlock requirements
- UI and UX: every screen and popup (layout, buttons, text, colours, where on screen, how you open/close it), onboarding and guidance, what is clear or confusing
- Level design: layout, scale, paths, landmarks, how the player is guided, locked areas and gates
- Art: style, models (what they are, how detailed), materials, lighting, colour palette, effects (VFX, UI animations, camera), sound cues heard as reflected on screen
- Monetization: every offer, price, where and when it appears, how hard it pushes
- Social: other players, leaderboards, trading, multiplayer features
- What works well and why; what is weak

End with "Notable frames": 4 to 10 lines `segNN @ m:ss | what it shows`, using the exact frame times listed for the sheets, for the moments that best illustrate this segment (every distinct UI screen, shop or offer, area of the map, model or effect worth seeing). Reply with the Markdown only."#;

const TEARDOWN_PROMPT: &str = r#"You are a senior Roblox game designer. Write the definitive teardown of this game from the segment studies below (made from frame-by-frame study of a real play session) plus the Roblox data and the owner's notes. It will be the source of truth for learning how to make games like it, so it must be specific, numeric and honest. Do not invent anything that the studies or data don't support; mark inferences "(likely)".

Write Markdown with exactly these sections:
# <Game name>
## Summary (3-5 sentences: what it is, who it's for, why it's popular)
## Scorecard (table: Hook, Clarity of first minutes, Core loop, Progression, UI/UX, Level design, Art and feel, Monetization, Retention, each 1-10 with one-line reason)
## The first 5 minutes (timeline with m:ss: what happens, what the player is told, first reward, first upgrade, when they understand the goal)
## Core loop (action → reward → spend → stronger, with the real numbers)
## Progression and economy (currencies, tables of costs and rates seen, multipliers, rebirth/prestige, gates, pacing)
## UI and UX (every screen: purpose, layout, key buttons and text; HUD layout; onboarding and guidance; what's clear and what's confusing)
## Level design and map (layout, areas, scale, paths, landmarks, how players are guided, gates)
## Art style, models and effects (style, palette, models and their detail, materials, lighting, VFX, juice and feedback)
## Monetization (every pass and product with price; where and when each is offered; how hard it pushes; what seems to sell)
## Social and retention (multiplayer, leaderboards, daily rewards, quests, events, reasons to come back)
## What makes it work
## Weaknesses
## Lessons for making our own games (8-15 concrete, reusable rules, each with the evidence from this game)

Illustrate it with screenshots: every section from "The first 5 minutes" to "Social and retention" gets at least one, placed right where it supports the text, 12 to 24 in total. Each on its own line, exactly: SCREENSHOT: segNN @ m:ss | caption (only moments listed in the studies' "Notable frames").
Reply with the Markdown only."#;

// ------------------------------------------------------------------ Claude

/// One headless Claude Code run in `workdir` with only the given built-in tools. Returns its final answer.
fn claude(prompt: &str, workdir: &Path, tools: &str, max_turns: u32, effort: &str, timeout: Duration) -> Result<String> {
    use std::io::{Read, Write};
    let c = ctx();
    let exe = roxai::autopilot::find_claude(&c).ok_or_else(|| anyhow!("Claude Code isn't installed on this PC."))?;
    let empty_mcp = home().join("mcp-none.json");
    std::fs::write(&empty_mcp, r#"{"mcpServers":{}}"#)?;
    let mut cmd = roxai::util::hidden_command(&exe);
    cmd.current_dir(workdir)
        .args(["-p", "--output-format", "json", "--max-turns", &max_turns.to_string(), "--tools", tools, "--strict-mcp-config", "--mcp-config"])
        .arg(&empty_mcp)
        .env("ENABLE_TOOL_SEARCH", "false");
    if !tools.is_empty() {
        cmd.args(["--allowedTools", tools]);
    }
    if !effort.is_empty() {
        cmd.args(["--effort", effort]);
    }
    let mut child = cmd.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().context("couldn't start Claude Code")?;
    if let Some(mut i) = child.stdin.take() {
        i.write_all(prompt.as_bytes())?;
    }
    let mut out_pipe = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = out_pipe.read_to_string(&mut s);
        s
    });
    let t = std::time::Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            break;
        }
        if t.elapsed() > timeout {
            let _ = child.kill();
            bail!("Claude took too long");
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    let raw = reader.join().unwrap_or_default();
    let low = raw.to_lowercase();
    if ["hit your limit", "usage limit", "session limit", "weekly limit"].iter().any(|k| low.contains(k)) {
        bail!("Claude's usage limit was reached. Push again after it resets.");
    }
    if ["not logged in", "please run /login", "oauth", "authentication_error"].iter().any(|k| low.contains(k)) && !low.contains("\"is_error\":false") {
        bail!("Claude needs you to sign in again (run `claude` in a terminal once).");
    }
    let v: Value = raw.trim().lines().last().and_then(|l| serde_json::from_str(l).ok()).ok_or_else(|| anyhow!("Claude gave no answer: {}", raw.chars().take(300).collect::<String>()))?;
    if v["is_error"].as_bool() == Some(true) {
        bail!("Claude failed: {}", v["result"].as_str().unwrap_or("unknown error"));
    }
    Ok(v["result"].as_str().unwrap_or("").trim().to_string())
}

// ------------------------------------------------------------------ UI commands

pub fn call(cmd: &str, a: Value) -> Result<Value> {
    let s = |k: &str| a.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let n = |k: &str| a.get(k).and_then(|v| v.as_u64());
    Ok(match cmd {
        "status" => json!({
            "ffmpeg": ffmpeg_path().is_some(),
            "claude": roxai::autopilot::find_claude(&ctx()).is_some(),
            "roblox": crate::rec::roblox_window().is_some(),
            "recording": recording_state(),
            "vault": vault(),
        }),
        "search" => search(&s("query"))?,
        "session_new" => serde_json::to_value(new_session(n("universe_id").ok_or_else(|| anyhow!("pick a game"))?)?)?,
        "sessions" => json!(list_sessions()
            .iter()
            .map(|x| json!({"id": x.id, "created": x.created, "game": x.game.name, "icon": x.game.icon, "status": x.status, "step": x.step,
                "segments": x.plan.len(), "recorded": x.segments.iter().filter(|g| !g.file.is_empty()).count(), "teardown": x.teardown}))
            .collect::<Vec<_>>()),
        "session" => {
            let x = load(&s("id"))?;
            let dir = dir_of(&x.id);
            json!({"session": x, "dir": dir})
        }
        "session_delete" => {
            let id = s("id");
            if id.is_empty() || id.contains("..") || id.contains('/') || id.contains('\\') {
                bail!("bad id");
            }
            std::fs::remove_dir_all(dir_of(&id))?;
            json!({"ok": true})
        }
        "notes" => {
            let i = n("index").unwrap_or(0) as usize;
            let text = s("notes");
            update(&s("id"), |x| {
                if let Some(g) = x.segments.get_mut(i) {
                    g.notes = text;
                }
            })?;
            json!({"ok": true})
        }
        "final_notes" => {
            let text = s("notes");
            update(&s("id"), |x| x.final_notes = text)?;
            json!({"ok": true})
        }
        "goto" => {
            let i = n("index").unwrap_or(0) as usize;
            update(&s("id"), |x| {
                x.current = i.min(x.plan.len());
                if x.status != "recording" && x.status != "analyzing" {
                    x.status = if x.current >= x.plan.len() { "recorded".into() } else { "ready".into() };
                }
            })?;
            json!({"ok": true})
        }
        "skip" => {
            let i = n("index").unwrap_or(0) as usize;
            update(&s("id"), |x| {
                if let Some(g) = x.segments.get_mut(i) {
                    g.skipped = true;
                }
                x.current = (i + 1).min(x.plan.len());
                x.status = if x.current >= x.plan.len() { "recorded".into() } else { "ready".into() };
            })?;
            json!({"ok": true})
        }
        "rec_start" => rec_start(&s("id"), n("index").unwrap_or(0) as usize)?,
        "rec_stop" => rec_stop(&s("id"), &s("notes"))?,
        // check: record one segment for a few seconds in one go
        "rec_test" => {
            let (id, i) = (s("id"), n("index").unwrap_or(0) as usize);
            rec_start(&id, i)?;
            std::thread::sleep(Duration::from_secs(n("seconds").unwrap_or(5)));
            rec_stop(&id, &s("notes"))?
        }
        "recording" => recording_state(),
        "push" => push(&s("id"), &s("notes"))?,
        // check: turn SCREENSHOT lines into saved frames for a session whose videos still exist
        "screenshots" => {
            let x = load(&s("id"))?;
            json!({"markdown": attach_frames(&ensure_ffmpeg()?, &x, &dir_of(&x.id), &s("markdown"))?})
        }
        "teardown" => {
            let x = load(&s("id"))?;
            json!({"markdown": std::fs::read_to_string(&x.teardown).unwrap_or_default(), "path": x.teardown})
        }
        "ffmpeg_install" => json!({"path": ensure_ffmpeg()?}),
        "open" => {
            let p = match s("what").as_str() {
                "vault" => vault(),
                "session" => dir_of(&s("id")),
                "file" => {
                    let id = s("id");
                    dir_of(&id).join(s("file").replace("..", ""))
                }
                "teardown" => PathBuf::from(load(&s("id"))?.teardown),
                w => bail!("unknown target {w}"),
            };
            roxai::util::open_path(&p.to_string_lossy());
            json!({"ok": true})
        }
        "launch" => {
            let place = n("place_id").ok_or_else(|| anyhow!("place_id required"))?;
            roxai::util::open_path(&format!("roblox://experiences/start?placeId={place}"));
            json!({"ok": true})
        }
        c => bail!("unknown command {c}"),
    })
}
