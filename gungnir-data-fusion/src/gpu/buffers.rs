//! Persistent GPU buffer management -- created once, resized only on point-count
//! change, never recreated per frame (rust-ui-architecture-coding-standards.md §5).

pub struct PersistentPointBuffer {
    pub buffer: Option<wgpu::Buffer>,
    pub capacity_points: usize,
}
