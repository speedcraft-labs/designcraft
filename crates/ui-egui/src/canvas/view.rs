use craft_view::{Anchor, ZoomPolicy};

use super::{CanvasLayout, DRect, DVec2, DesignApp, Document, Point, Pos2, Rect, SpreadRef, Value, View, Xf};

/// Fit the current spread (or page) in the canvas.
pub fn fit(app: &mut DesignApp, rect: Rect, what: &str) {
    let Some(st) = app.session.active() else { return };
    let layout = CanvasLayout::new(&st.doc, st.editing_parents);
    let cur = current_slot(app, &layout).unwrap_or(0);
    let Some(slot) = layout.slots.get(cur) else { return };
    let mut b = slot.bounds;
    if what == "page"
        && let Some(sp) = st.doc.spread(slot.spread)
    {
        let center = app.view().map(|v| v.origin.x + rect.width() as f64 / 2.0 / v.zoom).unwrap_or(0.0);
        let c = slot.to_spread(Point::new(center, slot.bounds.center().y));
        let pi = sp.page_at_x(c.x).unwrap_or(0);
        b = slot.xf.transform_rect_bbox(sp.pages[pi].bounds());
    }
    if what == "all" {
        b = layout.slots.iter().map(|s| s.bounds).reduce(|a, b| a.union(b)).unwrap_or(b);
    }
    if what == "selection" {
        // Fit Selection in Window: the selected objects (on their spread's slot).
        let sel: Vec<designcraft_geom::Rect> = st
            .selection
            .items
            .iter()
            .filter_map(|id| {
                let loc = st.doc.find(*id)?;
                let slot = layout.slots.iter().find(|s| s.spread == loc.spread)?;
                Some(slot.xf.transform_rect_bbox(st.doc.item(*id)?.bounds()))
            })
            .collect();
        let Some(u) = sel.into_iter().reduce(|a, b| a.union(b)) else { return };
        b = u.inflate(u.width().max(u.height()) * 0.05 + 1.0, u.width().max(u.height()) * 0.05 + 1.0);
    }
    fit_bounds(app, rect, b);
}

/// Zoom and scroll so canvas rectangle `b` fills the canvas `rect`.
fn fit_bounds(app: &mut DesignApp, rect: Rect, b: DRect) {
    let pad = 40.0;
    let zoom =
        ((rect.width() as f64 - 2.0 * pad) / b.width().max(1e-6)).min((rect.height() as f64 - 2.0 * pad) / b.height().max(1e-6)).clamp(0.05, 40.0);
    let origin = Point::new(b.center().x - rect.width() as f64 / 2.0 / zoom, b.center().y - rect.height() as f64 / 2.0 / zoom);
    if let Some(v) = app.view_mut() {
        *v = View { zoom, origin, fitted: true, rotation: v.rotation };
    }
}

/// Fit spread `r` (a document or parent spread on the canvas as it is laid out now) in the canvas.
/// Before the canvas has a size, the next frame fits whatever spread is current.
pub fn show_spread(app: &mut DesignApp, r: SpreadRef) {
    let Some(st) = app.session.active() else { return };
    let Some(b) = CanvasLayout::new(&st.doc, st.editing_parents).slot(r).map(|s| s.bounds) else { return };
    match app.canvas_rect {
        Some(rect) => fit_bounds(app, rect, b),
        None => {
            if let Some(v) = app.view_mut() {
                v.fitted = false;
            }
        }
    }
}

/// Index of the slot nearest the view centre.
pub fn current_slot(app: &DesignApp, layout: &CanvasLayout) -> Option<usize> {
    let v = app.view()?;
    let r = app.canvas_rect?;
    let c = Point::new(v.origin.x + r.width() as f64 / 2.0 / v.zoom, v.origin.y + r.height() as f64 / 2.0 / v.zoom);
    layout
        .slots
        .iter()
        .enumerate()
        .min_by(|a, b| {
            let da = (a.1.bounds.center().y - c.y).abs();
            let db = (b.1.bounds.center().y - c.y).abs();
            da.total_cmp(&db)
        })
        .map(|(i, _)| i)
}

/// The spread under the view centre (a parent spread while parents are edited).
pub fn current_spread(app: &DesignApp) -> Option<SpreadRef> {
    let st = app.session.active()?;
    let layout = CanvasLayout::new(&st.doc, st.editing_parents);
    Some(layout.slots.get(current_slot(app, &layout)?)?.spread)
}

/// The spread nearest the view centre and the centre in that spread's coordinates.
pub fn view_centre(app: &DesignApp) -> Option<(SpreadRef, Point)> {
    let st = app.session.active()?;
    let layout = CanvasLayout::new(&st.doc, st.editing_parents);
    let slot = layout.slots.get(current_slot(app, &layout)?)?;
    let v = app.view()?;
    let r = app.canvas_rect?;
    let c = Point::new(v.origin.x + r.width() as f64 / 2.0 / v.zoom, v.origin.y + r.height() as f64 / 2.0 / v.zoom);
    Some((slot.spread, slot.to_spread(c)))
}

/// Where a file dropped on the window lands: the spread under the pointer, or under the view
/// centre when the pointer isn't over the canvas, and that point in spread coordinates.
pub fn drop_point(app: &DesignApp, screen: Option<Pos2>) -> Option<(SpreadRef, Point)> {
    let rect = app.canvas_rect?;
    let Some(pos) = screen.filter(|p| rect.contains(*p)) else { return view_centre(app) };
    let st = app.session.active()?;
    let layout = CanvasLayout::new(&st.doc, st.editing_parents);
    layout.spread_at(Xf::new(rect, app.view()?).to_canvas(pos))
}

/// Absolute page index under the view centre.
pub fn current_page(app: &DesignApp) -> Option<usize> {
    let st = app.session.active()?;
    let layout = CanvasLayout::new(&st.doc, st.editing_parents);
    let i = current_slot(app, &layout)?;
    let slot = layout.slots.get(i)?;
    let SpreadRef::Doc(si) = slot.spread else { return None };
    let v = app.view()?;
    let r = app.canvas_rect?;
    let c = slot.to_spread(Point::new(v.origin.x + r.width() as f64 / 2.0 / v.zoom, slot.bounds.center().y));
    let pi = st.doc.spreads[si].page_at_x(c.x).unwrap_or(0);
    Some(st.doc.first_page_of_spread(si) + pi)
}

/// The Page tool selection, or the page in view, without crossing canvas modes.
pub fn page_target(app: &DesignApp) -> Option<(SpreadRef, usize)> {
    let st = app.session.active()?;
    let selected = CanvasLayout::new(&st.doc, st.editing_parents).selected_page(&st.doc, &st.selection);
    if st.editing_parents && st.selection.parent_page.is_some() {
        return selected;
    }
    selected.or_else(|| current_spread_page(app))
}

/// The spread (document or parent) under the view centre and the page index within it: where
/// commands that act "on the page I am working on" should go.
pub fn current_spread_page(app: &DesignApp) -> Option<(SpreadRef, usize)> {
    let st = app.session.active()?;
    let layout = CanvasLayout::new(&st.doc, st.editing_parents);
    let i = current_slot(app, &layout)?;
    let slot = layout.slots.get(i)?;
    let v = app.view()?;
    let r = app.canvas_rect?;
    let c = slot.to_spread(Point::new(v.origin.x + r.width() as f64 / 2.0 / v.zoom, slot.bounds.center().y));
    let pg = st.doc.spread(slot.spread)?.page_at_x(c.x).unwrap_or(0);
    Some((slot.spread, pg))
}

/// The selected parent page, or the parent page under the view centre.
pub fn current_parent_page(app: &DesignApp) -> Option<(usize, usize)> {
    let (SpreadRef::Parent(si), pi) = page_target(app)? else { return None };
    Some((si, pi))
}

/// The page box's entries (the status bar's, and Properties › Page), as InDesign's: the
/// document's pages by name, then the parents by label ("A-Parent").
pub struct PageBox {
    pub names: Vec<String>,
    /// The entry in view: the parent shown while parents are edited, else the page.
    pub current: usize,
    /// How many of `names` are pages; the parents follow.
    pub pages: usize,
    parents: Vec<designcraft_doc::SpreadId>,
}

impl PageBox {
    /// The name of the entry in view ("3", "A-Parent").
    pub fn current_name(&self) -> &str {
        self.names.get(self.current).map_or("", String::as_str)
    }
}

pub fn page_box(app: &DesignApp) -> Option<PageBox> {
    let st = app.session.active()?;
    let pages = st.doc.page_count();
    let mut names: Vec<String> = (0..pages).map(|i| app.session.page_label(i)).collect();
    names.extend(st.doc.parents.iter().map(|p| p.parent.as_ref().map(|i| i.label()).unwrap_or_default()));
    let current = match current_parent_page(app) {
        Some((pi, _)) => pages + pi,
        None => current_page(app).unwrap_or(0).min(pages.saturating_sub(1)),
    };
    Some(PageBox { names, current, pages, parents: st.doc.parents.iter().map(|p| p.id).collect() })
}

/// Shows entry `i` of `b`: a page (leaving the parents, if they are shown) or a parent.
pub fn page_box_go(app: &mut DesignApp, b: &PageBox, i: usize) {
    if i < b.pages {
        if app.session.active().is_some_and(|st| st.editing_parents) {
            let _ = app.run("layout.parents.edit", serde_json::json!({"on": false}));
            // Fit the page's spread first, then centre the page: a later refit would centre the
            // spread, and the page box would then show its other page.
            let spread = app.session.active().and_then(|st| st.doc.page_loc(i)).map(|(si, _)| SpreadRef::Doc(si));
            if let Some(r) = spread {
                show_spread(app, r);
            }
        }
        go_to_page(app, i);
    } else if let Some(id) = b.parents.get(i - b.pages) {
        let _ = app.run("layout.parents.edit", serde_json::json!({"parent": id.0}));
    }
}

/// Scroll so absolute page `abs` is centred (keeps zoom). A jump Go Back can return from.
pub fn go_to_page(app: &mut DesignApp, abs: usize) {
    note_jump(app, false);
    centre_page(app, abs);
    note_jump(app, true);
}

fn centre_page(app: &mut DesignApp, abs: usize) {
    let Some(st) = app.session.active() else { return };
    let layout = CanvasLayout::new(&st.doc, false);
    let Some(pr) = layout.page_rect(&st.doc, abs) else { return };
    let Some(rect) = app.canvas_rect else { return };
    if let Some(v) = app.view_mut() {
        v.origin = Point::new(pr.center().x - rect.width() as f64 / 2.0 / v.zoom, pr.center().y - rect.height() as f64 / 2.0 / v.zoom);
    }
}

/// The document's pages by id, in page order.
fn page_ids(doc: &Document) -> Vec<designcraft_doc::ids::PageId> {
    doc.spreads.iter().flat_map(|s| s.pages.iter().map(|p| p.id)).collect()
}

/// The page in view and the view centre's offset from that page's centre. Parent pages have none,
/// nor has a view still waiting to be fitted (leaving parent pages for a document page).
pub(crate) fn page_in_view(app: &DesignApp) -> Option<(designcraft_doc::ids::PageId, DVec2)> {
    let st = app.session.active().filter(|st| !st.editing_parents)?;
    let (v, r) = (app.view().filter(|v| v.fitted)?, app.canvas_rect?);
    let abs = current_page(app)?;
    let id = st.doc.page(abs)?.id;
    let pr = CanvasLayout::new(&st.doc, false).page_rect(&st.doc, abs)?;
    let c = Point::new(v.origin.x + r.width() as f64 / 2.0 / v.zoom, v.origin.y + r.height() as f64 / 2.0 / v.zoom);
    Some((id, c - pr.center()))
}

/// Layout ▸ Go Back / Go Forward record jumps (Go to Page, the page and spread commands, the Pages
/// panel), not scrolling: where the view is when a jump starts, and where the jump took it.
fn note_jump(app: &mut DesignApp, to: bool) {
    let Some((id, offset)) = page_in_view(app) else { return };
    if let Some(h) = app.page_history_mut() {
        if to { h.visit(id, offset) } else { h.leave(id, offset) }
    }
}

/// Whether Layout ▸ Go Back (`step` < 0) or Go Forward has a page to go to from the view as it is.
pub fn can_go_through_history(app: &DesignApp, step: isize) -> bool {
    let Some(st) = app.session.active().filter(|st| !st.editing_parents) else { return false };
    // A view still waiting to be fitted is refitted next frame, which would replace the destination.
    if !app.view().is_some_and(|v| v.fitted) {
        return false;
    }
    let ids = page_ids(&st.doc);
    let Some(mut h) = app.page_history().cloned() else { return false };
    if let Some((id, offset)) = page_in_view(app) {
        h.leave(id, offset);
    }
    h.can_go(step, |id| ids.contains(&id))
}

/// Layout ▸ Go Back (`step` < 0) or Go Forward: show the page viewed before or after this one, where
/// the view was on it (zoom kept).
pub fn go_through_history(app: &mut DesignApp, step: isize) {
    if !app.view().is_some_and(|v| v.fitted) {
        return;
    }
    note_jump(app, false);
    let Some(ids) = app.session.active().filter(|st| !st.editing_parents).map(|st| page_ids(&st.doc)) else { return };
    let Some((id, offset)) = app.page_history_mut().and_then(|h| h.go(step, |id| ids.contains(&id))) else { return };
    let Some(st) = app.session.active() else { return };
    let Some(pr) = ids.iter().position(|p| *p == id).and_then(|abs| CanvasLayout::new(&st.doc, false).page_rect(&st.doc, abs)) else { return };
    let Some(rect) = app.canvas_rect else { return };
    let c = pr.center() + offset;
    if let Some(v) = app.view_mut() {
        v.origin = Point::new(c.x - rect.width() as f64 / 2.0 / v.zoom, c.y - rect.height() as f64 / 2.0 / v.zoom);
    }
}

/// Presentation: the spread in view, centred and as large as `rect` allows.
pub(super) fn present_fit(app: &mut DesignApp, rect: Rect) {
    let Some(st) = app.session.active() else { return };
    let layout = CanvasLayout::new(&st.doc, st.editing_parents);
    let Some(b) = current_slot(app, &layout).and_then(|i| layout.slots.get(i)).map(|s| s.bounds) else { return };
    if b.width() <= 0.0 || b.height() <= 0.0 {
        return;
    }
    let zoom = (rect.width() as f64 / b.width()).min(rect.height() as f64 / b.height()).clamp(0.05, 40.0);
    let origin = Point::new(b.center().x - rect.width() as f64 / 2.0 / zoom, b.center().y - rect.height() as f64 / 2.0 / zoom);
    if let Some(v) = app.view_mut() {
        v.zoom = zoom;
        v.origin = origin;
    }
}

/// Centre the spread `delta` spreads after (or before) the one in view (keeps zoom); stops at the
/// first and last spread. Parent spreads while they are being edited.
pub fn step_spread(app: &mut DesignApp, delta: isize) {
    // At the first or last spread only the view moves, within the spread: no jump for Go Back.
    let jump = spread_step(app, delta).is_some_and(|(cur, to)| cur != to);
    if jump {
        note_jump(app, false);
    }
    centre_spread(app, delta);
    if jump {
        note_jump(app, true);
    }
}

/// The slot in view and the one `delta` spreads after (or before) it, stopping at the ends.
fn spread_step(app: &DesignApp, delta: isize) -> Option<(usize, usize)> {
    let st = app.session.active()?;
    let layout = CanvasLayout::new(&st.doc, st.editing_parents);
    let cur = current_slot(app, &layout)?;
    Some((cur, cur.saturating_add_signed(delta).min(layout.slots.len().saturating_sub(1))))
}

fn centre_spread(app: &mut DesignApp, delta: isize) {
    let Some((_, to)) = spread_step(app, delta) else { return };
    let Some(st) = app.session.active() else { return };
    let layout = CanvasLayout::new(&st.doc, st.editing_parents);
    let Some(b) = layout.slots.get(to).map(|s| s.bounds) else { return };
    let Some(rect) = app.canvas_rect else { return };
    if let Some(v) = app.view_mut() {
        v.origin = Point::new(b.center().x - rect.width() as f64 / 2.0 / v.zoom, b.center().y - rect.height() as f64 / 2.0 / v.zoom);
    }
}

pub fn zoom_at(app: &mut DesignApp, screen: Pos2, factor: f64) {
    let Some(rect) = app.canvas_rect else { return };
    let Some(v) = app.view_mut() else { return };
    let mut xf = Xf::new(rect, v).inner;
    xf.zoom_about(screen, v.zoom * factor, &ZoomPolicy { min: 0.05, max: 40.0, steps: &[], anchor: Anchor::Pointer });
    let [x, y] = xf.origin();
    (v.zoom, v.origin) = (xf.zoom, Point::new(x, y));
}

pub fn set_zoom(app: &mut DesignApp, z: f64) {
    let Some(rect) = app.canvas_rect else { return };
    let cur = app.view().map(|v| v.zoom).unwrap_or(1.0);
    zoom_at(app, rect.center(), z / cur);
}

pub fn apply_view_request(app: &mut DesignApp, p: &Value) {
    if let Some(r) = p.get("showSpread").and_then(|v| serde_json::from_value::<SpreadRef>(v.clone()).ok()) {
        show_spread(app, r);
    }
    // Power Zoom (Hand tool, Alt-press): zoom out to the spread, aim a view-sized rectangle, zoom
    // back in where it was released.
    if let Some(phase) = p.get("powerZoom").and_then(Value::as_str) {
        let at = p.get("at").and_then(Value::as_array).map(|a| Point::new(a[0].as_f64().unwrap_or(0.0), a[1].as_f64().unwrap_or(0.0)));
        match phase {
            "start" => {
                if let (Some(v), Some(rect)) = (app.view().copied(), app.canvas_rect) {
                    let centre = Xf::new(rect, &v).to_canvas(rect.center());
                    app.power_zoom = Some((v.zoom, at.unwrap_or(centre)));
                    fit(app, rect, "spread");
                }
            }
            "move" => {
                if let (Some(pz), Some(a)) = (&mut app.power_zoom, at) {
                    pz.1 = a;
                }
            }
            _ => {
                if let (Some((z, c)), Some(rect)) = (app.power_zoom.take(), app.canvas_rect)
                    && let Some(v) = app.view_mut()
                {
                    let c = at.unwrap_or(c);
                    v.zoom = z;
                    v.origin = Point::new(c.x - rect.width() as f64 / 2.0 / z, c.y - rect.height() as f64 / 2.0 / z);
                }
            }
        }
    }
    if let Some(d) = p.get("pan").and_then(Value::as_array) {
        let (dx, dy) = (d.first().and_then(Value::as_f64).unwrap_or(0.0), d.get(1).and_then(Value::as_f64).unwrap_or(0.0));
        if let Some(v) = app.view_mut() {
            v.origin = Point::new(v.origin.x - dx / v.zoom, v.origin.y - dy / v.zoom);
        }
    }
    if let (Some(a), Some(rect)) = (p.get("zoomAt").and_then(Value::as_array), app.canvas_rect)
        && let Some(v) = app.view()
    {
        let c = Point::new(a[0].as_f64().unwrap_or(0.0), a[1].as_f64().unwrap_or(0.0));
        let xf = Xf::new(rect, v);
        let s = xf.to_screen(c);
        zoom_at(app, s, p.get("factor").and_then(Value::as_f64).unwrap_or(2.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, vec2};
    struct OldXf {
        min: Pos2,
        origin: Point,
        zoom: f64,
        rot: u8,
        pivot: Pos2,
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
    impl OldXf {
        fn new(rect: Rect, v: &View) -> Self {
            Self { min: rect.min, origin: v.origin, zoom: v.zoom, rot: v.rotation % 4, pivot: rect.center() }
        }
        fn to_screen(&self, p: Point) -> Pos2 {
            let s = pos2(self.min.x + ((p.x - self.origin.x) * self.zoom) as f32, self.min.y + ((p.y - self.origin.y) * self.zoom) as f32);
            if self.rot == 0 { s } else { quarter(s, self.pivot, self.rot) }
        }
        fn unrotate(&self, p: Pos2) -> Pos2 {
            if self.rot == 0 { p } else { quarter(p, self.pivot, 4 - self.rot) }
        }
        fn to_canvas(&self, p: Pos2) -> Point {
            let p = self.unrotate(p);
            Point::new(self.origin.x + (p.x - self.min.x) as f64 / self.zoom, self.origin.y + (p.y - self.min.y) as f64 / self.zoom)
        }
    }
    fn old_zoom_at(app: &mut DesignApp, screen: Pos2, factor: f64) {
        let (Some(rect), Some(v)) = (app.canvas_rect, app.view_mut()) else { return };
        let xf = OldXf::new(rect, v);
        let (c, screen, nz) = (xf.to_canvas(screen), xf.unrotate(screen), (v.zoom * factor).clamp(0.05, 40.0));
        (v.zoom, v.origin) = (nz, Point::new(c.x - (screen.x - rect.min.x) as f64 / nz, c.y - (screen.y - rect.min.y) as f64 / nz));
    }
    #[test]
    fn craft_view_identity_2000_cases() {
        let mut seed = 0x72_cafe_f00d_u64;
        let mut next = |lo: f64, hi: f64| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            lo + (hi - lo) * ((seed >> 11) as f64 / ((1u64 << 53) as f64))
        };
        let mut app_a = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        let mut app_b = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app_a.run("file.new", serde_json::json!({})).unwrap();
        app_b.run("file.new", serde_json::json!({})).unwrap();
        for i in 0..2000 {
            let min = pos2(next(-3000.0, 3000.0).round() as f32, next(-3000.0, 3000.0).round() as f32);
            let rect = Rect::from_min_size(min, vec2(next(1.0, 2000.0).round() as f32, next(1.0, 2000.0).round() as f32));
            let v =
                View { origin: Point::new(next(-1e5, 1e5), next(-1e5, 1e5)), zoom: next(0.05, 40.0), fitted: i % 2 == 0, rotation: (i % 4) as u8 };
            let screen = pos2(next(-3000.0, 3000.0).round() as f32, next(-3000.0, 3000.0).round() as f32);
            let (factor, pt) = (next(0.25, 4.0), Point::new(v.origin.x + next(-500.0, 500.0) / v.zoom, v.origin.y + next(-500.0, 500.0) / v.zoom));
            let (old, new) = (OldXf::new(rect, &v), Xf::new(rect, &v));
            let (ds, dc, oc) = (new.to_screen(pt) - old.to_screen(pt), new.to_canvas(screen), old.to_canvas(screen));
            assert!(ds.x.abs() <= 1e-3 && ds.y.abs() <= 1e-3 && (dc.x - oc.x).abs() <= 1e-9 && (dc.y - oc.y).abs() <= 1e-9, "case {i}");
            for a in [&mut app_a, &mut app_b] {
                (a.canvas_rect, *a.view_mut().unwrap()) = (Some(rect), v);
            }
            old_zoom_at(&mut app_a, screen, factor);
            zoom_at(&mut app_b, screen, factor);
            let (va, vb) = (app_a.view().unwrap(), app_b.view().unwrap());
            assert!(vb.zoom == va.zoom && (vb.origin.x - va.origin.x).abs() <= 1e-9 && (vb.origin.y - va.origin.y).abs() <= 1e-9, "case {i}");
        }
    }
}
