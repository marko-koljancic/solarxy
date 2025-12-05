#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LightUniform {
    pub position: [f32; 3],
    pub pos_padding: u32,
    pub color: [f32; 3],
    pub color_padding: u32,
}
