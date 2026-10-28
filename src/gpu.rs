//! Draw the image as one textured quad, then optional confirmation text.

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use cosmic_text::{Align, Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Wrap};
use glyphon::{
    Cache, Color, Resolution, SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport,
};
use wgpu::CurrentSurfaceTexture;
use winit::dpi::PhysicalSize;
use winit::window::Window;

use crate::decode::DecodedImage;
use crate::transport::Layout;

const SHADER: &str = r#"
struct VertexOutput {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}

@vertex
fn vs_main(
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
) -> VertexOutput {
    var out: VertexOutput;
    out.clip = vec4<f32>(pos, 0.0, 1.0);
    out.uv = uv;
    out.color = color;
    return out;
}

@group(0) @binding(0) var image_texture: texture_2d<f32>;
@group(0) @binding(1) var image_sampler: sampler;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(image_texture, image_sampler, in.uv) * in.color;
}
"#;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    pos: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
}

pub struct TransportDraw<'a> {
    pub layout: Layout,
    pub show_play_icon: bool,
    pub label: &'a str,
}

pub struct Frame<'a> {
    pub image_width: f32,
    pub image_height: f32,
    pub scale: f32,
    pub pan_x: f32,
    pub pan_y: f32,
    pub dim: bool,
    pub text: Option<&'a str>,
    pub transport: Option<TransportDraw<'a>>,
    pub scale_factor: f32,
}

pub struct Gpu {
    instance: wgpu::Instance,
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    bind_group_layout: wgpu::BindGroupLayout,
    vertices: wgpu::Buffer,
    white_bind: wgpu::BindGroup,
    image: Option<ImageTexture>,
    text: TextLayer,
    max_edge: u32,
}

struct ImageTexture {
    texture: wgpu::Texture,
    bind: wgpu::BindGroup,
    width: u32,
    height: u32,
}

struct TextLayer {
    font_system: FontSystem,
    swash_cache: SwashCache,
    viewport: Viewport,
    atlas: TextAtlas,
    renderer: TextRenderer,
    buffer: Buffer,
    laid_out: String,
    laid_out_width: u32,
    laid_out_scale: f32,
    bar_buffer: Buffer,
    bar_laid_out: String,
    bar_laid_out_width: u32,
    bar_laid_out_scale: f32,
}

impl Gpu {
    pub async fn new(
        window: Arc<Window>,
        display_handle: winit::event_loop::OwnedDisplayHandle,
    ) -> Result<Self, String> {
        let size = window.inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);

        let instance = wgpu::Instance::new(
            wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(display_handle)),
        );
        let surface = instance
            .create_surface(window.clone())
            .map_err(|err| format!("Couldn't create a drawing surface. {err}"))?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .map_err(|err| format!("Couldn't find a GPU. {err}"))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("matejko"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default().using_resolution(adapter.limits()),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|err| format!("Couldn't open the GPU. {err}"))?;

        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(|format| format.is_srgb())
            .unwrap_or(capabilities.formats[0]);
        let mut config = surface
            .get_default_config(&adapter, width, height)
            .ok_or_else(|| "The GPU can't draw to this window.".to_string())?;
        config.format = format;
        config.view_formats = vec![format];
        config.present_mode = wgpu::PresentMode::Fifo;
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("image"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("image"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("image"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("image"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x2,
                        1 => Float32x2,
                        2 => Float32x4
                    ],
                })],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("image"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let vertices = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("vertices"),
            size: (std::mem::size_of::<Vertex>() * 64) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let white = white_texture(&device, &queue);
        let white_view = white.create_view(&wgpu::TextureViewDescriptor::default());
        let white_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("white"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&white_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let text = TextLayer::new(&device, &queue, format);
        let max_edge = device.limits().max_texture_dimension_2d;

        Ok(Self {
            instance,
            window,
            surface,
            device,
            queue,
            config,
            pipeline,
            sampler,
            bind_group_layout,
            vertices,
            white_bind,
            image: None,
            text,
            max_edge,
        })
    }

    pub fn max_edge(&self) -> u32 {
        self.max_edge
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        // Cap to the device limit so a mixed-DPI drag that still reports a
        // huge size cannot ask wgpu for a surface larger than it allows.
        self.config.width = size.width.clamp(1, self.max_edge);
        self.config.height = size.height.clamp(1, self.max_edge);
        self.surface.configure(&self.device, &self.config);
    }

    pub fn clear_image(&mut self) {
        self.image = None;
    }

    pub fn set_image(&mut self, image: &DecodedImage) -> Result<(), String> {
        if image.width == 0 || image.height == 0 {
            return Err("This image has no pixels.".into());
        }
        if image.width > self.max_edge || image.height > self.max_edge {
            return Err(format!(
                "This image is too large to display ({} × {}).",
                image.width, image.height
            ));
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("photo"),
            size: wgpu::Extent3d {
                width: image.width,
                height: image.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        upload_rgba(&self.queue, &texture, image);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("photo"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        self.image = Some(ImageTexture {
            texture,
            bind,
            width: image.width,
            height: image.height,
        });
        Ok(())
    }

    /// Upload pixels into the current texture when the size matches, so a video
    /// frame does not reset zoom. A different size allocates a new texture.
    pub fn replace_pixels(&mut self, image: &DecodedImage) -> Result<(), String> {
        if let Some(existing) = &self.image {
            if existing.width == image.width && existing.height == image.height {
                upload_rgba(&self.queue, &existing.texture, image);
                return Ok(());
            }
        }
        self.set_image(image)
    }

    pub fn draw(&mut self, frame: Frame<'_>) -> Result<(), String> {
        let texture = match self.surface.get_current_texture() {
            CurrentSurfaceTexture::Success(texture) => texture,
            CurrentSurfaceTexture::Timeout | CurrentSurfaceTexture::Occluded => {
                self.window.request_redraw();
                return Ok(());
            }
            CurrentSurfaceTexture::Suboptimal(_) | CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.config);
                self.window.request_redraw();
                return Ok(());
            }
            CurrentSurfaceTexture::Lost => {
                self.surface = self
                    .instance
                    .create_surface(self.window.clone())
                    .map_err(|err| format!("Couldn't recreate the drawing surface. {err}"))?;
                self.surface.configure(&self.device, &self.config);
                self.window.request_redraw();
                return Ok(());
            }
            CurrentSurfaceTexture::Validation => {
                return Err("The GPU rejected a frame.".into());
            }
        };

        let width = self.config.width as f32;
        let height = self.config.height as f32;
        let mut vertices = Vec::with_capacity(12);
        let image_count = if self.image.is_some()
            && frame.image_width > 0.0
            && frame.image_height > 0.0
            && width > 0.0
            && height > 0.0
        {
            push_quad(
                &mut vertices,
                frame.pan_x,
                frame.pan_y,
                frame.pan_x + frame.image_width * frame.scale,
                frame.pan_y + frame.image_height * frame.scale,
                width,
                height,
                [1.0, 1.0, 1.0, 1.0],
                true,
            );
            6
        } else {
            0
        };
        let chrome_start = vertices.len() as u32;
        if frame.dim {
            push_quad(
                &mut vertices,
                0.0,
                0.0,
                width,
                height,
                width,
                height,
                [0.0, 0.0, 0.0, 0.62],
                false,
            );
        }
        if let Some(transport) = &frame.transport {
            push_transport(&mut vertices, transport, width, height);
        }
        let chrome_count = vertices.len() as u32 - chrome_start;
        if !vertices.is_empty() {
            self.queue
                .write_buffer(&self.vertices, 0, bytemuck::cast_slice(&vertices));
        }

        let draw_text = self.text.prepare(
            &self.device,
            &self.queue,
            frame.text,
            frame.transport,
            self.config.width,
            self.config.height,
            frame.scale_factor,
        );

        let view = texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if image_count > 0 || chrome_count > 0 {
                pass.set_pipeline(&self.pipeline);
                pass.set_vertex_buffer(0, self.vertices.slice(..));
            }
            if image_count > 0 {
                if let Some(image) = &self.image {
                    pass.set_bind_group(0, &image.bind, &[]);
                    pass.draw(0..image_count, 0..1);
                }
            }
            if chrome_count > 0 {
                pass.set_bind_group(0, &self.white_bind, &[]);
                pass.draw(chrome_start..chrome_start + chrome_count, 0..1);
            }
            if draw_text {
                let _ = self
                    .text
                    .renderer
                    .render(&self.text.atlas, &self.text.viewport, &mut pass);
            }
        }
        self.queue.submit(std::iter::once(encoder.finish()));
        self.window.pre_present_notify();
        self.queue.present(texture);
        Ok(())
    }
}

impl TextLayer {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let cache = Cache::new(device);
        let viewport = Viewport::new(device, &cache);
        let mut atlas = TextAtlas::new(device, queue, &cache, format);
        let renderer =
            TextRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);
        let mut font_system = FontSystem::new();
        let buffer = Buffer::new(&mut font_system, Metrics::new(20.0, 28.0));
        let bar_buffer = Buffer::new(&mut font_system, Metrics::new(16.0, 20.0));
        Self {
            font_system,
            swash_cache: SwashCache::new(),
            viewport,
            atlas,
            renderer,
            buffer,
            laid_out: String::new(),
            laid_out_width: 0,
            laid_out_scale: 0.0,
            bar_buffer,
            bar_laid_out: String::new(),
            bar_laid_out_width: 0,
            bar_laid_out_scale: 0.0,
        }
    }

    /// Lay out overlay text and the optional video-bar label.
    fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        text: Option<&str>,
        transport: Option<TransportDraw<'_>>,
        width: u32,
        height: u32,
        scale_factor: f32,
    ) -> bool {
        let scale = scale_factor.max(1.0);
        let overlay = text.unwrap_or("");
        if text.is_some()
            && (overlay != self.laid_out
                || width != self.laid_out_width
                || (scale - self.laid_out_scale).abs() > 0.01)
        {
            self.buffer.set_wrap(Wrap::Glyph);
            self.buffer.set_metrics_and_size(
                Metrics::new(20.0 * scale, 28.0 * scale),
                Some(width as f32),
                None,
            );
            let attrs = Attrs::new().family(Family::SansSerif);
            self.buffer
                .set_text(overlay, &attrs, Shaping::Advanced, Some(Align::Center));
            self.buffer.shape_until_scroll(&mut self.font_system, false);
            self.laid_out = overlay.to_string();
            self.laid_out_width = width;
            self.laid_out_scale = scale;
        } else if text.is_some() {
            self.buffer.shape_until_scroll(&mut self.font_system, false);
        }

        let has_bar = transport.is_some();
        let (bar_label, bar_layout) = match transport {
            Some(bar) => (bar.label.to_string(), Some(bar.layout)),
            None => (String::new(), None),
        };
        let bar_label = bar_label.as_str();
        if has_bar
            && (bar_label != self.bar_laid_out
                || width != self.bar_laid_out_width
                || (scale - self.bar_laid_out_scale).abs() > 0.01)
        {
            let size = bar_layout.map(|layout| layout.label_size).unwrap_or(16.0);
            let line = bar_layout.map(|layout| layout.label_line).unwrap_or(20.0);
            self.bar_buffer.set_wrap(Wrap::None);
            self.bar_buffer.set_metrics_and_size(
                Metrics::new(size, line),
                Some(width as f32),
                None,
            );
            let attrs = Attrs::new().family(Family::SansSerif);
            self.bar_buffer
                .set_text(bar_label, &attrs, Shaping::Advanced, Some(Align::Left));
            self.bar_buffer
                .shape_until_scroll(&mut self.font_system, false);
            self.bar_laid_out = bar_label.to_string();
            self.bar_laid_out_width = width;
            self.bar_laid_out_scale = scale;
        } else if has_bar {
            self.bar_buffer
                .shape_until_scroll(&mut self.font_system, false);
        }

        if text.is_none() && !has_bar {
            return false;
        }

        let mut min_y = f32::MAX;
        let mut max_y = 0.0_f32;
        let mut any = false;
        if text.is_some() {
            for run in self.buffer.layout_runs() {
                any = true;
                min_y = min_y.min(run.line_y);
                max_y = max_y.max(run.line_y + run.line_height);
            }
        }
        let top = if any {
            ((height as f32) - (max_y - min_y)) / 2.0 - min_y
        } else {
            0.0
        };

        self.viewport.update(queue, Resolution { width, height });
        let bounds = TextBounds {
            left: 0,
            top: 0,
            right: width as i32,
            bottom: height as i32,
        };
        let mut areas = Vec::new();
        if text.is_some() {
            areas.push(TextArea {
                buffer: &self.buffer,
                left: 0.0,
                top,
                scale: 1.0,
                bounds,
                default_color: Color::rgb(255, 255, 255),
                custom_glyphs: &[],
            });
        }
        if let Some(layout) = bar_layout {
            areas.push(TextArea {
                buffer: &self.bar_buffer,
                left: layout.label_x,
                top: layout.label_top,
                scale: 1.0,
                bounds,
                default_color: Color::rgb(255, 255, 255),
                custom_glyphs: &[],
            });
        }
        self.renderer
            .prepare(
                device,
                queue,
                &mut self.font_system,
                &mut self.atlas,
                &self.viewport,
                areas,
                &mut self.swash_cache,
            )
            .is_ok()
    }
}

fn push_transport(
    vertices: &mut Vec<Vertex>,
    transport: &TransportDraw<'_>,
    width: f32,
    height: f32,
) {
    let layout = transport.layout;
    push_quad(
        vertices,
        layout.bar.x0,
        layout.bar.y0,
        layout.bar.x1,
        layout.bar.y1,
        width,
        height,
        [0.0, 0.0, 0.0, 0.72],
        false,
    );
    push_quad(
        vertices,
        layout.track.x0,
        layout.track.y0,
        layout.track.x1,
        layout.track.y1,
        width,
        height,
        [1.0, 1.0, 1.0, 0.35],
        false,
    );
    if layout.fill.width() > 0.5 {
        push_quad(
            vertices,
            layout.fill.x0,
            layout.fill.y0,
            layout.fill.x1,
            layout.fill.y1,
            width,
            height,
            [1.0, 1.0, 1.0, 0.95],
            false,
        );
    }
    let button = layout.button;
    let white = [1.0, 1.0, 1.0, 1.0];
    if transport.show_play_icon {
        let inset = button.width() * 0.32;
        push_triangle(
            vertices,
            button.x0 + inset,
            button.y0 + inset * 0.7,
            button.x0 + inset,
            button.y1 - inset * 0.7,
            button.x1 - inset * 0.75,
            (button.y0 + button.y1) / 2.0,
            width,
            height,
            white,
        );
    } else {
        let inset_x = button.width() * 0.3;
        let inset_y = button.height() * 0.28;
        let gap = button.width() * 0.16;
        let bar_w = ((button.width() - inset_x * 2.0 - gap) / 2.0).max(1.0);
        push_quad(
            vertices,
            button.x0 + inset_x,
            button.y0 + inset_y,
            button.x0 + inset_x + bar_w,
            button.y1 - inset_y,
            width,
            height,
            white,
            false,
        );
        push_quad(
            vertices,
            button.x1 - inset_x - bar_w,
            button.y0 + inset_y,
            button.x1 - inset_x,
            button.y1 - inset_y,
            width,
            height,
            white,
            false,
        );
    }
}

fn push_triangle(
    vertices: &mut Vec<Vertex>,
    ax: f32,
    ay: f32,
    bx: f32,
    by: f32,
    cx: f32,
    cy: f32,
    width: f32,
    height: f32,
    color: [f32; 4],
) {
    for (x, y) in [(ax, ay), (bx, by), (cx, cy)] {
        vertices.push(Vertex {
            pos: ndc(x, y, width, height),
            uv: [0.5, 0.5],
            color,
        });
    }
}

fn white_texture(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::Texture {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("white"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let mut pixel = vec![0u8; wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize];
    pixel[0] = 255;
    pixel[1] = 255;
    pixel[2] = 255;
    pixel[3] = 255;
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &pixel,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT),
            rows_per_image: Some(1),
        },
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    texture
}

fn upload_rgba(queue: &wgpu::Queue, texture: &wgpu::Texture, image: &DecodedImage) {
    let tight = image.width * 4;
    let aligned =
        tight.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    if aligned == tight {
        queue.write_texture(
            copy_dst(texture),
            &image.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(tight),
                rows_per_image: Some(image.height),
            },
            extent(image),
        );
        return;
    }
    let row_tight = tight as usize;
    let row_aligned = aligned as usize;
    let mut padded = vec![0u8; row_aligned * image.height as usize];
    for row in 0..image.height as usize {
        let from = row * row_tight;
        let to = row * row_aligned;
        padded[to..to + row_tight].copy_from_slice(&image.rgba[from..from + row_tight]);
    }
    queue.write_texture(
        copy_dst(texture),
        &padded,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(aligned),
            rows_per_image: Some(image.height),
        },
        extent(image),
    );
}

fn copy_dst(texture: &wgpu::Texture) -> wgpu::TexelCopyTextureInfo<'_> {
    wgpu::TexelCopyTextureInfo {
        texture,
        mip_level: 0,
        origin: wgpu::Origin3d::ZERO,
        aspect: wgpu::TextureAspect::All,
    }
}

fn extent(image: &DecodedImage) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width: image.width,
        height: image.height,
        depth_or_array_layers: 1,
    }
}

fn push_quad(
    vertices: &mut Vec<Vertex>,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    width: f32,
    height: f32,
    color: [f32; 4],
    image_uv: bool,
) {
    let lt = ndc(left, top, width, height);
    let lb = ndc(left, bottom, width, height);
    let rt = ndc(right, top, width, height);
    let rb = ndc(right, bottom, width, height);
    let (uv_lt, uv_lb, uv_rt, uv_rb) = if image_uv {
        ([0.0, 0.0], [0.0, 1.0], [1.0, 0.0], [1.0, 1.0])
    } else {
        ([0.5, 0.5], [0.5, 0.5], [0.5, 0.5], [0.5, 0.5])
    };
    let corners = [
        (lt, uv_lt),
        (lb, uv_lb),
        (rt, uv_rt),
        (rt, uv_rt),
        (lb, uv_lb),
        (rb, uv_rb),
    ];
    for (pos, uv) in corners {
        vertices.push(Vertex { pos, uv, color });
    }
}

fn ndc(x: f32, y: f32, width: f32, height: f32) -> [f32; 2] {
    if width <= 0.0 || height <= 0.0 {
        return [0.0, 0.0];
    }
    [x / width * 2.0 - 1.0, 1.0 - y / height * 2.0]
}
