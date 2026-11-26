//! 二阶阻尼谐振子物理弹簧动画（`Spring` / `SpringConfig`，100% `#![no_std]`、零堆、`Copy`）。
//!
//! 相比固定时长缓动曲线（[`super::Tween`]），物理弹簧在目标频繁变化或中途打断时
//! **自动继承当前瞬时速度 `velocity`**，带来丝滑自然的物理回弹与阻尼手感。

use crate::svg::math::abs_f32;

/// 物理弹簧刚度、阻尼与质量参数配置。
#[derive(Copy, Clone, Debug)]
pub struct SpringConfig {
    /// 弹簧刚度系数 `k`（越大响应越迅速，默认 `220.0`）。
    pub stiffness: f32,
    /// 阻尼系数 `c`（越大过冲越少，默认 `24.0`）。
    pub damping: f32,
    /// 振子质量 `m`（越大惯性越强，默认 `1.0`）。
    pub mass: f32,
    /// 静止收敛阈值（位移差与速度均小于此阈值后锁定在目标值并进入 0% CPU 休眠）。
    pub rest_eps: f32,
}

impl SpringConfig {
    /// 柔和舒缓（抽屉/弹窗展开）。
    pub const GENTLE: Self = Self {
        stiffness: 120.0,
        damping: 18.0,
        mass: 1.0,
        rest_eps: 0.05,
    };
    /// 均衡默认（开关/按钮/卡片过渡）。
    pub const DEFAULT: Self = Self {
        stiffness: 220.0,
        damping: 24.0,
        mass: 1.0,
        rest_eps: 0.05,
    };
    /// 干脆利落（高刷桌面跟手切换）。
    pub const SNAPPY: Self = Self {
        stiffness: 380.0,
        damping: 32.0,
        mass: 1.0,
        rest_eps: 0.05,
    };
    /// 富有弹性回弹（微交互/通知气泡）。
    pub const BOUNCY: Self = Self {
        stiffness: 260.0,
        damping: 14.0,
        mass: 1.0,
        rest_eps: 0.05,
    };
    /// 高刚度无明显过冲（精确控件复位）。
    pub const STIFF: Self = Self {
        stiffness: 500.0,
        damping: 42.0,
        mass: 1.0,
        rest_eps: 0.05,
    };

    #[inline(always)]
    pub const fn new(stiffness: f32, damping: f32, mass: f32) -> Self {
        Self {
            stiffness,
            damping,
            mass,
            rest_eps: 0.05,
        }
    }
}

impl Default for SpringConfig {
    #[inline(always)]
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// 单自由度物理弹簧状态机（`Copy`，可直接嵌入 `View` 状态字段）。
#[derive(Copy, Clone, Debug)]
pub struct Spring {
    value: f32,
    velocity: f32,
    target: f32,
    config: SpringConfig,
    settled: bool,
}

impl Spring {
    /// 创建一个初始静止在 `initial` 处的物理弹簧。
    #[inline(always)]
    pub const fn new(initial: f32, config: SpringConfig) -> Self {
        Self {
            value: initial,
            velocity: 0.0,
            target: initial,
            config,
            settled: true,
        }
    }

    /// 获取当前实时浮点值。
    #[inline(always)]
    pub const fn value(&self) -> f32 {
        self.value
    }

    /// 获取四舍五入后的整型值（便于直接传入坐标/宽高/偏移）。
    #[inline(always)]
    pub fn value_i32(&self) -> i32 {
        if self.value >= 0.0 {
            (self.value + 0.5) as i32
        } else {
            (self.value - 0.5) as i32
        }
    }

    /// 获取当前瞬时速度（单位/秒）。
    #[inline(always)]
    pub const fn velocity(&self) -> f32 {
        self.velocity
    }

    /// 获取当前目标值。
    #[inline(always)]
    pub const fn target(&self) -> f32 {
        self.target
    }

    /// 弹簧是否已完全收敛静止（静止后可停止调用 `cx.request_frame()` 进入 0% CPU 休眠）。
    #[inline(always)]
    pub const fn is_settled(&self) -> bool {
        self.settled
    }

    /// 弹簧是否仍在运动中。
    #[inline(always)]
    pub const fn is_running(&self) -> bool {
        !self.settled
    }

    /// 平滑更改目标值（自动保留当前瞬时速度 `velocity`，打断进行中的动画绝不突变）。
    #[inline]
    pub fn animate_to(&mut self, target: f32) {
        if abs_f32(target - self.target) <= 1e-6 && self.settled {
            return;
        }
        self.target = target;
        self.settled = false;
    }

    /// 对弹簧施加一个瞬时速度冲量（例如松手惯性甩动或弹性点击反馈）。
    #[inline]
    pub fn impulse(&mut self, delta_velocity: f32) {
        self.velocity += delta_velocity;
        if abs_f32(self.velocity) > self.config.rest_eps {
            self.settled = false;
        }
    }

    /// 直接设置瞬时速度（单位/秒）。
    #[inline]
    pub fn set_velocity(&mut self, velocity: f32) {
        self.velocity = velocity;
        if abs_f32(self.velocity) > self.config.rest_eps {
            self.settled = false;
        }
    }

    /// 动态更新弹簧物理参数（刚度、阻尼、质量），不打断当前位移与速度。
    #[inline(always)]
    pub fn set_config(&mut self, config: SpringConfig) {
        self.config = config;
    }

    /// 立即跳转至指定值并清零速度。
    #[inline(always)]
    pub fn jump_to(&mut self, value: f32) {
        self.value = value;
        self.target = value;
        self.velocity = 0.0;
        self.settled = true;
    }

    /// 以毫秒步长 `dt_ms` 推进弹簧物理积分（内部自动拆分 `<= 4ms` 子步长保证高刚度数值绝对稳定）。
    pub fn step(&mut self, dt_ms: u16) -> f32 {
        if self.settled || dt_ms == 0 {
            return self.value;
        }
        // 将大帧间隔钳位在 100ms 内，避免后台切回造成爆炸位移
        let total_s = (dt_ms.min(100) as f32) * 0.001;
        let max_sub_s = 0.004f32;
        let sub_steps = ((total_s / max_sub_s) as u32).max(1);
        let h = total_s / (sub_steps as f32);
        let inv_m = 1.0 / self.config.mass.max(0.01);
        let k = self.config.stiffness.max(1.0);
        let c = self.config.damping.max(0.1);

        for _ in 0..sub_steps {
            let disp = self.value - self.target;
            let accel = (-k * disp - c * self.velocity) * inv_m;
            // 半隐式欧拉（Symplectic Euler）：能量守恒性极佳，不会发散振荡
            self.velocity += accel * h;
            self.value += self.velocity * h;
        }

        let eps = self.config.rest_eps.max(1e-4);
        if abs_f32(self.value - self.target) <= eps && abs_f32(self.velocity) <= eps * 10.0 {
            self.value = self.target;
            self.velocity = 0.0;
            self.settled = true;
        }
        self.value
    }
}
