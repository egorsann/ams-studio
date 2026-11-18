//! Graphics > Save Graphics To File: the resolution (dpi) of each pane, and the render that gives it.
//!
//! The old export cut the pane out of the window's own picture, so it could never have more pixels than the screen shows.
//! Now every plot pane is drawn again, by the same code that draws it on screen, in a second (invisible) egui context
//! whose scale is `dpi / 96`, into an off-screen wgpu texture (in tiles, so a large picture does not hit the graphics
//! card's texture limit), and read back. The picture is therefore as sharp as the dpi says: lines, text and markers are
//! drawn at the final size, not stretched. A pane is saved at the size it has on screen (96 dpi = 1 point per pixel).
//!
//! The core viewer is drawn the same way (slot 4): its photos are decoded again at the size they are drawn at (the
//! window only keeps 256 px thumbnails), just the cubes in view. The GIS viewer keeps its map tiles in the window's own
//! textures, so it is still cut out of the window at screen resolution.

use super::{tools_ui, App, W_SHOT};
use eframe::egui_wgpu;
use eframe::wgpu;
use egui::{Color32, Pos2, Rect, RichText, Vec2};

/// What 1 point on the screen is, in dpi.
pub(super) const REF_DPI: f32 = 96.0;
pub(super) const DPI_MIN: f32 = 72.0;
pub(super) const DPI_MAX: f32 = 2400.0;
pub(super) const DPI_DEFAULT: f32 = 300.0;
/// Above this the window warns.
pub(super) const DPI_WARN: f32 = 800.0;
/// A picture of more pixels than this is refused (4 bytes each, plus the copy that is encoded).
const MAX_PIXELS: u64 = 400_000_000;
/// Edge of one render tile in pixels (well below the 8192 every wgpu device has).
const TILE: u32 = 4096;

/// Pixel size of a pane of `size` points saved at `dpi`.
pub(super) fn pixel_size(size: Vec2, dpi: f32) -> (u32, u32) {
    let s = dpi / REF_DPI;
    (((size.x * s).round() as u32).max(1), ((size.y * s).round() as u32).max(1))
}

/// A readable size: bytes of the uncompressed picture in MB.
fn megabytes(w: u32, h: u32) -> f64 {
    w as f64 * h as f64 * 4.0 / 1.0e6
}

/// `png` bytes with a `pHYs` chunk (pixels per metre) right after the header, so that other programs know the dpi.
pub(super) fn with_dpi(png: Vec<u8>, dpi: f32) -> Vec<u8> {
    // signature (8) + IHDR chunk (4 length + 4 type + 13 data + 4 crc)
    const AFTER_IHDR: usize = 8 + 25;
    if png.len() < AFTER_IHDR || &png[1..4] != b"PNG" {
        return png;
    }
    let ppm = (dpi as f64 / 0.0254).round() as u32;
    let mut data = Vec::with_capacity(9);
    data.extend_from_slice(&ppm.to_be_bytes());
    data.extend_from_slice(&ppm.to_be_bytes());
    data.push(1); // unit: metre
    let mut chunk = Vec::with_capacity(21);
    chunk.extend_from_slice(&9u32.to_be_bytes());
    let mut body = b"pHYs".to_vec();
    body.extend_from_slice(&data);
    chunk.extend_from_slice(&body);
    chunk.extend_from_slice(&crc32(&body).to_be_bytes());
    let mut out = Vec::with_capacity(png.len() + chunk.len());
    out.extend_from_slice(&png[..AFTER_IHDR]);
    out.extend_from_slice(&chunk);
    out.extend_from_slice(&png[AFTER_IHDR..]);
    out
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in bytes {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

/// Encodes RGBA pixels as a PNG that carries its dpi.
pub(super) fn png_bytes(w: u32, h: u32, rgba: &[u8], dpi: f32) -> Result<Vec<u8>, String> {
    use image::ImageEncoder;
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out).write_image(rgba, w, h, image::ExtendedColorType::Rgba8).map_err(|e| e.to_string())?;
    Ok(with_dpi(out, dpi))
}

/// One pane's row in the window: tick box, dpi slider, resulting size, and the notes under it. Returns the dpi when it is
/// above the warning limit (and the pane is ticked).
fn dpi_row(ui: &mut egui::Ui, tick: &mut bool, label: &str, dpi: &mut f32, size: Vec2) -> Option<f32> {
    let amber = Color32::from_rgb(214, 150, 40);
    let red = Color32::from_rgb(214, 70, 70);
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(Vec2::new(190.0, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| ui.checkbox(tick, label));
        ui.add_enabled_ui(*tick, |ui| {
            super::thin_slider(ui, egui::Slider::new(dpi, DPI_MIN..=DPI_MAX).logarithmic(true).integer().suffix(" dpi"));
        });
    });
    let (w, h) = pixel_size(size, *dpi);
    ui.label(RichText::new(format!("      {w} \u{d7} {h} px   ({:.1} \u{d7} {:.1} in)", w as f32 / *dpi, h as f32 / *dpi)).weak().small());
    if !*tick {
        return None;
    }
    if w as u64 * h as u64 > MAX_PIXELS {
        ui.label(RichText::new(format!("      {label} is too large ({:.0} MB of pixels): lower its dpi.", megabytes(w, h))).color(red));
        None
    } else if *dpi > DPI_WARN {
        ui.label(RichText::new(format!("      {:.0} dpi: this picture needs about {:.0} MB of memory to make and the file gets large.", *dpi, megabytes(w, h))).color(amber));
        Some(*dpi)
    } else {
        None
    }
}

impl App {
    /// The window that opens with Save Graphics To File: which panes, and the dpi of each.
    pub(super) fn shot_window(&mut self, ctx: &egui::Context) {
        if !self.shot_dlg {
            return;
        }
        let names = self.pane_names();
        let size_of = |this: &Self, id: super::PaneId| this.pane_rects.iter().find(|(p, _)| *p == id).map(|(_, r)| r.size());
        let slot_ids = [super::PaneId::Stereo, super::PaneId::Plot(1), super::PaneId::Plot(2), super::PaneId::Plot(3), super::PaneId::Core];
        let sizes: [Option<Vec2>; 5] = [0, 1, 2, 3, 4].map(|i| size_of(self, slot_ids[i]));
        let gis_on = self.gis_on;
        let dark = self.dark;
        let (mut open, mut go) = (true, false);
        self.base_window(ctx, "Save Graphics To File", W_SHOT, Vec2::new(860.0, 400.0)).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            tools_ui::pop_style(ui, 1.25);
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
            ui.spacing_mut().slider_width = 260.0;
            ui.horizontal(|ui| {
                ui.label(RichText::new("Resolution of each saved picture").strong());
                super::info_icon(
                    ui,
                    "A pane is saved at the size it has on screen, counted at 96 dpi. 300 dpi gives about 3.1 times as many pixels in each direction, and the lines, text and markers are drawn at that size, so the picture is sharp. The dpi is also stored in the PNG, so Word, LaTeX and Inkscape place it at pixels / dpi inches.",
                );
            });
            ui.label(RichText::new("One PNG file per ticked pane.").weak());
            ui.add_space(2.0);
            let mut worst: Option<f32> = None;
            for i in 0..5 {
                let Some(size) = sizes[i] else { continue };
                let (label, tick) = if i < 4 { (names[i].clone(), &mut self.export[i]) } else { ("Core viewer".to_string(), &mut self.cv.export) };
                if let Some(d) = dpi_row(ui, tick, &label, &mut self.shot_dpi[i], size) {
                    worst = Some(worst.map_or(d, |m: f32| m.max(d)));
                }
            }
            if gis_on {
                ui.horizontal(|ui| {
                    ui.allocate_ui_with_layout(Vec2::new(190.0, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| ui.checkbox(&mut self.gis_export, "GIS viewer"));
                    ui.label(RichText::new("screen resolution").weak());
                    super::info_icon(ui, "The map tiles of this pane live in the window's own memory, so it is cut out of the window as it is shown (the pixels of your screen).");
                });
            }
            if let Some(m) = worst {
                ui.add_space(2.0);
                let amber = Color32::from_rgb(214, 150, 40);
                ui.label(RichText::new(format!("Warning: more than {DPI_WARN:.0} dpi (up to {m:.0}). Journals usually ask for 300 to 600 dpi for photographs and 600 to 1200 dpi for line art, and a picture this sharp can be slow to save and heavy to open.")).color(amber));
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let b = egui::Button::new(RichText::new("Choose file and save ...").strong().color(Color32::WHITE)).fill(if dark { Color32::from_rgb(52, 104, 200) } else { Color32::from_rgb(47, 111, 222) }).min_size(Vec2::new(190.0, 0.0));
                go = ui.add(b).clicked();
                if ui.button("Cancel").clicked() {
                    self.shot_dlg = false;
                }
            });
        });
        if !open {
            self.shot_dlg = false;
        }
        if go {
            self.shot_dlg = false;
            self.start_save_graphics(ctx);
        }
        self.dirty_prefs_soon();
    }

    /// Asks for the file name, then the next frame cuts or renders the ticked panes.
    fn start_save_graphics(&mut self, ctx: &egui::Context) {
        if self.doc.is_none() {
            return;
        }
        let stem = self.doc.as_ref().and_then(|d| d.path.file_stem()).map(|s| s.to_string_lossy().to_string()).unwrap_or("ams-studio".into());
        let Some(base) = crate::fdialog::FileDialog::new().add_filter("PNG base name", &["png"]).set_file_name(format!("{stem}.png")).set_title("Save Graphics To File").save_file() else { return };
        self.shot = Some(super::ShotJob { copy: false, base: Some(base), rects: vec![], dpi: self.shot_dpi });
        self.request_shot(ctx);
    }

    fn dirty_prefs_soon(&mut self) {
        // the dpi values are small preferences: the layout saver writes them with the rest shortly after
        self.layout_changed.get_or_insert(0.0);
    }

    /// One plot pane drawn again at `dpi` (see the module text); returns width, height and RGBA pixels.
    pub(super) fn render_pane_hires(&mut self, slot: usize, size: Vec2, dpi: f32) -> Result<(u32, u32, Vec<u8>), String> {
        let rs = self.wgpu_rs.clone().ok_or("this computer draws without wgpu, so the dpi cannot be raised")?;
        let scale = dpi / REF_DPI;
        let (w, h) = pixel_size(size, dpi);
        if w as u64 * h as u64 > MAX_PIXELS {
            return Err(format!("{w} x {h} px is too large; lower the dpi"));
        }
        // the pane is drawn by the code of the window, with the same look, in a context of its own
        let ctx2 = egui::Context::default();
        super::apply_style(&ctx2, self.dark);
        // the window's own context finds symbols (the pane's arrow) in the system fonts; this one has to be told the same
        static FONTS: std::sync::OnceLock<std::sync::Arc<egui_system_font_provider::SystemFontProvider>> = std::sync::OnceLock::new();
        ctx2.add_font_provider(FONTS.get_or_init(|| std::sync::Arc::new(egui_system_font_provider::SystemFontProvider::new())).clone());
        ctx2.set_pixels_per_point(scale);
        let th = self.theme();
        let d = self.slot_data();
        let saved_arrow = self.arrow_rect;
        let saved_hover = std::mem::take(&mut self.hover);
        // the core viewer: its photos again at the drawn size, as textures of this context
        let core = slot == 4;
        let saved_cv = self.cv_export_state();
        let saved_tex = core.then(|| {
            let t = self.cv_hires_textures(&ctx2, Rect::from_min_size(Pos2::ZERO, size), scale);
            self.cv_swap_textures(t)
        });
        let area = Rect::from_min_size(Pos2::ZERO, size);
        let mut input = egui::RawInput::default();
        input.screen_rect = Some(area);
        // texture changes in the order egui made them, drained at once (egui panics on a dropped, unhandled delta)
        let mut sets: Vec<(egui::TextureId, egui::epaint::ImageDelta)> = Vec::new();
        let mut frees: Vec<egui::TextureId> = Vec::new();
        let mut last = None;
        // two passes: the first one lays the text out, the second one is what is drawn
        for _ in 0..2 {
            let mut out = ctx2.run_ui(input.clone(), |ui| {
                if core {
                    self.cv_pane(ui, area, &th);
                } else {
                    self.plot_slot(ui, slot, area, &d, &th);
                }
            });
            let mut dl = std::mem::take(&mut out.textures_delta);
            for (id, ds) in dl.set.drain() {
                sets.extend(ds.into_iter().map(|x| (id, x)));
            }
            frees.extend(dl.free.drain());
            last = Some(out);
        }
        self.arrow_rect = saved_arrow;
        self.hover = saved_hover;
        if let Some(t) = saved_tex {
            drop(self.cv_swap_textures(t));
        }
        self.cv_restore_state(saved_cv);
        let out = last.ok_or("no frame")?;
        let ppp = out.pixels_per_point;
        let prims = ctx2.tessellate(out.shapes, ppp);
        let clear = ctx2.global_style().visuals.panel_fill;

        let (device, queue) = (&rs.device, &rs.queue);
        let mut renderer = egui_wgpu::Renderer::new(device, wgpu::TextureFormat::Rgba8Unorm, egui_wgpu::RendererOptions { dithering: false, ..Default::default() });
        for (id, delta) in &sets {
            renderer.update_texture(device, queue, *id, delta);
        }
        let mut img = vec![0u8; w as usize * h as usize * 4];
        let [cr, cg, cb, _] = clear.to_normalized_gamma_f32().map(|v| v as f64);
        let mut ty = 0;
        while ty < h {
            let th_px = TILE.min(h - ty);
            let mut tx = 0;
            while tx < w {
                let tw_px = TILE.min(w - tx);
                // this tile's primitives, moved so the tile's corner is the origin
                let off = Vec2::new(tx as f32, ty as f32) / ppp;
                let tile_pts = Rect::from_min_size(Pos2::ZERO, Vec2::new(tw_px as f32, th_px as f32) / ppp);
                let mut jobs: Vec<egui::ClippedPrimitive> = Vec::new();
                for p in &prims {
                    let clip = p.clip_rect.translate(-off);
                    if !clip.intersects(tile_pts) {
                        continue;
                    }
                    let mut p2 = egui::ClippedPrimitive { clip_rect: clip, primitive: p.primitive.clone() };
                    if let egui::epaint::Primitive::Mesh(m) = &mut p2.primitive {
                        m.translate(-off);
                    }
                    jobs.push(p2);
                }
                let sd = egui_wgpu::ScreenDescriptor { size_in_pixels: [tw_px, th_px], pixels_per_point: ppp };
                let tex = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("ams-export-tile"),
                    size: wgpu::Extent3d { width: tw_px, height: th_px, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[],
                });
                let view = tex.create_view(&Default::default());
                let row_bytes = tw_px * 4;
                let padded = row_bytes.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
                let buf = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("ams-export-readback"),
                    size: padded as u64 * th_px as u64,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                });
                let mut enc = device.create_command_encoder(&Default::default());
                let extra = renderer.update_buffers(device, queue, &mut enc, &jobs, &sd);
                {
                    let mut pass = enc
                        .begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("ams-export"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &view,
                                depth_slice: None,
                                resolve_target: None,
                                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: cr, g: cg, b: cb, a: 1.0 }), store: wgpu::StoreOp::Store },
                            })],
                            depth_stencil_attachment: None,
                            timestamp_writes: None,
                            occlusion_query_set: None,
                            multiview_mask: None,
                        })
                        .forget_lifetime();
                    renderer.render(&mut pass, &jobs, &sd);
                }
                enc.copy_texture_to_buffer(
                    wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                    wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(th_px) } },
                    wgpu::Extent3d { width: tw_px, height: th_px, depth_or_array_layers: 1 },
                );
                queue.submit(extra.into_iter().chain(std::iter::once(enc.finish())));
                let (tx_ch, rx_ch) = std::sync::mpsc::channel();
                buf.slice(..).map_async(wgpu::MapMode::Read, move |r| {
                    let _ = tx_ch.send(r);
                });
                device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| format!("graphics card: {e}"))?;
                rx_ch.recv().map_err(|e| e.to_string())?.map_err(|e| format!("graphics card: {e}"))?;
                {
                    let data = buf.slice(..).get_mapped_range().map_err(|e| format!("graphics card: {e}"))?;
                    for row in 0..th_px as usize {
                        let src = &data[row * padded as usize..row * padded as usize + row_bytes as usize];
                        let dst = ((ty as usize + row) * w as usize + tx as usize) * 4;
                        img[dst..dst + row_bytes as usize].copy_from_slice(src);
                    }
                }
                buf.unmap();
                tex.destroy();
                tx += TILE;
            }
            ty += TILE;
        }
        for id in &frees {
            renderer.free_texture(id);
        }
        // the clear was opaque and egui blends over it, so alpha is already 255; make sure
        for px in img.chunks_exact_mut(4) {
            px[3] = 255;
        }
        Ok((w, h, img))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_size_follows_dpi() {
        assert_eq!(pixel_size(Vec2::new(600.0, 400.0), 96.0), (600, 400));
        assert_eq!(pixel_size(Vec2::new(600.0, 400.0), 300.0), (1875, 1250));
        assert_eq!(pixel_size(Vec2::new(0.1, 0.1), 72.0), (1, 1));
    }

    #[test]
    fn warning_threshold_is_above_800() {
        assert!(!(800.0 > DPI_WARN));
        assert!(801.0 > DPI_WARN);
    }

    #[test]
    fn png_carries_its_dpi_and_still_decodes() {
        let px: Vec<u8> = (0..4 * 3 * 4).map(|i| i as u8).collect();
        let png = png_bytes(4, 3, &px, 300.0).unwrap();
        // the pHYs chunk sits right behind IHDR with 300 dpi = 11811 pixels per metre
        assert_eq!(&png[37..41], b"pHYs");
        assert_eq!(u32::from_be_bytes([png[41], png[42], png[43], png[44]]), 11811);
        assert_eq!(u32::from_be_bytes([png[45], png[46], png[47], png[48]]), 11811);
        assert_eq!(png[49], 1);
        let img = image::load_from_memory(&png).unwrap();
        assert_eq!((img.width(), img.height()), (4, 3));
    }

    #[test]
    fn crc_matches_the_known_value() {
        // CRC-32 of "123456789"
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }
}
