//! Compare DesignCraft's CPU damage patch with an egui/wgpu solid-rectangle drag preview.

use std::time::{Duration, Instant};

use designcraft_doc::{Document, ItemId, SpreadRef};
use designcraft_engine::Session;
use designcraft_geom::{Affine, Rect as DRect};
use designcraft_render::{Placed, RenderOptions, Rendered, Renderer};
use egui::{Color32, CornerRadius, Id, LayerId, Order, Pos2, Rect, TextureHandle, TextureOptions, pos2, vec2};
use egui_wgpu::{Renderer as EpaintRenderer, RendererOptions, ScreenDescriptor, wgpu};
use serde_json::json;
use sha2::{Digest, Sha256};

const SCALE: f64 = 2.0;
const WIDTH: u32 = 1224;
const HEIGHT: u32 = 1584;
const FRAMES: usize = 30;
const MAX_MAE: f64 = 0.02;

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: EpaintRenderer,
    context: egui::Context,
    target: wgpu::Texture,
    target_view: wgpu::TextureView,
    readback: wgpu::Buffer,
    row_bytes: u32,
    screen: ScreenDescriptor,
    adapter: String,
}

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mode = args.next().ok_or_else(|| "usage: issue237_drag_preview cpu|gpu PDF".to_string())?;
    let pdf = args.next().ok_or_else(|| "usage: issue237_drag_preview cpu|gpu PDF".to_string())?;
    if args.next().is_some() || !matches!(mode.as_str(), "cpu" | "gpu") {
        return Err("usage: issue237_drag_preview cpu|gpu PDF".into());
    }

    let (mut session, id, initial_doc) = scene(&pdf)?;
    session.begin_interaction("Move").map_err(|e| e.to_string())?;
    let placed = [Placed { spread: SpreadRef::Doc(0), xf: Affine::IDENTITY }];
    let view = Affine::scale(SCALE);
    // Match DesignCraft's default UI display option while exercising the built-in black swatch.
    let opts = RenderOptions { rich_black: true, ..Default::default() };
    let mut cpu = Renderer::new();
    let initial = cpu.render(&initial_doc, &session.cache, &placed, (WIDTH, HEIGHT), view, &opts);
    let mut texture = Gpu::new(&initial)?;
    let mut cpu_texture = texture.context.load_texture("DesignCraft CPU canvas", color_image(&initial), TextureOptions::LINEAR);
    let original = rect(&initial_doc, id)?;
    let (background, fill) = colors(&initial_doc, id)?;
    let mut shown_doc = initial_doc;
    let mut durations = Vec::with_capacity(FRAMES);

    for frame in 0..FRAMES {
        let at = Instant::now();
        let old_doc = shown_doc.clone();
        let dx = (frame + 1) as f64 * 2.5;
        let dy = (frame + 1) as f64 * 0.5;
        session.preview("transform.move", json!({"ids": [id.0], "dx": dx, "dy": dy})).map_err(|e| e.to_string())?;
        let new_doc = session.active().ok_or("document was closed during preview")?.doc.clone();

        if mode == "cpu" {
            let (x, y, patch) = cpu_patch(&mut cpu, &session, &old_doc, &new_doc, &placed, view, &opts)?;
            cpu_texture.set_partial([x as usize, y as usize], color_image(&patch), TextureOptions::LINEAR);
            texture.paint_frame(&cpu_texture, None, None, background, fill)?;
        } else {
            let current = rect(&new_doc, id)?;
            texture.paint_frame(&cpu_texture, Some(original), Some(current), background, fill)?;
        }
        shown_doc = new_doc;
        durations.push(at.elapsed());
    }

    let oracle = cpu.render(&shown_doc, &session.cache, &placed, (WIDTH, HEIGHT), view, &opts);
    let (actual, readback_ns) = texture.read_output()?;
    let (mae, max_error) = error(&oracle.pixels, &actual)?;
    if mae > MAX_MAE {
        return Err(format!("{mode} framebuffer parity failed: RGBA MAE {mae:.8} > {MAX_MAE:.5}"));
    }
    let hash = format!("{:x}", Sha256::digest(&oracle.pixels));
    println!("DesignCraft drag preview ({mode}; {FRAMES} pointer frames; {WIDTH}x{HEIGHT}; error bound MAE <= {MAX_MAE})");
    println!("adapter: {}", texture.adapter);
    println!("RGBA MAE: {mae:.8}; max channel error: {max_error}; final readback ns: {readback_ns}");
    println!("CRAFT_JOURNEY_NS={}", median_ns(&mut durations));
    println!("CRAFT_OUTPUT_SHA256={hash}");
    println!("CRAFT_GPU_ADAPTER={}", texture.adapter);
    Ok(())
}

fn scene(pdf: &str) -> Result<(Session, ItemId, std::sync::Arc<Document>), String> {
    let mut session = Session::new();
    session.execute("file.new", &json!({"pages": 1, "facingPages": false, "width": 612, "height": 792})).map_err(|e| e.to_string())?;
    let created = session.execute("frame.create", &json!({"rect": [390, 610, 500, 700], "content": "unassigned"})).map_err(|e| e.to_string())?;
    let id = ItemId(created.get("id").and_then(serde_json::Value::as_u64).ok_or("frame.create returned no item id")?);
    session.execute("object.fill", &json!({"ids": [id.0], "swatch": "[Black]"})).map_err(|e| e.to_string())?;
    session.execute("object.stroke", &json!({"ids": [id.0], "swatch": "[None]"})).map_err(|e| e.to_string())?;
    session.execute("file.place", &json!({"path": pdf, "x": 52, "y": 52, "width": 260, "pdfPage": 1})).map_err(|e| e.to_string())?;
    let state = session.active_mut().ok_or("file.new did not create a document")?;
    state.selection.items = vec![id];
    let doc = state.doc.clone();
    let Some(spread) = doc.spreads.first() else { return Err("new document has no spread".into()) };
    let item = doc.item(id).ok_or("new rectangle is missing")?;
    if spread.items.len() != 2 || !simple_rectangle(&doc, item) {
        return Err("generated page did not meet the isolated solid-rectangle precondition".into());
    }
    Ok((session, id, doc))
}

fn simple_rectangle(doc: &Document, item: &designcraft_doc::Item) -> bool {
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
        && item.fill.gradient_angle.is_none()
        && item.fill.gradient_vector.is_none()
        && !item.fill.is_none()
        && doc.resolve_color(&item.fill.swatch, item.fill.tint).is_some()
}

fn rect(doc: &Document, id: ItemId) -> Result<DRect, String> {
    let item = doc.item(id).ok_or("preview rectangle disappeared")?;
    let b = item.bounds();
    if ![b.x0, b.y0, b.x1, b.y1].iter().all(|v| v.is_finite()) || b.width() <= 0.0 || b.height() <= 0.0 {
        return Err("preview rectangle has invalid bounds".into());
    }
    Ok(b)
}

fn colors(doc: &Document, id: ItemId) -> Result<(Color32, Color32), String> {
    let paper = doc.resolve_color(designcraft_color::swatch::PAPER, 1.0).ok_or("paper swatch is unavailable")?.to_rgba8(1.0);
    let item = doc.item(id).ok_or("preview rectangle disappeared")?;
    let fill = doc.resolve_color(&item.fill.swatch, item.fill.tint).ok_or("rectangle fill is not a solid swatch")?.to_rgba8(1.0);
    Ok((Color32::from_rgba_unmultiplied(paper[0], paper[1], paper[2], paper[3]), Color32::from_rgba_unmultiplied(fill[0], fill[1], fill[2], fill[3])))
}

fn cpu_patch(
    renderer: &mut Renderer,
    session: &Session,
    old: &Document,
    new: &Document,
    placed: &[Placed],
    view: Affine,
    opts: &RenderOptions,
) -> Result<(u32, u32, Rendered), String> {
    let regions = designcraft_render::damage::damage_with(old, new, Some(&session.cache)).ok_or("CPU damage calculation failed")?;
    let screen = DRect::new(0.0, 0.0, WIDTH as f64, HEIGHT as f64);
    let mut damage: Option<DRect> = None;
    for (spread, region) in regions {
        if spread != SpreadRef::Doc(0) {
            continue;
        }
        let px = view.transform_rect_bbox(region).intersect(screen);
        if px.width() > 0.0 && px.height() > 0.0 {
            damage = Some(damage.map_or(px, |current| current.union(px)));
        }
    }
    let area = damage.ok_or("CPU damage calculation returned no visible patch")?;
    let (x, y) = (area.x0.floor().max(0.0) as u32, area.y0.floor().max(0.0) as u32);
    let (x1, y1) = (area.x1.ceil().min(WIDTH as f64) as u32, area.y1.ceil().min(HEIGHT as f64) as u32);
    let (w, h) = (x1.saturating_sub(x), y1.saturating_sub(y));
    if w == 0 || h == 0 {
        return Err("CPU damage patch has an empty size".into());
    }
    let patch_view = Affine::translate((-(x as f64), -(y as f64))) * view;
    let patch = renderer.render(new, &session.cache, placed, (w, h), patch_view, opts);
    Ok((x, y, patch))
}

fn color_image(img: &Rendered) -> egui::ColorImage {
    egui::ColorImage::from_rgba_premultiplied([img.width as usize, img.height as usize], &img.pixels)
}

fn screen_rect(bounds: DRect) -> Rect {
    Rect::from_min_max(pos2((bounds.x0 * SCALE) as f32, (bounds.y0 * SCALE) as f32), pos2((bounds.x1 * SCALE) as f32, (bounds.y1 * SCALE) as f32))
}

fn error(reference: &[u8], actual: &[u8]) -> Result<(f64, u8), String> {
    if reference.len() != actual.len() {
        return Err(format!("CPU and GPU framebuffer sizes differ: {} vs {} bytes", reference.len(), actual.len()));
    }
    let mut sum = 0.0;
    let mut max = 0_u8;
    for (a, b) in reference.iter().zip(actual) {
        let d = a.abs_diff(*b);
        sum += f64::from(d) / 255.0;
        max = max.max(d);
    }
    Ok((sum / reference.len().max(1) as f64, max))
}

fn median_ns(values: &mut [Duration]) -> u64 {
    values.sort_unstable();
    values.get(values.len() / 2).map_or(0, |d| d.as_nanos().min(u128::from(u64::MAX)) as u64)
}

impl Gpu {
    fn new(initial: &Rendered) -> Result<Self, String> {
        pollster::block_on(Self::new_async(initial))
    }

    async fn new_async(initial: &Rendered) -> Result<Self, String> {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::VULKAN;
        let instance = wgpu::Instance::new(desc);
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
                ..Default::default()
            })
            .await
            .map_err(|e| format!("Vulkan adapter request failed: {e}"))?;
        let info = adapter.get_info();
        let expected = std::env::var("CRAFT_EXPECT_GPU_ADAPTER").unwrap_or_else(|_| "RTX 2080 Ti".into());
        if !info.name.contains(&expected) {
            return Err(format!("selected GPU adapter {:?} does not contain {expected:?}", info.name));
        }
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("DesignCraft issue 237 drag preview"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|e| format!("GPU device request failed: {e}"))?;
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("DesignCraft offscreen canvas"),
            size: wgpu::Extent3d { width: initial.width, height: initial.height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target_view = target.create_view(&Default::default());
        let row_bytes = (initial.width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("DesignCraft canvas readback"),
            size: u64::from(row_bytes) * u64::from(initial.height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Ok(Self {
            renderer: EpaintRenderer::new(&device, format, RendererOptions::default()),
            context: egui::Context::default(),
            device,
            queue,
            target,
            target_view,
            readback,
            row_bytes,
            screen: ScreenDescriptor { size_in_pixels: [initial.width, initial.height], pixels_per_point: 1.0 },
            adapter: info.name,
        })
    }

    fn paint_frame(
        &mut self,
        texture: &TextureHandle,
        original: Option<DRect>,
        current: Option<DRect>,
        background: Color32,
        fill: Color32,
    ) -> Result<(), String> {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(WIDTH as f32, HEIGHT as f32));
        let output = self.context.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| {
            let painter = ui.ctx().layer_painter(LayerId::new(Order::Background, Id::unique("issue237-drag")));
            painter.image(texture.id(), screen, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
            if let (Some(original), Some(current)) = (original, current) {
                painter.rect_filled(screen_rect(original), CornerRadius::ZERO, background);
                painter.rect_filled(screen_rect(current), CornerRadius::ZERO, fill);
            }
        });
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("DesignCraft drag frame") });
        for (id, deltas) in &output.textures_delta.set {
            for delta in deltas {
                self.renderer.update_texture(&self.device, &self.queue, *id, delta);
            }
        }
        let jobs = self.context.tessellate(output.shapes, output.pixels_per_point);
        let commands = self.renderer.update_buffers(&self.device, &self.queue, &mut encoder, &jobs, &self.screen);
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("DesignCraft epaint canvas"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.target_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::WHITE), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.renderer.render(&mut pass.forget_lifetime(), &jobs, &self.screen);
        }
        for id in &output.textures_delta.free {
            self.renderer.free_texture(id);
        }
        let submission = self.queue.submit(commands.into_iter().chain([encoder.finish()]));
        self.device
            .poll(wgpu::PollType::Wait { submission_index: Some(submission), timeout: Some(Duration::from_secs(5)) })
            .map_err(|e| format!("waiting for GPU frame: {e}"))?;
        Ok(())
    }

    fn read_output(&mut self) -> Result<(Vec<u8>, u64), String> {
        let at = Instant::now();
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("DesignCraft final output copy") });
        encoder.copy_texture_to_buffer(
            self.target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(self.row_bytes), rows_per_image: Some(HEIGHT) },
            },
            wgpu::Extent3d { width: WIDTH, height: HEIGHT, depth_or_array_layers: 1 },
        );
        let submission = self.queue.submit([encoder.finish()]);
        let slice = self.readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result.map_err(|e| e.to_string()));
        });
        self.device
            .poll(wgpu::PollType::Wait { submission_index: Some(submission), timeout: Some(Duration::from_secs(5)) })
            .map_err(|e| format!("waiting for framebuffer copy: {e}"))?;
        rx.recv_timeout(Duration::from_secs(5)).map_err(|e| e.to_string())??;
        let mapped = slice.get_mapped_range().map_err(|e| e.to_string())?;
        let mut pixels = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);
        for y in 0..HEIGHT as usize {
            let row = y * self.row_bytes as usize;
            pixels.extend_from_slice(&mapped[row..row + (WIDTH * 4) as usize]);
        }
        drop(mapped);
        self.readback.unmap();
        Ok((pixels, at.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64))
    }
}
