//! LRU GPU-resource cache keyed by `TileId`, per §2.1: GPU buffers created once per
//! tile and reused, never recreated per frame.
pub struct TileCache;
impl TileCache {
    pub fn resident_objects(&self) -> impl Iterator<Item = &dyn three_d::Object> {
        std::iter::empty()
    }
}
