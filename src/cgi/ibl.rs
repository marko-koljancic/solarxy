use std::path::Path;

use half::f16;

pub(crate) struct IblState {
    #[allow(dead_code)]
    pub(crate) irradiance_texture: wgpu::Texture,
    pub(crate) irradiance_view: wgpu::TextureView,
    pub(crate) sampler: wgpu::Sampler,
    #[allow(dead_code)]
    pub(crate) prefiltered_texture: wgpu::Texture,
    pub(crate) prefiltered_view: wgpu::TextureView,
    pub(crate) prefiltered_sampler: wgpu::Sampler,
    #[allow(dead_code)]
    pub(crate) brdf_lut_texture: wgpu::Texture,
    pub(crate) brdf_lut_view: wgpu::TextureView,
    pub(crate) brdf_lut_sampler: wgpu::Sampler,
}

fn write_rgba16f(buf: &mut [u8], offset: usize, r: f32, g: f32, b: f32, a: f32) {
    let bytes = [
        f16::from_f32(r).to_ne_bytes(),
        f16::from_f32(g).to_ne_bytes(),
        f16::from_f32(b).to_ne_bytes(),
        f16::from_f32(a).to_ne_bytes(),
    ];
    buf[offset] = bytes[0][0];
    buf[offset + 1] = bytes[0][1];
    buf[offset + 2] = bytes[1][0];
    buf[offset + 3] = bytes[1][1];
    buf[offset + 4] = bytes[2][0];
    buf[offset + 5] = bytes[2][1];
    buf[offset + 6] = bytes[3][0];
    buf[offset + 7] = bytes[3][1];
}

fn write_rg16f(buf: &mut [u8], offset: usize, r: f32, g: f32) {
    let rb = f16::from_f32(r).to_ne_bytes();
    let gb = f16::from_f32(g).to_ne_bytes();
    buf[offset] = rb[0];
    buf[offset + 1] = rb[1];
    buf[offset + 2] = gb[0];
    buf[offset + 3] = gb[1];
}

const PI: f32 = std::f32::consts::PI;
const PREFILTERED_SIZE: u32 = 128;
const PREFILTERED_MIP_COUNT: u32 = 6;
const BRDF_LUT_SIZE: u32 = 512;
const BRDF_LUT_SAMPLES: u32 = 1024;

impl IblState {
    pub(crate) fn fallback(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let mut pixel_bytes = [0u8; 8];
        write_rgba16f(&mut pixel_bytes, 0, 0.2, 0.2, 0.2, 1.0);

        let irradiance_texture = create_cubemap(device, "IBL Fallback Irradiance", 1, 1);
        for face in 0..6u32 {
            write_cubemap_face(queue, &irradiance_texture, face, 0, 1, &pixel_bytes);
        }

        let mut black_pixel = [0u8; 8];
        write_rgba16f(&mut black_pixel, 0, 0.0, 0.0, 0.0, 1.0);

        let prefiltered_texture = create_cubemap(device, "IBL Fallback Prefiltered", 1, 1);
        for face in 0..6u32 {
            write_cubemap_face(queue, &prefiltered_texture, face, 0, 1, &black_pixel);
        }

        let brdf_lut_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("IBL Fallback BRDF LUT"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rg16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let mut lut_pixel = [0u8; 4];
        write_rg16f(&mut lut_pixel, 0, 1.0, 0.0);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &brdf_lut_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &lut_pixel,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );

        Self::from_parts(
            device,
            irradiance_texture,
            prefiltered_texture,
            brdf_lut_texture,
        )
    }

    pub(crate) fn from_sky_colors(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        top: [f32; 3],
        bottom: [f32; 3],
    ) -> Self {
        let irradiance_texture = generate_irradiance_sky(device, queue, top, bottom);
        let prefiltered_texture = generate_prefiltered_sky(device, queue, top, bottom);
        let brdf_lut_texture = generate_brdf_lut(device, queue);

        Self::from_parts(
            device,
            irradiance_texture,
            prefiltered_texture,
            brdf_lut_texture,
        )
    }

    pub(crate) fn from_hdri(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        path: &Path,
    ) -> anyhow::Result<Self> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        let (width, height, pixels) = match ext.as_str() {
            "hdr" => load_hdr(path)?,
            "exr" => load_exr(path)?,
            _ => anyhow::bail!("Unsupported IBL format: .{}", ext),
        };

        let irradiance = convolve_equirect(width, height, &pixels);
        let irradiance_texture = irradiance_faces_to_texture(device, queue, &irradiance);
        let prefiltered_texture =
            generate_prefiltered_equirect(device, queue, width, height, &pixels);
        let brdf_lut_texture = generate_brdf_lut(device, queue);

        Ok(Self::from_parts(
            device,
            irradiance_texture,
            prefiltered_texture,
            brdf_lut_texture,
        ))
    }

    fn from_parts(
        device: &wgpu::Device,
        irradiance_texture: wgpu::Texture,
        prefiltered_texture: wgpu::Texture,
        brdf_lut_texture: wgpu::Texture,
    ) -> Self {
        let irradiance_view = irradiance_texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("IBL Irradiance View"),
            dimension: Some(wgpu::TextureViewDimension::Cube),
            ..Default::default()
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("IBL Irradiance Sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });

        let prefiltered_view = prefiltered_texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("IBL Prefiltered View"),
            dimension: Some(wgpu::TextureViewDimension::Cube),
            ..Default::default()
        });

        let prefiltered_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("IBL Prefiltered Sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });

        let brdf_lut_view = brdf_lut_texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("IBL BRDF LUT View"),
            ..Default::default()
        });

        let brdf_lut_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("IBL BRDF LUT Sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });

        Self {
            irradiance_texture,
            irradiance_view,
            sampler,
            prefiltered_texture,
            prefiltered_view,
            prefiltered_sampler,
            brdf_lut_texture,
            brdf_lut_view,
            brdf_lut_sampler,
        }
    }
}

fn create_cubemap(device: &wgpu::Device, label: &str, size: u32, mip_levels: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 6,
        },
        mip_level_count: mip_levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn write_cubemap_face(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    face: u32,
    mip_level: u32,
    size: u32,
    data: &[u8],
) {
    let bytes_per_row = size * 8;
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level,
            origin: wgpu::Origin3d {
                x: 0,
                y: 0,
                z: face,
            },
            aspect: wgpu::TextureAspect::All,
        },
        data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(bytes_per_row),
            rows_per_image: Some(size),
        },
        wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
    );
}

fn generate_irradiance_sky(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    top: [f32; 3],
    bottom: [f32; 3],
) -> wgpu::Texture {
    const SIZE: u32 = 32;
    const SAMPLES: u32 = 256;

    let texture = create_cubemap(device, "IBL Sky Irradiance Cubemap", SIZE, 1);
    let face_bytes = (SIZE * SIZE * 8) as usize;
    let mut face_data = vec![0u8; face_bytes];

    for face in 0..6u32 {
        for y in 0..SIZE {
            for x in 0..SIZE {
                let u = (x as f32 + 0.5) / SIZE as f32 * 2.0 - 1.0;
                let v = (y as f32 + 0.5) / SIZE as f32 * 2.0 - 1.0;
                let dir = normalize(face_direction(face, u, v));

                let (t, b, n) = build_tbn(dir);
                let mut acc = [0.0_f32; 3];

                for i in 0..SAMPLES {
                    let (u1, u2) = hammersley(i, SAMPLES);
                    let phi = 2.0 * PI * u1;
                    let cos_theta = u2.sqrt();
                    let sin_theta = (1.0 - u2).sqrt();

                    let local = [sin_theta * phi.cos(), sin_theta * phi.sin(), cos_theta];
                    let world = [
                        t[0] * local[0] + b[0] * local[1] + n[0] * local[2],
                        t[1] * local[0] + b[1] * local[1] + n[1] * local[2],
                        t[2] * local[0] + b[2] * local[1] + n[2] * local[2],
                    ];

                    let blend = world[1] * 0.5 + 0.5;
                    acc[0] += lerp(bottom[0], top[0], blend);
                    acc[1] += lerp(bottom[1], top[1], blend);
                    acc[2] += lerp(bottom[2], top[2], blend);
                }

                let inv = 1.0 / SAMPLES as f32;
                let offset = ((y * SIZE + x) * 8) as usize;
                write_rgba16f(
                    &mut face_data,
                    offset,
                    acc[0] * inv,
                    acc[1] * inv,
                    acc[2] * inv,
                    1.0,
                );
            }
        }

        write_cubemap_face(queue, &texture, face, 0, SIZE, &face_data);
    }

    texture
}

fn irradiance_faces_to_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    faces: &[Vec<[f32; 3]>; 6],
) -> wgpu::Texture {
    const SIZE: u32 = 32;
    let texture = create_cubemap(device, "IBL HDRI Irradiance Cubemap", SIZE, 1);

    for (face_idx, face) in faces.iter().enumerate() {
        let mut data = vec![0u8; (SIZE * SIZE * 8) as usize];
        for (i, rgb) in face.iter().enumerate() {
            write_rgba16f(&mut data, i * 8, rgb[0], rgb[1], rgb[2], 1.0);
        }
        write_cubemap_face(queue, &texture, face_idx as u32, 0, SIZE, &data);
    }

    texture
}

fn generate_prefiltered_sky(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    top: [f32; 3],
    bottom: [f32; 3],
) -> wgpu::Texture {
    let texture = create_cubemap(
        device,
        "IBL Prefiltered Sky",
        PREFILTERED_SIZE,
        PREFILTERED_MIP_COUNT,
    );

    for mip in 0..PREFILTERED_MIP_COUNT {
        let roughness = mip as f32 / (PREFILTERED_MIP_COUNT - 1) as f32;
        let face_size = (PREFILTERED_SIZE >> mip).max(2);
        let sample_count = (512u32 >> mip).max(16);
        let face_bytes = (face_size * face_size * 8) as usize;
        let mut face_data = vec![0u8; face_bytes];

        for face in 0..6u32 {
            for y in 0..face_size {
                for x in 0..face_size {
                    let u = (x as f32 + 0.5) / face_size as f32 * 2.0 - 1.0;
                    let v = (y as f32 + 0.5) / face_size as f32 * 2.0 - 1.0;
                    let n = normalize(face_direction(face, u, v));
                    let (t, b, nn) = build_tbn(n);

                    let mut acc = [0.0_f32; 3];
                    let mut total_weight = 0.0_f32;

                    for i in 0..sample_count {
                        let xi = hammersley(i, sample_count);
                        let h_local = importance_sample_ggx(xi, roughness);
                        let h = [
                            t[0] * h_local[0] + b[0] * h_local[1] + nn[0] * h_local[2],
                            t[1] * h_local[0] + b[1] * h_local[1] + nn[1] * h_local[2],
                            t[2] * h_local[0] + b[2] * h_local[1] + nn[2] * h_local[2],
                        ];
                        let n_dot_h = dot(n, h).max(0.0);

                        let l = [
                            2.0 * n_dot_h * h[0] - n[0],
                            2.0 * n_dot_h * h[1] - n[1],
                            2.0 * n_dot_h * h[2] - n[2],
                        ];
                        let n_dot_l = dot(n, l);
                        if n_dot_l > 0.0 {
                            let blend = l[1] * 0.5 + 0.5;
                            acc[0] += lerp(bottom[0], top[0], blend) * n_dot_l;
                            acc[1] += lerp(bottom[1], top[1], blend) * n_dot_l;
                            acc[2] += lerp(bottom[2], top[2], blend) * n_dot_l;
                            total_weight += n_dot_l;
                        }
                    }

                    if total_weight > 0.0 {
                        let inv = 1.0 / total_weight;
                        acc[0] *= inv;
                        acc[1] *= inv;
                        acc[2] *= inv;
                    }

                    let offset = ((y * face_size + x) * 8) as usize;
                    write_rgba16f(&mut face_data, offset, acc[0], acc[1], acc[2], 1.0);
                }
            }

            write_cubemap_face(queue, &texture, face, mip, face_size, &face_data);
        }
    }

    texture
}

fn generate_prefiltered_equirect(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    width: u32,
    height: u32,
    pixels: &[[f32; 3]],
) -> wgpu::Texture {
    let texture = create_cubemap(
        device,
        "IBL Prefiltered HDRI",
        PREFILTERED_SIZE,
        PREFILTERED_MIP_COUNT,
    );

    for mip in 0..PREFILTERED_MIP_COUNT {
        let roughness = mip as f32 / (PREFILTERED_MIP_COUNT - 1) as f32;
        let face_size = (PREFILTERED_SIZE >> mip).max(2);
        let sample_count = (512u32 >> mip).max(16);
        let face_bytes = (face_size * face_size * 8) as usize;
        let mut face_data = vec![0u8; face_bytes];

        for face in 0..6u32 {
            for y in 0..face_size {
                for x in 0..face_size {
                    let u = (x as f32 + 0.5) / face_size as f32 * 2.0 - 1.0;
                    let v = (y as f32 + 0.5) / face_size as f32 * 2.0 - 1.0;
                    let n = normalize(face_direction(face, u, v));
                    let (t, b, nn) = build_tbn(n);

                    let mut acc = [0.0_f32; 3];
                    let mut total_weight = 0.0_f32;

                    for i in 0..sample_count {
                        let xi = hammersley(i, sample_count);
                        let h_local = importance_sample_ggx(xi, roughness);
                        let h = [
                            t[0] * h_local[0] + b[0] * h_local[1] + nn[0] * h_local[2],
                            t[1] * h_local[0] + b[1] * h_local[1] + nn[1] * h_local[2],
                            t[2] * h_local[0] + b[2] * h_local[1] + nn[2] * h_local[2],
                        ];
                        let n_dot_h = dot(n, h).max(0.0);
                        let l = [
                            2.0 * n_dot_h * h[0] - n[0],
                            2.0 * n_dot_h * h[1] - n[1],
                            2.0 * n_dot_h * h[2] - n[2],
                        ];
                        let n_dot_l = dot(n, l);
                        if n_dot_l > 0.0 {
                            let sample = sample_equirect(width, height, pixels, l);
                            acc[0] += sample[0] * n_dot_l;
                            acc[1] += sample[1] * n_dot_l;
                            acc[2] += sample[2] * n_dot_l;
                            total_weight += n_dot_l;
                        }
                    }

                    if total_weight > 0.0 {
                        let inv = 1.0 / total_weight;
                        acc[0] *= inv;
                        acc[1] *= inv;
                        acc[2] *= inv;
                    }

                    let offset = ((y * face_size + x) * 8) as usize;
                    write_rgba16f(&mut face_data, offset, acc[0], acc[1], acc[2], 1.0);
                }
            }

            write_cubemap_face(queue, &texture, face, mip, face_size, &face_data);
        }
    }

    texture
}

fn generate_brdf_lut(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::Texture {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("IBL BRDF LUT"),
        size: wgpu::Extent3d {
            width: BRDF_LUT_SIZE,
            height: BRDF_LUT_SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rg16Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    let bytes_per_row = BRDF_LUT_SIZE * 4;
    let total_bytes = (BRDF_LUT_SIZE * BRDF_LUT_SIZE * 4) as usize;
    let mut data = vec![0u8; total_bytes];

    for y in 0..BRDF_LUT_SIZE {
        for x in 0..BRDF_LUT_SIZE {
            let n_dot_v = (x as f32 + 0.5) / BRDF_LUT_SIZE as f32;
            let roughness = (y as f32 + 0.5) / BRDF_LUT_SIZE as f32;

            let (scale, bias) = integrate_brdf(n_dot_v.max(0.001), roughness.max(0.001));

            let offset = ((y * BRDF_LUT_SIZE + x) * 4) as usize;
            write_rg16f(&mut data, offset, scale, bias);
        }
    }

    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(bytes_per_row),
            rows_per_image: Some(BRDF_LUT_SIZE),
        },
        wgpu::Extent3d {
            width: BRDF_LUT_SIZE,
            height: BRDF_LUT_SIZE,
            depth_or_array_layers: 1,
        },
    );

    texture
}

fn integrate_brdf(n_dot_v: f32, roughness: f32) -> (f32, f32) {
    let sin_v = (1.0 - n_dot_v * n_dot_v).max(0.0).sqrt();
    let v = [sin_v, 0.0, n_dot_v];

    let mut scale = 0.0_f32;
    let mut bias = 0.0_f32;

    for i in 0..BRDF_LUT_SAMPLES {
        let xi = hammersley(i, BRDF_LUT_SAMPLES);
        let h = importance_sample_ggx(xi, roughness);

        let v_dot_h = (v[0] * h[0] + v[1] * h[1] + v[2] * h[2]).max(0.0);
        let l = [
            2.0 * v_dot_h * h[0] - v[0],
            2.0 * v_dot_h * h[1] - v[1],
            2.0 * v_dot_h * h[2] - v[2],
        ];

        let n_dot_l = l[2].max(0.0);
        let n_dot_h = h[2].max(0.0);

        if n_dot_l > 0.0 {
            let g = g_smith_ibl(n_dot_v, n_dot_l, roughness);
            let g_vis = (g * v_dot_h) / (n_dot_h * n_dot_v).max(0.001);
            let fc = (1.0 - v_dot_h).max(0.0).powi(5);

            scale += g_vis * (1.0 - fc);
            bias += g_vis * fc;
        }
    }

    let inv = 1.0 / BRDF_LUT_SAMPLES as f32;
    (scale * inv, bias * inv)
}

fn importance_sample_ggx(xi: (f32, f32), roughness: f32) -> [f32; 3] {
    let a = roughness * roughness;
    let phi = 2.0 * PI * xi.0;
    let cos_theta = ((1.0 - xi.1) / (1.0 + (a * a - 1.0) * xi.1)).sqrt();
    let sin_theta = (1.0 - cos_theta * cos_theta).max(0.0).sqrt();
    [sin_theta * phi.cos(), sin_theta * phi.sin(), cos_theta]
}

fn g_schlick_ibl(n_dot_v: f32, roughness: f32) -> f32 {
    let k = (roughness * roughness) / 2.0;
    n_dot_v / (n_dot_v * (1.0 - k) + k)
}

fn g_smith_ibl(n_dot_v: f32, n_dot_l: f32, roughness: f32) -> f32 {
    g_schlick_ibl(n_dot_v, roughness) * g_schlick_ibl(n_dot_l, roughness)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len < 1e-10 {
        return [0.0, 1.0, 0.0];
    }
    [v[0] / len, v[1] / len, v[2] / len]
}

fn face_direction(face: u32, u: f32, v: f32) -> [f32; 3] {
    match face {
        0 => [1.0, -v, -u],
        1 => [-1.0, -v, u],
        2 => [u, 1.0, v],
        3 => [u, -1.0, -v],
        4 => [u, -v, 1.0],
        _ => [-u, -v, -1.0],
    }
}

fn build_tbn(n: [f32; 3]) -> ([f32; 3], [f32; 3], [f32; 3]) {
    let up = if n[1].abs() < 0.999 {
        [0.0, 1.0, 0.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let t = normalize(cross(up, n));
    let b = cross(n, t);
    (t, b, n)
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn hammersley(i: u32, n: u32) -> (f32, f32) {
    (i as f32 / n as f32, radical_inverse_vdc(i))
}

fn radical_inverse_vdc(mut bits: u32) -> f32 {
    bits = bits.rotate_right(16);
    bits = ((bits & 0x55555555) << 1) | ((bits & 0xAAAAAAAA) >> 1);
    bits = ((bits & 0x33333333) << 2) | ((bits & 0xCCCCCCCC) >> 2);
    bits = ((bits & 0x0F0F0F0F) << 4) | ((bits & 0xF0F0F0F0) >> 4);
    bits = ((bits & 0x00FF00FF) << 8) | ((bits & 0xFF00FF00) >> 8);
    bits as f32 * 2.328_306_4e-10
}

fn load_hdr(path: &Path) -> anyhow::Result<(u32, u32, Vec<[f32; 3]>)> {
    let img = image::ImageReader::open(path)?.decode()?;
    let rgb32f = img.to_rgb32f();
    let w = rgb32f.width();
    let h = rgb32f.height();
    let pixels: Vec<[f32; 3]> = rgb32f.pixels().map(|p| [p[0], p[1], p[2]]).collect();
    Ok((w, h, pixels))
}

fn load_exr(path: &Path) -> anyhow::Result<(u32, u32, Vec<[f32; 3]>)> {
    let image = exr::prelude::read_first_rgba_layer_from_file(
        path,
        |resolution, _| vec![[0.0_f32; 4]; resolution.width() * resolution.height()],
        |pixels, pos, (r, g, b, _a): (f32, f32, f32, f32)| {
            pixels[pos.y() * pos.width() + pos.x()] = [r, g, b, 1.0];
        },
    )?;

    let w = image.layer_data.size.width() as u32;
    let h = image.layer_data.size.height() as u32;
    let pixels: Vec<[f32; 3]> = image
        .layer_data
        .channel_data
        .pixels
        .into_iter()
        .map(|p| [p[0], p[1], p[2]])
        .collect();
    Ok((w, h, pixels))
}

fn sample_equirect(width: u32, height: u32, pixels: &[[f32; 3]], dir: [f32; 3]) -> [f32; 3] {
    let theta = dir[1].clamp(-1.0, 1.0).acos();
    let phi = dir[2].atan2(dir[0]);
    let u = (phi + PI) / (2.0 * PI);
    let v = theta / PI;
    let px = ((u * width as f32) as u32).min(width - 1);
    let py = ((v * height as f32) as u32).min(height - 1);
    pixels[(py * width + px) as usize]
}

fn convolve_equirect(width: u32, height: u32, pixels: &[[f32; 3]]) -> [Vec<[f32; 3]>; 6] {
    const SIZE: u32 = 32;
    const SAMPLES: u32 = 256;

    std::array::from_fn(|face| {
        let mut face_data = Vec::with_capacity((SIZE * SIZE) as usize);
        for y in 0..SIZE {
            for x in 0..SIZE {
                let u = (x as f32 + 0.5) / SIZE as f32 * 2.0 - 1.0;
                let v = (y as f32 + 0.5) / SIZE as f32 * 2.0 - 1.0;
                let dir = normalize(face_direction(face as u32, u, v));

                let (t, b, n) = build_tbn(dir);
                let mut acc = [0.0_f32; 3];

                for i in 0..SAMPLES {
                    let (u1, u2) = hammersley(i, SAMPLES);
                    let phi = 2.0 * PI * u1;
                    let cos_theta = u2.sqrt();
                    let sin_theta = (1.0 - u2).sqrt();

                    let local = [sin_theta * phi.cos(), sin_theta * phi.sin(), cos_theta];
                    let world = normalize([
                        t[0] * local[0] + b[0] * local[1] + n[0] * local[2],
                        t[1] * local[0] + b[1] * local[1] + n[1] * local[2],
                        t[2] * local[0] + b[2] * local[1] + n[2] * local[2],
                    ]);

                    let sample = sample_equirect(width, height, pixels, world);
                    acc[0] += sample[0];
                    acc[1] += sample[1];
                    acc[2] += sample[2];
                }

                let inv = 1.0 / SAMPLES as f32;
                face_data.push([acc[0] * inv, acc[1] * inv, acc[2] * inv]);
            }
        }
        face_data
    })
}
