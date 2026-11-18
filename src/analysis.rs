//! Pure-data analytics (no AI): trends, momentum, monetization breakdowns, formatting.

use crate::api::Item;
use crate::store::Snap;
use crate::util::day_gap;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::OnceLock;

pub fn like_ratio(up: u64, down: u64) -> f64 {
    let t = up + down;
    if t == 0 {
        0.0
    } else {
        (up as f64 / t as f64 * 1000.0).round() / 10.0
    }
}

pub fn pct(a: f64, b: f64) -> Option<f64> {
    if b == 0.0 {
        None
    } else {
        Some(((a - b) / b * 1000.0).round() / 10.0)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Trend {
    pub rank: u32,
    pub playing: u64,
    pub days_tracked: usize,
    pub best_rank: u32,
    pub rank_change_1d: Option<i64>, // + = climbing
    pub rank_change_7d: Option<i64>,
    pub ccu_change_1d_pct: Option<f64>,
    pub ccu_change_7d_pct: Option<f64>,
    pub visits_per_day: Option<u64>,
    pub favorites_per_day: Option<u64>,
    pub momentum: u32,
    pub flags: Vec<String>,
}

/// history: oldest -> newest.
pub fn trend(h: &[Snap]) -> Trend {
    let Some(cur) = h.last() else {
        return Trend {
            momentum: 50,
            ..Default::default()
        };
    };
    let mut t = Trend {
        rank: cur.rank,
        playing: cur.playing,
        days_tracked: h.len(),
        best_rank: h
            .iter()
            .map(|s| s.rank)
            .filter(|r| *r > 0)
            .min()
            .unwrap_or(cur.rank),
        ..Default::default()
    };
    if h.len() >= 2 {
        let prev = &h[h.len() - 2];
        if prev.rank > 0 && cur.rank > 0 {
            t.rank_change_1d = Some(prev.rank as i64 - cur.rank as i64);
        }
        t.ccu_change_1d_pct = pct(cur.playing as f64, prev.playing as f64);
        let dd = day_gap(&prev.day, &cur.day).unwrap_or(1).max(1) as f64;
        if cur.visits > 0 && prev.visits > 0 {
            t.visits_per_day = Some((cur.visits.saturating_sub(prev.visits) as f64 / dd) as u64);
        }
        if cur.favorites > 0 && prev.favorites > 0 {
            t.favorites_per_day =
                Some((cur.favorites.saturating_sub(prev.favorites) as f64 / dd) as u64);
        }
    }
    let week: Vec<&Snap> = h
        .iter()
        .filter(|s| day_gap(&s.day, &cur.day).unwrap_or(0) <= 7)
        .collect();
    if week.len() >= 2 {
        t.ccu_change_7d_pct = pct(cur.playing as f64, week[0].playing as f64);
        if week[0].rank > 0 && cur.rank > 0 {
            t.rank_change_7d = Some(week[0].rank as i64 - cur.rank as i64);
        }
    }
    t.momentum = momentum(&t);
    t.flags = flags(&t);
    t
}

/// 0-100 score mixing CCU growth and rank climbing. 50 = flat.
pub fn momentum(t: &Trend) -> u32 {
    let mut s = 50.0;
    if let Some(p) = t.ccu_change_1d_pct {
        s += (p / 2.0).clamp(-20.0, 20.0);
    }
    if let Some(p) = t.ccu_change_7d_pct {
        s += (p / 5.0).clamp(-20.0, 20.0);
    }
    if let Some(r) = t.rank_change_1d {
        s += (r as f64).clamp(-10.0, 10.0);
    }
    s.round().clamp(0.0, 100.0) as u32
}

pub fn flags(t: &Trend) -> Vec<String> {
    let mut f = vec![];
    if t.days_tracked == 1 {
        f.push("new-entry".to_string());
    }
    let r1 = t.rank_change_1d.unwrap_or(0);
    let c1 = t.ccu_change_1d_pct.unwrap_or(0.0);
    if r1 >= 5 || c1 >= 40.0 {
        f.push("rising-fast".into());
    }
    if r1 <= -5 || c1 <= -35.0 {
        f.push("falling".into());
    }
    if t.momentum >= 70 {
        f.push("hot".into());
    }
    f
}

fn categories() -> &'static Vec<(&'static str, Regex)> {
    static C: OnceLock<Vec<(&'static str, Regex)>> = OnceLock::new();
    C.get_or_init(|| {
        [
            ("multiplier", r"\b(x\s?\d|\d\s?x|double|triple|2x|3x|boost|multiplier)\b"),
            ("vip/status", r"\b(vip|premium|mvp|elite|supporter|exclusive|tag|title)\b"),
            ("automation", r"\b(auto|afk|idle|offline)\b"),
            ("luck/gacha", r"\b(luck|lucky|egg|hatch|spin|crate|roll|chest|gacha|pet)\b"),
            ("currency", r"\b(coins?|cash|gems?|money|bucks|robux|currency|tokens?)\b"),
            ("speed/qol", r"\b(speed|fast|walk|teleport|tp|skip|instant|storage|inventory|slot|backpack|capacity)\b"),
            ("cosmetic", r"\b(skin|trail|aura|color|cosmetic|emote|effect|glow|title)\b"),
            ("power/item", r"\b(sword|gun|weapon|tool|power|ability|item|admin|god|carpet|coil)\b"),
        ]
        .into_iter()
        .map(|(c, r)| (c, Regex::new(r).unwrap()))
        .collect()
    })
}

pub fn categorize(name: &str, desc: &str) -> String {
    let text = format!("{name} {desc}").to_lowercase();
    for (c, rx) in categories() {
        if rx.is_match(&text) {
            return c.to_string();
        }
    }
    "other".into()
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PriceStats {
    pub count: usize,
    pub for_sale: usize,
    pub min: Option<u64>,
    pub max: Option<u64>,
    pub median: Option<u64>,
    pub avg: Option<u64>,
    pub total: u64,
}

pub fn price_stats(items: &[Item]) -> PriceStats {
    let mut prices: Vec<u64> = items
        .iter()
        .filter_map(|i| i.price)
        .filter(|p| *p > 0)
        .collect();
    prices.sort_unstable();
    let n = prices.len();
    if n == 0 {
        return PriceStats {
            count: items.len(),
            ..Default::default()
        };
    }
    let median = if n % 2 == 1 {
        prices[n / 2]
    } else {
        (prices[n / 2 - 1] + prices[n / 2]) / 2
    };
    let total: u64 = prices.iter().sum();
    PriceStats {
        count: items.len(),
        for_sale: n,
        min: prices.first().copied(),
        max: prices.last().copied(),
        median: Some(median),
        avg: Some(total / n as u64),
        total,
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Monetization {
    pub passes: PriceStats,
    pub products: PriceStats,
    pub pass_categories: Vec<(String, usize)>,
    pub buy_all_passes_robux: u64,
    pub has_starter_pack: bool,
    pub has_vip: bool,
    pub has_auto: bool,
    pub has_luck: bool,
}

/// Also fills `category` on every item.
pub fn monetization(passes: &mut [Item], products: &mut [Item]) -> Monetization {
    let mut cats: BTreeMap<String, usize> = BTreeMap::new();
    for p in passes.iter_mut() {
        p.category = categorize(&p.name, &p.description);
        *cats.entry(p.category.clone()).or_default() += 1;
    }
    for p in products.iter_mut() {
        p.category = categorize(&p.name, &p.description);
    }
    let mut pc: Vec<(String, usize)> = cats.into_iter().collect();
    pc.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let ps = price_stats(passes);
    let starter = Regex::new(r"starter|beginner|newbie").unwrap();
    Monetization {
        buy_all_passes_robux: ps.total,
        passes: ps,
        products: price_stats(products),
        pass_categories: pc,
        has_starter_pack: passes
            .iter()
            .chain(products.iter())
            .any(|p| starter.is_match(&p.name.to_lowercase())),
        has_vip: passes.iter().any(|p| p.category == "vip/status"),
        has_auto: passes.iter().any(|p| p.category == "automation"),
        has_luck: passes
            .iter()
            .chain(products.iter())
            .any(|p| p.category == "luck/gacha"),
    }
}

pub fn fmt_num(n: Option<f64>) -> String {
    let Some(n) = n else { return "–".into() };
    for (unit, div) in [("B", 1e9), ("M", 1e6), ("K", 1e3)] {
        if n.abs() >= div {
            let s = format!("{:.1}", n / div);
            let s = s.strip_suffix(".0").unwrap_or(&s).to_string();
            return format!("{s}{unit}");
        }
    }
    format!("{}", n as i64)
}

pub fn n(v: u64) -> String {
    fmt_num(Some(v as f64))
}

pub fn fmt_pct(p: Option<f64>) -> String {
    match p {
        None => "–".into(),
        Some(p) if p > 0.0 => format!("+{p:.1}%"),
        Some(p) => format!("{p:.1}%"),
    }
}

pub fn fmt_delta(d: Option<i64>) -> String {
    match d {
        None => "–".into(),
        Some(d) if d > 0 => format!("▲{d}"),
        Some(d) if d < 0 => format!("▼{}", -d),
        _ => "=".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(day: &str, rank: u32, playing: u64, visits: u64) -> Snap {
        Snap {
            day: day.into(),
            rank,
            universe_id: 1,
            playing,
            visits,
            favorites: visits / 100,
            ..Default::default()
        }
    }

    #[test]
    fn trend_math() {
        let t = trend(&[
            snap("2026-01-01", 10, 100, 1000),
            snap("2026-01-02", 4, 200, 3000),
        ]);
        assert_eq!(t.rank_change_1d, Some(6));
        assert_eq!(t.ccu_change_1d_pct, Some(100.0));
        assert_eq!(t.visits_per_day, Some(2000));
        assert!(t.flags.contains(&"rising-fast".to_string()));
        assert!(t.momentum > 60);
        assert_eq!(t.best_rank, 4);
    }

    #[test]
    fn monetization_breakdown() {
        let it = |n: &str, p: u64| Item {
            name: n.into(),
            price: Some(p),
            for_sale: true,
            ..Default::default()
        };
        let mut passes = vec![it("x2 Cash", 99), it("VIP", 400), it("Auto Collect", 150)];
        let mut prods = vec![it("Starter Pack", 49), it("Instant money", 25)];
        let m = monetization(&mut passes, &mut prods);
        assert_eq!(m.passes.median, Some(150));
        assert_eq!(m.buy_all_passes_robux, 649);
        assert!(m.has_vip && m.has_auto && m.has_starter_pack);
        assert_eq!(prods[1].category, "currency");
    }

    #[test]
    fn formatting() {
        assert_eq!(fmt_num(Some(290_707_003.0)), "290.7M");
        assert_eq!(fmt_num(Some(1000.0)), "1K");
        assert_eq!(fmt_num(Some(999.0)), "999");
        assert_eq!(fmt_delta(Some(3)), "▲3");
        assert_eq!(fmt_pct(Some(100.0)), "+100.0%");
    }
}
