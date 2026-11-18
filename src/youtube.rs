//! Learn games from YouTube instead of playing them (doesn't touch your screen).
//!
//! * `search`  — YouTube's public results page (what a browser loads), parsed from `ytInitialData`.
//! * `video`   — the public watch page: title, channel, length, description, chapters and the
//!               video's caption track (transcript) when YouTube serves it to a normal page load.
//! * Optional "video reader": the open-source `yt-dlp` tool kept inside ROX-AI's folder, used only to
//!   read captions when the page doesn't expose them. Videos are never downloaded.

use anyhow::{anyhow, bail, Context, Result};
use regex::Regex;
use serde::Serialize;
use serde_json::Value;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0 Safari/537.36";
const MAX_TRANSCRIPT: usize = 24_000;

#[derive(Clone, Debug, Default, Serialize)]
pub struct Video {
    pub id: String,
    pub title: String,
    pub channel: String,
    pub length: String,
    pub views: String,
    pub published: String,
    pub url: String,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct VideoInfo {
    pub id: String,
    pub url: String,
    pub title: String,
    pub channel: String,
    pub length_seconds: u64,
    pub views: u64,
    pub description: String,
    pub chapters: Vec<String>,
    pub transcript: Option<String>,
    pub transcript_source: String,
    pub note: String,
}

fn agent() -> Result<ureq::Agent> {
    Ok(ureq::AgentBuilder::new()
        .tls_connector(Arc::new(native_tls::TlsConnector::new()?))
        .try_proxy_from_env(true)
        .timeout(Duration::from_secs(30))
        .user_agent(UA)
        .build())
}

fn get_text(url: &str) -> Result<String> {
    let r = agent()?
        .get(url)
        .set("Accept-Language", "en-US,en;q=0.9")
        .set("Cookie", "CONSENT=YES+1; SOCS=CAI")
        .call()
        .map_err(|e| anyhow!("YouTube request failed: {e}"))?;
    if r.get_url().contains("google.com/sorry") {
        bail!("YouTube is rate-limiting this network right now (try again later)");
    }
    let mut s = String::new();
    r.into_reader().take(8_000_000).read_to_string(&mut s)?;
    Ok(s)
}

/// Extract a `var NAME = {...};` JSON blob from a YouTube page.
pub fn extract_json(html: &str, var: &str) -> Option<Value> {
    let start = html.find(&format!("{var} = "))? + var.len() + 3;
    let bytes = html.as_bytes();
    if bytes.get(start) != Some(&b'{') {
        return None;
    }
    // walk braces, respecting strings
    let (mut depth, mut in_str, mut esc) = (0i32, false, false);
    for (i, &c) in bytes[start..].iter().enumerate() {
        if in_str {
            match (esc, c) {
                (true, _) => esc = false,
                (false, b'\\') => esc = true,
                (false, b'"') => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return serde_json::from_str(&html[start..start + i + 1]).ok();
                }
            }
            _ => {}
        }
    }
    None
}

fn text(v: &Value) -> String {
    if let Some(s) = v.get("simpleText").and_then(|x| x.as_str()) {
        return s.to_string();
    }
    v.get("runs")
        .and_then(|r| r.as_array())
        .map(|r| {
            r.iter()
                .filter_map(|x| x["text"].as_str())
                .collect::<String>()
        })
        .unwrap_or_default()
}

pub fn parse_search(html: &str, limit: usize) -> Vec<Video> {
    let Some(d) = extract_json(html, "var ytInitialData") else {
        return vec![];
    };
    let mut out = vec![];
    fn walk(v: &Value, out: &mut Vec<Video>, limit: usize) {
        if out.len() >= limit {
            return;
        }
        match v {
            Value::Object(m) => {
                if let Some(vr) = m.get("videoRenderer") {
                    if let Some(id) = vr["videoId"].as_str() {
                        if !out.iter().any(|x: &Video| x.id == id) {
                            out.push(Video {
                                id: id.into(),
                                title: text(&vr["title"]),
                                channel: text(&vr["ownerText"]),
                                length: text(&vr["lengthText"]),
                                views: text(&vr["viewCountText"]),
                                published: text(&vr["publishedTimeText"]),
                                url: format!("https://www.youtube.com/watch?v={id}"),
                            });
                        }
                    }
                    return;
                }
                for x in m.values() {
                    walk(x, out, limit);
                }
            }
            Value::Array(a) => {
                for x in a {
                    walk(x, out, limit);
                }
            }
            _ => {}
        }
    }
    walk(&d, &mut out, limit);
    out
}

/// Search YouTube (videos only).
pub fn search(query: &str, limit: usize) -> Result<Vec<Video>> {
    let q: String = query
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() {
                (b as char).to_string()
            } else if b == b' ' {
                "+".into()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    // YouTube occasionally serves a different page variant: retry, then try without the "videos only" filter
    let mut last = anyhow!("no videos found (or YouTube changed its page)");
    for (i, url) in [
        format!("https://www.youtube.com/results?search_query={q}&sp=EgIQAQ%253D%253D"),
        format!("https://www.youtube.com/results?search_query={q}&sp=EgIQAQ%253D%253D"),
        format!("https://www.youtube.com/results?search_query={q}"),
    ]
    .iter()
    .enumerate()
    {
        if i > 0 {
            std::thread::sleep(Duration::from_millis(800));
        }
        match get_text(url) {
            Ok(html) => {
                let v = parse_search(&html, limit.clamp(1, 30));
                if !v.is_empty() {
                    return Ok(v);
                }
            }
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// "12:34 Topic" lines in a description → chapters.
pub fn chapters(description: &str) -> Vec<String> {
    let rx = Regex::new(r"(?m)^\s*\(?(\d{1,2}:\d{2}(?::\d{2})?)\)?\s*[-–—:]?\s*(.+)$").unwrap();
    rx.captures_iter(description)
        .map(|c| format!("{} {}", &c[1], c[2].trim()))
        .take(40)
        .collect()
}

fn clock(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// Caption formats → plain text with a [mm:ss] marker roughly every 30 s.
pub fn parse_captions(body: &str) -> Option<String> {
    let body = body.trim();
    if body.is_empty() {
        return None;
    }
    let mut parts: Vec<(u64, String)> = vec![];
    if body.starts_with('{') {
        let v: Value = serde_json::from_str(body).ok()?;
        for e in v["events"].as_array()? {
            let t = e["tStartMs"].as_u64().unwrap_or(0);
            let s: String = e["segs"]
                .as_array()
                .map(|a| a.iter().filter_map(|x| x["utf8"].as_str()).collect())
                .unwrap_or_default();
            if !s.trim().is_empty() {
                parts.push((t, s));
            }
        }
    } else {
        // XML: <text start="1.2" dur=".."> or <p t="1200" d="..">
        let rx = Regex::new(r#"(?s)<(?:text|p)\b([^>]*)>(.*?)</(?:text|p)>"#).unwrap();
        let st = Regex::new(r#"start="([\d.]+)"|\bt="(\d+)""#).unwrap();
        let tags = Regex::new(r"<[^>]+>").unwrap();
        for c in rx.captures_iter(body) {
            let t = st
                .captures(&c[1])
                .map(|m| {
                    m.get(1)
                        .map(|x| (x.as_str().parse::<f64>().unwrap_or(0.0) * 1000.0) as u64)
                        .or_else(|| m.get(2).and_then(|x| x.as_str().parse().ok()))
                        .unwrap_or(0)
                })
                .unwrap_or(0);
            let s = tags.replace_all(&c[2], "");
            let s = s
                .replace("&amp;", "&")
                .replace("&#39;", "'")
                .replace("&quot;", "\"")
                .replace("&lt;", "<")
                .replace("&gt;", ">");
            if !s.trim().is_empty() {
                parts.push((t, s));
            }
        }
    }
    if parts.is_empty() {
        return None;
    }
    let mut out = String::new();
    let mut last_mark: Option<u64> = None;
    for (t, s) in parts {
        if last_mark.is_none_or(|m| t >= m + 30_000) {
            out.push_str(&format!("\n[{}] ", clock(t)));
            last_mark = Some(t);
        }
        out.push_str(s.replace('\n', " ").trim());
        out.push(' ');
    }
    let out = out.trim().to_string();
    Some(if out.len() > MAX_TRANSCRIPT {
        let cut = (0..=MAX_TRANSCRIPT)
            .rev()
            .find(|i| out.is_char_boundary(*i))
            .unwrap_or(0);
        format!("{}\n…(transcript truncated)", &out[..cut])
    } else {
        out
    })
}

pub fn video_id(s: &str) -> String {
    let rx = Regex::new(r"(?:v=|youtu\.be/|shorts/|embed/)([A-Za-z0-9_-]{11})").unwrap();
    rx.captures(s)
        .map(|c| c[1].to_string())
        .unwrap_or_else(|| s.trim().to_string())
}

/// Read one video: details + transcript (built-in first, then the optional video reader).
pub fn video(id_or_url: &str, tools_dir: &Path) -> Result<VideoInfo> {
    let id = video_id(id_or_url);
    if !Regex::new(r"^[A-Za-z0-9_-]{11}$").unwrap().is_match(&id) {
        bail!("not a YouTube video id: {id_or_url}");
    }
    let url = format!("https://www.youtube.com/watch?v={id}");
    let mut info = VideoInfo {
        id: id.clone(),
        url: url.clone(),
        ..Default::default()
    };
    let mut tracks: Vec<Value> = vec![];
    match get_text(&format!("{url}&hl=en")) {
        Ok(html) => {
            if let Some(p) = extract_json(&html, "var ytInitialPlayerResponse") {
                let d = &p["videoDetails"];
                info.title = d["title"].as_str().unwrap_or("").into();
                info.channel = d["author"].as_str().unwrap_or("").into();
                info.length_seconds = d["lengthSeconds"]
                    .as_str()
                    .and_then(|x| x.parse().ok())
                    .unwrap_or(0);
                info.views = d["viewCount"]
                    .as_str()
                    .and_then(|x| x.parse().ok())
                    .unwrap_or(0);
                info.description = d["shortDescription"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .take(5000)
                    .collect();
                info.chapters = chapters(&info.description);
                tracks = p["captions"]["playerCaptionsTracklistRenderer"]["captionTracks"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
            }
        }
        Err(e) => info.note = e.to_string(),
    }
    // prefer human English captions, then auto-generated English, then anything
    tracks.sort_by_key(|t| {
        let en = t["languageCode"].as_str().unwrap_or("").starts_with("en");
        let asr = t["kind"].as_str() == Some("asr");
        match (en, asr) {
            (true, false) => 0,
            (true, true) => 1,
            _ => 2,
        }
    });
    for t in tracks.iter().take(2) {
        if let Some(base) = t["baseUrl"].as_str() {
            for u in [format!("{base}&fmt=json3"), base.to_string()] {
                if let Some(tx) = get_text(&u).ok().and_then(|b| parse_captions(&b)) {
                    info.transcript = Some(tx);
                    info.transcript_source = "captions".into();
                    return Ok(info);
                }
            }
        }
    }
    if let Some(ytdlp) = ytdlp_path(tools_dir) {
        match ytdlp_captions(&ytdlp, &url, tools_dir) {
            Ok(Some(tx)) => {
                info.transcript = Some(tx);
                info.transcript_source = "video reader".into();
            }
            Ok(None) => info.note = "this video has no captions".into(),
            Err(e) => info.note = format!("video reader: {e}"),
        }
    } else if info.note.is_empty() {
        info.note = "no transcript from the page (install the Video reader in ROX-AI → Setup → Apps for better results)".into();
    }
    if info.title.is_empty() && info.transcript.is_none() {
        bail!("couldn't read this video: {}", info.note);
    }
    Ok(info)
}

pub fn thumbnail(id: &str) -> Option<Vec<u8>> {
    let r = agent()
        .ok()?
        .get(&format!(
            "https://i.ytimg.com/vi/{}/hqdefault.jpg",
            video_id(id)
        ))
        .call()
        .ok()?;
    let mut b = vec![];
    r.into_reader().take(2_000_000).read_to_end(&mut b).ok()?;
    (b.len() > 1000).then_some(b)
}

// ------------------------------------------------------- video reader ---

pub fn ytdlp_path(tools_dir: &Path) -> Option<PathBuf> {
    let p = tools_dir.join(if cfg!(windows) {
        "yt-dlp.exe"
    } else {
        "yt-dlp"
    });
    p.is_file().then_some(p)
}

/// Read captions only (never the video) via yt-dlp into a temp folder.
fn ytdlp_captions(exe: &Path, url: &str, tools_dir: &Path) -> Result<Option<String>> {
    let tmp = tools_dir.join("subs-tmp");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)?;
    let out = tmp.join("%(id)s.%(ext)s").to_string_lossy().into_owned();
    let (code, log) = crate::util::run_capture(
        &exe.to_string_lossy(),
        &[
            "--skip-download",
            "--write-subs",
            "--write-auto-subs",
            "--sub-langs",
            "en.*,en",
            "--sub-format",
            "json3",
            "--no-warnings",
            "--no-progress",
            "-o",
            &out,
            url,
        ],
        Duration::from_secs(120),
    )?;
    let file = std::fs::read_dir(&tmp)?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "json3"));
    let res = match file {
        Some(f) => Ok(parse_captions(&std::fs::read_to_string(f)?)),
        None if code == 0 => Ok(None),
        None => Err(anyhow!("{}", log.lines().last().unwrap_or("failed"))),
    };
    let _ = std::fs::remove_dir_all(&tmp);
    res
}

/// Download yt-dlp (the official release) into ROX-AI's tools folder.
pub fn install_ytdlp(tools_dir: &Path, report: crate::deps::Report) -> Result<PathBuf> {
    std::fs::create_dir_all(tools_dir)?;
    let name = if cfg!(windows) {
        "yt-dlp.exe"
    } else {
        "yt-dlp"
    };
    report("Downloading the video reader…", Some(0.0));
    let resp = agent()?
        .get(&format!(
            "https://github.com/yt-dlp/yt-dlp/releases/latest/download/{name}"
        ))
        .call()
        .map_err(|e| anyhow!("download failed: {e}"))?;
    let total: Option<u64> = resp.header("content-length").and_then(|v| v.parse().ok());
    let mut rd = resp.into_reader();
    let mut data = vec![];
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = rd.read(&mut buf)?;
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n]);
        if let Some(t) = total {
            report(
                "Downloading the video reader…",
                Some(data.len() as f32 / t as f32),
            );
        }
    }
    if data.len() < 1_000_000 {
        bail!("download was incomplete");
    }
    let p = tools_dir.join(name);
    std::fs::write(&p, data).context("can't save the video reader")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755));
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_search_page() {
        let html = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/yt_search.html"
        ))
        .unwrap();
        let v = parse_search(&html, 10);
        assert!(v.len() >= 3, "{v:?}");
        assert_eq!(v[0].id.len(), 11);
        assert!(!v[0].title.is_empty() && !v[0].channel.is_empty() && v[0].length.contains(':'));
        assert!(v[0].url.ends_with(&v[0].id));
    }

    #[test]
    fn captions_json3_and_xml() {
        let j = r#"{"events":[{"tStartMs":0,"segs":[{"utf8":"hello "},{"utf8":"world"}]},{"tStartMs":31000,"segs":[{"utf8":"buy the x2 pass"}]}]}"#;
        let t = parse_captions(j).unwrap();
        assert!(
            t.contains("[0:00] hello world") && t.contains("[0:31] buy the x2 pass"),
            "{t}"
        );
        let x = r#"<transcript><text start="0.5" dur="2">first &amp; best</text><text start="65" dur="2">rebirth at 1M</text></transcript>"#;
        let t = parse_captions(x).unwrap();
        assert!(
            t.contains("[0:00] first & best") && t.contains("[1:05] rebirth at 1M"),
            "{t}"
        );
        assert!(parse_captions("").is_none());
    }

    #[test]
    fn ids_and_chapters() {
        assert_eq!(
            video_id("https://www.youtube.com/watch?v=efs6pVYxkCA&t=3"),
            "efs6pVYxkCA"
        );
        assert_eq!(video_id("https://youtu.be/efs6pVYxkCA"), "efs6pVYxkCA");
        assert_eq!(video_id("efs6pVYxkCA"), "efs6pVYxkCA");
        let c = chapters("Intro\n0:00 Start\n1:23 - First rebirth\n12:05:00 Ending");
        assert_eq!(
            c,
            vec!["0:00 Start", "1:23 First rebirth", "12:05:00 Ending"]
        );
    }

    #[test]
    fn json_extraction_handles_braces_in_strings() {
        let html =
            r#"<script>var ytInitialPlayerResponse = {"a":"x}y{","b":{"c":1}};var z=1</script>"#;
        let v = extract_json(html, "var ytInitialPlayerResponse").unwrap();
        assert_eq!(v["b"]["c"], 1);
    }
}
