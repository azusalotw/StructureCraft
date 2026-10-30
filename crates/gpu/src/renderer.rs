//! The wgpu side: pipelines, buffers, and the egui paint callback.

use std::sync::Arc;

use egui_wgpu::wgpu;
use structurecraft_geom::{DMat4, DVec3};
use wgpu::util::DeviceExt;

use crate::scene::{AREA_STRIDE, LINE_STRIDE, POINT_STRIDE, Scene};

/// What changes every frame.
#[derive(Debug, Clone, Copy)]
pub struct Uniforms {
    /// Model → clip, in f64; the renderer moves it to the scene origin before going to f32.
    pub view_proj: DMat4,
    /// Viewport size in physical pixels.
    pub viewport: [f32; 2],
}

const UNIFORM_SIZE: u64 = 64 + 16;

struct Pipelines {
    point: wgpu::RenderPipeline,
    line: wgpu::RenderPipeline,
    area: wgpu::RenderPipeline,
    point_top: wgpu::RenderPipeline,
    line_top: wgpu::RenderPipeline,
    area_top: wgpu::RenderPipeline,
}

#[derive(Default)]
struct Buffers {
    version: Option<u64>,
    points: Option<(wgpu::Buffer, u32)>,
    lines: Option<(wgpu::Buffer, u32)>,
    triangles: Option<(wgpu::Buffer, u32)>,
}

impl Buffers {
    fn upload(&mut self, device: &wgpu::Device, scene: &Scene) {
        if self.version == Some(scene.version) {
            return;
        }
        let make = |label: &str, bytes: &[u8], count: u32| {
            (count > 0).then(|| {
                (
                    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some(label),
                        contents: bytes,
                        usage: wgpu::BufferUsages::VERTEX,
                    }),
                    count,
                )
            })
        };
        self.points = make("nodes", &scene.points, scene.point_count());
        self.lines = make("frames", &scene.lines, scene.line_count());
        self.triangles = make("areas", &scene.triangles, scene.vertex_count());
        self.version = Some(scene.version);
    }
}

/// Lives in egui-wgpu's callback resources for the life of the window.
pub struct ViewRenderer {
    pipelines: Pipelines,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    srgb_target: bool,
    model: Buffers,
    selection: Buffers,
    origin: DVec3,
}

fn attributes(list: &[(wgpu::VertexFormat, u64)]) -> Vec<wgpu::VertexAttribute> {
    list.iter()
        .enumerate()
        .map(|(i, (format, offset))| wgpu::VertexAttribute { format: *format, offset: *offset, shader_location: i as u32 })
        .collect()
}

impl ViewRenderer {
    /// `samples` and `depth_format` must match the egui renderer's (eframe's `multisampling`
    /// and `depth_buffer` options).
    pub fn new(device: &wgpu::Device, target: wgpu::TextureFormat, depth_format: Option<wgpu::TextureFormat>, samples: u32) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("structurecraft scene"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scene uniforms"),
            size: UNIFORM_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene uniforms"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene uniforms"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });

        use wgpu::VertexFormat::{Float32, Float32x3, Unorm8x4};
        let point_attrs = attributes(&[(Float32x3, 0), (Unorm8x4, 12), (Float32, 16)]);
        let line_attrs = attributes(&[(Float32x3, 0), (Float32x3, 12), (Unorm8x4, 24), (Float32, 28)]);
        let area_attrs = attributes(&[(Float32x3, 0), (Unorm8x4, 12)]);

        // Opaque nodes and frames write depth; translucent areas test it but do not write, so
        // the frames on a slab's edge stay visible. Selected elements draw on top of their own
        // depth (LessEqual).
        let pipeline = |label: &str,
                        entry: &str,
                        stride: u64,
                        step: wgpu::VertexStepMode,
                        attrs: &[wgpu::VertexAttribute],
                        write: bool,
                        compare: wgpu::CompareFunction| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    buffers: &[wgpu::VertexBufferLayout { array_stride: stride, step_mode: step, attributes: attrs }],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: target,
                        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
                depth_stencil: depth_format.map(|format| wgpu::DepthStencilState {
                    format,
                    depth_write_enabled: write,
                    depth_compare: compare,
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState { count: samples.max(1), ..Default::default() },
                multiview: None,
                cache: None,
            })
        };
        use wgpu::CompareFunction::{Less, LessEqual};
        use wgpu::VertexStepMode::{Instance, Vertex};
        let pipelines = Pipelines {
            point: pipeline("nodes", "vs_point", POINT_STRIDE, Instance, &point_attrs, true, Less),
            line: pipeline("frames", "vs_line", LINE_STRIDE, Instance, &line_attrs, true, Less),
            area: pipeline("areas", "vs_area", AREA_STRIDE, Vertex, &area_attrs, false, Less),
            point_top: pipeline("selected nodes", "vs_point", POINT_STRIDE, Instance, &point_attrs, true, LessEqual),
            line_top: pipeline("selected frames", "vs_line", LINE_STRIDE, Instance, &line_attrs, true, LessEqual),
            area_top: pipeline("selected areas", "vs_area", AREA_STRIDE, Vertex, &area_attrs, false, LessEqual),
        };
        Self {
            pipelines,
            uniforms,
            bind_group,
            srgb_target: target.is_srgb(),
            model: Buffers::default(),
            selection: Buffers::default(),
            origin: DVec3::ZERO,
        }
    }

    fn write_uniforms(&self, queue: &wgpu::Queue, u: &Uniforms) {
        // Move the model to the scene origin in f64, then drop to f32.
        let m = (u.view_proj * DMat4::from_translation(self.origin)).as_mat4();
        let mut bytes = Vec::with_capacity(UNIFORM_SIZE as usize);
        for v in m.to_cols_array() {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        for v in [u.viewport[0].max(1.0), u.viewport[1].max(1.0), if self.srgb_target { 1.0 } else { 0.0 }, 0.0] {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        queue.write_buffer(&self.uniforms, 0, &bytes);
    }

    fn draw<'p>(&self, pass: &mut wgpu::RenderPass<'p>, buffers: &Buffers, top: bool) {
        let p = &self.pipelines;
        let (point, line, area) = if top { (&p.point_top, &p.line_top, &p.area_top) } else { (&p.point, &p.line, &p.area) };
        pass.set_bind_group(0, &self.bind_group, &[]);
        if let Some((buffer, n)) = &buffers.lines {
            pass.set_pipeline(line);
            pass.set_vertex_buffer(0, buffer.slice(..));
            pass.draw(0..6, 0..*n);
        }
        if let Some((buffer, n)) = &buffers.points {
            pass.set_pipeline(point);
            pass.set_vertex_buffer(0, buffer.slice(..));
            pass.draw(0..6, 0..*n);
        }
        if let Some((buffer, n)) = &buffers.triangles {
            pass.set_pipeline(area);
            pass.set_vertex_buffer(0, buffer.slice(..));
            pass.draw(0..*n, 0..1);
        }
    }
}

/// One frame's draw of the scene; give it to egui with
/// `egui_wgpu::Callback::new_paint_callback(rect, callback)`.
pub struct SceneCallback {
    pub uniforms: Uniforms,
    pub model: Arc<Scene>,
    pub selection: Arc<Scene>,
}

impl egui_wgpu::CallbackTrait for SceneCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(r) = resources.get_mut::<ViewRenderer>() {
            r.origin = self.model.origin;
            r.model.upload(device, &self.model);
            r.selection.upload(device, &self.selection);
            r.write_uniforms(queue, &self.uniforms);
        }
        Vec::new()
    }

    fn paint(&self, _info: egui::PaintCallbackInfo, pass: &mut wgpu::RenderPass<'static>, resources: &egui_wgpu::CallbackResources) {
        if let Some(r) = resources.get::<ViewRenderer>() {
            r.draw(pass, &r.model, false);
            r.draw(pass, &r.selection, true);
        }
    }
}
