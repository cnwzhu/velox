//! 零外部依赖原生矢量 SVG 解析与高精度抗锯齿光栅化引擎（`#![no_std]` 与 `std` 双模）。
//!
//! 模块职责划分：
//! - `math`：纯 `core` 浮点数学原语与 SVG 2D 仿射变换矩阵（`Transform2D`）；
//! - `color`：SVG / CSS 颜色与 `currentColor` / `none` 绘制源解析；
//! - `path`：完整 SVG Path `d` 语法解析（20 个指令含贝塞尔曲线与 W3C 椭圆弧）及基础几何形状展开；
//! - `raster`：4× 垂直 + 解析水平覆盖率抗锯齿扫描线填充器（`NonZero` / `EvenOdd`）与逐行最大覆盖率 SDF 描边器；
//! - `xml`：流式零依赖 SVG XML 解析器（支持 `<svg>`、`<g>` 嵌套继承、行内 `style` 与多色矢量绘制）。

mod color;
mod math;
mod path;
mod raster;
mod xml;

pub use color::{apply_opacity, parse_svg_color, parse_svg_paint, SvgPaint};
pub use math::Transform2D;
pub use path::{FlattenedPath, SubPath};
pub use raster::{fill_path, stroke_path, SvgFillRule, SvgLineCap, SvgLineJoin};
pub use xml::{parse_svg_viewbox, rasterize_svg, rasterize_svg_path, SvgRenderOptions, SvgViewBox};
