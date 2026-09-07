// Uniform-grid spatial hash build for the target cloud (§3.4 stage 1).
// Bucket-sort points by cell; grids favored over a GPU BVH/kd-tree since
// LiDAR-class point density is fairly uniform (§3.4 rationale).
