//! 3D reflectivity raymarch: a fullscreen-triangle MIP raymarch of a volume texture, rendered
//! into an egui rect via an `egui_wgpu` paint callback (mirrors the map callback pattern).

use glam::{Mat4, Vec3};

/// Raymarch uniform block (matches `shaders/raymarch.wgsl`).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Uniforms {
    inv_view_proj: [[f32; 4]; 4],
    cam_pos: [f32; 4],
    box_min: [f32; 4],
    box_max: [f32; 4],
    dims: [f32; 4], // nx, ny, nz, step_count
    ctl: [f32; 4],  // minimum reflectivity index to draw; rest spare
    clip_min: [f32; 4],
    clip_max: [f32; 4],
    /// See `shaders/raymarch.wgsl`'s own field of the same name: xy a world-space unit normal, z
    /// the signed distance along it, w whether the plane is active at all.
    plane: [f32; 4],
}

/// A new volume grid to upload: `data` is `n×n×nz` R8 indices, `lut` a 256-entry RGBA table.
#[derive(Clone)]
pub struct Volume3dUpload {
    pub data: Vec<u8>,
    pub n: u32,
    pub nz: u32,
    pub lut: Vec<u8>,
    pub half_km: f32,
    pub top_km: f32,
}

/// Box extents of the rendered volume (z exaggerated for legibility).
const BOX_MIN: Vec3 = Vec3::new(-1.0, -1.0, 0.0);
const BOX_MAX: Vec3 = Vec3::new(1.0, 1.0, 0.5);

/// An additional vertical clip plane at any bearing (Phase H4) — the axis-aligned `clip` slab can
/// only ever cut along the box's own east-west/north-south faces, so cutting into a storm at the
/// angle it actually leans or approaches from needs a plane that isn't locked to those axes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VerticalPlane {
    /// Which way the plane's normal points, degrees clockwise from north (the volume is kept on
    /// the side the normal points toward) — the same bearing convention as everywhere else in
    /// this app (storm motion, azimuth).
    pub bearing_deg: f32,
    /// Signed offset of the plane from the box center along its normal, as a fraction of the
    /// box's half-width (`-1.0..=1.0`) — matches `View3d::clip`'s own fraction-of-box convention,
    /// so it reads the same regardless of which box (the orbit window's fixed one, or the
    /// main map's dynamically-sized one) it is applied to.
    pub offset: f32,
}

/// What the viewer is looking at, beyond the camera: the reflectivity floor and the slab the
/// raymarch is confined to. Defaults draw the whole volume, which is the old behaviour.
#[derive(Clone, Copy, Debug)]
pub struct View3d {
    /// Minimum volume index (2..=255) a voxel must reach to be drawn. 2 = everything.
    pub threshold_idx: f32,
    /// Slab bounds as fractions of the box, `[x0, x1, y0, y1, z0, z1]`.
    pub clip: [f32; 6],
    /// `None` disables the plane clip entirely (the common case).
    pub plane: Option<VerticalPlane>,
}

impl Default for View3d {
    fn default() -> Self {
        Self {
            threshold_idx: 2.0,
            clip: [0.0, 1.0, 0.0, 1.0, 0.0, 1.0],
            plane: None,
        }
    }
}

/// The `plane` uniform field for a box spanning `box_min..box_max`: world-space unit normal, the
/// signed distance along it, and whether it's active — `[0,0,0,0]` (inert; `pos.x*0+pos.y*0 < 0`
/// is never true) when `plane` is `None`, so callers don't need their own separate enable check.
fn plane_uniform(plane: Option<VerticalPlane>, box_min: Vec3, box_max: Vec3) -> [f32; 4] {
    let Some(p) = plane else {
        return [0.0, 0.0, 0.0, 0.0];
    };
    let theta = p.bearing_deg.to_radians();
    // Compass bearing (0 = north, 90 = east) onto the box's own x = east, y = north axes.
    let (nx, ny) = (theta.sin(), theta.cos());
    let center = (box_min + box_max) * 0.5;
    let half_extent = ((box_max.x - box_min.x).abs()).max((box_max.y - box_min.y).abs()) * 0.5;
    let d = nx * center.x + ny * center.y + p.offset * half_extent;
    [nx, ny, d, 1.0]
}

/// Orbit-camera uniforms: azimuth/elevation in degrees, `dist` from the box center, view `aspect`.
#[allow(clippy::too_many_arguments)]
pub fn orbit_uniform(
    az_deg: f32,
    el_deg: f32,
    dist: f32,
    aspect: f32,
    n: u32,
    nz: u32,
    steps: u32,
    v3: View3d,
) -> Uniforms {
    let center = (BOX_MIN + BOX_MAX) * 0.5;
    let (az, el) = (az_deg.to_radians(), el_deg.to_radians());
    let dir = Vec3::new(el.cos() * az.sin(), el.cos() * az.cos(), el.sin());
    let eye = center + dir * dist;
    let view = Mat4::look_at_rh(eye, center, Vec3::Z);
    let proj = Mat4::perspective_rh(45f32.to_radians(), aspect.max(0.1), 0.01, 100.0);
    let inv = (proj * view).inverse();
    Uniforms {
        inv_view_proj: inv.to_cols_array_2d(),
        cam_pos: [eye.x, eye.y, eye.z, 1.0],
        box_min: [BOX_MIN.x, BOX_MIN.y, BOX_MIN.z, 0.0],
        box_max: [BOX_MAX.x, BOX_MAX.y, BOX_MAX.z, 0.0],
        dims: [n as f32, n as f32, nz as f32, steps as f32],
        ctl: [v3.threshold_idx, 1.0, 0.0, 0.0],
        clip_min: [v3.clip[0], v3.clip[2], v3.clip[4], 0.0],
        clip_max: [v3.clip[1], v3.clip[3], v3.clip[5], 0.0],
        plane: plane_uniform(v3.plane, BOX_MIN, BOX_MAX),
    }
}

/// Main-map raymarch uniforms. The Cartesian texture stays radar-relative, while the box is
/// expressed in the same local pixel coordinates as the pitched geographic camera.
#[allow(clippy::too_many_arguments)]
pub fn map_uniform(
    camera: &crate::render::mercator::Camera,
    viewport: (f32, f32),
    radar_lon: f64,
    radar_lat: f64,
    antenna_altitude_m: f32,
    upload: &Volume3dUpload,
    steps: u32,
    view: View3d,
    vertical_exaggeration: f32,
    opacity: f32,
) -> Uniforms {
    let radar_world = crate::render::mercator::lonlat_to_world(radar_lon, radar_lat);
    let wpp = camera.world_per_pixel();
    let dx = (radar_world.0 - camera.center.0 + 0.5).rem_euclid(1.0) - 0.5;
    let dy = camera.center.1 - radar_world.1;
    let metres_to_px = crate::render::mercator::Camera::world_units_per_metre(radar_lat) / wpp;
    let half_px = upload.half_km as f64 * 1_000.0 * metres_to_px;
    let z0 = antenna_altitude_m as f64 * metres_to_px * vertical_exaggeration as f64;
    let z1 = (antenna_altitude_m as f64 + upload.top_km as f64 * 1_000.0)
        * metres_to_px
        * vertical_exaggeration as f64;
    let box_min = Vec3::new(
        (dx / wpp - half_px) as f32,
        (dy / wpp - half_px) as f32,
        z0 as f32,
    );
    let box_max = Vec3::new(
        (dx / wpp + half_px) as f32,
        (dy / wpp + half_px) as f32,
        z1 as f32,
    );
    let eye = camera.eye_position(viewport);
    Uniforms {
        inv_view_proj: camera.view_projection(viewport).inverse().to_cols_array_2d(),
        cam_pos: [eye.x, eye.y, eye.z, 1.0],
        box_min: [box_min.x, box_min.y, box_min.z, 0.0],
        box_max: [box_max.x, box_max.y, box_max.z, 0.0],
        dims: [upload.n as f32, upload.n as f32, upload.nz as f32, steps as f32],
        ctl: [view.threshold_idx, opacity.clamp(0.0, 1.0), 0.0, 0.0],
        clip_min: [view.clip[0], view.clip[2], view.clip[4], 0.0],
        clip_max: [view.clip[1], view.clip[3], view.clip[5], 0.0],
        plane: plane_uniform(view.plane, box_min, box_max),
    }
}

/// Convert a dBZ threshold into the volume's 2..=255 index space. `range` is the volume's
/// `(value_min, value_max)`; the mapping mirrors `volume3d::build`.
pub fn threshold_index(dbz: f32, range: (f32, f32)) -> f32 {
    let (lo, hi) = range;
    let span = (hi - lo).max(f32::EPSILON);
    (2.0 + ((dbz - lo) / span) * 253.0).clamp(2.0, 255.0)
}

struct Gpu {
    _tex: wgpu::Texture,
    _lut: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

/// Long-lived raymarch resources (pipeline + latest volume), stored in egui's callback map.
///
/// The pipeline is built the first time a 3D volume is uploaded, not at startup: compiling the
/// raymarch shader costs real time on a phone's driver, and most sessions never open the 3D view.
/// The target format the 3D pipeline must be compiled against, parked in the callback resources
/// so the first 3D frame can build [`Volume3dResources`] itself.
#[derive(Clone, Copy)]
pub struct Volume3dFormat(pub wgpu::TextureFormat);

pub struct Volume3dResources {
    format: wgpu::TextureFormat,
    pipeline: Option<wgpu::RenderPipeline>,
    bgl: wgpu::BindGroupLayout,
    uniform_buf: wgpu::Buffer,
    gpu: Option<Gpu>,
}

impl Volume3dResources {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("raymarch_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D3,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("raymarch_uniform"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            format,
            pipeline: None,
            bgl,
            uniform_buf,
            gpu: None,
        }
    }

    /// Compile the raymarch pipeline on first use.
    fn ensure_pipeline(&mut self, device: &wgpu::Device) {
        if self.pipeline.is_some() {
            return;
        }
        let format = self.format;
        let bgl = &self.bgl;
        let shader = device.create_shader_module(wgpu::include_wgsl!("shaders/raymarch.wgsl"));
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("raymarch_layout"),
            bind_group_layouts: &[Some(bgl)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("raymarch_pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    // Premultiplied alpha (shader outputs rgb*a, a).
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        self.pipeline = Some(pipeline);
    }

    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, up: &Volume3dUpload) {
        // Every route to a 3D draw goes through here (the egui callback and `render_once`), so
        // this is the one place the pipeline has to exist by.
        self.ensure_pipeline(device);
        let size = wgpu::Extent3d {
            width: up.n,
            height: up.n,
            depth_or_array_layers: up.nz,
        };
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("volume3d_tex"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format: wgpu::TextureFormat::R8Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &up.data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(up.n),
                rows_per_image: Some(up.n),
            },
            size,
        );
        let lut_size = wgpu::Extent3d {
            width: 256,
            height: 1,
            depth_or_array_layers: 1,
        };
        let lut = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("volume3d_lut"),
            size: lut_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &lut,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &up.lut,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256 * 4),
                rows_per_image: Some(1),
            },
            lut_size,
        );
        let tex_view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let lut_view = lut.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("raymarch_bg"),
            layout: &self.bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&tex_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&lut_view),
                },
            ],
        });
        self.gpu = Some(Gpu {
            _tex: tex,
            _lut: lut,
            bind_group,
        });
    }

    fn record(&self, pass: &mut wgpu::RenderPass<'_>) {
        if let (Some(gpu), Some(pipeline)) = (&self.gpu, &self.pipeline) {
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &gpu.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    /// Upload a volume + camera and raymarch it once into `view` (headless verify harness).
    pub fn render_once(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        upload: &Volume3dUpload,
        uniform: Uniforms,
        clear: wgpu::Color,
    ) {
        self.upload(device, queue, upload);
        queue.write_buffer(&self.uniform_buf, 0, bytemuck::bytes_of(&uniform));
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("raymarch_headless"),
        });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("raymarch_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.record(&mut pass);
        }
        queue.submit(Some(enc.finish()));
    }
}

/// Per-frame raymarch draw: an optional new volume upload + the current camera uniforms.
pub struct Volume3dCallback {
    pub upload: Option<Volume3dUpload>,
    pub uniform: Uniforms,
}

impl egui_wgpu::CallbackTrait for Volume3dCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        // Built on first use rather than at startup: the raymarch pipeline is a few hundred ms
        // of shader compilation in front of the first frame, for a window most sessions never
        // open. `Volume3dFormat` is inserted at startup so we know what to compile against.
        if resources.get::<Volume3dResources>().is_none() {
            if let Some(Volume3dFormat(fmt)) = resources.get::<Volume3dFormat>().copied() {
                resources.insert(Volume3dResources::new(device, fmt));
            }
        }
        if let Some(res) = resources.get_mut::<Volume3dResources>() {
            if let Some(up) = &self.upload {
                res.upload(device, queue, up);
            }
            queue.write_buffer(&res.uniform_buf, 0, bytemuck::bytes_of(&self.uniform));
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        if let Some(res) = resources.get::<Volume3dResources>() {
            res.record(pass);
        }
    }
}

/// The map-pitch "Smooth" representation: the same raymarch shader as the standalone 3D window,
/// but resident per pane rather than as the one volume the window shows. A deliberately separate
/// type from [`Volume3dResources`] — both live in the same `egui_wgpu::CallbackResources`
/// type-map, keyed by type, so sharing one would mean the window and an on-map pane fight over a
/// single GPU texture the moment they show different volumes at once.
///
/// Bounded to 4 panes (the app's own pane-count ceiling; see `PaletteAction::SetPanes`) rather
/// than a `HashMap`, since the index is always small and known ahead of time.
pub struct MapVolume3dResources {
    format: wgpu::TextureFormat,
    pipeline: Option<wgpu::RenderPipeline>,
    bgl: wgpu::BindGroupLayout,
    panes: [Option<Gpu>; 4],
    uniform_bufs: [Option<wgpu::Buffer>; 4],
}

impl MapVolume3dResources {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        // Identical layout to `Volume3dResources`'s — both compile the same `raymarch.wgsl`
        // against the same three bindings, just into separate pipeline objects so an on-map pane
        // and the popup window never share a bind group.
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("map_raymarch_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D3,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        Self {
            format,
            pipeline: None,
            bgl,
            // `[None; 4]` needs `Option<Gpu>: Copy`, which a `wgpu::Texture`/`BindGroup` inside
            // it is not; `from_fn` builds the array without that requirement.
            panes: std::array::from_fn(|_| None),
            uniform_bufs: std::array::from_fn(|_| None),
        }
    }

    fn ensure_pipeline(&mut self, device: &wgpu::Device) {
        if self.pipeline.is_some() {
            return;
        }
        let format = self.format;
        let bgl = &self.bgl;
        // The same shader file as the window's raymarch — a second, independent pipeline object
        // compiled from it, not a shared one; see the type's own doc comment for why.
        let shader = device.create_shader_module(wgpu::include_wgsl!("shaders/raymarch.wgsl"));
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("map_raymarch_layout"),
            bind_group_layouts: &[Some(bgl)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("map_raymarch_pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        self.pipeline = Some(pipeline);
    }

    fn upload_for_pane(
        &mut self,
        pane: usize,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        up: &Volume3dUpload,
    ) {
        self.ensure_pipeline(device);
        if self.uniform_bufs[pane].is_none() {
            self.uniform_bufs[pane] = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("map_raymarch_uniform"),
                size: std::mem::size_of::<Uniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let size = wgpu::Extent3d {
            width: up.n,
            height: up.n,
            depth_or_array_layers: up.nz,
        };
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("map_volume3d_tex"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format: wgpu::TextureFormat::R8Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &up.data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(up.n),
                rows_per_image: Some(up.n),
            },
            size,
        );
        let lut_size = wgpu::Extent3d {
            width: 256,
            height: 1,
            depth_or_array_layers: 1,
        };
        let lut = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("map_volume3d_lut"),
            size: lut_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &lut,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &up.lut,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256 * 4),
                rows_per_image: Some(1),
            },
            lut_size,
        );
        let tex_view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let lut_view = lut.create_view(&wgpu::TextureViewDescriptor::default());
        let uniform_buf = self.uniform_bufs[pane].as_ref().expect("just created above");
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("map_raymarch_bg"),
            layout: &self.bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&tex_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&lut_view),
                },
            ],
        });
        self.panes[pane] = Some(Gpu {
            _tex: tex,
            _lut: lut,
            bind_group,
        });
    }

    fn set_uniform_for_pane(&mut self, pane: usize, queue: &wgpu::Queue, uniform: Uniforms) {
        if let Some(buf) = &self.uniform_bufs[pane] {
            queue.write_buffer(buf, 0, bytemuck::bytes_of(&uniform));
        }
    }

    fn record_for_pane(&self, pane: usize, pass: &mut wgpu::RenderPass<'_>) {
        if let (Some(gpu), Some(pipeline)) = (&self.panes[pane], &self.pipeline) {
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &gpu.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

/// Per-frame draw for one pane's map-pitch smooth volume: which pane (indexes
/// [`MapVolume3dResources`]'s fixed-size pane array), an optional new volume upload, and this
/// frame's camera/radar uniforms (built by [`map_uniform`]).
pub struct MapVolume3dCallback {
    pub pane: u32,
    pub upload: Option<Volume3dUpload>,
    pub uniform: Uniforms,
}

impl egui_wgpu::CallbackTrait for MapVolume3dCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if resources.get::<MapVolume3dResources>().is_none() {
            if let Some(Volume3dFormat(fmt)) = resources.get::<Volume3dFormat>().copied() {
                resources.insert(MapVolume3dResources::new(device, fmt));
            }
        }
        if let Some(res) = resources.get_mut::<MapVolume3dResources>() {
            let pane = self.pane as usize;
            if let Some(up) = &self.upload {
                res.upload_for_pane(pane, device, queue, up);
            }
            res.set_uniform_for_pane(pane, queue, self.uniform);
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        if let Some(res) = resources.get::<MapVolume3dResources>() {
            res.record_for_pane(self.pane as usize, pass);
        }
    }
}

#[cfg(test)]
mod plane_tests {
    use super::{plane_uniform, VerticalPlane};
    use glam::Vec3;

    // A 2x2x1 box centered on the origin, same shape `orbit_uniform` and `map_uniform` both hand
    // in (they differ only in where that box sits and how big it is).
    const BOX_MIN: Vec3 = Vec3::new(-1.0, -1.0, 0.0);
    const BOX_MAX: Vec3 = Vec3::new(1.0, 1.0, 1.0);

    #[test]
    fn disabled_plane_is_inert() {
        assert_eq!(plane_uniform(None, BOX_MIN, BOX_MAX), [0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn north_bearing_is_a_unit_normal_pointing_north() {
        let p = VerticalPlane { bearing_deg: 0.0, offset: 0.0 };
        let [nx, ny, _, on] = plane_uniform(Some(p), BOX_MIN, BOX_MAX);
        assert_eq!(on, 1.0);
        assert!(nx.abs() < 1e-5, "north has no east component: {nx}");
        assert!((ny - 1.0).abs() < 1e-5, "north is +y: {ny}");
    }

    #[test]
    fn east_bearing_is_a_unit_normal_pointing_east() {
        let p = VerticalPlane { bearing_deg: 90.0, offset: 0.0 };
        let [nx, ny, _, _] = plane_uniform(Some(p), BOX_MIN, BOX_MAX);
        assert!((nx - 1.0).abs() < 1e-5, "east is +x: {nx}");
        assert!(ny.abs() < 1e-5, "east has no north component: {ny}");
    }

    #[test]
    fn zero_offset_passes_through_the_box_center() {
        // Center is (0,0) here, so the plane's distance along any normal is 0.
        let p = VerticalPlane { bearing_deg: 37.0, offset: 0.0 };
        let [.., d, _] = plane_uniform(Some(p), BOX_MIN, BOX_MAX);
        assert!(d.abs() < 1e-5, "plane through a centered box's own center: {d}");
    }

    #[test]
    fn offset_scales_with_the_box_half_width_not_a_fixed_distance() {
        // This box's half-width is 1.0 (spans -1..1); offset 0.5 should land the plane at
        // world distance 0.5 along its normal from the box center.
        let p = VerticalPlane { bearing_deg: 0.0, offset: 0.5 };
        let [_, ny, d, _] = plane_uniform(Some(p), BOX_MIN, BOX_MAX);
        assert!((ny - 1.0).abs() < 1e-5);
        assert!((d - 0.5).abs() < 1e-5, "d: {d}");

        // A box twice as wide (still centered on the origin) scales the same fractional offset
        // to twice the world distance.
        let wide_min = Vec3::new(-2.0, -2.0, 0.0);
        let wide_max = Vec3::new(2.0, 2.0, 1.0);
        let [_, _, d_wide, _] = plane_uniform(Some(p), wide_min, wide_max);
        assert!((d_wide - 1.0).abs() < 1e-5, "d_wide: {d_wide}");
    }

    #[test]
    fn a_box_not_centered_on_the_origin_offsets_the_plane_with_it() {
        // Same shape as BOX_MIN..BOX_MAX but shifted +5 in x and +3 in y — as map_uniform's box
        // is, sitting wherever the radar is on screen rather than at a fixed origin.
        let shifted_min = BOX_MIN + Vec3::new(5.0, 3.0, 0.0);
        let shifted_max = BOX_MAX + Vec3::new(5.0, 3.0, 0.0);
        let p = VerticalPlane { bearing_deg: 0.0, offset: 0.0 };
        let [_, ny, d, _] = plane_uniform(Some(p), shifted_min, shifted_max);
        assert!((ny - 1.0).abs() < 1e-5);
        // The plane through the (shifted) center: d = normal . center = 1*3 = 3.
        assert!((d - 3.0).abs() < 1e-5, "d: {d}");
    }
}
