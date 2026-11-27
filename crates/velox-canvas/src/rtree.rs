//! 静态扁平空间索引（`FlatRTree`）：Hilbert 空间填充曲线排序打包 + 块级视锥裁剪 + OBB/多边形精确拾取 + 框选查询。
//!
//! 所有热路径查询（`cull_chunks` / `hit_test` / `query_marquee`）均零动态堆分配：
//! 查询结果直接写入 `ScratchArena` 或定长 `SmallVec`。

use velox_engine::arena::{ScratchArena, SmallVec};
use velox_engine::canvas_scene::{CanvasPrimitiveKind, CanvasStaticGeom};

/// 每个空间叶子块包含的连续图元数（兼顾 GPU Draw Call 合并率与视锥剔除粒度）。
pub const CHUNK_LEAF_SIZE: usize = 64;

/// 连续图元分块元数据（对应 GPU 静态缓冲中的一个连续区间 `start..end`）。
#[derive(Copy, Clone, Debug, Default)]
pub struct ChunkRange {
    pub aabb: [f32; 4],
    pub start: u32,
    pub end: u32,
}

/// 扁平 R-Tree 内部/叶子节点（按缓存友好数组连续存放）。
#[derive(Copy, Clone, Debug, Default)]
pub struct RTreeNode {
    pub aabb: [f32; 4],
    /// `child_count == 0` 表示叶子节点，指向 `chunks[first_child]`；否则指向子节点 `nodes[first_child..first_child+child_count]`。
    pub first_child: u32,
    pub child_count: u16,
    pub _pad: u16,
}

/// 闭合多边形拾取区域（用于异形看台/船舱热区点击测试）。
#[derive(Clone, Debug)]
pub struct PolygonHitArea {
    pub id: u32,
    pub aabb: [f32; 4],
    pub points: Vec<[f32; 2]>,
}

impl PolygonHitArea {
    pub fn new(id: u32, points: Vec<[f32; 2]>) -> Self {
        let mut aabb = [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];
        for &[x, y] in &points {
            aabb[0] = aabb[0].min(x);
            aabb[1] = aabb[1].min(y);
            aabb[2] = aabb[2].max(x);
            aabb[3] = aabb[3].max(y);
        }
        if !aabb[0].is_finite() {
            aabb = [0.0; 4];
        }
        Self { id, aabb, points }
    }
}

/// 2D Hilbert 空间填充曲线编码（16-bit 网格），保证空间邻近图元在显存和 CPU 缓存中连续排列。
#[inline]
pub fn hilbert_index_16(mut x: u32, mut y: u32) -> u32 {
    x &= 0xFFFF;
    y &= 0xFFFF;
    let mut d = 0u32;
    let mut s = 1u32 << 15;
    while s > 0 {
        let rx = u32::from((x & s) != 0);
        let ry = u32::from((y & s) != 0);
        d += s * s * ((3 * rx) ^ ry);
        if ry == 0 {
            if rx == 1 {
                x = 0xFFFF - x;
                y = 0xFFFF - y;
            }
            core::mem::swap(&mut x, &mut y);
        }
        s >>= 1;
    }
    d
}

#[inline(always)]
pub fn aabb_intersects(a: [f32; 4], b: [f32; 4]) -> bool {
    a[0] <= b[2] && a[2] >= b[0] && a[1] <= b[3] && a[3] >= b[1]
}

#[inline(always)]
pub fn aabb_contains_pt(a: [f32; 4], x: f32, y: f32) -> bool {
    x >= a[0] && x <= a[2] && y >= a[1] && y <= a[3]
}

#[inline(always)]
pub fn aabb_union(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]
}

/// 精确 OBB / 圆形 / 胶囊体逆旋转点选判定。
#[inline]
pub fn geom_contains_point(g: &CanvasStaticGeom, wx: f32, wy: f32) -> bool {
    let dx = wx - g.cx;
    let dy = wy - g.cy;
    let lx = dx * g.cos + dy * g.sin;
    let ly = -dx * g.sin + dy * g.cos;
    if g.kind_flags == CanvasPrimitiveKind::Circle as u32 {
        let r = g.w.min(g.h) * 0.5;
        lx * lx + ly * ly <= r * r
    } else if g.kind_flags == CanvasPrimitiveKind::Capsule as u32 {
        let half_seg = g.w * 0.5;
        let r = g.h * 0.5;
        let qx = (lx.abs() - half_seg).max(0.0);
        qx * qx + ly * ly <= r * r
    } else {
        lx.abs() <= g.w * 0.5 && ly.abs() <= g.h * 0.5
    }
}

/// 奇偶射线法多边形精确点选判定（适用于任意凹/凸异形看台热区）。
pub fn point_in_polygon(pts: &[[f32; 2]], x: f32, y: f32) -> bool {
    let n = pts.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let xi = pts[i][0];
        let yi = pts[i][1];
        let xj = pts[j][0];
        let yj = pts[j][1];
        let intersect =
            ((yi > y) != (yj > y)) && (x < (xj - xi) * (y - yi) / ((yj - yi) + 1e-12) + xi);
        if intersect {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// 静态扁平 R-Tree 空间索引（按 Hilbert 曲线打包，支持零堆视锥裁剪、微秒级拾取与批量框选）。
#[derive(Clone, Debug, Default)]
pub struct FlatRTree {
    pub world_bounds: [f32; 4],
    pub chunks: Vec<ChunkRange>,
    pub nodes: Vec<RTreeNode>,
    pub root_idx: Option<usize>,
    pub polygons: Vec<PolygonHitArea>,
}

impl FlatRTree {
    /// 计算一组图元的 Hilbert 重排顺序（返回排序后的原索引表），并构建扁平 R-Tree 与分块表。
    ///
    /// 调用方按返回的 `order` 重排 `geoms` / `states` / `labels` 后，即可实现 GPU 连续分块 `draw(0..6, chunk.start..chunk.end)`。
    pub fn build_with_hilbert_order(geoms: &[CanvasStaticGeom]) -> (Self, Vec<usize>) {
        let n = geoms.len();
        if n == 0 {
            return (Self::default(), Vec::new());
        }

        let mut wb = [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];
        for g in geoms {
            let b = g.world_aabb();
            wb = aabb_union(wb, b);
        }
        let span_x = (wb[2] - wb[0]).max(1.0);
        let span_y = (wb[3] - wb[1]).max(1.0);

        let mut keyed: Vec<(u32, usize)> = Vec::with_capacity(n);
        for (i, g) in geoms.iter().enumerate() {
            let nx = (((g.cx - wb[0]) / span_x).clamp(0.0, 1.0) * 65535.0) as u32;
            let ny = (((g.cy - wb[1]) / span_y).clamp(0.0, 1.0) * 65535.0) as u32;
            keyed.push((hilbert_index_16(nx, ny), i));
        }
        keyed.sort_unstable_by_key(|&(k, _)| k);
        let order: Vec<usize> = keyed.into_iter().map(|(_, idx)| idx).collect();

        // 生成分块（每块 64 个空间连续图元）
        let chunk_count = n.div_ceil(CHUNK_LEAF_SIZE);
        let mut chunks = Vec::with_capacity(chunk_count);
        for c in 0..chunk_count {
            let start = c * CHUNK_LEAF_SIZE;
            let end = (start + CHUNK_LEAF_SIZE).min(n);
            let mut caabb = [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];
            for &orig_idx in &order[start..end] {
                caabb = aabb_union(caabb, geoms[orig_idx].world_aabb());
            }
            chunks.push(ChunkRange {
                aabb: caabb,
                start: start as u32,
                end: end as u32,
            });
        }

        // 自底向上按扇出 8 打包扁平 RTree 节点
        let mut nodes: Vec<RTreeNode> = Vec::with_capacity(chunk_count * 2);
        let mut level_start = 0usize;
        for (ci, ch) in chunks.iter().enumerate() {
            nodes.push(RTreeNode {
                aabb: ch.aabb,
                first_child: ci as u32,
                child_count: 0,
                _pad: 0,
            });
        }
        let mut level_end = nodes.len();
        const FANOUT: usize = 8;
        while level_end - level_start > 1 {
            let next_start = nodes.len();
            let mut i = level_start;
            while i < level_end {
                let end = (i + FANOUT).min(level_end);
                let mut parent_aabb =
                    [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];
                for child in &nodes[i..end] {
                    parent_aabb = aabb_union(parent_aabb, child.aabb);
                }
                nodes.push(RTreeNode {
                    aabb: parent_aabb,
                    first_child: i as u32,
                    child_count: (end - i) as u16,
                    _pad: 0,
                });
                i = end;
            }
            level_start = next_start;
            level_end = nodes.len();
        }
        let root_idx = if nodes.is_empty() {
            None
        } else {
            Some(nodes.len() - 1)
        };

        (
            Self {
                world_bounds: wb,
                chunks,
                nodes,
                root_idx,
                polygons: Vec::new(),
            },
            order,
        )
    }

    /// 零堆分配视锥块级裁剪：将所有与 `visible_world` 相交的连续区间合并后写入 `ScratchArena`。
    pub fn cull_chunks<'a>(
        &self,
        visible_world: [f32; 4],
        arena: &'a ScratchArena,
    ) -> &'a [(u32, u32)] {
        let Some(root) = self.root_idx else {
            return &[];
        };
        let mut stack: SmallVec<usize, 64> = SmallVec::new();
        let mut ranges: SmallVec<(u32, u32), 256> = SmallVec::new();
        let _ = stack.push(root);

        while let Some(ni) = stack.pop() {
            let node = &self.nodes[ni];
            if !aabb_intersects(node.aabb, visible_world) {
                continue;
            }
            if node.child_count == 0 {
                let ch = &self.chunks[node.first_child as usize];
                if let Some(last) = ranges.last_mut() {
                    if last.1 == ch.start {
                        last.1 = ch.end;
                        continue;
                    }
                }
                if !ranges.push((ch.start, ch.end)) {
                    // 超出 256 段上限时回退为单段完整外包区间，保证绝不丢图元
                    let first = ranges.first().map(|r| r.0).unwrap_or(0);
                    let total_end = self.chunks.last().map(|c| c.end).unwrap_or(0);
                    return arena.alloc_iter([(first, total_end)]);
                }
            } else {
                // 逆序压栈以保持弹出顺序与 chunk 顺序一致，最大化相邻区间合并
                let base = node.first_child as usize;
                let cnt = node.child_count as usize;
                for k in (0..cnt).rev() {
                    let _ = stack.push(base + k);
                }
            }
        }
        arena.alloc_iter(ranges.as_slice().iter().copied())
    }

    /// 微秒级精确点选拾取：自动处理 OBB 逆旋转、圆形、胶囊线段，返回重排后的 `item_idx`。
    pub fn hit_test(&self, geoms: &[CanvasStaticGeom], wx: f32, wy: f32) -> Option<u32> {
        let root = self.root_idx?;
        let mut stack: SmallVec<usize, 64> = SmallVec::new();
        let _ = stack.push(root);

        while let Some(ni) = stack.pop() {
            let node = &self.nodes[ni];
            if !aabb_contains_pt(node.aabb, wx, wy) {
                continue;
            }
            if node.child_count == 0 {
                let ch = &self.chunks[node.first_child as usize];
                for idx in (ch.start as usize)..(ch.end as usize).min(geoms.len()) {
                    if geom_contains_point(&geoms[idx], wx, wy) {
                        return Some(idx as u32);
                    }
                }
            } else {
                let base = node.first_child as usize;
                let cnt = node.child_count as usize;
                for k in (0..cnt).rev() {
                    let _ = stack.push(base + k);
                }
            }
        }
        None
    }

    /// 异形多边形看台/船舱热区点选拾取（射线法）。
    pub fn hit_test_polygon(&self, wx: f32, wy: f32) -> Option<u32> {
        for poly in self.polygons.iter().rev() {
            if aabb_contains_pt(poly.aabb, wx, wy) && point_in_polygon(&poly.points, wx, wy) {
                return Some(poly.id);
            }
        }
        None
    }

    /// 零堆批量框选查询（船舶配载 / 票务后台框选）：将中心点或包围盒落入 `world_rect` 的图元索引写入 `out`。
    pub fn query_marquee<const N: usize>(
        &self,
        geoms: &[CanvasStaticGeom],
        world_rect: [f32; 4],
        out: &mut SmallVec<u32, N>,
    ) {
        out.clear();
        let Some(root) = self.root_idx else {
            return;
        };
        let norm = [
            world_rect[0].min(world_rect[2]),
            world_rect[1].min(world_rect[3]),
            world_rect[0].max(world_rect[2]),
            world_rect[1].max(world_rect[3]),
        ];
        let mut stack: SmallVec<usize, 64> = SmallVec::new();
        let _ = stack.push(root);

        while let Some(ni) = stack.pop() {
            let node = &self.nodes[ni];
            if !aabb_intersects(node.aabb, norm) {
                continue;
            }
            if node.child_count == 0 {
                let ch = &self.chunks[node.first_child as usize];
                for idx in (ch.start as usize)..(ch.end as usize).min(geoms.len()) {
                    let g = &geoms[idx];
                    if aabb_intersects(g.world_aabb(), norm) {
                        if !out.push(idx as u32) {
                            return;
                        }
                    }
                }
            } else {
                let base = node.first_child as usize;
                let cnt = node.child_count as usize;
                for k in (0..cnt).rev() {
                    let _ = stack.push(base + k);
                }
            }
        }
    }

    /// 键盘方向键焦点导航：在指定方向 `(dir_x, dir_y)` 上寻找距离 `from_idx` 最近的有效图元（支撑 `:focus-visible` 键盘选座）。
    pub fn find_nearest_in_dir(
        &self,
        geoms: &[CanvasStaticGeom],
        from_idx: u32,
        dir_x: f32,
        dir_y: f32,
    ) -> Option<u32> {
        let origin = geoms.get(from_idx as usize)?;
        let search_r = (origin.w.max(origin.h) * 6.0).max(80.0);
        let search_box = [
            origin.cx - search_r,
            origin.cy - search_r,
            origin.cx + search_r,
            origin.cy + search_r,
        ];
        let mut candidates: SmallVec<u32, 128> = SmallVec::new();
        self.query_marquee(geoms, search_box, &mut candidates);

        let mut best: Option<(u32, f32)> = None;
        for &idx in candidates.as_slice() {
            if idx == from_idx {
                continue;
            }
            let g = &geoms[idx as usize];
            let dx = g.cx - origin.cx;
            let dy = g.cy - origin.cy;
            let proj = dx * dir_x + dy * dir_y;
            if proj <= 1.0 {
                continue;
            }
            let perp = (dx * dir_y - dy * dir_x).abs();
            // 优先同一排/同列，其次最短欧氏距离
            let score = proj + perp * 2.4;
            if best.is_none_or(|(_, bs)| score < bs) {
                best = Some((idx, score));
            }
        }
        best.map(|(idx, _)| idx)
    }
}
