use super::bind_groups::BindGroupLayouts;
use wgpu::util::DeviceExt;

const KERNEL_SIZE: usize = 64;
const NOISE_SIZE: u32 = 64;

pub struct SsaoState {
    pub gbuffer_normal_texture: wgpu::Texture,
    pub gbuffer_normal_view: wgpu::TextureView,
    pub gbuffer_depth_texture: wgpu::Texture,
    pub gbuffer_depth_view: wgpu::TextureView,

    pub ssao_raw_texture: wgpu::Texture,
    pub ssao_raw_view: wgpu::TextureView,
    pub ssao_blur_texture: wgpu::Texture,
    pub ssao_blur_view: wgpu::TextureView,
    pub ssao_output_texture: wgpu::Texture,
    pub ssao_output_view: wgpu::TextureView,

    pub noise_texture: wgpu::Texture,
    pub noise_view: wgpu::TextureView,
    pub kernel_buffer: wgpu::Buffer,
    pub sampler: wgpu::Sampler,

    pub white_texture: wgpu::Texture,
    pub white_view: wgpu::TextureView,

    pub ssao_bind_group: wgpu::BindGroup,
    pub blur_h_bind_group: wgpu::BindGroup,
    pub blur_v_bind_group: wgpu::BindGroup,
    pub read_bind_group: wgpu::BindGroup,
    pub read_off_bind_group: wgpu::BindGroup,
}

impl SsaoState {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layouts: &BindGroupLayouts,
        camera_buffer: &wgpu::Buffer,
        width: u32,
        height: u32,
    ) -> Self {
        let kernel_data = generate_kernel();
        let kernel_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("SSAO Kernel Buffer"),
            contents: bytemuck::cast_slice(&kernel_data),
            usage: wgpu::BufferUsages::UNIFORM,
        });

        let noise_data = generate_noise();
        let noise_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("SSAO Noise Texture"),
            size: wgpu::Extent3d {
                width: NOISE_SIZE,
                height: NOISE_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Snorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &noise_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &noise_data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(NOISE_SIZE * 4),
                rows_per_image: Some(NOISE_SIZE),
            },
            wgpu::Extent3d {
                width: NOISE_SIZE,
                height: NOISE_SIZE,
                depth_or_array_layers: 1,
            },
        );
        let noise_view = noise_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("SSAO Sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let white_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("SSAO White Fallback"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &white_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &[255u8],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(1),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        let white_view = white_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let (gbuffer_normal_texture, gbuffer_normal_view) =
            create_normal_texture(device, width, height);
        let (gbuffer_depth_texture, gbuffer_depth_view) =
            create_gbuffer_depth_texture(device, width, height);
        let (ssao_raw_texture, ssao_raw_view) =
            create_ao_texture(device, width, height, "SSAO Raw");
        let (ssao_blur_texture, ssao_blur_view) =
            create_ao_texture(device, width, height, "SSAO Blur Scratch");
        let (ssao_output_texture, ssao_output_view) =
            create_ao_texture(device, width, height, "SSAO Output");

        let ssao_bind_group = create_ssao_bind_group(
            device,
            &layouts.ssao,
            &gbuffer_depth_view,
            &gbuffer_normal_view,
            &noise_view,
            &sampler,
            camera_buffer,
            &kernel_buffer,
        );
        let blur_h_bind_group = create_blur_bind_group(
            device,
            &layouts.ssao_blur,
            &ssao_raw_view,
            &gbuffer_depth_view,
            &sampler,
            camera_buffer,
            "SSAO Blur H",
        );
        let blur_v_bind_group = create_blur_bind_group(
            device,
            &layouts.ssao_blur,
            &ssao_blur_view,
            &gbuffer_depth_view,
            &sampler,
            camera_buffer,
            "SSAO Blur V",
        );
        let read_bind_group =
            create_read_bind_group(device, &layouts.ssao_read, &ssao_output_view, &sampler);
        let read_off_bind_group =
            create_read_bind_group(device, &layouts.ssao_read, &white_view, &sampler);

        Self {
            gbuffer_normal_texture,
            gbuffer_normal_view,
            gbuffer_depth_texture,
            gbuffer_depth_view,
            ssao_raw_texture,
            ssao_raw_view,
            ssao_blur_texture,
            ssao_blur_view,
            ssao_output_texture,
            ssao_output_view,
            noise_texture,
            noise_view,
            kernel_buffer,
            sampler,
            white_texture,
            white_view,
            ssao_bind_group,
            blur_h_bind_group,
            blur_v_bind_group,
            read_bind_group,
            read_off_bind_group,
        }
    }

    pub(crate) fn resize(
        &mut self,
        device: &wgpu::Device,
        layouts: &BindGroupLayouts,
        camera_buffer: &wgpu::Buffer,
        width: u32,
        height: u32,
    ) {
        let (nt, nv) = create_normal_texture(device, width, height);
        self.gbuffer_normal_texture = nt;
        self.gbuffer_normal_view = nv;

        let (dt, dv) = create_gbuffer_depth_texture(device, width, height);
        self.gbuffer_depth_texture = dt;
        self.gbuffer_depth_view = dv;

        let (rt, rv) = create_ao_texture(device, width, height, "SSAO Raw");
        self.ssao_raw_texture = rt;
        self.ssao_raw_view = rv;

        let (bt, bv) = create_ao_texture(device, width, height, "SSAO Blur Scratch");
        self.ssao_blur_texture = bt;
        self.ssao_blur_view = bv;

        let (ot, ov) = create_ao_texture(device, width, height, "SSAO Output");
        self.ssao_output_texture = ot;
        self.ssao_output_view = ov;

        self.rebuild_bind_groups(device, layouts, camera_buffer);
    }

    pub(crate) fn rebuild_bind_groups(
        &mut self,
        device: &wgpu::Device,
        layouts: &BindGroupLayouts,
        camera_buffer: &wgpu::Buffer,
    ) {
        self.ssao_bind_group = create_ssao_bind_group(
            device,
            &layouts.ssao,
            &self.gbuffer_depth_view,
            &self.gbuffer_normal_view,
            &self.noise_view,
            &self.sampler,
            camera_buffer,
            &self.kernel_buffer,
        );
        self.blur_h_bind_group = create_blur_bind_group(
            device,
            &layouts.ssao_blur,
            &self.ssao_raw_view,
            &self.gbuffer_depth_view,
            &self.sampler,
            camera_buffer,
            "SSAO Blur H",
        );
        self.blur_v_bind_group = create_blur_bind_group(
            device,
            &layouts.ssao_blur,
            &self.ssao_blur_view,
            &self.gbuffer_depth_view,
            &self.sampler,
            camera_buffer,
            "SSAO Blur V",
        );
        self.read_bind_group = create_read_bind_group(
            device,
            &layouts.ssao_read,
            &self.ssao_output_view,
            &self.sampler,
        );
        self.read_off_bind_group =
            create_read_bind_group(device, &layouts.ssao_read, &self.white_view, &self.sampler);
    }
}

fn create_normal_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("G-Buffer Normals"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

fn create_gbuffer_depth_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("G-Buffer Depth"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

fn create_ao_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    label: &str,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

#[allow(clippy::too_many_arguments)]
fn create_ssao_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    depth_view: &wgpu::TextureView,
    normal_view: &wgpu::TextureView,
    noise_view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    camera_buffer: &wgpu::Buffer,
    kernel_buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("SSAO Bind Group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(depth_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(normal_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(noise_view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: camera_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: kernel_buffer.as_entire_binding(),
            },
        ],
    })
}

fn create_blur_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    ao_view: &wgpu::TextureView,
    depth_view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    camera_buffer: &wgpu::Buffer,
    label: &str,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(ao_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(depth_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: camera_buffer.as_entire_binding(),
            },
        ],
    })
}

fn create_read_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    ao_view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("SSAO Read Bind Group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(ao_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

fn generate_kernel() -> [[f32; 4]; KERNEL_SIZE] {
    let mut samples = [[0.0f32; 4]; KERNEL_SIZE];
    for (i, sample) in samples.iter_mut().enumerate() {
        let fi = i as f32;
        let u = halton(i as u32 + 1, 2);
        let v = halton(i as u32 + 1, 3);

        let theta = 2.0 * std::f32::consts::PI * u;
        let cos_phi = (1.0 - v).sqrt();
        let sin_phi = v.sqrt();

        let x = theta.cos() * sin_phi;
        let y = theta.sin() * sin_phi;
        let z = cos_phi;

        let scale = lerp(0.1, 1.0, (fi / KERNEL_SIZE as f32).powi(2));
        *sample = [x * scale, y * scale, z * scale, 0.0];
    }
    samples
}

fn generate_noise() -> Vec<u8> {
    let count = (NOISE_SIZE * NOISE_SIZE) as usize;
    let mut data = Vec::with_capacity(count * 4);
    for i in 0..count as u32 {
        let mut h = i.wrapping_mul(0x9E37_79B9);
        h ^= h >> 16;
        h = h.wrapping_mul(0x45D9_F3B);
        h ^= h >> 16;
        let angle = (h as f32 / u32::MAX as f32) * 2.0 * std::f32::consts::PI;
        let x = angle.cos();
        let y = angle.sin();
        data.push((x * 127.0) as i8 as u8);
        data.push((y * 127.0) as i8 as u8);
        data.push(0u8);
        data.push(0u8);
    }
    data
}

fn halton(mut index: u32, base: u32) -> f32 {
    let mut result = 0.0f32;
    let mut f = 1.0 / base as f32;
    while index > 0 {
        result += f * (index % base) as f32;
        index /= base;
        f /= base as f32;
    }
    result
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
