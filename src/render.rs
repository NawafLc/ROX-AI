//! Pictures of a game's 3D world without Roblox: the parts exported by `luau/scene.luau` (shape, CFrame, size,
//! colour, transparency) drawn with a small software rasterizer: flat shading, a sun, sky, distance haze and 2×
//! supersampling. It shows layout, scale, colours and what covers what — not Roblox's exact lighting or textures.

use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct V3(pub f32, pub f32, pub f32);

impl V3 {
    fn add(self, o: V3) -> V3 {
        V3(self.0 + o.0, self.1 + o.1, self.2 + o.2)
    }
    fn sub(self, o: V3) -> V3 {
        V3(self.0 - o.0, self.1 - o.1, self.2 - o.2)
    }
    fn mul(self, k: f32) -> V3 {
        V3(self.0 * k, self.1 * k, self.2 * k)
    }
    fn dot(self, o: V3) -> f32 {
        self.0 * o.0 + self.1 * o.1 + self.2 * o.2
    }
    fn cross(self, o: V3) -> V3 {
        V3(self.1 * o.2 - self.2 * o.1, self.2 * o.0 - self.0 * o.2, self.0 * o.1 - self.1 * o.0)
    }
    fn len(self) -> f32 {
        self.dot(self).sqrt()
    }
    fn norm(self) -> V3 {
        let l = self.len();
        if l < 1e-9 {
            self
        } else {
            self.mul(1.0 / l)
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Part {
    pub n: String,
    pub s: String,
    pub cf: Vec<f32>,
    pub size: [f32; 3],
    pub c: [f32; 3],
    #[serde(default)]
    pub t: f32,
    #[serde(default)]
    pub neon: bool,
    #[serde(default)]
    pub spawn: bool,
}

impl Part {
    fn pos(&self) -> V3 {
        V3(self.cf[0], self.cf[1], self.cf[2])
    }
    /// local → world
    fn world(&self, l: V3) -> V3 {
        let r = &self.cf[3..12];
        V3(
            self.cf[0] + r[0] * l.0 + r[1] * l.1 + r[2] * l.2,
            self.cf[1] + r[3] * l.0 + r[4] * l.1 + r[5] * l.2,
            self.cf[2] + r[6] * l.0 + r[7] * l.1 + r[8] * l.2,
        )
    }
    /// The way the part's front (-Z) faces
    pub fn look(&self) -> V3 {
        let r = &self.cf[3..12];
        V3(-r[2], -r[5], -r[8])
    }
    fn max_size(&self) -> f32 {
        self.size[0].max(self.size[1]).max(self.size[2])
    }
}

#[derive(Debug, Deserialize)]
pub struct Scene {
    pub parts: Vec<Part>,
    #[serde(default)]
    pub invisible: usize,
    #[serde(default, rename = "setupError")]
    pub setup_error: Option<String>,
    #[serde(default)]
    pub logs: Vec<String>,
}

struct Tri {
    v: [V3; 3],
    color: [f32; 3],
    alpha: f32,
    neon: bool,
}

/// Triangles for one part, in world space.
fn mesh(p: &Part, out: &mut Vec<Tri>) {
    if p.cf.len() < 12 {
        return;
    }
    let [sx, sy, sz] = p.size;
    let (hx, hy, hz) = (sx / 2.0, sy / 2.0, sz / 2.0);
    let mut quads: Vec<[V3; 4]> = vec![];
    let mut tris: Vec<[V3; 3]> = vec![];
    match p.s.as_str() {
        "ball" => {
            let r = sx.min(sy).min(sz) / 2.0;
            let (rings, segs) = (8, 14);
            let pt = |i: usize, j: usize| {
                let th = std::f32::consts::PI * i as f32 / rings as f32;
                let ph = 2.0 * std::f32::consts::PI * j as f32 / segs as f32;
                V3(r * th.sin() * ph.cos(), r * th.cos(), r * th.sin() * ph.sin())
            };
            for i in 0..rings {
                for j in 0..segs {
                    quads.push([pt(i, j), pt(i + 1, j), pt(i + 1, j + 1), pt(i, j + 1)]);
                }
            }
        }
        "cylinder" => {
            // axis along X; radius from the smaller of Y and Z
            let r = sy.min(sz) / 2.0;
            let segs = 16;
            let pt = |x: f32, j: usize| {
                let a = 2.0 * std::f32::consts::PI * j as f32 / segs as f32;
                V3(x, r * a.cos(), r * a.sin())
            };
            for j in 0..segs {
                quads.push([pt(-hx, j), pt(hx, j), pt(hx, j + 1), pt(-hx, j + 1)]);
                tris.push([V3(hx, 0.0, 0.0), pt(hx, j), pt(hx, j + 1)]);
                tris.push([V3(-hx, 0.0, 0.0), pt(-hx, j + 1), pt(-hx, j)]);
            }
        }
        "wedge" | "cornerwedge" => {
            // full bottom and back (+Z) faces, slope from the top back edge down to the front bottom edge
            let (b0, b1, b2, b3) = (V3(-hx, -hy, -hz), V3(hx, -hy, -hz), V3(hx, -hy, hz), V3(-hx, -hy, hz));
            let (t2, t3) = (V3(hx, hy, hz), V3(-hx, hy, hz));
            quads.push([b0, b1, b2, b3]);
            quads.push([b3, b2, t2, t3]);
            quads.push([b0, t3, t2, b1]);
            tris.push([b0, b3, t3]);
            tris.push([b1, t2, b2]);
        }
        _ => {
            let c = |x: f32, y: f32, z: f32| V3(x * hx, y * hy, z * hz);
            quads.push([c(-1., -1., -1.), c(1., -1., -1.), c(1., 1., -1.), c(-1., 1., -1.)]);
            quads.push([c(-1., -1., 1.), c(-1., 1., 1.), c(1., 1., 1.), c(1., -1., 1.)]);
            quads.push([c(-1., -1., -1.), c(-1., 1., -1.), c(-1., 1., 1.), c(-1., -1., 1.)]);
            quads.push([c(1., -1., -1.), c(1., -1., 1.), c(1., 1., 1.), c(1., 1., -1.)]);
            quads.push([c(-1., 1., -1.), c(1., 1., -1.), c(1., 1., 1.), c(-1., 1., 1.)]);
            quads.push([c(-1., -1., -1.), c(-1., -1., 1.), c(1., -1., 1.), c(1., -1., -1.)]);
        }
    }
    let alpha = (1.0 - p.t).clamp(0.0, 1.0);
    let mut push = |a: V3, b: V3, c: V3| out.push(Tri { v: [p.world(a), p.world(b), p.world(c)], color: p.c, alpha, neon: p.neon });
    for q in quads {
        push(q[0], q[1], q[2]);
        push(q[0], q[2], q[3]);
    }
    for t in tris {
        push(t[0], t[1], t[2]);
    }
}

/// A camera: where it is and what it looks at.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub eye: V3,
    pub target: V3,
    pub fov_deg: f32,
}

pub struct View {
    pub name: String,
    pub camera: Camera,
}

const SKY_TOP: [f32; 3] = [0.36, 0.62, 0.92];
const SKY_LOW: [f32; 3] = [0.80, 0.89, 0.97];

/// Draw the scene from one camera. Returns RGB pixels (w × h).
pub fn draw(scene: &Scene, cam: Camera, w: usize, h: usize) -> Vec<u8> {
    let ss = 2usize;
    let (w2, h2) = (w * ss, h * ss);
    let mut tris = vec![];
    for p in &scene.parts {
        mesh(p, &mut tris);
    }
    let fwd = cam.target.sub(cam.eye).norm();
    let right = fwd.cross(V3(0.0, 1.0, 0.0)).norm();
    let right = if right.len() < 0.5 { V3(1.0, 0.0, 0.0) } else { right };
    let up = right.cross(fwd).norm();
    let f = 1.0 / (cam.fov_deg.to_radians() / 2.0).tan();
    let aspect = w2 as f32 / h2 as f32;
    let sun = V3(0.35, 0.85, 0.4).norm();
    let near = 0.3f32;
    // haze only well beyond what the camera is looking at, so far-away overviews keep their colours
    let focus = cam.target.sub(cam.eye).len().max(50.0);

    // sky
    let mut color = vec![[0f32; 3]; w2 * h2];
    for y in 0..h2 {
        // ray direction's height for this row, so the horizon sits where the camera's horizon is
        let ny = 1.0 - 2.0 * (y as f32 + 0.5) / h2 as f32;
        let dir_y = fwd.1 + up.1 * ny / f;
        let k = (dir_y * 2.5).clamp(0.0, 1.0);
        let c = [0, 1, 2].map(|i| SKY_LOW[i] + (SKY_TOP[i] - SKY_LOW[i]) * k);
        for x in 0..w2 {
            color[y * w2 + x] = c;
        }
    }
    let mut depth = vec![f32::INFINITY; w2 * h2];

    // camera space: x right, y up, z forward
    let to_cam = |p: V3| {
        let d = p.sub(cam.eye);
        V3(d.dot(right), d.dot(up), d.dot(fwd))
    };
    let project = |c: V3| ((c.0 * f / aspect / c.2 + 1.0) * 0.5 * w2 as f32, (1.0 - c.1 * f / c.2) * 0.5 * h2 as f32, c.2);

    let shade = |t: &Tri| -> [f32; 3] {
        if t.neon {
            return t.color.map(|c| (c * 1.25 + 0.1).min(1.0));
        }
        let n = t.v[1].sub(t.v[0]).cross(t.v[2].sub(t.v[0])).norm();
        // face toward the camera for lighting (two-sided)
        let n = if n.dot(cam.eye.sub(t.v[0])) < 0.0 { n.mul(-1.0) } else { n };
        let light = 0.5 + 0.5 * n.dot(sun).max(0.0) + 0.08 * n.1.max(0.0);
        t.color.map(|c| (c * light).min(1.0))
    };

    let mut raster = |t: &Tri, blend: bool| {
        let cv = t.v.map(to_cam);
        // clip against the near plane
        let mut poly: Vec<V3> = vec![];
        for i in 0..3 {
            let (a, b) = (cv[i], cv[(i + 1) % 3]);
            if a.2 >= near {
                poly.push(a);
            }
            if (a.2 >= near) != (b.2 >= near) {
                let k = (near - a.2) / (b.2 - a.2);
                poly.push(a.add(b.sub(a).mul(k)));
            }
        }
        if poly.len() < 3 {
            return;
        }
        let base = shade(t);
        for i in 1..poly.len() - 1 {
            let p = [project(poly[0]), project(poly[i]), project(poly[i + 1])];
            let minx = p.iter().map(|q| q.0).fold(f32::INFINITY, f32::min).floor().max(0.0) as usize;
            let maxx = p.iter().map(|q| q.0).fold(f32::NEG_INFINITY, f32::max).ceil().min(w2 as f32 - 1.0);
            let miny = p.iter().map(|q| q.1).fold(f32::INFINITY, f32::min).floor().max(0.0) as usize;
            let maxy = p.iter().map(|q| q.1).fold(f32::NEG_INFINITY, f32::max).ceil().min(h2 as f32 - 1.0);
            if maxx < 0.0 || maxy < 0.0 {
                continue;
            }
            let (maxx, maxy) = (maxx as usize, maxy as usize);
            let area = (p[1].0 - p[0].0) * (p[2].1 - p[0].1) - (p[2].0 - p[0].0) * (p[1].1 - p[0].1);
            if area.abs() < 1e-6 {
                continue;
            }
            for y in miny..=maxy {
                for x in minx..=maxx {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = ((p[1].0 - px) * (p[2].1 - py) - (p[2].0 - px) * (p[1].1 - py)) / area;
                    let w1 = ((p[2].0 - px) * (p[0].1 - py) - (p[0].0 - px) * (p[2].1 - py)) / area;
                    let w2b = 1.0 - w0 - w1;
                    if w0 < 0.0 || w1 < 0.0 || w2b < 0.0 {
                        continue;
                    }
                    // perspective-correct depth
                    let inv_z = w0 / p[0].2 + w1 / p[1].2 + w2b / p[2].2;
                    let z = 1.0 / inv_z;
                    let i = y * w2 + x;
                    if z >= depth[i] {
                        continue;
                    }
                    let haze = ((z - focus * 1.2) / (focus * 4.0)).clamp(0.0, 0.6);
                    let c = [0, 1, 2].map(|k| base[k] + (SKY_LOW[k] - base[k]) * haze);
                    if blend {
                        let a = t.alpha;
                        color[i] = [0, 1, 2].map(|k| color[i][k] * (1.0 - a) + c[k] * a);
                    } else {
                        color[i] = c;
                        depth[i] = z;
                    }
                }
            }
        }
    };

    let (mut solid, mut glass): (Vec<&Tri>, Vec<&Tri>) = tris.iter().partition(|t| t.alpha >= 0.99);
    for t in solid.drain(..) {
        raster(t, false);
    }
    // see-through parts last, far to near
    glass.sort_by(|a, b| {
        let d = |t: &Tri| t.v[0].add(t.v[1]).add(t.v[2]).mul(1.0 / 3.0).sub(cam.eye).len();
        d(b).partial_cmp(&d(a)).unwrap_or(std::cmp::Ordering::Equal)
    });
    for t in glass {
        raster(t, true);
    }

    // 2× supersampling → final size
    let mut out = vec![0u8; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0f32; 3];
            for dy in 0..ss {
                for dx in 0..ss {
                    let c = color[(y * ss + dy) * w2 + x * ss + dx];
                    for k in 0..3 {
                        acc[k] += c[k];
                    }
                }
            }
            for k in 0..3 {
                out[(y * w + x) * 3 + k] = ((acc[k] / (ss * ss) as f32).clamp(0.0, 1.0).powf(1.0 / 1.1) * 255.0 + 0.5) as u8;
            }
        }
    }
    out
}

/// JPEG bytes of RGB pixels.
pub fn jpeg(rgb: &[u8], w: usize, h: usize) -> Result<Vec<u8>> {
    let img = image::RgbImage::from_raw(w as u32, h as u32, rgb.to_vec()).ok_or_else(|| anyhow!("bad image size"))?;
    let mut buf = std::io::Cursor::new(vec![]);
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 85).encode_image(&img)?;
    Ok(buf.into_inner())
}

/// Parts that are part of the map (not one giant ground or sea plate), for framing the views.
fn map_parts(scene: &Scene) -> Vec<&Part> {
    let v: Vec<&Part> = scene.parts.iter().filter(|p| p.max_size() < 1000.0).collect();
    if v.is_empty() {
        scene.parts.iter().collect()
    } else {
        v
    }
}

fn bounds(parts: &[&Part]) -> (V3, V3) {
    let mut lo = V3(f32::INFINITY, f32::INFINITY, f32::INFINITY);
    let mut hi = V3(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    for p in parts {
        let c = p.pos();
        let r = p.max_size() / 2.0;
        lo = V3(lo.0.min(c.0 - r), lo.1.min(c.1 - r), lo.2.min(c.2 - r));
        hi = V3(hi.0.max(c.0 + r), hi.1.max(c.1 + r), hi.2.max(c.2 + r));
    }
    (lo, hi)
}

/// What a new player sees: behind and above the spawn, looking the way it faces (like Roblox's default camera).
pub fn spawn_camera(scene: &Scene) -> Option<Camera> {
    let s = scene.parts.iter().find(|p| p.spawn)?;
    let mut look = s.look();
    look.1 = 0.0;
    let look = if look.len() < 0.1 { V3(0.0, 0.0, -1.0) } else { look.norm() };
    let head = s.pos().add(V3(0.0, s.size[1] / 2.0 + 4.5, 0.0));
    let mut eye = head.sub(look.mul(14.0)).add(V3(0.0, 4.0, 0.0));
    // like Roblox's camera: move in front of anything between it and the player
    let steps = 60;
    for i in 1..=steps {
        let p = head.add(eye.sub(head).mul(i as f32 / steps as f32));
        if scene.parts.iter().any(|q| !q.spawn && q.t < 0.5 && inside(q, p)) {
            eye = head.add(eye.sub(head).mul(((i as f32 - 1.5) / steps as f32).max(0.05)));
            break;
        }
    }
    Some(Camera { eye, target: head.add(look.mul(30.0)).sub(V3(0.0, 2.0, 0.0)), fov_deg: 70.0 })
}

/// The whole map from high up at an angle.
pub fn overview_camera(scene: &Scene) -> Camera {
    let (lo, hi) = bounds(&map_parts(scene));
    let center = lo.add(hi).mul(0.5);
    let radius = hi.sub(lo).len().max(20.0) / 2.0;
    let eye = center.add(V3(0.55, 0.75, 0.7).norm().mul(radius * 1.55));
    Camera { eye, target: center, fov_deg: 55.0 }
}

/// Straight down onto the map (a plan).
pub fn top_camera(scene: &Scene) -> Camera {
    let (lo, hi) = bounds(&map_parts(scene));
    let center = lo.add(hi).mul(0.5);
    let span = (hi.0 - lo.0).max(hi.2 - lo.2).max(20.0);
    Camera { eye: center.add(V3(0.0, span * 0.95, 0.01)), target: center, fov_deg: 60.0 }
}

/// Views by name ("spawn", "overview", "top") or {"name", "eye": [x,y,z], "look_at": [x,y,z]}.
pub fn views(scene: &Scene, wanted: &[Value]) -> Result<Vec<View>> {
    let mut out = vec![];
    for w in wanted {
        if let Some(name) = w.as_str() {
            let camera = match name {
                "spawn" => spawn_camera(scene).ok_or_else(|| anyhow!("there is no SpawnLocation in the world"))?,
                "overview" => overview_camera(scene),
                "top" => top_camera(scene),
                _ => return Err(anyhow!("unknown view {name}: use spawn, overview, top or {{eye, look_at}}")),
            };
            out.push(View { name: name.into(), camera });
        } else {
            let v3 = |k: &str| -> Result<V3> {
                let a = w[k].as_array().filter(|a| a.len() == 3).ok_or_else(|| anyhow!("a custom view needs {k}: [x, y, z]"))?;
                let n = |i: usize| a[i].as_f64().unwrap_or(0.0) as f32;
                Ok(V3(n(0), n(1), n(2)))
            };
            let name = w["name"].as_str().unwrap_or("custom").to_string();
            out.push(View { name, camera: Camera { eye: v3("eye")?, target: v3("look_at")?, fov_deg: w["fov"].as_f64().unwrap_or(70.0) as f32 } });
        }
    }
    Ok(out)
}

/// Plain-words checks a picture can hide: is the spawn buried, is it floating over nothing.
pub fn findings(scene: &Scene) -> Vec<String> {
    let mut f = vec![];
    if let Some(s) = scene.parts.iter().find(|p| p.spawn) {
        let top = s.pos().add(V3(0.0, s.size[1] / 2.0 + 3.0, 0.0));
        for p in &scene.parts {
            if p.spawn || p.t > 0.9 {
                continue;
            }
            if inside(p, top) {
                f.push(format!("The spawn is buried: a player standing on it is inside {}.", p.n));
                break;
            }
        }
    } else {
        f.push("There is no SpawnLocation: players appear at the world origin.".into());
    }
    f
}

/// Is a world point inside a part's box?
fn inside(p: &Part, w: V3) -> bool {
    if p.cf.len() < 12 {
        return false;
    }
    let d = w.sub(p.pos());
    let r = &p.cf[3..12];
    // local = Rᵀ · d
    let l = V3(r[0] * d.0 + r[3] * d.1 + r[6] * d.2, r[1] * d.0 + r[4] * d.1 + r[7] * d.2, r[2] * d.0 + r[5] * d.1 + r[8] * d.2);
    l.0.abs() <= p.size[0] / 2.0 && l.1.abs() <= p.size[1] / 2.0 && l.2.abs() <= p.size[2] / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(name: &str, pos: [f32; 3], size: [f32; 3], color: [f32; 3]) -> Part {
        Part {
            n: name.into(),
            s: "block".into(),
            cf: vec![pos[0], pos[1], pos[2], 1., 0., 0., 0., 1., 0., 0., 0., 1.],
            size,
            c: color,
            t: 0.0,
            neon: false,
            spawn: name == "SpawnLocation",
        }
    }

    #[test]
    fn draws_a_red_block_in_the_middle_and_sky_around_it() {
        let scene = Scene { parts: vec![part("Block", [0., 0., 0.], [4., 4., 4.], [1., 0., 0.])], invisible: 0, setup_error: None, logs: vec![] };
        let cam = Camera { eye: V3(0., 0., 12.), target: V3(0., 0., 0.), fov_deg: 60.0 };
        let px = draw(&scene, cam, 64, 36);
        let at = |x: usize, y: usize| &px[(y * 64 + x) * 3..(y * 64 + x) * 3 + 3];
        let mid = at(32, 18);
        assert!(mid[0] > 100 && mid[1] < 40 && mid[2] < 40, "middle is red: {mid:?}");
        let corner = at(1, 1);
        assert!(corner[2] > corner[0], "corner is sky: {corner:?}");
    }

    #[test]
    fn spots_a_buried_spawn() {
        let mut scene = Scene {
            parts: vec![part("SpawnLocation", [0., 0.5, 0.], [12., 1., 12.], [0.6; 3]), part("Workspace.Baseplate", [0., 0., 0.], [512., 20., 512.], [0.3, 0.6, 0.3])],
            invisible: 0,
            setup_error: None,
            logs: vec![],
        };
        assert_eq!(findings(&scene).len(), 1);
        scene.parts[1].cf[1] = -10.0;
        assert!(findings(&scene).is_empty());
        let v = views(&scene, &[Value::from("spawn"), Value::from("overview"), Value::from("top")]).unwrap();
        assert_eq!(v.len(), 3);
    }
}
