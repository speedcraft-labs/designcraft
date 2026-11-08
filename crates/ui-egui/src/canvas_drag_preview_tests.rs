use super::*;
use crate::Shown;

fn move_case(overlap: bool, multiple: bool) -> (DesignApp, egui::Context, Rect, Xf, CanvasLayout) {
    let mut session = designcraft_engine::Session::new();
    session.execute("file.new", &json!({"pages": 1, "facingPages": false, "width": 100, "height": 100})).unwrap();
    let first = session.execute("frame.create", &json!({"rect": [20, 20, 40, 40], "content": "unassigned"})).unwrap();
    let id = designcraft_doc::ItemId(first["id"].as_u64().unwrap());
    session.execute("object.fill", &json!({"ids": [id.0], "swatch": "[Black]"})).unwrap();
    session.execute("object.stroke", &json!({"ids": [id.0], "swatch": "[None]"})).unwrap();
    let mut ids = vec![id];
    if overlap || multiple {
        let second = session.execute("frame.create", &json!({"rect": [35, 20, 55, 40], "content": "unassigned"})).unwrap();
        let sibling = designcraft_doc::ItemId(second["id"].as_u64().unwrap());
        session.execute("object.fill", &json!({"ids": [sibling.0], "swatch": "[Black]"})).unwrap();
        session.execute("object.stroke", &json!({"ids": [sibling.0], "swatch": "[None]"})).unwrap();
        if multiple {
            ids.push(sibling);
        }
    }
    session.active_mut().unwrap().selection.items = ids.clone();
    let moved = ids.iter().map(|id| id.0).collect::<Vec<_>>();
    let base = session.active().unwrap().doc.clone();
    session.begin_interaction("Move").unwrap();
    session.preview("transform.move", json!({"ids": moved, "dx": 10, "dy": 0})).unwrap();

    let mut app = DesignApp::new(session, crate::Services::default());
    let ctx = egui::Context::default();
    let shown_texture =
        ctx.load_texture("canvas drag preview test", egui::ColorImage::filled([100, 100], Color32::WHITE), egui::TextureOptions::LINEAR);
    let st = app.session.active().unwrap();
    let interaction = st.interaction.as_ref().unwrap();
    let shown = Shown {
        doc: (st.uid, std::sync::Arc::as_ptr(&base) as u64, interaction.revision, false),
        editing_parents: false,
        origin: designcraft_geom::Point::new(0.0, 0.0),
        zoom: 1.0,
        size: (100.0, 100.0),
    };
    app.canvas.shown = Some(shown);
    app.canvas.shown_doc = Some(base);
    app.canvas.texture = Some(shown_texture);
    let rect = Rect::from_min_size(Pos2::ZERO, vec2(100.0, 100.0));
    let xf = Xf { min: rect.min, origin: designcraft_geom::Point::new(0.0, 0.0), zoom: 1.0, rot: 0, pivot: rect.center() };
    let layout = CanvasLayout::new(&st.doc, false);
    (app, ctx, rect, xf, layout)
}

fn candidate(app: &DesignApp, ctx: &egui::Context, rect: Rect, xf: &Xf, layout: &CanvasLayout) -> Option<render::DragPreview> {
    let st = app.session.active()?;
    let key = (st.uid, std::sync::Arc::as_ptr(&st.doc) as u64, st.revision, false);
    render::drag_preview(app, ctx, rect, xf, layout, key, false)
}

#[test]
fn isolated_opaque_rectangle_move_uses_the_gpu_preview() {
    let (app, ctx, rect, xf, layout) = move_case(false, false);
    assert!(candidate(&app, &ctx, rect, &xf, &layout).is_some(), "simple move should be previewed with rich-black display enabled");
}

#[test]
fn overlap_stale_background_and_multiple_selection_use_cpu_fallback() {
    let (app, ctx, rect, xf, layout) = move_case(true, false);
    assert!(candidate(&app, &ctx, rect, &xf, &layout).is_none(), "overlapping sibling needs normal z-order rendering");

    let (mut stale, ctx, rect, xf, layout) = move_case(false, false);
    stale.canvas.shown_doc = stale.session.active().map(|state| state.doc.clone());
    assert!(candidate(&stale, &ctx, rect, &xf, &layout).is_none(), "a stale CPU texture must not be masked");

    let (multiple, ctx, rect, xf, layout) = move_case(false, true);
    assert!(candidate(&multiple, &ctx, rect, &xf, &layout).is_none(), "multiple selection stays on the CPU renderer");
}
