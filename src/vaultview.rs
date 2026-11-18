//! In-app vault viewer: a small Markdown renderer for ROX-AI's notes (headings, paragraphs, lists,
//! tables, callouts/quotes, code) with clickable [[wikilinks]] and web links.

use eframe::egui::{self, Color32, RichText};

pub const ACC: Color32 = Color32::from_rgb(0, 214, 168);
const MUTED: Color32 = Color32::from_rgb(140, 148, 168);
const CARD: Color32 = Color32::from_rgb(30, 34, 48);
const CALLOUT: Color32 = Color32::from_rgb(34, 40, 58);
const AMBER: Color32 = Color32::from_rgb(255, 196, 82);

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Heading(u8, String),
    Para(String),
    Bullet(String, usize),
    Quote(Option<String>, Vec<String>),
    Table(Vec<Vec<String>>),
    Code(String, String),
    Image(String),
}

/// Characters the app font can't draw (emoji) are dropped for a clean look.
pub fn clean(s: &str) -> String {
    s.chars()
        .filter(|c| {
            let u = *c as u32;
            !((0x1F000..=0x1FAFF).contains(&u)
                || (0x25A0..=0x25FF).contains(&u)
                || (0x2600..=0x27BF).contains(&u)
                || u == 0xFE0F
                || u == 0x200D
                || (0x2B00..=0x2BFF).contains(&u))
        })
        .collect::<String>()
        .trim()
        .to_string()
}

fn strip_html(s: &str) -> String {
    let rx = regex::Regex::new(r"</?(sub|sup|details|summary|br|b|i|span)[^>]*>").unwrap();
    rx.replace_all(s, "").to_string()
}

pub fn parse(md: &str) -> (Vec<(String, String)>, Vec<Block>) {
    let (fm, body) = crate::vault::split_frontmatter(md);
    let fm: Vec<(String, String)> = fm
        .into_iter()
        .map(|(k, v)| {
            (
                k,
                v.replace("\n  - ", ", ")
                    .trim_start_matches(", ")
                    .trim_matches('"')
                    .to_string(),
            )
        })
        .collect();
    let mut blocks = vec![];
    let lines: Vec<&str> = body.lines().collect();
    let mut i = 0;
    let mut para: Vec<String> = vec![];
    let flush = |para: &mut Vec<String>, blocks: &mut Vec<Block>| {
        if !para.is_empty() {
            blocks.push(Block::Para(para.join(" ")));
            para.clear();
        }
    };
    while i < lines.len() {
        let raw = lines[i];
        let l = raw.trim();
        if l.starts_with("<!--") || l == "<details>" || l == "</details>" {
            i += 1;
            continue;
        }
        if l.starts_with("```") {
            flush(&mut para, &mut blocks);
            let lang = l.trim_start_matches('`').to_string();
            let mut code = vec![];
            i += 1;
            while i < lines.len() && !lines[i].trim().starts_with("```") {
                code.push(lines[i]);
                i += 1;
            }
            blocks.push(Block::Code(lang, code.join("\n")));
            i += 1;
            continue;
        }
        if l.is_empty() {
            flush(&mut para, &mut blocks);
            i += 1;
            continue;
        }
        if let Some(h) = l.strip_prefix('#') {
            flush(&mut para, &mut blocks);
            let level = 1 + h.chars().take_while(|c| *c == '#').count() as u8;
            blocks.push(Block::Heading(level, clean(h.trim_start_matches('#'))));
            i += 1;
            continue;
        }
        if l.starts_with("![[") || l.starts_with("![") {
            flush(&mut para, &mut blocks);
            blocks.push(Block::Image(l.to_string()));
            i += 1;
            continue;
        }
        if l.starts_with('|') {
            flush(&mut para, &mut blocks);
            let mut rows = vec![];
            while i < lines.len() && lines[i].trim().starts_with('|') {
                let row = lines[i].trim();
                if !row.replace(['|', '-', ':', ' '], "").is_empty() {
                    rows.push(split_row(row));
                }
                i += 1;
            }
            blocks.push(Block::Table(rows));
            continue;
        }
        if l.starts_with('>') {
            flush(&mut para, &mut blocks);
            let mut kind = None;
            let mut q = vec![];
            while i < lines.len() && lines[i].trim().starts_with('>') {
                let t = lines[i].trim().trim_start_matches('>').trim();
                if let Some(rest) = t.strip_prefix("[!") {
                    let (k, title) = rest.split_once(']').unwrap_or((rest, ""));
                    kind = Some(k.to_lowercase());
                    if !title.trim().is_empty() {
                        q.push(format!("**{}**", title.trim()));
                    }
                } else if !t.is_empty() {
                    q.push(strip_html(t));
                }
                i += 1;
            }
            blocks.push(Block::Quote(kind, q));
            continue;
        }
        let indent = raw.len() - raw.trim_start().len();
        let bullet = l
            .strip_prefix("- ")
            .or_else(|| l.strip_prefix("* "))
            .map(String::from)
            .or_else(|| {
                let d: String = l.chars().take_while(|c| c.is_ascii_digit()).collect();
                (!d.is_empty() && l[d.len()..].starts_with(". "))
                    .then(|| format!("{d}. {}", &l[d.len() + 2..]))
            });
        if let Some(b) = bullet {
            flush(&mut para, &mut blocks);
            if !b.trim().is_empty() {
                blocks.push(Block::Bullet(strip_html(&b), indent / 2));
            }
            i += 1;
            continue;
        }
        para.push(strip_html(l));
        i += 1;
    }
    flush(&mut para, &mut blocks);
    (fm, blocks)
}

fn split_row(row: &str) -> Vec<String> {
    // split on | that isn't escaped (\|)
    let inner = row.trim().trim_start_matches('|').trim_end_matches('|');
    let mut cells = vec![];
    let mut cur = String::new();
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&'|') {
            cur.push('|');
            chars.next();
        } else if c == '|' {
            cells.push(cur.trim().to_string());
            cur.clear();
        } else {
            cur.push(c);
        }
    }
    cells.push(cur.trim().to_string());
    cells
}

#[derive(Debug, PartialEq)]
enum Tok {
    Text(String, bool, bool), // text, bold, code
    Wiki(String, String),     // target, label
    Url(String, String),      // url, label
}

fn tokens(s: &str) -> Vec<Tok> {
    let rx = regex::Regex::new(r"\[\[([^\]|]+)(?:\|([^\]]+))?\]\]|\[([^\]]+)\]\((https?://[^)\s]+)\)|\*\*([^*]+)\*\*|`([^`]+)`").unwrap();
    let mut out = vec![];
    let mut last = 0;
    for c in rx.captures_iter(s) {
        let m = c.get(0).unwrap();
        if m.start() > last {
            out.push(Tok::Text(s[last..m.start()].to_string(), false, false));
        }
        if let Some(t) = c.get(1) {
            let target = t.as_str().to_string();
            let label = c
                .get(2)
                .map(|x| x.as_str().to_string())
                .unwrap_or_else(|| target.rsplit('/').next().unwrap_or(&target).to_string());
            out.push(Tok::Wiki(target, label));
        } else if let Some(l) = c.get(3) {
            out.push(Tok::Url(c[4].to_string(), l.as_str().to_string()));
        } else if let Some(b) = c.get(5) {
            out.push(Tok::Text(b.as_str().to_string(), true, false));
        } else if let Some(k) = c.get(6) {
            out.push(Tok::Text(k.as_str().to_string(), false, true));
        }
        last = m.end();
    }
    if last < s.len() {
        out.push(Tok::Text(s[last..].to_string(), false, false));
    }
    out
}

/// Render inline markdown; returns a wikilink target if one was clicked.
pub fn inline(ui: &mut egui::Ui, s: &str, size: f32, color: Color32) -> Option<String> {
    inline_impl(ui, s, size, color, true)
}

/// Table cells: one line, never wrapped letter by letter.
pub fn inline_cell(ui: &mut egui::Ui, s: &str, size: f32, color: Color32) -> Option<String> {
    inline_impl(ui, s, size, color, false)
}

fn inline_impl(
    ui: &mut egui::Ui,
    s: &str,
    size: f32,
    color: Color32,
    wrap: bool,
) -> Option<String> {
    let mut nav = None;
    let mut body = |ui: &mut egui::Ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for t in tokens(&clean(s)) {
            match t {
                Tok::Text(text, bold, code) => {
                    for word in text.split_inclusive(' ') {
                        let mut rt = RichText::new(word).size(size).color(color);
                        if bold {
                            rt = rt.strong().color(Color32::WHITE);
                        }
                        if code {
                            rt = rt.code();
                        }
                        ui.label(rt);
                    }
                }
                Tok::Wiki(target, label) => {
                    if ui
                        .link(RichText::new(clean(&label)).size(size).color(ACC))
                        .clicked()
                    {
                        nav = Some(target);
                    }
                    ui.label(" ");
                }
                Tok::Url(url, label) => {
                    ui.hyperlink_to(RichText::new(clean(&label)).size(size), url);
                    ui.label(" ");
                }
            }
        }
    };
    if wrap {
        ui.horizontal_wrapped(|ui| body(ui));
    } else {
        ui.horizontal(|ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            body(ui)
        });
    }
    nav
}

/// Render a whole note. Returns the wikilink target the user clicked, if any.
pub fn render(ui: &mut egui::Ui, blocks: &[Block], fm: &[(String, String)]) -> Option<String> {
    let mut nav = None;
    let keys = [
        "chart_rank",
        "playing",
        "like_ratio",
        "momentum",
        "genre",
        "analysis_source",
        "last_analysis",
        "one_liner",
    ];
    let props: Vec<&(String, String)> = fm
        .iter()
        .filter(|(k, v)| keys.contains(&k.as_str()) && !v.is_empty())
        .collect();
    if !props.is_empty() {
        egui::Frame::new()
            .fill(CARD)
            .corner_radius(8.0)
            .inner_margin(10.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    for (k, v) in props {
                        ui.label(
                            RichText::new(format!("{}: ", k.replace('_', " ")))
                                .color(MUTED)
                                .small(),
                        );
                        ui.label(RichText::new(clean(v)).small().strong());
                        ui.add_space(10.0);
                    }
                });
            });
        ui.add_space(8.0);
    }
    for b in blocks {
        match b {
            Block::Heading(l, t) => {
                ui.add_space(if *l <= 2 { 12.0 } else { 6.0 });
                let size = match l {
                    1 => 24.0,
                    2 => 19.0,
                    _ => 16.0,
                };
                ui.label(RichText::new(t).size(size).strong().color(if *l == 2 {
                    ACC
                } else {
                    Color32::WHITE
                }));
            }
            Block::Para(t) => {
                if let Some(n) = inline(ui, t, 14.0, Color32::from_gray(220)) {
                    nav = Some(n);
                }
            }
            Block::Bullet(t, indent) => {
                ui.horizontal(|ui| {
                    ui.add_space(8.0 + *indent as f32 * 16.0);
                    ui.label(RichText::new("•").color(ACC));
                    if let Some(n) = inline(ui, t, 14.0, Color32::from_gray(220)) {
                        nav = Some(n);
                    }
                });
            }
            Block::Quote(kind, lines) => {
                let accent = match kind.as_deref() {
                    Some("todo") => AMBER,
                    _ => ACC,
                };
                egui::Frame::new()
                    .fill(CALLOUT)
                    .corner_radius(8.0)
                    .inner_margin(10.0)
                    .stroke(egui::Stroke::new(1.0, accent.gamma_multiply(0.5)))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        if kind.as_deref() == Some("todo") {
                            ui.label(RichText::new("Not analyzed yet").color(AMBER).strong());
                        }
                        for l in lines {
                            if let Some(n) = inline(ui, l, 13.0, MUTED) {
                                nav = Some(n);
                            }
                        }
                    });
            }
            Block::Table(rows) => {
                egui::ScrollArea::horizontal()
                    .id_salt(
                        rows.len() as u64
                            ^ rows.first().map(|r| r.len() as u64 * 7919).unwrap_or(0)
                            ^ (ui.next_auto_id().value()),
                    )
                    .show(ui, |ui| {
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                        egui::Grid::new(ui.next_auto_id())
                            .striped(true)
                            .spacing([14.0, 6.0])
                            .show(ui, |ui| {
                                for (ri, row) in rows.iter().enumerate() {
                                    for cell in row {
                                        if ri == 0 {
                                            ui.label(
                                                RichText::new(clean(cell)).strong().color(MUTED),
                                            );
                                        } else if let Some(n) =
                                            inline_cell(ui, cell, 13.0, Color32::from_gray(225))
                                        {
                                            nav = Some(n);
                                        }
                                    }
                                    ui.end_row();
                                }
                            });
                    });
            }
            Block::Code(lang, code) => {
                if lang == "dataview" {
                    ui.label(
                        RichText::new("Live table — open this note in Obsidian to see it.")
                            .color(MUTED)
                            .italics()
                            .small(),
                    );
                } else {
                    egui::Frame::new()
                        .fill(CARD)
                        .corner_radius(6.0)
                        .inner_margin(8.0)
                        .show(ui, |ui| {
                            ui.label(RichText::new(code).monospace().small());
                        });
                }
            }
            Block::Image(_) => {}
        }
        ui.add_space(4.0);
    }
    nav
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_game_note() {
        let md = "---\nname: \"X\"\nchart_rank: 3\ntags:\n  - \"a\"\n  - \"b\"\n---\n# 🎮 Title\n> [!info] Head\n> line one\n\n<!-- roxai:auto:x -->\n## 📊 Snapshot\nSome **bold** text with [[02 Games/Foo|Foo]] and [yt](https://youtu.be/abc).\n\n| A | B |\n|---|---|\n| 1 | [[02 Games/Bar\\|Bar]] |\n\n- item one\n  - nested\n1. first\n```dataview\nTABLE x\n```\n";
        let (fm, b) = parse(md);
        assert!(fm.iter().any(|(k, v)| k == "chart_rank" && v == "3"));
        assert!(fm
            .iter()
            .any(|(k, v)| k == "tags" && v == "\"a\", \"b\"" || k == "tags" && v.contains('a')));
        assert_eq!(b[0], Block::Heading(1, "Title".into()));
        assert!(matches!(&b[1], Block::Quote(Some(k), l) if k == "info" && l.len() == 2));
        assert_eq!(b[2], Block::Heading(2, "Snapshot".into()));
        assert!(
            matches!(&b[4], Block::Table(r) if r.len() == 2 && r[1][1] == "[[02 Games/Bar|Bar]]")
        );
        assert!(matches!(&b[5], Block::Bullet(t, 0) if t == "item one"));
        assert!(matches!(&b[6], Block::Bullet(t, 1) if t == "nested"));
        assert!(matches!(&b[7], Block::Bullet(t, _) if t == "1. first"));
        assert!(matches!(&b[8], Block::Code(l, _) if l == "dataview"));
        let toks = tokens("go [[02 Games/Foo|Foo]] now **x** `y` [a](https://b.c)");
        assert_eq!(toks[1], Tok::Wiki("02 Games/Foo".into(), "Foo".into()));
        assert_eq!(toks[3], Tok::Text("x".into(), true, false));
        assert_eq!(toks[5], Tok::Text("y".into(), false, true));
        assert_eq!(toks[7], Tok::Url("https://b.c".into(), "a".into()));
        assert_eq!(clean("📊 Snapshot ✅"), "Snapshot");
    }
}
