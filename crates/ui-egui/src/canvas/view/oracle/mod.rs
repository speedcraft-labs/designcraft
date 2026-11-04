//! #188: OLD (hand-written quarter-turn `Xf`) vs NEW (`craft-view`) canvas view transforms.
//!
//! OLD is vendored in `old.rs` (designcraft @ e79d682e02dc66f14370958b193bfa46072b2f41, the
//! merge-base of `algo/72-use-craft-view-v2` with `origin/main`); NEW is this crate's `Xf`,
//! `zoom_at` and `pan`. Compared over random viewports, origins, zooms and all four quarter turns:
//! canvas<->screen, screen deltas, rects, the vello affine, zoom about a point, and pan.
//! `fit` is unchanged by the swap (it never touches `Xf`) and is covered by the existing tests.
//!
//! Tolerance: f64 outputs `|a - b| <= 1e-9 * max(1, |a|, |b|)`. Outputs that are `f32` screen
//! points are compared to 8 f32 ulps of the largest quantity in the sum (OLD rounds to f32 before
//! rotating while NEW rounds once at the end). OLD also does its screen arithmetic in f32
//! (`p.x - min.x`, `pivot + d - pivot`) where NEW works in f64, so for arbitrary f32 screen points
//! the two differ by that f32 rounding divided by the zoom: the `*_f32_bound` tests assert that
//! bound, and the `*_exact` tests (quarter-point inputs, where f32 arithmetic is exact) assert 1e-9.

mod old;

use super::super::scroll::pan;
use super::*;
use egui::{pos2, vec2};
use old::*;
use proptest::prelude::*;
use serde_json::json;

const TOL: f64 = 1e-9;

fn close(a: f64, b: f64) -> bool {
    (a.is_finite() && b.is_finite()) && (a - b).abs() <= TOL * 1.0f64.max(a.abs()).max(b.abs())
}

fn close32(a: f32, b: f32) -> bool {
    (a - b).abs() <= 4.0 * f32::EPSILON * 1.0f32.max(a.abs()).max(b.abs())
}

/// Round to a quarter point, so that f32 sums and differences of such values below 2^15 are exact.
fn snap(x: f32) -> f32 {
    (x * 4.0).round() / 4.0
}

/// A view rectangle with every f32 coordinate on the quarter-point grid (centre on the eighth).
fn rect() -> impl Strategy<Value = Rect> {
    (-3000.0f32..3000.0, -3000.0f32..3000.0, 1.0f32..2000.0, 1.0f32..2000.0)
        .prop_map(|(x, y, w, h)| Rect::from_min_size(pos2(snap(x), snap(y)), vec2((w / 2.0).round() * 2.0, (h / 2.0).round() * 2.0)))
}

fn rect_any() -> impl Strategy<Value = Rect> {
    (-3000.0f32..3000.0, -3000.0f32..3000.0, 1.0f32..2000.0, 1.0f32..2000.0).prop_map(|(x, y, w, h)| Rect::from_min_size(pos2(x, y), vec2(w, h)))
}

fn view() -> impl Strategy<Value = View> {
    (0.05f64..40.0, -1e5f64..1e5, -1e5f64..1e5, 0u8..=255).prop_map(|(zoom, x, y, rotation)| View {
        zoom,
        origin: Point::new(x, y),
        fitted: true,
        rotation,
    })
}

fn app() -> DesignApp {
    let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
    app.run("file.new", json!({})).unwrap();
    app
}

/// The most NEW (f64) and OLD (f32 screen arithmetic) can differ by in a canvas coordinate:
/// 8 f32 epsilons of the largest screen quantity involved, over the smallest zoom (a screen error
/// of `e` points is `e / zoom` canvas units).
fn f32_bound(r: Rect, screen: &[f32], zoom_a: f64, zoom_b: f64) -> f64 {
    let s = [r.min.x, r.min.y, r.max.x, r.max.y].iter().chain(screen).fold(1.0f32, |m, v| m.max(v.abs()));
    8.0 * f64::from(f32::EPSILON) * f64::from(s) / zoom_a.min(zoom_b)
}

fn near(a: f64, b: f64, abs: f64) -> bool {
    (a - b).abs() <= abs + TOL * 1.0f64.max(a.abs()).max(b.abs())
}

fn ok32(a: f32, b: f32, mag: f32) -> bool {
    (a - b).abs() <= 8.0 * f32::EPSILON * 1.0f32.max(a.abs()).max(b.abs()).max(mag)
}

fn setup(r: Rect, v: View) -> (DesignApp, DesignApp) {
    let (mut a, mut b) = (app(), app());
    for x in [&mut a, &mut b] {
        x.canvas_rect = Some(r);
        *x.view_mut().unwrap() = v;
    }
    (a, b)
}

fn assert_views(a: &View, b: &View, tol: f64) -> Result<(), TestCaseError> {
    prop_assert!(close(a.zoom, b.zoom) && near(a.origin.x, b.origin.x, tol) && near(a.origin.y, b.origin.y, tol), "old {a:?} new {b:?} bound {tol}");
    prop_assert_eq!(a.rotation, b.rotation);
    prop_assert_eq!(a.fitted, b.fitted);
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 3000, ..ProptestConfig::default() })]

    /// Exact f32 inputs (quarter points): every mapping agrees to 1e-9 (f32 outputs to 4 ulps).
    #[test]
    fn xf_maps_agree_exact(r in rect(), v in view(), px in -1e5f64..1e5, py in -1e5f64..1e5, sx in -4000.0f32..4000.0, sy in -4000.0f32..4000.0, ppp in 1.0f64..3.0) {
        let (sx, sy) = (snap(sx), snap(sy));
        let (o, n) = (OldXf::new(r, &v), Xf::new(r, &v));
        let mag = ((px - v.origin.x).abs().max((py - v.origin.y).abs()) * v.zoom) as f32;
        let (a, b) = (o.to_screen(Point::new(px, py)), n.to_screen(Point::new(px, py)));
        prop_assert!(ok32(a.x, b.x, mag) && ok32(a.y, b.y, mag), "to_screen old {a:?} new {b:?}");
        let (a, b) = (o.to_canvas(pos2(sx, sy)), n.to_canvas(pos2(sx, sy)));
        prop_assert!(close(a.x, b.x) && close(a.y, b.y), "to_canvas old {a:?} new {b:?}");
        let d = vec2(snap(sx / 10.0), snap(sy / 10.0));
        let (a, b) = (o.unrotate_delta(d), n.unrotate_delta(d));
        prop_assert!(ok32(a.x, b.x, 0.0) && ok32(a.y, b.y, 0.0), "unrotate_delta old {a:?} new {b:?}");
        let dr = DRect::new(px, py, px + 300.0, py + 200.0);
        let (a, b) = (o.rect(dr), n.rect(dr));
        prop_assert!(ok32(a.min.x, b.min.x, mag) && ok32(a.min.y, b.min.y, mag) && ok32(a.max.x, b.max.x, mag) && ok32(a.max.y, b.max.y, mag), "rect old {a:?} new {b:?}");
        let (a, b) = (o.affine(ppp, r.min).as_coeffs(), n.affine(ppp, r.min).as_coeffs());
        for k in 0..6 {
            prop_assert!(close(a[k], b[k]), "affine[{k}] old {a:?} new {b:?}");
        }
        prop_assert_eq!((o.rot, o.zoom, o.origin), (n.rot, n.zoom, n.origin));
    }

    /// Arbitrary f32 screen points: canvas coordinates differ by at most the f32 rounding OLD does.
    #[test]
    fn xf_maps_agree_f32_bound(r in rect_any(), v in view(), sx in -4000.0f32..4000.0, sy in -4000.0f32..4000.0) {
        let (o, n) = (OldXf::new(r, &v), Xf::new(r, &v));
        let (a, b) = (o.to_canvas(pos2(sx, sy)), n.to_canvas(pos2(sx, sy)));
        let tol = f32_bound(r, &[sx, sy], v.zoom, v.zoom);
        prop_assert!(near(a.x, b.x, tol) && near(a.y, b.y, tol), "to_canvas old {a:?} new {b:?} bound {tol}");
        let d = vec2(sx / 10.0, sy / 10.0);
        let (a, b) = (o.unrotate_delta(d), n.unrotate_delta(d));
        let m = f32_bound(r, &[], 1.0, 1.0) as f32 / 8.0 / f32::EPSILON;
        prop_assert!(ok32(a.x, b.x, m) && ok32(a.y, b.y, m), "unrotate_delta old {a:?} new {b:?}");
    }

    /// Zoom about a screen point, through the real `zoom_at`.
    #[test]
    fn zoom_at_agrees_exact(r in rect(), v in view(), sx in -3000.0f32..3000.0, sy in -3000.0f32..3000.0, factor in 0.05f64..8.0) {
        let (mut a, mut b) = setup(r, v);
        let p = pos2(snap(sx), snap(sy));
        old_zoom_at(&mut a, p, factor);
        zoom_at(&mut b, p, factor);
        assert_views(a.view().unwrap(), b.view().unwrap(), 0.0)?;
    }

    #[test]
    fn zoom_at_agrees_f32_bound(r in rect_any(), v in view(), sx in -3000.0f32..3000.0, sy in -3000.0f32..3000.0, factor in 0.05f64..8.0) {
        let (mut a, mut b) = setup(r, v);
        old_zoom_at(&mut a, pos2(sx, sy), factor);
        zoom_at(&mut b, pos2(sx, sy), factor);
        let (va, vb) = (*a.view().unwrap(), *b.view().unwrap());
        assert_views(&va, &vb, f32_bound(r, &[sx, sy], v.zoom, va.zoom))?;
    }

    /// Pan by a screen distance, through the real `pan`, in every quarter turn.
    #[test]
    fn pan_agrees_exact(r in rect(), v in view(), dx in -2000.0f32..2000.0, dy in -2000.0f32..2000.0) {
        let (mut a, mut b) = setup(r, v);
        let d = vec2(snap(dx), snap(dy));
        old_pan(&mut a, &OldXf::new(r, &v), d);
        pan(&mut b, &Xf::new(r, &v), d);
        assert_views(a.view().unwrap(), b.view().unwrap(), 0.0)?;
    }

    #[test]
    fn pan_agrees_f32_bound(r in rect_any(), v in view(), dx in -2000.0f32..2000.0, dy in -2000.0f32..2000.0) {
        let (mut a, mut b) = setup(r, v);
        old_pan(&mut a, &OldXf::new(r, &v), vec2(dx, dy));
        pan(&mut b, &Xf::new(r, &v), vec2(dx, dy));
        assert_views(a.view().unwrap(), b.view().unwrap(), f32_bound(r, &[dx, dy], v.zoom, v.zoom))?;
    }

    /// A run of zooms and pans on exact inputs, so rounding differences would accumulate.
    #[test]
    fn zoom_pan_sequences_agree_exact(r in rect(), v in view(), steps in prop::collection::vec((any::<bool>(), -1500.0f32..1500.0, -1500.0f32..1500.0, 0.2f64..5.0), 1..12)) {
        let (mut a, mut b) = setup(r, v);
        for (zoom, x, y, f) in steps {
            let (x, y) = (snap(x), snap(y));
            if zoom {
                old_zoom_at(&mut a, pos2(x, y), f);
                zoom_at(&mut b, pos2(x, y), f);
            } else {
                let (va, vb) = (*a.view().unwrap(), *b.view().unwrap());
                old_pan(&mut a, &OldXf::new(r, &va), vec2(x, y));
                pan(&mut b, &Xf::new(r, &vb), vec2(x, y));
            }
            assert_views(a.view().unwrap(), b.view().unwrap(), 0.0)?;
        }
    }
}

/// Quarter turns 0..3 (and 4..7, which wrap): the corners of a canvas rect land on the same
/// screen points, and a screen point maps back to the same canvas point.
#[test]
fn quarter_turns_map_the_same_corners() {
    let r = Rect::from_min_size(pos2(40.0, 30.0), vec2(1000.0, 600.0));
    for rotation in 0u8..8 {
        let v = View { zoom: 1.25, origin: Point::new(-120.0, 75.5), fitted: true, rotation };
        let (o, n) = (OldXf::new(r, &v), Xf::new(r, &v));
        let rv = old_view_rect(r, rotation);
        assert_eq!(rv, super::super::view_rect(r, rotation));
        for (x, y) in [(0.0, 0.0), (500.0, 0.0), (500.0, 400.0), (0.0, 400.0), (-80.0, 30.0)] {
            let (a, b) = (o.to_screen(Point::new(x, y)), n.to_screen(Point::new(x, y)));
            assert!(close32(a.x, b.x) && close32(a.y, b.y), "rotation {rotation} ({x}, {y}): old {a:?} new {b:?}");
            let (ca, cb) = (o.to_canvas(b), n.to_canvas(b));
            assert!(close(ca.x, cb.x) && close(ca.y, cb.y), "rotation {rotation}: old {ca:?} new {cb:?}");
            assert!((cb.x - x).abs() < 1e-3 && (cb.y - y).abs() < 1e-3, "round trip drifted: {cb:?} vs ({x}, {y})");
        }
    }
}

/// The comparison is sensitive: a different zoom factor is caught.
#[test]
fn comparison_detects_a_difference() {
    let r = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
    let v = View { zoom: 1.0, origin: Point::new(0.0, 0.0), fitted: true, rotation: 1 };
    let (mut a, mut b) = setup(r, v);
    old_zoom_at(&mut a, pos2(100.0, 100.0), 2.0);
    zoom_at(&mut b, pos2(100.0, 100.0), 2.0);
    assert!(assert_views(a.view().unwrap(), b.view().unwrap(), 0.0).is_ok());
    zoom_at(&mut b, pos2(100.0, 100.0), 1.000001);
    assert!(assert_views(a.view().unwrap(), b.view().unwrap(), 0.0).is_err());
}
