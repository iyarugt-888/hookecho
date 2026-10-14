//! Test-only offscreen egui captures using the app's GPU renderer and fonts.

pub(crate) struct Snapshot {
    device: wgpu::Device,
    queue: wgpu::Queue,
}

impl Snapshot {
    pub(crate) fn new() -> anyhow::Result<Self> {
        let rt = tokio::runtime::Runtime::new()?;
        let (device, queue, _) = super::init_gpu(&rt)?;
        Ok(Self { device, queue })
    }

    pub(crate) fn save(
        &self,
        path: &std::path::Path,
        width: u32,
        height: u32,
        draw: impl FnMut(&mut egui::Ui),
    ) -> anyhow::Result<()> {
        use eframe::egui_wgpu::{Renderer, RendererOptions};
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let mut renderer = Renderer::new(&self.device, format, RendererOptions::PREDICTABLE);
        capture(
            &self.device,
            &self.queue,
            &mut renderer,
            format,
            path,
            (width, height),
            3,
            draw,
        )
    }
}

/// [`Snapshot::save`] through an app's own render state, so the app's GPU paint callbacks (the
/// radar and 3D panes, registered on that renderer) draw too. `frames` passes are run before the
/// capture, for work that lands over several frames.
pub(crate) fn save_with_state(
    state: &eframe::egui_wgpu::RenderState,
    path: &std::path::Path,
    size: (u32, u32),
    frames: usize,
    draw: impl FnMut(&mut egui::Ui),
) -> anyhow::Result<()> {
    let mut renderer = state.renderer.write();
    capture(
        &state.device,
        &state.queue,
        &mut renderer,
        state.target_format,
        path,
        size,
        frames,
        draw,
    )
}

#[allow(clippy::too_many_arguments)]
fn capture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &mut eframe::egui_wgpu::Renderer,
    format: wgpu::TextureFormat,
    path: &std::path::Path,
    (width, height): (u32, u32),
    frames: usize,
    mut draw: impl FnMut(&mut egui::Ui),
) -> anyhow::Result<()> {
    use eframe::egui_wgpu::ScreenDescriptor;
    let ctx = egui::Context::default();
    ctx.set_fonts(crate::fonts::base());
    crate::theme::apply(&ctx, None);
    let size = width.max(height);
    let target = super::new_target(device, format, size);
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let screen = ScreenDescriptor {
        size_in_pixels: [size, size],
        pixels_per_point: 1.0,
    };
    for frame in 0..frames.max(1) {
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width as f32, height as f32),
                )),
                time: Some(frame as f64 / 60.0),
                ..Default::default()
            },
            |ui| draw(ui),
        );
        for (id, delta) in &output.textures_delta.set {
            renderer.update_texture(device, queue, *id, delta);
        }
        let jobs = ctx.tessellate(output.shapes, output.pixels_per_point);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let callbacks = renderer.update_buffers(device, queue, &mut encoder, &jobs, &screen);
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("UI snapshot"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            renderer.render(&mut pass.forget_lifetime(), &jobs, &screen);
        }
        queue.submit(
            callbacks
                .into_iter()
                .chain(std::iter::once(encoder.finish())),
        );
        for id in &output.textures_delta.free {
            renderer.free_texture(id);
        }
    }
    let rgba = super::read_target(device, queue, &target, size);
    let square = image::RgbaImage::from_raw(size, size, rgba).expect("RGBA target size");
    image::imageops::crop_imm(&square, 0, 0, width, height)
        .to_image()
        .save(path)?;
    Ok(())
}
