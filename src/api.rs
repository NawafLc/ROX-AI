//! Roblox public web APIs (no login needed).
//!
//! Every call retries with backoff and handles rate limits, and everything degrades gracefully
//! (empty data instead of a crash) because Roblox endpoints change and sometimes fail.
//! The `RobloxApi` trait lets tests run fully offline.

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ChartEntry {
    pub rank: u32,
    pub universe_id: u64,
    pub place_id: u64,
    pub name: String,
    pub playing: u64,
    pub up_votes: u64,
    pub down_votes: u64,
    pub genre: String,
    pub maturity: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Item {
    pub id: Value,
    pub name: String,
    pub price: Option<u64>,
    pub for_sale: bool,
    pub description: String,
    #[serde(default)]
    pub category: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Badge {
    pub name: String,
    pub description: String,
    pub awarded: u64,
    pub win_rate: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ServerSample {
    pub servers_sampled: usize,
    pub avg_fps: f64,
    pub avg_ping: f64,
    pub full_pct: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GameDetails {
    pub universe_id: u64,
    pub place_id: u64,
    pub name: String,
    pub description: String,
    pub creator_name: String,
    pub creator_id: u64,
    pub creator_type: String,
    pub creator_verified: bool,
    pub playing: u64,
    pub visits: u64,
    pub favorites: u64,
    pub max_players: u64,
    pub created: String,
    pub updated: String,
    pub genre: String,
    pub subgenre: String,
    pub avatar_type: String,
    pub vip_servers: bool,
    pub up_votes: u64,
    pub down_votes: u64,
    pub icon_url: String,
    pub thumbnail_urls: Vec<String>,
    pub game_passes: Vec<Item>,
    pub dev_products: Vec<Item>,
    pub badges: Vec<Badge>,
    pub badge_total: usize,
    pub server_sample: Option<ServerSample>,
}

impl GameDetails {
    pub fn like_ratio(&self) -> f64 {
        crate::analysis::like_ratio(self.up_votes, self.down_votes)
    }
    pub fn url(&self) -> String {
        format!("https://www.roblox.com/games/{}", self.place_id)
    }
}

pub trait RobloxApi: Send + Sync {
    fn chart(&self, sort_id: &str, limit: usize, country: &str) -> Result<Vec<ChartEntry>>;
    fn sorts(&self, country: &str) -> Result<Vec<Value>>;
    fn games_info(&self, ids: &[u64]) -> HashMap<u64, Value>;
    fn game_details(&self, uid: u64, deep: bool) -> GameDetails;
    fn download(&self, url: &str) -> Option<Vec<u8>>;
}

pub struct HttpApi {
    agent: ureq::Agent,
}

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) ROX-AI/2.0";

fn s(v: &Value, k: &str) -> String {
    v.get(k)
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string()
}
fn u(v: &Value, k: &str) -> u64 {
    v.get(k)
        .and_then(|x| x.as_u64().or_else(|| x.as_f64().map(|f| f as u64)))
        .unwrap_or(0)
}
fn b(v: &Value, k: &str) -> bool {
    v.get(k).and_then(|x| x.as_bool()).unwrap_or(false)
}
fn arr<'a>(v: &'a Value, k: &str) -> &'a [Value] {
    v.get(k)
        .and_then(|x| x.as_array())
        .map(|a| a.as_slice())
        .unwrap_or(&[])
}
fn ids_csv(ids: &[u64]) -> String {
    ids.iter()
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

impl HttpApi {
    pub fn new() -> Result<HttpApi> {
        let tls = native_tls::TlsConnector::new()?;
        let agent = ureq::AgentBuilder::new()
            .tls_connector(Arc::new(tls))
            .try_proxy_from_env(true)
            .timeout(Duration::from_secs(25))
            .user_agent(UA)
            .build();
        Ok(HttpApi { agent })
    }

    /// GET JSON with retries on 429/5xx/network errors. 4xx (except 429) fail immediately.
    pub fn get_json(
        &self,
        url: &str,
        params: &[(&str, String)],
        tries: u32,
        max_wait: f64,
    ) -> Result<Value> {
        let mut last = anyhow!("no attempt");
        for attempt in 0..tries {
            let mut req = self.agent.get(url).set("Accept", "application/json");
            for (k, v) in params {
                req = req.query(k, v);
            }
            match req.call() {
                Ok(resp) => match resp.into_json::<Value>() {
                    Ok(v) => return Ok(v),
                    Err(e) => last = anyhow!("bad JSON from {url}: {e}"),
                },
                Err(ureq::Error::Status(code, resp)) if code == 429 || code >= 500 => {
                    let ra: f64 = resp
                        .header("retry-after")
                        .and_then(|h| h.parse().ok())
                        .unwrap_or(0.0);
                    let wait = if ra > 0.0 {
                        ra
                    } else {
                        2f64.powi(attempt as i32) + 0.5
                    };
                    crate::warn!("HTTP {code} from {url} (attempt {})", attempt + 1);
                    last = anyhow!("HTTP {code}");
                    if attempt + 1 < tries {
                        std::thread::sleep(Duration::from_secs_f64(wait.min(max_wait)));
                    }
                    continue;
                }
                Err(ureq::Error::Status(code, _)) => bail!("HTTP {code} from {url}"),
                Err(e) => last = anyhow!("network error for {url}: {e}"),
            }
            if attempt + 1 < tries {
                std::thread::sleep(Duration::from_secs_f64(
                    (2f64.powi(attempt as i32)).min(max_wait),
                ));
            }
        }
        Err(last)
    }

    fn get(&self, url: &str, params: &[(&str, String)]) -> Result<Value> {
        self.get_json(url, params, 5, 30.0)
    }

    pub fn icons(&self, ids: &[u64]) -> HashMap<u64, String> {
        let mut out = HashMap::new();
        for c in ids.chunks(50) {
            match self.get(
                "https://thumbnails.roblox.com/v1/games/icons",
                &[
                    ("universeIds", ids_csv(c)),
                    ("size", "512x512".into()),
                    ("format", "Png".into()),
                    ("isCircular", "false".into()),
                ],
            ) {
                Ok(v) => {
                    for d in arr(&v, "data") {
                        if s(d, "state") == "Completed" && !s(d, "imageUrl").is_empty() {
                            out.insert(u(d, "targetId"), s(d, "imageUrl"));
                        }
                    }
                }
                Err(e) => crate::warn!("icons failed: {e}"),
            }
        }
        out
    }

    fn thumbnails(&self, uid: u64) -> Result<Vec<String>> {
        let v = self.get(
            "https://thumbnails.roblox.com/v1/games/multiget/thumbnails",
            &[
                ("universeIds", uid.to_string()),
                ("size", "768x432".into()),
                ("format", "Png".into()),
                ("countPerUniverse", "3".into()),
            ],
        )?;
        Ok(arr(&v, "data")
            .iter()
            .flat_map(|x| arr(x, "thumbnails").iter())
            .filter(|t| s(t, "state") == "Completed")
            .map(|t| s(t, "imageUrl"))
            .filter(|x| !x.is_empty())
            .collect())
    }

    fn votes(&self, ids: &[u64]) -> HashMap<u64, (u64, u64)> {
        let mut out = HashMap::new();
        for c in ids.chunks(50) {
            if let Ok(v) = self.get(
                "https://games.roblox.com/v1/games/votes",
                &[("universeIds", ids_csv(c))],
            ) {
                for d in arr(&v, "data") {
                    out.insert(u(d, "id"), (u(d, "upVotes"), u(d, "downVotes")));
                }
            }
        }
        out
    }

    pub fn game_passes(&self, uid: u64) -> Result<Vec<Item>> {
        let mut out = vec![];
        let mut token = String::new();
        for _ in 0..10 {
            let mut p = vec![
                ("passView", "Full".to_string()),
                ("pageSize", "100".to_string()),
            ];
            if !token.is_empty() {
                p.push(("pageToken", token.clone()));
            }
            let v = self.get(
                &format!("https://apis.roblox.com/game-passes/v1/universes/{uid}/game-passes"),
                &p,
            )?;
            for g in arr(&v, "gamePasses") {
                let fs = b(g, "isForSale");
                let name = if s(g, "displayName").is_empty() {
                    s(g, "name")
                } else {
                    s(g, "displayName")
                };
                out.push(Item {
                    id: g.get("id").cloned().unwrap_or(Value::Null),
                    name,
                    price: if fs {
                        g.get("price").and_then(|x| x.as_u64())
                    } else {
                        None
                    },
                    for_sale: fs,
                    description: s(g, "displayDescription"),
                    category: String::new(),
                });
            }
            token = s(&v, "nextPageToken");
            if token.is_empty() {
                break;
            }
        }
        Ok(out)
    }

    pub fn dev_products(&self, uid: u64) -> Result<Vec<Item>> {
        let mut out = vec![];
        let mut cursor = String::new();
        for _ in 0..10 {
            let mut p = vec![("limit", "50".to_string())];
            if !cursor.is_empty() {
                p.push(("cursor", cursor.clone()));
            }
            let v = self.get(&format!("https://apis.roblox.com/developer-products/v2/universes/{uid}/developerproducts"), &p)?;
            for g in arr(&v, "developerProducts") {
                let name = if s(g, "displayName").is_empty() {
                    s(g, "Name")
                } else {
                    s(g, "displayName")
                };
                let desc = if s(g, "displayDescription").is_empty() {
                    s(g, "Description")
                } else {
                    s(g, "displayDescription")
                };
                out.push(Item {
                    id: g.get("ProductId").cloned().unwrap_or(Value::Null),
                    name,
                    price: g.get("PriceInRobux").and_then(|x| x.as_u64()),
                    for_sale: b(g, "IsForSale"),
                    description: desc,
                    category: String::new(),
                });
            }
            cursor = s(&v, "nextPageCursor");
            if cursor.is_empty() {
                break;
            }
        }
        Ok(out)
    }

    pub fn badges(&self, uid: u64, max_pages: usize) -> Result<Vec<Badge>> {
        let mut out = vec![];
        let mut cursor = String::new();
        for _ in 0..max_pages {
            let mut p = vec![
                ("limit", "100".to_string()),
                ("sortOrder", "Asc".to_string()),
            ];
            if !cursor.is_empty() {
                p.push(("cursor", cursor.clone()));
            }
            let v = self.get(
                &format!("https://badges.roblox.com/v1/universes/{uid}/badges"),
                &p,
            )?;
            for x in arr(&v, "data") {
                let st = x.get("statistics").cloned().unwrap_or(json!({}));
                let name = if s(x, "displayName").is_empty() {
                    s(x, "name")
                } else {
                    s(x, "displayName")
                };
                let desc = if s(x, "displayDescription").is_empty() {
                    s(x, "description")
                } else {
                    s(x, "displayDescription")
                };
                out.push(Badge {
                    name,
                    description: desc,
                    awarded: u(&st, "awardedCount"),
                    win_rate: st
                        .get("winRatePercentage")
                        .and_then(|w| w.as_f64())
                        .unwrap_or(0.0),
                });
            }
            cursor = s(&v, "nextPageCursor");
            if cursor.is_empty() {
                break;
            }
        }
        Ok(out)
    }

    fn server_sample(&self, place_id: u64) -> Result<Option<ServerSample>> {
        // optional + heavily rate-limited: fail fast instead of slowing the whole scan
        let v = self.get_json(
            &format!("https://games.roblox.com/v1/games/{place_id}/servers/Public"),
            &[("limit", "100".into())],
            2,
            3.0,
        )?;
        let sv = arr(&v, "data");
        if sv.is_empty() {
            return Ok(None);
        }
        let n = sv.len() as f64;
        let fps: f64 = sv
            .iter()
            .map(|x| x.get("fps").and_then(|f| f.as_f64()).unwrap_or(0.0))
            .sum();
        let ping: f64 = sv
            .iter()
            .map(|x| x.get("ping").and_then(|f| f.as_f64()).unwrap_or(0.0))
            .sum();
        let full = sv
            .iter()
            .filter(|x| {
                u(x, "playing") > 0
                    && u(x, "maxPlayers") > 0
                    && u(x, "playing") >= u(x, "maxPlayers")
            })
            .count() as f64;
        Ok(Some(ServerSample {
            servers_sampled: sv.len(),
            avg_fps: (fps / n * 10.0).round() / 10.0,
            avg_ping: (ping / n).round(),
            full_pct: (full / n * 100.0).round(),
        }))
    }
}

fn parse_chart(games: &[Value], limit: usize) -> Vec<ChartEntry> {
    let mut out = vec![];
    for g in games {
        if b(g, "isSponsored") {
            continue; // ads are not real chart positions
        }
        out.push(ChartEntry {
            rank: out.len() as u32 + 1,
            universe_id: u(g, "universeId"),
            place_id: u(g, "rootPlaceId"),
            name: s(g, "name"),
            playing: u(g, "playerCount"),
            up_votes: u(g, "totalUpVotes"),
            down_votes: u(g, "totalDownVotes"),
            genre: s(g, "genreL1"),
            maturity: s(g, "contentMaturity"),
        });
        if out.len() >= limit {
            break;
        }
    }
    out
}

impl RobloxApi for HttpApi {
    fn chart(&self, sort_id: &str, limit: usize, country: &str) -> Result<Vec<ChartEntry>> {
        let sid = crate::util::uuid4();
        let base = [
            ("sessionId", sid.clone()),
            ("device", "computer".to_string()),
            ("country", country.to_string()),
        ];
        let mut p = base.to_vec();
        p.push(("sortId", sort_id.to_string()));
        let mut games = match self.get(
            "https://apis.roblox.com/explore-api/v1/get-sort-content",
            &p,
        ) {
            Ok(v) => arr(&v, "games").to_vec(),
            Err(e) => {
                crate::warn!(
                    "get-sort-content failed for {sort_id}: {e}; falling back to get-sorts"
                );
                vec![]
            }
        };
        if games.is_empty() {
            let v = self.get("https://apis.roblox.com/explore-api/v1/get-sorts", &base)?;
            for st in arr(&v, "sorts") {
                if s(st, "sortId") == sort_id {
                    games = arr(st, "games").to_vec();
                }
            }
        }
        Ok(parse_chart(&games, limit))
    }

    fn sorts(&self, country: &str) -> Result<Vec<Value>> {
        let v = self.get(
            "https://apis.roblox.com/explore-api/v1/get-sorts",
            &[
                ("sessionId", crate::util::uuid4()),
                ("device", "computer".into()),
                ("country", country.into()),
            ],
        )?;
        Ok(arr(&v, "sorts").iter().filter(|x| s(x, "contentType") == "Games").map(|x| json!({"sortId": s(x, "sortId"), "name": s(x, "sortDisplayName"), "count": arr(x, "games").len()})).collect())
    }

    fn games_info(&self, ids: &[u64]) -> HashMap<u64, Value> {
        let mut out = HashMap::new();
        for c in ids.chunks(50) {
            match self.get(
                "https://games.roblox.com/v1/games",
                &[("universeIds", ids_csv(c))],
            ) {
                Ok(v) => {
                    for g in arr(&v, "data") {
                        out.insert(u(g, "id"), g.clone());
                    }
                }
                Err(e) => crate::warn!("games_info failed: {e}"),
            }
        }
        out
    }

    fn game_details(&self, uid: u64, deep: bool) -> GameDetails {
        let mut d = GameDetails {
            universe_id: uid,
            ..Default::default()
        };
        if let Some(info) = self.games_info(&[uid]).remove(&uid) {
            let cr = info.get("creator").cloned().unwrap_or(json!({}));
            d.place_id = u(&info, "rootPlaceId");
            d.name = s(&info, "name");
            d.description = info
                .get("description")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            d.creator_name = s(&cr, "name");
            d.creator_id = u(&cr, "id");
            d.creator_type = s(&cr, "type");
            d.creator_verified = b(&cr, "hasVerifiedBadge");
            d.playing = u(&info, "playing");
            d.visits = u(&info, "visits");
            d.favorites = u(&info, "favoritedCount");
            d.max_players = u(&info, "maxPlayers");
            d.created = s(&info, "created");
            d.updated = s(&info, "updated");
            d.genre = if s(&info, "genre_l1").is_empty() {
                s(&info, "genre")
            } else {
                s(&info, "genre_l1")
            };
            d.subgenre = s(&info, "genre_l2");
            d.avatar_type = s(&info, "universeAvatarType");
            d.vip_servers = b(&info, "createVipServersAllowed");
        }
        if let Some((up, down)) = self.votes(&[uid]).get(&uid) {
            d.up_votes = *up;
            d.down_votes = *down;
        }
        d.icon_url = self.icons(&[uid]).remove(&uid).unwrap_or_default();
        if deep {
            d.thumbnail_urls = self.thumbnails(uid).unwrap_or_default();
            d.game_passes = self.game_passes(uid).unwrap_or_else(|e| {
                crate::warn!("game passes {uid}: {e}");
                vec![]
            });
            d.dev_products = self.dev_products(uid).unwrap_or_else(|e| {
                crate::warn!("dev products {uid}: {e}");
                vec![]
            });
            d.badges = self.badges(uid, 3).unwrap_or_default();
            d.badge_total = d.badges.len();
            if d.place_id > 0 {
                d.server_sample = self.server_sample(d.place_id).ok().flatten();
            }
        }
        d
    }

    fn download(&self, url: &str) -> Option<Vec<u8>> {
        match self.agent.get(url).call() {
            Ok(r) => {
                let mut buf = vec![];
                r.into_reader()
                    .take(30_000_000)
                    .read_to_end(&mut buf)
                    .ok()?;
                Some(buf)
            }
            Err(e) => {
                crate::warn!("download failed {url}: {e}");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chart_parsing_skips_ads() {
        let games = vec![
            json!({"universeId": 1, "rootPlaceId": 11, "name": "Ad", "isSponsored": true}),
            json!({"universeId": 2, "rootPlaceId": 22, "name": " A ", "playerCount": 5, "totalUpVotes": 9, "totalDownVotes": 1, "genreL1": "Obby"}),
            json!({"universeId": 3, "rootPlaceId": 33, "name": "B"}),
        ];
        let c = parse_chart(&games, 1);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].universe_id, 2);
        assert_eq!(c[0].rank, 1);
        assert_eq!(c[0].name, "A");
    }
}
