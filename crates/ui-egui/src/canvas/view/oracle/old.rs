//! OLD canvas view maths, vendored for the #188 old-vs-new equivalence test.
//!
//! Source: https://github.com/storytold/designcraft @ e79d682e02dc66f14370958b193bfa46072b2f41
//! (merge-base of cloudbring/designcraft `algo/72-use-craft-view-v2` with `origin/main`):
//!   * `crates/ui-egui/src/canvas.rs`: `Xf`, `quarter`, `view_rect`, `impl Xf`
//!   * `crates/ui-egui/src/canvas/view.rs`: `zoom_at`
//!   * `crates/ui-egui/src/canvas/scroll.rs`: `pan`
//!
//! Bodies are verbatim; the only edits are the `Old` prefixes. They are private to the crate (a git
//! dependency could not reach them), which is why they are vendored instead of linked.

#![allow(clippy::wrong_self_convention)]

use super::super::super::{DRect, DesignApp, Point, Pos2, Rect, View};
use designcraft_geom::Affine;
use egui::{pos2, vec2};

#[derive(Clone, Copy, Debug)]
pub struct OldXf {
    pub min: Pos2,
    pub origin: Point,
    pub zoom: f64,
    pub rot: u8,
    pub pivot: Pos2,
}

fn quarter(p: Pos2, c: Pos2, q: u8) -> Pos2 {
    let (dx, dy) = (p.x - c.x, p.y - c.y);
    let (x, y) = match q % 4 {
        0 => (dx, dy),
        1 => (-dy, dx),
        2 => (-dx, -dy),
        _ => (dy, -dx),
    };
    pos2(c.x + x, c.y + y)
}

pub fn old_view_rect(screen: Rect, rot: u8) -> Rect {
    if rot % 2 == 1 { Rect::from_center_size(screen.center(), vec2(screen.height(), screen.width())) } else { screen }
}

impl OldXf {
    pub fn new(rect: Rect, v: &View) -> Self {
        OldXf { min: rect.min, origin: v.origin, zoom: v.zoom, rot: v.rotation % 4, pivot: rect.center() }
    }
    pub fn to_screen(&self, p: Point) -> Pos2 {
        let s = pos2(self.min.x + ((p.x - self.origin.x) * self.zoom) as f32, self.min.y + ((p.y - self.origin.y) * self.zoom) as f32);
        if self.rot == 0 { s } else { quarter(s, self.pivot, self.rot) }
    }
    pub fn unrotate(&self, p: Pos2) -> Pos2 {
        if self.rot == 0 { p } else { quarter(p, self.pivot, 4 - self.rot) }
    }
    pub fn unrotate_delta(&self, d: egui::Vec2) -> egui::Vec2 {
        self.unrotate(self.pivot + d) - self.pivot
    }
    pub fn to_canvas(&self, p: Pos2) -> Point {
        let p = self.unrotate(p);
        Point::new(self.origin.x + (p.x - self.min.x) as f64 / self.zoom, self.origin.y + (p.y - self.min.y) as f64 / self.zoom)
    }
    pub fn rect(&self, r: DRect) -> Rect {
        Rect::from_two_pos(self.to_screen(Point::new(r.x0, r.y0)), self.to_screen(Point::new(r.x1, r.y1)))
    }
    pub fn affine(&self, ppp: f64, area_min: Pos2) -> Affine {
        let unrotated = Affine::translate((self.min.x as f64, self.min.y as f64))
            * Affine::scale(self.zoom)
            * Affine::translate((-self.origin.x, -self.origin.y));
        let (cx, cy) = (self.pivot.x as f64, self.pivot.y as f64);
        let rot = Affine::translate((cx, cy)) * Affine::rotate(self.rot as f64 * std::f64::consts::FRAC_PI_2) * Affine::translate((-cx, -cy));
        Affine::scale(ppp) * Affine::translate((-(area_min.x as f64), -(area_min.y as f64))) * rot * unrotated
    }
}

pub fn old_zoom_at(app: &mut DesignApp, screen: Pos2, factor: f64) {
    let Some(rect) = app.canvas_rect else { return };
    let Some(v) = app.view_mut() else { return };
    let xf = OldXf::new(rect, v);
    let c = xf.to_canvas(screen);
    let screen = xf.unrotate(screen);
    let nz = (v.zoom * factor).clamp(0.05, 40.0);
    v.zoom = nz;
    v.origin = Point::new(c.x - (screen.x - rect.min.x) as f64 / nz, c.y - (screen.y - rect.min.y) as f64 / nz);
}

pub fn old_pan(app: &mut DesignApp, xf: &OldXf, d: egui::Vec2) {
    let d = xf.unrotate_delta(d);
    if let Some(v) = app.view_mut() {
        v.origin = Point::new(v.origin.x - d.x as f64 / v.zoom, v.origin.y - d.y as f64 / v.zoom);
    }
}
