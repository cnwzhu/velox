//! 电子表格公式求值引擎（支持聚合函数 `SUM` / `AVG` / `MIN` / `MAX` 与二元四则表达式，
//! 独立成文件便于后续扩展括号表达式 AST、条件函数 `IF`、统计函数与 DAG 依赖增量重算）。

use super::model::SheetGridModel;
use super::types::parse_a1_ref;

/// 求值公式字符串（含循环引用深度保护 `depth <= 16`）。
pub fn eval_formula(model: &SheetGridModel, expr: &str, depth: u8) -> Option<f64> {
    if depth > 16 {
        return None;
    }
    let body = expr.trim().strip_prefix('=').unwrap_or(expr.trim()).trim();
    let upper = body.to_ascii_uppercase();
    for func in ["SUM(", "AVG(", "MIN(", "MAX("] {
        if upper.starts_with(func) && upper.ends_with(')') {
            let inner = &body[func.len()..body.len() - 1];
            let mut parts = inner.split(':');
            let a = parse_a1_ref(parts.next()?)?;
            let b = parts.next().and_then(parse_a1_ref).unwrap_or(a);
            let r0 = a.row.min(b.row);
            let r1 = a.row.max(b.row);
            let c0 = a.col.min(b.col);
            let c1 = a.col.max(b.col);
            let mut vals = Vec::new();
            for r in r0..=r1 {
                for c in c0..=c1 {
                    if let Some(v) = model.eval_number_depth(r, c, depth + 1) {
                        vals.push(v);
                    }
                }
            }
            if vals.is_empty() {
                return Some(0.0);
            }
            return match func {
                "SUM(" => Some(vals.iter().sum()),
                "AVG(" => Some(vals.iter().sum::<f64>() / vals.len() as f64),
                "MIN(" => vals.into_iter().reduce(f64::min),
                "MAX(" => vals.into_iter().reduce(f64::max),
                _ => None,
            };
        }
    }
    // 简单二元运算（如 B2*C2、D2*0.12、A1+B1）
    for (op_ch, op_fn) in [
        ('+', (|a, b| a + b) as fn(f64, f64) -> f64),
        ('-', |a, b| a - b),
        ('*', |a, b| a * b),
        ('/', |a, b| if b.abs() < 1e-12 { 0.0 } else { a / b }),
    ] {
        if let Some(idx) = body.find(op_ch) {
            if idx > 0 && idx + 1 < body.len() {
                let lhs = eval_operand(model, &body[..idx], depth + 1)?;
                let rhs = eval_operand(model, &body[idx + 1..], depth + 1)?;
                return Some(op_fn(lhs, rhs));
            }
        }
    }
    eval_operand(model, body, depth + 1)
}

fn eval_operand(model: &SheetGridModel, token: &str, depth: u8) -> Option<f64> {
    let t = token.trim();
    if let Ok(n) = t.parse::<f64>() {
        return Some(n);
    }
    let coord = parse_a1_ref(t)?;
    model.eval_number_depth(coord.row, coord.col, depth + 1)
}
