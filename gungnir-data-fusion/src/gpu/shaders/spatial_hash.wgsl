// Copyright (C) 2026 Roessling Digital Solutions LLC
// SPDX-License-Identifier: AGPL-3.0-or-later
// Additional terms under AGPL section 7 apply: see LICENSE-ADDITIONAL-TERMS.md

// Uniform-grid spatial hash build for the target cloud (§3.4 stage 1).
// Bucket-sort points by cell; grids favored over a GPU BVH/kd-tree since
// LiDAR-class point density is fairly uniform (§3.4 rationale).
