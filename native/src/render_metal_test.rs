//! Mac-only prototype. Export screen-space triangles in software draw order.
use super::*;

#[repr(C)]
pub struct Vertex {
    pub x: f32, pub y: f32, pub z: f32,
    pub u: f32, pub v: f32,
    pub base_u: f32, pub base_v: f32,
    pub width: f32, pub depth: f32,
    pub mode: u32,
}

#[repr(C)]
pub struct Batch {
    pub pixels: *const u8,
    pub width: u32, pub height: u32,
    pub start: u32, pub count: u32, pub overlay: bool,
}

#[repr(C)]
pub struct MeshView {
    pub vertices: *const Vertex,
    pub batches: *const Batch,
    pub vertex_count: u32,
    pub batch_count: u32,
}

#[derive(Default)]
pub struct Mesh {
    vertices: Vec<Vertex>,
    batches: Vec<Batch>,
}

pub(crate) fn shade_table() -> *const u8 { shade_lut().as_ptr().cast() }

impl Pose {
    pub(crate) fn needs_software_overlays(&self) -> bool {
        self.hand_alpha > 0.004 || self.question_height().is_some()
            || !self.notes.is_empty() || self.feed_start.is_some()
    }
}

impl Mesh {
    pub(super) fn push_triangle(&mut self, positions: [Vec3; 3], uv: [Vec2; 3],
                                texture: &RgbaImage, face: usize, color: Option<[u8; 3]>,
                                detailed: bool, base: Vec2, size: Vec3) {
        let pixels = texture.as_ptr();
        if self.batches.last().is_none_or(|batch| batch.pixels != pixels) {
            self.batches.push(Batch { pixels, width: texture.width(), height: texture.height(),
                start: self.vertices.len() as u32, count: 0, overlay: false });
        }
        self.batches.last_mut().unwrap().count += 3;
        let mode = color.map_or(face as u32, |[r,g,b]| {
            1 << 31 | u32::from(detailed) << 30 | u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)
        });
        self.vertices.extend(positions.into_iter().zip(uv).map(|(p, t)| Vertex {
            x: p.x, y: p.y, z: p.z, u: t.x, v: t.y, base_u: base.x,
            base_v: base.y, width: size.x, depth: size.z, mode,
        }));
    }

    pub(super) fn push_glyph(&mut self, atlas: &RgbaImage, index: usize,
                             left: i32, top: i32, size: i32, scale: f32,
                             opacity: f32, [r, g, b]: [u8; 3]) {
        if opacity <= 0. { return; }
        let pixels = atlas.as_ptr();
        if self.batches.last().is_none_or(|batch| batch.pixels != pixels) {
            self.batches.push(Batch { pixels, width: atlas.width(), height: atlas.height(),
                start: self.vertices.len() as u32, count: 0, overlay: true });
        }
        self.batches.last_mut().unwrap().count += 6;
        let mode = 1 << 29 | u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b);
        for (x, y) in [(0,0), (0,size), (size,0), (0,size), (size,size), (size,0)] {
            self.vertices.push(Vertex {
                x: (left + x) as f32, y: (top + y) as f32, z: 0.,
                u: (x as f32 - 0.5) / scale, v: (y as f32 - 0.5) / scale,
                base_u: opacity, base_v: (index * 24) as f32, width: 0., depth: 0., mode,
            });
        }
    }

    pub(crate) fn clear(&mut self) { self.vertices.clear(); self.batches.clear(); }

    pub(crate) fn view(&self) -> MeshView {
        MeshView { vertices: self.vertices.as_ptr(), batches: self.batches.as_ptr(),
            vertex_count: self.vertices.len() as u32, batch_count: self.batches.len() as u32 }
    }

    pub(crate) fn hit(&self, x: f32, y: f32) -> bool {
        let point = Vec3::new(x, y, 0.);
        for batch in &self.batches {
            let vertices = &self.vertices[batch.start as usize..(batch.start + batch.count) as usize];
            for v in vertices.chunks_exact(3) {
                let [a,b,c] = [v[0].position(), v[1].position(), v[2].position()];
                if x < a.x.min(b.x).min(c.x) || x > a.x.max(b.x).max(c.x)
                    || y < a.y.min(b.y).min(c.y) || y > a.y.max(b.y).max(c.y) { continue; }
                let inv = edge(a,b,c).recip();
                let u = edge(b,c,point) * inv;
                let w = edge(a,b,point) * inv;
                if u < 0. || w < 0. || u + w > 1. { continue; }
                if v[0].mode & (1 << 31) != 0 { return true; }
                let uv = |a: &Vertex| Vec2::new(a.u, a.v);
                let t = uv(&v[1]) + (uv(&v[0])-uv(&v[1]))*u + (uv(&v[2])-uv(&v[1]))*w;
                let (sx, sy) = if batch.overlay {
                    ((t.x as u32).min(23), (t.y as u32).min(23) + v[0].base_v as u32)
                } else {
                    let t = t * (batch.width as f32 / 64.);
                    ((t.x as u32).min(batch.width - 1), (t.y as u32).min(batch.height - 1))
                };
                let offset = (sy as usize * batch.width as usize + sx as usize) * 4 + 3;
                // Mesh lifetime is bounded by PetCore and is reset before any texture replacement.
                let alpha = unsafe { *batch.pixels.add(offset) };
                if if batch.overlay { f32::from(alpha) * v[0].base_u >= 0.5 } else { alpha >= 16 } {
                    return true;
                }
            }
        }
        false
    }
}

impl Vertex {
    fn position(&self) -> Vec3 { Vec3::new(self.x, self.y, self.z) }
}
