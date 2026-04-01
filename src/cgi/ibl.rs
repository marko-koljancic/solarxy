use std::path::Path;

use half::f16;

pub(crate) struct IblState {
    #[allow(dead_code)]
    pub(crate) irradiance_texture: wgpu::Texture,
    pub(crate) irradiance_view: wgpu::TextureView,
    pub(crate) sampler: wgpu::Sampler,
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

impl IblState {
    pub(crate) fn fallback(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Self {
        let mut pixel_bytes = [0u8; 8];
        write_rgba16f(&mut pixel_bytes, 0, 0.2, 0.2, 0.2, 1.0);

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("IBL Fallback Cubemap"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 6,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        for face in 0..6u32 {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: face,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &pixel_bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(8),
                    rows_per_image: Some(1),
                },
                wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
        }

        Self::from_texture(device, texture)
    }

    pub(crate) fn from_sky_colors(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        top: [f32; 3],
        bottom: [f32; 3],
    ) -> Self {
        const SIZE: u32 = 32;
        const SAMPLES: u32 = 256;

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("IBL Sky Irradiance Cubemap"),
            size: wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 6,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let bytes_per_row = SIZE * 8;
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
                        let phi = 2.0 * std::f32::consts::PI * u1;
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

            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: face,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &face_data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(SIZE),
                },
                wgpu::Extent3d {
                    width: SIZE,
                    height: SIZE,
                    depth_or_array_layers: 1,
                },
            );
        }

        Self::from_texture(device, texture)
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
        Ok(Self::from_irradiance_faces(device, queue, &irradiance))
    }

    fn from_texture(device: &wgpu::Device, texture: wgpu::Texture) -> Self {
        let irradiance_view = texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("IBL Irradiance View"),
            dimension: Some(wgpu::TextureViewDimension::Cube),
            ..Default::default()
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("IBL Sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });

        Self {
            irradiance_texture: texture,
            irradiance_view,
            sampler,
        }
    }

    fn from_irradiance_faces(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        faces: &[Vec<[f32; 3]>; 6],
    ) -> Self {
        const SIZE: u32 = 32;
        let bytes_per_row = SIZE * 8;

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("IBL HDRI Irradiance Cubemap"),
            size: wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 6,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        for (face_idx, face) in faces.iter().enumerate() {
            let mut data = vec![0u8; (SIZE * SIZE * 8) as usize];
            for (i, rgb) in face.iter().enumerate() {
                write_rgba16f(&mut data, i * 8, rgb[0], rgb[1], rgb[2], 1.0);
            }

            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: face_idx as u32,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(SIZE),
                },
                wgpu::Extent3d {
                    width: SIZE,
                    height: SIZE,
                    depth_or_array_layers: 1,
                },
            );
        }

        Self::from_texture(device, texture)
    }
}

// --- Math helpers ---

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
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
        0 => [1.0, -v, -u],  // +X
        1 => [-1.0, -v, u],  // -X
        2 => [u, 1.0, v],    // +Y
        3 => [u, -1.0, -v],  // -Y
        4 => [u, -v, 1.0],   // +Z
        _ => [-u, -v, -1.0], // -Z
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
    bits as f32 * 2.328_306_4e-10 // 0x100000000 as f32
}

// --- HDRI loading ---

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
    let u = (phi + std::f32::consts::PI) / (2.0 * std::f32::consts::PI);
    let v = theta / std::f32::consts::PI;
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
                    let phi = 2.0 * std::f32::consts::PI * u1;
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
