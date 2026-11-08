use super::{Affine, CanvasLayout, Color32, DRect, DesignApp, Point, Pos2, Rect, ScreenMode, SpreadRef, Xf};

pub(super) fn c32(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// The colour the canvas shows at screen point `pos` (the Color Picker's eyedropper): one device
/// pixel rendered as the page is, over its paper. None off the pages.
pub fn color_at(app: &mut DesignApp, ctx: &egui::Context, pos: Pos2) -> Option<[u8; 3]> {
    let rect = app.canvas_rect.filter(|r| r.contains(pos))?;
    let v = *app.view()?;
    let xf = Xf::new(rect, &v);
    let at = xf.to_canvas(pos);
    let st = app.session.active()?;
    let doc = st.doc.clone();
    let layout = CanvasLayout::new(&doc, st.editing_parents);
    let on_page = layout
        .slots
        .iter()
        .any(|slot| doc.spread(slot.spread).is_some_and(|sp| sp.pages.iter().any(|page| slot.xf.transform_rect_bbox(page.bounds()).contains(at))));
    if !on_page {
        return None;
    }
    let placed: Vec<designcraft_render::Placed> = layout.slots.iter().map(|s| designcraft_render::Placed { spread: s.spread, xf: s.xf }).collect();
    let ppp = ctx.pixels_per_point() as f64;
    let view = Affine::scale(ppp) * Affine::scale(xf.zoom) * Affine::translate((-at.x, -at.y));
    let opts = screen_options(app, &layout, app.ui.screen_mode != ScreenMode::Normal, ppp);
    let mut img = app.canvas.patcher.render(&doc, &app.session.cache, &placed, (1, 1), view, &opts);
    // The colour as the canvas shows it (proofed).
    screen_post(app, &mut img);
    let [r, g, b, a] = <[u8; 4]>::try_from(img.pixels.get(..4)?).ok()?;
    // Premultiplied over the paper.
    let paper = doc.resolve_color(designcraft_color::swatch::PAPER, 1.0).unwrap_or(designcraft_color::Color::WHITE).to_rgba8(1.0);
    let over = |c: u8, p: u8| (u32::from(c) + u32::from(p) * (255 - u32::from(a)) / 255).min(255) as u8;
    Some([over(r, paper[0]), over(g, paper[1]), over(b, paper[2])])
}

/// Render options for the canvas texture.
pub(super) fn screen_options(app: &DesignApp, layout: &CanvasLayout, preview: bool, ppp: f64) -> designcraft_render::RenderOptions {
    designcraft_render::RenderOptions {
        printing_only: preview,
        greek_below_px: 3.0 * ppp,
        highlight_missing_fonts: !preview && app.session.prefs.highlight_substituted_fonts,
        highlight_hj: !preview && app.session.prefs.highlight_hj,
        condition_indicators: !preview,
        note_indicators: !preview && app.session.prefs.show_note_anchors,
        change_markup: !preview && app.session.prefs.show_added_text,
        tag_markers: !preview && app.ui.tag_markers,
        highlight_keeps: !preview && app.session.prefs.highlight_keeps,
        highlight_custom_tracking: !preview && app.session.prefs.highlight_custom_tracking,
        quality: app.ui.display_quality,
        rich_black: app.ui.rich_black,
        overprint_preview: app.ui.overprint_preview,
        blend_space_view: !app.ui.proof_colors,
        document_grid: if app.ui.document_grid && !preview { grid_areas(layout) } else { vec![] },
        ..Default::default()
    }
}

/// The area each spread's document grid covers, in spread coordinates: the full-width band of
/// the pasteboard nearest that spread, the band in which `CanvasLayout::spread_at` (and so
/// snapping) picks it.
fn grid_areas(layout: &CanvasLayout) -> Vec<(designcraft_doc::SpreadRef, DRect)> {
    let pb = layout.pasteboard;
    let slots = &layout.slots;
    slots
        .iter()
        .enumerate()
        .map(|(i, slot)| {
            let lo = i.checked_sub(1).and_then(|j| slots.get(j)).map_or(pb.y0, |prev| (prev.bounds.y1 + slot.bounds.y0) / 2.0);
            let hi = slots.get(i + 1).map_or(pb.y1, |next| (slot.bounds.y1 + next.bounds.y0) / 2.0);
            (slot.spread, slot.xf.inverse().transform_rect_bbox(DRect::new(pb.x0, lo, pb.x1, hi)))
        })
        .collect()
}

/// The display options that change how the canvas texture looks without touching the document.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplayKey {
    quality: designcraft_render::DisplayQuality,
    rich_black: bool,
    overprint_preview: bool,
    proof_colors: bool,
    tag_markers: bool,
    document_grid: bool,
    highlight_substituted_fonts: bool,
    highlight_hj: bool,
    show_note_anchors: bool,
    show_added_text: bool,
    highlight_keeps: bool,
    highlight_custom_tracking: bool,
}

fn display_key(app: &DesignApp) -> DisplayKey {
    let (ui, prefs) = (&app.ui, &app.session.prefs);
    DisplayKey {
        quality: ui.display_quality,
        rich_black: ui.rich_black,
        overprint_preview: ui.overprint_preview,
        proof_colors: ui.proof_colors,
        tag_markers: ui.tag_markers,
        document_grid: ui.document_grid,
        highlight_substituted_fonts: prefs.highlight_substituted_fonts,
        highlight_hj: prefs.highlight_hj,
        show_note_anchors: prefs.show_note_anchors,
        show_added_text: prefs.show_added_text,
        highlight_keeps: prefs.highlight_keeps,
        highlight_custom_tracking: prefs.highlight_custom_tracking,
    }
}

/// When a display option changed, drop the texture and any render in flight: they were drawn with the old options.
pub(super) fn invalidate_on_display_change(app: &mut DesignApp) {
    let key = display_key(app);
    if app.canvas.display_key != Some(key) {
        app.canvas.display_key = Some(key);
        app.canvas.shown = None;
        app.canvas.pending = None;
        app.canvas.pending_doc = None;
    }
}

/// Keep the canvas texture current. Panning inside the overscanned region just shifts the
/// texture; zooming shows the old texture scaled until the background render arrives; document
/// edits render synchronously so editing never lags.
#[derive(Clone, Copy, Debug)]
pub(super) struct DragPreview {
    spread: SpreadRef,
    original: DRect,
    current: DRect,
    paper: Color32,
    fill: Color32,
}

pub(super) fn paint_drag_preview(painter: &egui::Painter, xf: &Xf, layout: &CanvasLayout, preview: DragPreview) {
    let spread_xf = layout.xf(preview.spread);
    painter.rect_filled(xf.rect(spread_xf.transform_rect_bbox(preview.original)), 0.0, preview.paper);
    painter.rect_filled(xf.rect(spread_xf.transform_rect_bbox(preview.current)), 0.0, preview.fill);
}

pub(super) fn render_texture(
    app: &mut DesignApp,
    ctx: &egui::Context,
    rect: Rect,
    xf: &Xf,
    layout: &CanvasLayout,
    preview: bool,
) -> Option<DragPreview> {
    invalidate_on_display_change(app);
    let st = app.session.active()?;
    let ppp = ctx.pixels_per_point() as f64;
    let doc_key = (st.uid, std::sync::Arc::as_ptr(&st.doc) as u64, st.revision, preview);
    let editing_parents = st.editing_parents;
    // Overscan: 25% of the viewport on every side, within the GPU's maximum texture size.
    let max_side = ctx.input(|i| i.max_texture_side) as f32 / ppp as f32;
    let (ow, oh) =
        ((rect.width() * 0.25).min((max_side - rect.width()) / 2.0).max(0.0), (rect.height() * 0.25).min((max_side - rect.height()) / 2.0).max(0.0));
    let want = |origin: Point| crate::Shown {
        doc: doc_key,
        editing_parents,
        origin: Point::new(origin.x - ow as f64 / xf.zoom, origin.y - oh as f64 / xf.zoom),
        zoom: xf.zoom,
        size: (rect.width() + 2.0 * ow, rect.height() + 2.0 * oh),
    };
    let target = want(xf.origin);
    // Does the current texture cover the viewport at this zoom and document state?
    let covers = |s: &crate::Shown| {
        s.doc == doc_key && s.editing_parents == editing_parents && (s.zoom - xf.zoom).abs() < 1e-9 && {
            let x0 = (xf.origin.x - s.origin.x) * s.zoom;
            let y0 = (xf.origin.y - s.origin.y) * s.zoom;
            x0 >= -0.5 && y0 >= -0.5 && x0 + rect.width() as f64 <= s.size.0 as f64 + 0.5 && y0 + rect.height() as f64 <= s.size.1 as f64 + 0.5
        }
    };
    // Collect finished background renders.
    #[cfg(not(target_arch = "wasm32"))]
    if let (Some(w), Some((tok, shown))) = (app.canvas.worker.as_ref(), app.canvas.pending)
        && let Some(done) = w.poll()
        && done.token == tok
    {
        upload(app, ctx, done.image);
        app.canvas.shown = Some(shown);
        app.canvas.shown_doc = app.canvas.pending_doc.take();
        app.canvas.pending = None;
        app.perf.render_ms = done.ms;
    }
    if let Some(drag) = drag_preview(app, ctx, rect, xf, layout, doc_key, preview) {
        return Some(drag);
    }
    if app.canvas.shown.is_some_and(|s| covers(&s)) {
        return None;
    }
    let doc_changed = app.canvas.shown.is_none_or(|s| s.doc != doc_key);
    let mode_changed = app.canvas.shown.is_some_and(|s| s.editing_parents != editing_parents);
    let zoom_same = app.canvas.shown.is_some_and(|s| (s.zoom - xf.zoom).abs() < 1e-9);
    let job = |app: &DesignApp| {
        let st = app.session.active()?;
        let placed: Vec<designcraft_render::Placed> =
            layout.slots.iter().map(|s| designcraft_render::Placed { spread: s.spread, xf: s.xf }).collect();
        let w = (target.size.0 as f64 * ppp).round().max(1.0) as u32;
        let h = (target.size.1 as f64 * ppp).round().max(1.0) as u32;
        let view = Affine::scale(ppp) * Affine::scale(xf.zoom) * Affine::translate((-target.origin.x, -target.origin.y));
        let opts = screen_options(app, layout, preview, ppp);
        Some((st.doc.clone(), placed, w, h, view, opts))
    };
    // Synchronous path: document edits (keep editing crisp), first frame, or no worker.
    #[cfg(not(target_arch = "wasm32"))]
    if !app.canvas.worker_started {
        app.canvas.worker_started = true;
        app.canvas.worker = render_worker_spawn(ctx);
    }
    #[cfg(not(target_arch = "wasm32"))]
    let has_worker = app.canvas.worker.is_some();
    #[cfg(target_arch = "wasm32")]
    let has_worker = false;
    // An edit with the view unchanged: repaint just the regions it changed.
    if doc_changed && zoom_same && patch_texture(app, layout, ppp, doc_key, preview) {
        return None;
    }
    // A mode switch changes every spread's meaning, even if the document has no damage.
    // Show the new canvas immediately, including when fitting it changes the zoom.
    if !has_worker || app.canvas.shown.is_none() || mode_changed || (doc_changed && zoom_same) {
        let t0 = crate::now_ms();
        let (doc, placed, w, h, view, opts) = job(app)?;
        let img = app.canvas.renderer.render(&doc, &app.session.cache, &placed, (w, h), view, &opts);
        upload(app, ctx, img);
        app.canvas.shown = Some(target);
        app.canvas.shown_doc = Some(doc);
        app.canvas.pending = None;
        app.canvas.pending_doc = None;
        app.perf.render_ms = crate::now_ms() - t0;
        return None;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        // Already rendering this view?
        if app.canvas.pending.is_some_and(|(_, p)| {
            p.doc == doc_key && p.editing_parents == editing_parents && (p.zoom - xf.zoom).abs() < 1e-9 && {
                let x0 = (xf.origin.x - p.origin.x) * p.zoom;
                let y0 = (xf.origin.y - p.origin.y) * p.zoom;
                x0 >= 0.0 && y0 >= 0.0 && x0 + rect.width() as f64 <= p.size.0 as f64 && y0 + rect.height() as f64 <= p.size.1 as f64
            }
        }) {
            return None;
        }
        let (doc, placed, w, h, view, opts) = job(app)?;
        app.canvas.pending_doc = Some(doc.clone());
        app.canvas.token += 1;
        let token = app.canvas.token;
        if let Some(wk) = app.canvas.worker.as_ref() {
            wk.submit(crate::render_worker::Job { token, doc, cache: app.session.cache.clone(), placed, w, h, view, opts });
        }
        app.canvas.pending = Some((token, target));
    }
    None
}

/// A one-item move can skip CPU rasterising while its exact starting canvas texture is visible:
/// mask the old bounds with paper and draw the new bounds through egui/wgpu. Any uncertainty uses
/// the normal CPU damage patch below.
pub(super) fn drag_preview(
    app: &DesignApp,
    ctx: &egui::Context,
    rect: Rect,
    xf: &Xf,
    layout: &CanvasLayout,
    doc_key: (u64, u64, u64, bool),
    screen_preview: bool,
) -> Option<DragPreview> {
    if screen_preview
        || app.ui.screen_mode != ScreenMode::Normal
        || app.ui.proof_colors
        || app.ui.overprint_preview
        || app.ui.document_grid
        || app.ui.separations.is_some()
    {
        return None;
    }
    let st = app.session.active()?;
    if st.editing_parents {
        return None;
    }
    let interaction = st.interaction.as_ref()?;
    if interaction.preview.as_ref().map(|p| p.0.as_str()) != Some("transform.move") {
        return None;
    }
    if doc_key != (st.uid, std::sync::Arc::as_ptr(&st.doc) as u64, st.revision, screen_preview) {
        return None;
    }
    let [id] = interaction.selection.items.as_slice() else { return None };
    if st.selection.items.as_slice() != [*id]
        || !interaction.selection.anchors.is_empty()
        || interaction.selection.content
        || interaction.selection.text.is_some()
        || interaction.selection.cells.is_some()
        || st.selection.anchors != interaction.selection.anchors
        || st.selection.content != interaction.selection.content
        || st.selection.text != interaction.selection.text
        || st.selection.cells != interaction.selection.cells
        || std::sync::Arc::ptr_eq(&interaction.doc, &st.doc)
    {
        return None;
    }
    let shown = app.canvas.shown?;
    let shown_doc = app.canvas.shown_doc.as_ref()?;
    if !std::sync::Arc::ptr_eq(shown_doc, &interaction.doc)
        || shown.doc != (st.uid, std::sync::Arc::as_ptr(&interaction.doc) as u64, interaction.revision, false)
        || shown.editing_parents
        || shown.doc.3
        || shown.zoom.to_bits() != xf.zoom.to_bits()
        || app.canvas.pending.is_some()
    {
        return None;
    }
    let ppp = ctx.pixels_per_point() as f64;
    if !ppp.is_finite() || ppp <= 0.0 {
        return None;
    }
    let (tw, th) = ((shown.size.0 as f64 * ppp).round() as i64, (shown.size.1 as f64 * ppp).round() as i64);
    let full = [usize::try_from(tw).ok()?, usize::try_from(th).ok()?];
    if app.canvas.texture.as_ref()?.size() != full {
        return None;
    }
    let x0 = (xf.origin.x - shown.origin.x) * shown.zoom;
    let y0 = (xf.origin.y - shown.origin.y) * shown.zoom;
    if x0 < -0.5 || y0 < -0.5 || x0 + rect.width() as f64 > shown.size.0 as f64 + 0.5 || y0 + rect.height() as f64 > shown.size.1 as f64 + 0.5 {
        return None;
    }

    let before = interaction.doc.as_ref();
    let after = st.doc.as_ref();
    let before_loc = before.find(*id)?;
    if before_loc.path.len() != 1 || !matches!(before_loc.spread, SpreadRef::Doc(_)) {
        return None;
    }
    layout.slot(before_loc.spread)?;
    let after_loc = after.find(*id)?;
    if after_loc != before_loc {
        return None;
    }
    let before_item = before.item(*id)?;
    let after_item = after.item(*id)?;
    if !simple_solid_rectangle(before, before_item) || !simple_solid_rectangle(after, after_item) {
        return None;
    }
    // Rich-black proof changes only the built-in black swatch. Its display color is pure black;
    // custom CMYK swatches stay on the CPU path while rich-black mode is enabled.
    if app.ui.rich_black && before_item.fill.swatch != designcraft_color::swatch::BLACK {
        return None;
    }
    let old_layer = before.layer(before_item.layer)?;
    let new_layer = after.layer(after_item.layer)?;
    if !old_layer.visible || old_layer.locked || !new_layer.visible || new_layer.locked {
        return None;
    }
    let mut expected = before_item.clone();
    expected.xf = after_item.xf;
    if expected != *after_item || before_item.xf.as_coeffs()[..4] != [1.0, 0.0, 0.0, 1.0] || after_item.xf.as_coeffs()[..4] != [1.0, 0.0, 0.0, 1.0] {
        return None;
    }
    let spread = before.spread(before_loc.spread)?;
    let current_spread = after.spread(after_loc.spread)?;
    if spread.pages.len() != current_spread.pages.len() || spread.parent.is_some() {
        return None;
    }
    let old_bounds = before_item.bounds();
    let new_bounds = after_item.bounds();
    let page = spread.pages.iter().find(|page| rect_contains(page.bounds(), old_bounds))?;
    let page_id = page.id;
    let new_page = current_spread.pages.iter().find(|candidate| candidate.id == page_id && rect_contains(candidate.bounds(), new_bounds))?;
    if page.id != new_page.id || page.show_parent_overlay {
        return None;
    }
    // Parent items are mapped through page-specific master transforms; only a blank visible
    // parent is safe to mask without asking the normal renderer.
    if page.show_parent_items
        && let Some(parent_id) = page.parent
        && before
            .parents
            .iter()
            .find(|parent| parent.id == parent_id)
            .is_none_or(|parent| parent.items.iter().any(|item| !item.hidden && before.layer(item.layer).is_none_or(|layer| layer.visible)))
    {
        return None;
    }
    let protected = old_bounds.union(new_bounds).inflate(1.0, 1.0);
    for sibling in &spread.items {
        if sibling.id == *id || sibling.hidden {
            continue;
        }
        let layer = before.layer(sibling.layer)?;
        if !layer.visible {
            continue;
        }
        let bounds = sibling.bounds().inflate(
            designcraft_render::effect_outset(sibling) + sibling.stroke.extent(),
            designcraft_render::effect_outset(sibling) + sibling.stroke.extent(),
        );
        if rects_overlap(bounds, protected) {
            return None;
        }
    }
    let paper = before.resolve_color(designcraft_color::swatch::PAPER, 1.0)?.to_rgba8(1.0);
    if paper[3] != u8::MAX {
        return None;
    }
    if after.resolve_color(designcraft_color::swatch::PAPER, 1.0) != before.resolve_color(designcraft_color::swatch::PAPER, 1.0) {
        return None;
    }
    let fill = before.resolve_color(&before_item.fill.swatch, before_item.fill.tint)?.to_rgba8(1.0);
    Some(DragPreview {
        spread: before_loc.spread,
        original: old_bounds,
        current: new_bounds,
        paper: Color32::from_rgb(paper[0], paper[1], paper[2]),
        fill: Color32::from_rgb(fill[0], fill[1], fill[2]),
    })
}

fn simple_solid_rectangle(doc: &designcraft_doc::Document, item: &designcraft_doc::Item) -> bool {
    use designcraft_doc::{Content, Shape};
    item.shape == Shape::Rectangle
        && matches!(item.content, Content::Unassigned)
        && item.path == designcraft_geom::shapes::rectangle(item.inner_bounds())
        && item.stroke.is_none()
        && item.corners == Default::default()
        && item.opacity == 1.0
        && item.blend == designcraft_color::BlendMode::Normal
        && item.effects == Default::default()
        && item.wrap == Default::default()
        && !item.hidden
        && !item.locked
        && !item.nonprinting
        && item.fill.gradient_angle.is_none()
        && item.fill.gradient_vector.is_none()
        && !item.fill.overprint
        && item.fill.tint.is_finite()
        && !item.fill.is_none()
        && doc.resolve_color(&item.fill.swatch, item.fill.tint).is_some_and(|color| color.to_rgba8(1.0)[3] == u8::MAX)
}

fn rect_contains(outer: DRect, inner: DRect) -> bool {
    inner.x0 >= outer.x0 + 1.0 && inner.y0 >= outer.y0 + 1.0 && inner.x1 <= outer.x1 - 1.0 && inner.y1 <= outer.y1 - 1.0
}

fn rects_overlap(a: DRect, b: DRect) -> bool {
    a.x0 < b.x1 && a.x1 > b.x0 && a.y0 < b.y1 && a.y1 > b.y0
}

/// Repaint only the damaged regions of the current texture after an edit (typing, nudging).
/// Returns false when a full render is needed (view moved, too much changed, or damage unknown).
fn patch_texture(app: &mut DesignApp, layout: &CanvasLayout, ppp: f64, doc_key: (u64, u64, u64, bool), preview: bool) -> bool {
    let (Some(sh), Some(old), Some(st)) = (app.canvas.shown, app.canvas.shown_doc.clone(), app.session.active()) else { return false };
    if sh.doc.0 != doc_key.0 || sh.doc.3 != doc_key.3 || sh.editing_parents != st.editing_parents {
        return false;
    }
    let (tw, th) = ((sh.size.0 as f64 * ppp).round() as i64, (sh.size.1 as f64 * ppp).round() as i64);
    // Patches are in full-resolution pixels: a texture `upload` shrank to the GPU's limit
    // (a viewport wider than `max_texture_side`) takes full renders instead.
    let full = [usize::try_from(tw).unwrap_or(0), usize::try_from(th).unwrap_or(0)];
    if app.canvas.texture.as_ref().is_none_or(|t| t.size() != full) {
        return false;
    }
    let new = st.doc.clone();
    let Some(regions) = designcraft_render::damage::damage_with(&old, &new, Some(&app.session.cache)) else { return false };
    let t0 = crate::now_ms();
    let k = sh.zoom * ppp;
    let mut px: Option<(i64, i64, i64, i64)> = None;
    for (r, rect) in &regions {
        let c = layout.xf(*r).transform_rect_bbox(*rect);
        let x0 = ((c.x0 - sh.origin.x) * k).floor() as i64 - 1;
        let y0 = ((c.y0 - sh.origin.y) * k).floor() as i64 - 1;
        let x1 = ((c.x1 - sh.origin.x) * k).ceil() as i64 + 1;
        let y1 = ((c.y1 - sh.origin.y) * k).ceil() as i64 + 1;
        px = Some(px.map_or((x0, y0, x1, y1), |p| (p.0.min(x0), p.1.min(y0), p.2.max(x1), p.3.max(y1))));
    }
    let done = |app: &mut DesignApp| {
        app.canvas.shown = Some(crate::Shown { doc: doc_key, ..sh });
        app.canvas.shown_doc = Some(new.clone());
        // A background render of the old document must not overwrite this.
        app.canvas.pending = None;
        app.canvas.pending_doc = None;
    };
    let Some((x0, y0, x1, y1)) = px.map(|p| (p.0.max(0), p.1.max(0), p.2.min(tw), p.3.min(th))) else {
        done(app);
        return true;
    };
    if x1 <= x0 || y1 <= y0 {
        done(app);
        return true;
    }
    // Big changes: a full render costs about the same.
    if ((x1 - x0) * (y1 - y0)) as f64 > 0.5 * (tw * th) as f64 {
        return false;
    }
    let placed: Vec<designcraft_render::Placed> = layout.slots.iter().map(|s| designcraft_render::Placed { spread: s.spread, xf: s.xf }).collect();
    let view = Affine::translate((-(x0 as f64), -(y0 as f64)))
        * Affine::scale(ppp)
        * Affine::scale(sh.zoom)
        * Affine::translate((-sh.origin.x, -sh.origin.y));
    let opts = screen_options(app, layout, preview, ppp);
    let img = app.canvas.patcher.render(&new, &app.session.cache, &placed, ((x1 - x0) as u32, (y1 - y0) as u32), view, &opts);
    let ci = egui::ColorImage::from_rgba_premultiplied([img.width as usize, img.height as usize], &img.pixels);
    if let Some(tex) = app.canvas.texture.as_mut() {
        tex.set_partial([x0 as usize, y0 as usize], ci, egui::TextureOptions::LINEAR);
    }
    done(app);
    app.canvas.patches += 1;
    app.perf.render_ms = crate::now_ms() - t0;
    true
}

#[cfg(not(target_arch = "wasm32"))]
fn render_worker_spawn(ctx: &egui::Context) -> Option<crate::render_worker::Worker> {
    if std::env::var_os("DESIGNCRAFT_SYNC_RENDER").is_some() {
        return None;
    }
    crate::render_worker::Worker::spawn(ctx.clone())
}

/// What the canvas does to a render before showing it: Proof Colors. The texture and the
/// eyedropper's sample both go through it.
fn screen_post(app: &DesignApp, img: &mut designcraft_render::Rendered) {
    if app.ui.proof_colors {
        designcraft_render::proof_view(img, &app.ui.proof_setup);
    }
}

fn upload(app: &mut DesignApp, ctx: &egui::Context, mut img: designcraft_render::Rendered) {
    screen_post(app, &mut img);
    let ci = egui::ColorImage::from_rgba_premultiplied([img.width as usize, img.height as usize], &img.pixels);
    let ci = crate::widgets::fit_texture(ci, ctx.input(|i| i.max_texture_side));
    match &mut app.canvas.texture {
        Some(tex) => tex.set(ci, egui::TextureOptions::LINEAR),
        None => app.canvas.texture = Some(ctx.load_texture("canvas", ci, egui::TextureOptions::LINEAR)),
    }
}
