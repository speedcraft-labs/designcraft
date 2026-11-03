use super::*;
use craft_view::ViewXform;

#[derive(Clone, Copy)]
struct OldXf {
    min: Pos2,
    origin: Point,
    zoom: f64,
    rot: u8,
    pivot: Pos2,
}

fn old_quarter(p: Pos2, c: Pos2, q: u8) -> Pos2 {
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

    fn to_screen(self, p: Point) -> Pos2 {
        let s = pos2(self.min.x + ((p.x - self.origin.x) * self.zoom) as f32, self.min.y + ((p.y - self.origin.y) * self.zoom) as f32);
        if self.rot == 0 { s } else { old_quarter(s, self.pivot, self.rot) }
    }

    fn unrotate(self, p: Pos2) -> Pos2 {
        if self.rot == 0 { p } else { old_quarter(p, self.pivot, 4 - self.rot) }
    }

    fn unrotate_delta(self, d: egui::Vec2) -> egui::Vec2 {
        self.unrotate(self.pivot + d) - self.pivot
    }

    fn to_canvas(self, p: Pos2) -> Point {
        let p = self.unrotate(p);
        Point::new(self.origin.x + (p.x - self.min.x) as f64 / self.zoom, self.origin.y + (p.y - self.min.y) as f64 / self.zoom)
    }
}

fn old_zoom_at(app: &mut DesignApp, screen: Pos2, factor: f64) {
    let Some(rect) = app.canvas_rect else { return };
    let Some(v) = app.view_mut() else { return };
    let xf = OldXf::new(rect, v);
    let c = xf.to_canvas(screen);
    let screen = xf.unrotate(screen);
    let nz = (v.zoom * factor).clamp(0.05, 40.0);
    v.zoom = nz;
    v.origin = Point::new(c.x - (screen.x - rect.min.x) as f64 / nz, c.y - (screen.y - rect.min.y) as f64 / nz);
}

fn old_set_zoom(app: &mut DesignApp, z: f64) {
    let Some(rect) = app.canvas_rect else { return };
    let cur = app.view().map(|v| v.zoom).unwrap_or(1.0);
    old_zoom_at(app, rect.center(), z / cur);
}

struct Seeded(u64);

impl Seeded {
    fn range(&mut self, min: f64, max: f64) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        min + (max - min) * ((self.0 >> 11) as f64 / ((1_u64 << 53) as f64))
    }

    fn screen(&mut self) -> Pos2 {
        pos2(self.range(-2000.0, 4000.0) as f32, self.range(-2000.0, 4000.0) as f32)
    }
}

fn app() -> DesignApp {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app
}

fn assert_point(actual: Point, expected: Point, case: usize) {
    assert!((actual.x - expected.x).abs() <= 1e-9 && (actual.y - expected.y).abs() <= 1e-9, "case {case}: {actual:?} != {expected:?}");
}

fn assert_view(actual: &View, expected: &View, case: usize) {
    assert_point(actual.origin, expected.origin, case);
    assert_eq!(actual.zoom, expected.zoom, "case {case}");
    assert_eq!(actual.rotation, expected.rotation, "case {case}");
    assert_eq!(actual.fitted, expected.fitted, "case {case}");
}

fn assert_xf(rect: Rect, view: &View, screen: Pos2, point: Point, case: usize) {
    let old = OldXf::new(rect, view);
    let new = Xf::new(rect, view);
    let delta = new.to_screen(point) - old.to_screen(point);
    assert!(delta.x.abs() <= 1e-3 && delta.y.abs() <= 1e-3, "case {case}: screen delta {delta:?}");
    assert_point(new.to_canvas(screen), old.to_canvas(screen), case);
    assert_eq!(new.unrotate(screen), old.unrotate(screen));
    assert_eq!(new.unrotate_delta(screen.to_vec2()), old.unrotate_delta(screen.to_vec2()));
    let expected_rect = if view.rotation % 2 == 1 { Rect::from_center_size(rect.center(), vec2(rect.height(), rect.width())) } else { rect };
    assert_eq!(view_rect(rect, view.rotation), expected_rect);
}

#[test]
fn seeded_old_canvas_identity() {
    let mut random = Seeded(0x72_cafe_f00d);
    let mut old_app = app();
    let mut new_app = app();
    for case in 0..2000 {
        let rect = Rect::from_min_size(random.screen(), vec2(random.range(1.0, 3000.0) as f32, random.range(1.0, 2000.0) as f32));
        let view = View {
            origin: Point::new(random.range(-10000.0, 10000.0), random.range(-10000.0, 10000.0)),
            zoom: random.range(0.05, 40.0),
            rotation: random.range(0.0, 4.0) as u8,
            fitted: case % 2 == 0,
        };
        let screen = random.screen();
        let factor = random.range(0.25, 4.0);
        let point = view.origin + DVec2::new(random.range(-1000.0, 2000.0), random.range(-1000.0, 2000.0));
        assert_xf(rect, &view, screen, point, case);
        for app in [&mut old_app, &mut new_app] {
            app.canvas_rect = Some(rect);
            *app.view_mut().unwrap() = view;
        }
        let anchor = OldXf::new(rect, &view).to_canvas(screen);
        old_zoom_at(&mut old_app, screen, factor);
        zoom_at(&mut new_app, screen, factor);
        let actual = new_app.view().unwrap();
        assert_view(actual, old_app.view().unwrap(), case);
        assert_point(Xf::new(rect, actual).to_canvas(screen), anchor, case);
        assert_xf(rect, actual, screen, point, case);
        for app in [&mut old_app, &mut new_app] {
            *app.view_mut().unwrap() = view;
        }
        old_set_zoom(&mut old_app, view.zoom * factor);
        set_zoom(&mut new_app, view.zoom * factor);
        assert_view(new_app.view().unwrap(), old_app.view().unwrap(), case);
    }
}

#[test]
fn shared_rotation_is_clockwise_about_viewport_centre() {
    let rect = Rect::from_min_size(pos2(20.0, 40.0), vec2(800.0, 600.0));
    for rotation in 0..4 {
        let view = View { origin: Point::new(-50.0, 70.0), zoom: 2.0, rotation, fitted: false };
        let centre = [view.origin.x + f64::from(rect.width()) / 2.0 / view.zoom, view.origin.y + f64::from(rect.height()) / 2.0 / view.zoom];
        let mut shared = ViewXform::new(rect, centre, view.zoom, 1.0);
        shared.rotation = f64::from(rotation) * std::f64::consts::FRAC_PI_2;
        assert_eq!(shared.to_screen(centre), rect.center());
        for point in [Point::new(centre[0] + 25.0, centre[1]), Point::new(centre[0], centre[1] + 25.0)] {
            let screen = OldXf::new(rect, &view).to_screen(point);
            assert!((shared.to_screen([point.x, point.y]) - screen).length() <= 1e-3);
            let [canvas_x, canvas_y] = shared.to_doc(screen);
            assert_point(Point::new(canvas_x, canvas_y), point, usize::from(rotation));
        }
    }
}

#[test]
fn centred_f32_subtraction_cannot_replace_origin_rounding() {
    let rect = Rect::from_min_max(pos2(-426.57333, 0.0), pos2(802.71436, 600.0));
    let view = View { origin: Point::ZERO, zoom: 1.0, ..View::default() };
    let screen = pos2(2957.7878, 100.0);
    let shared = ViewXform::new(rect, [f64::from(rect.width()) / 2.0, f64::from(rect.height()) / 2.0], 1.0, 1.0);
    let expected = OldXf::new(rect, &view).to_canvas(screen);
    assert!((shared.to_doc(screen)[0] - expected.x).abs() > 1e-9);
    assert_point(Xf::new(rect, &view).to_canvas(screen), expected, 0);
}

#[test]
fn zoom_limits_and_quarter_turns_preserve_old_identity() {
    let mut old_app = app();
    let mut new_app = app();
    let rect = Rect::from_min_size(pos2(19.25, -71.125), vec2(887.75, 531.25));
    for rotation in 0..4 {
        for zoom in [0.05, 1.0, 40.0] {
            for factor in [0.25, 1.0, 4.0] {
                for screen in [rect.min, rect.max, rect.center(), pos2(-312.125, 1842.75)] {
                    let view = View { origin: Point::new(100.125, -920.75), zoom, rotation, fitted: true };
                    for app in [&mut old_app, &mut new_app] {
                        app.canvas_rect = Some(rect);
                        *app.view_mut().unwrap() = view;
                    }
                    old_zoom_at(&mut old_app, screen, factor);
                    zoom_at(&mut new_app, screen, factor);
                    assert_view(new_app.view().unwrap(), old_app.view().unwrap(), 0);
                }
            }
        }
    }
}

#[test]
fn zoom_without_canvas_or_document_is_unchanged() {
    let mut app = app();
    let original = *app.view().unwrap();
    app.canvas_rect = None;
    zoom_at(&mut app, pos2(100.0, 200.0), 2.0);
    set_zoom(&mut app, 4.0);
    assert_eq!(*app.view().unwrap(), original);
    let mut empty = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    empty.canvas_rect = Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0)));
    zoom_at(&mut empty, pos2(100.0, 200.0), 2.0);
    set_zoom(&mut empty, 4.0);
    assert!(empty.view().is_none());
}
