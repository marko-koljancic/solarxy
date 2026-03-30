#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LightEntry {
    pub position: [f32; 3],
    pub _pad0: f32,
    pub color: [f32; 3],
    pub intensity: f32,
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LightsUniform {
    pub lights: [LightEntry; 3],
    pub sphere_scale: f32,
    pub _pad1: [f32; 3],
}

const _: () = assert!(std::mem::size_of::<LightEntry>() == 32);
const _: () = assert!(std::mem::size_of::<LightsUniform>() == 112);
