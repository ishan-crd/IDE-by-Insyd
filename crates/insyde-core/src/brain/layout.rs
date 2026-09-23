//! Force-directed layout for the brain graph (same forces as the design's
//! prototype: pairwise repulsion, typed springs, centering, damping).
//! Repulsion uses a uniform grid so a tick is ~O(n) instead of O(n²).

use super::{EdgeKind, Graph, Kind};
use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub pos: Vec<(f32, f32)>,
    vel: Vec<(f32, f32)>,
    edges: Vec<(usize, usize, EdgeKind)>,
}

const CUTOFF: f32 = 300.;

impl Layout {
    /// Seed positions: hubs on a ring, members scattered around their hub.
    pub fn new(g: &Graph) -> Self {
        let mut seed: u32 = 11;
        let mut rnd = move || {
            seed = (seed.wrapping_mul(9301).wrapping_add(49297)) % 233_280;
            seed as f32 / 233_280.
        };
        let n_groups = g.groups.len().max(1) as f32;
        let hub_pos = |gi: usize| {
            let a = gi as f32 / n_groups * std::f32::consts::TAU;
            (a.cos() * 220., a.sin() * 220.)
        };
        let pos = g
            .nodes
            .iter()
            .map(|n| match (n.kind, n.group) {
                (Kind::Root, _) | (_, None) => (0., 0.),
                (Kind::Module, Some(gi)) => hub_pos(gi),
                (Kind::Symbol, Some(gi)) => {
                    let (x, y) = hub_pos(gi);
                    (x + (rnd() - 0.5) * 120., y + (rnd() - 0.5) * 120.)
                }
                (_, Some(gi)) => {
                    let (x, y) = hub_pos(gi);
                    (x + (rnd() - 0.5) * 90., y + (rnd() - 0.5) * 90.)
                }
            })
            .collect::<Vec<_>>();
        Self {
            vel: vec![(0., 0.); pos.len()],
            pos,
            edges: g.edges.clone(),
        }
    }

    /// Run `n` ticks with a cooling schedule (initial settle).
    pub fn settle(&mut self, n: usize) {
        for i in 0..n {
            self.tick(1. - i as f32 / (n as f32 * 1.15), None);
        }
    }

    pub fn tick(&mut self, alpha: f32, pinned: Option<usize>) {
        let n = self.pos.len();
        // Grid buckets for repulsion.
        let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::with_capacity(n / 4 + 1);
        for (i, &(x, y)) in self.pos.iter().enumerate() {
            grid.entry(((x / CUTOFF).floor() as i32, (y / CUTOFF).floor() as i32))
                .or_default()
                .push(i);
        }
        for (&(cx, cy), bucket) in &grid {
            for dx in -1..=1 {
                for dy in -1..=1 {
                    let Some(other) = grid.get(&(cx + dx, cy + dy)) else {
                        continue;
                    };
                    for &i in bucket {
                        for &j in other {
                            if j <= i {
                                continue;
                            }
                            let (ax, ay) = self.pos[i];
                            let (bx, by) = self.pos[j];
                            let (mut ddx, mut ddy) = (ax - bx, ay - by);
                            let d2 = (ddx * ddx + ddy * ddy).max(1.);
                            if d2 > CUTOFF * CUTOFF {
                                continue;
                            }
                            let d = d2.sqrt();
                            let f = 900. * alpha / d2.max(36.);
                            ddx /= d;
                            ddy /= d;
                            self.vel[i].0 += ddx * f;
                            self.vel[i].1 += ddy * f;
                            self.vel[j].0 -= ddx * f;
                            self.vel[j].1 -= ddy * f;
                        }
                    }
                }
            }
        }
        for &(a, b, k) in &self.edges {
            if a >= n || b >= n {
                continue;
            }
            let (len, stiff) = match k {
                EdgeKind::Hub => (200., 0.03),
                EdgeKind::Child => (58., 0.08),
                EdgeKind::Sym => (24., 0.12),
                EdgeKind::Cross => (150., 0.006),
            };
            let (dx, dy) = (self.pos[b].0 - self.pos[a].0, self.pos[b].1 - self.pos[a].1);
            let d = (dx * dx + dy * dy).sqrt().max(1.);
            let f = (d - len) / d * stiff * alpha * 4.;
            self.vel[a].0 += dx * f;
            self.vel[a].1 += dy * f;
            self.vel[b].0 -= dx * f;
            self.vel[b].1 -= dy * f;
        }
        for i in 0..n {
            if Some(i) == pinned {
                self.vel[i] = (0., 0.);
                continue;
            }
            let (x, y) = self.pos[i];
            let v = &mut self.vel[i];
            v.0 -= x * 0.002 * alpha;
            v.1 -= y * 0.002 * alpha;
            v.0 *= 0.6;
            v.1 *= 0.6;
            self.pos[i] = (x + v.0, y + v.1);
        }
    }

    pub fn bounds(&self) -> (f32, f32, f32, f32) {
        let mut b = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for &(x, y) in &self.pos {
            b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
        }
        if self.pos.is_empty() {
            (0., 0., 0., 0.)
        } else {
            b
        }
    }
}
