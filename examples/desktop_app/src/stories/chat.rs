//! `AI Chat` 故事页（Story 15）：参照 `gpui-kit` Chat 打造的交互式对话体验，
//! 100% 基于 `velox-component`（`Message` / `Bubble` / `MessageScroller` / `TextInput` / `Button` / `Badge`）构建：
//! - 新消息侧向物理弹簧滑入（`Easing::Spring`）+ 非对称聊天气泡尾角；
//! - AI 思考中 3 颗相位错峰波浪跳动圆点动画（`typing(true)`）；
//! - 逐字流式打字机生成动画 + 平滑呼吸脉冲流式光标（`streaming(true)`），完成后自动停帧回归 0% CPU 休眠；
//! - 可交互表情 Reaction 胶囊微弹反馈、快捷 Prompt 芯片与底部弹性 `"Jump to latest"` 悬浮提示。

use super::{state_pill, story_header, story_section};
use crate::GalleryApp;
use velox_kit::component::{
    Badge, BubbleReaction, Button, EditableText, Message, MessageRole, MessageScrollerState,
    TextInput,
};
use velox_kit::prelude::*;

/// 单条聊天消息记录。
#[derive(Clone, Debug)]
pub struct ChatEntry {
    pub id: u32,
    pub role: MessageRole,
    pub author: &'static str,
    pub avatar: &'static str,
    pub role_badge: &'static str,
    pub timestamp: &'static str,
    pub content: String,
    pub liked: bool,
    pub like_count: u16,
    pub sparked: bool,
    pub spark_count: u16,
}

/// AI 对话流式输出状态机。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatStreamPhase {
    /// 空闲待机（0% CPU 休眠）。
    Idle,
    /// AI 思考中（展示 3 颗波浪跳动思考圆点）。
    Thinking {
        start_ms: u32,
        reply: String,
    },
    /// 流式打字机逐字输出（末尾带呼吸脉冲光标）。
    Streaming {
        start_ms: u32,
        full_reply: String,
        revealed_chars: usize,
    },
}

/// `AI Chat` 故事页独立状态。
pub struct ChatDemoState {
    pub messages: Vec<ChatEntry>,
    pub next_msg_id: u32,
    pub input: EditableText<160>,
    pub input_focused: bool,
    pub phase: ChatStreamPhase,
    pub scroller: MessageScrollerState,
    pub pending_trigger_reply: Option<String>,
    pub last_now_ms: u32,
}

impl Default for ChatDemoState {
    fn default() -> Self {
        Self::new()
    }
}

impl ChatDemoState {
    pub fn new() -> Self {
        Self {
            messages: vec![
                ChatEntry {
                    id: 6101,
                    role: MessageRole::Assistant,
                    author: "Velox AI",
                    avatar: "VX",
                    role_badge: "Velox-4o",
                    timestamp: "10:02",
                    content: String::from(
                        "Welcome to Velox AI Chat! Every bubble animates with spring physics, typing waves, and live token streaming.",
                    ),
                    liked: true,
                    like_count: 3,
                    sparked: false,
                    spark_count: 1,
                },
                ChatEntry {
                    id: 6102,
                    role: MessageRole::User,
                    author: "Alex",
                    avatar: "AX",
                    role_badge: "",
                    timestamp: "10:03",
                    content: String::from(
                        "How does Velox keep 120fps animations smooth while staying 0% CPU at idle?",
                    ),
                    liked: false,
                    like_count: 1,
                    sparked: true,
                    spark_count: 2,
                },
                ChatEntry {
                    id: 6103,
                    role: MessageRole::Assistant,
                    author: "Velox AI",
                    avatar: "VX",
                    role_badge: "Velox-4o",
                    timestamp: "10:03",
                    content: String::from(
                        "Pass-4-Only fast reflattening + L2 subview isolation + hardware SrcOver GPU composition. Once springs and streams finish, the event loop sleeps in ControlFlow::Wait.",
                    ),
                    liked: true,
                    like_count: 5,
                    sparked: true,
                    spark_count: 3,
                },
            ],
            next_msg_id: 6104,
            input: EditableText::from_str(""),
            input_focused: false,
            phase: ChatStreamPhase::Idle,
            scroller: MessageScrollerState::new(),
            pending_trigger_reply: None,
            last_now_ms: 0,
        }
    }

    /// 发送用户消息并排队触发 AI 思考 -> 流式回复动画。
    pub fn send_prompt(&mut self, prompt: &str) {
        let trimmed = prompt.trim();
        if trimmed.is_empty() {
            return;
        }
        let user_id = self.next_msg_id;
        self.next_msg_id = self.next_msg_id.saturating_add(1);
        self.messages.push(ChatEntry {
            id: user_id,
            role: MessageRole::User,
            author: "Alex",
            avatar: "AX",
            role_badge: "",
            timestamp: "Now",
            content: String::from(trimmed),
            liked: false,
            like_count: 0,
            sparked: false,
            spark_count: 0,
        });
        if self.messages.len() > 8 {
            self.messages.remove(0);
        }
        if !self.scroller.follow_tail {
            self.scroller.unread_count = self.scroller.unread_count.saturating_add(1);
        }
        let reply = Self::compose_reply(trimmed);
        self.pending_trigger_reply = Some(reply);
    }

    fn compose_reply(prompt: &str) -> String {
        let lower = prompt.to_ascii_lowercase();
        if lower.contains("spring") || lower.contains("motion") || lower.contains("anim") {
            String::from(
                "SpringAnimator solves a semi-implicit symplectic Euler 2nd-order spring per frame. Bubbles slide in from their alignment side, and settled channels auto-sleep at 0% CPU.",
            )
        } else if lower.contains("gpu") || lower.contains("effect") || lower.contains("shader") {
            String::from(
                "The GPU pipeline uses hardware PREMULTIPLIED_ALPHA_BLENDING for SrcOver layers (0 copies), sub-rect snapshots for 26 W3C blend modes & BackdropFilter, and a persistent BindGroup cache.",
            )
        } else if lower.contains("layout") || lower.contains("arena") || lower.contains("alloc") {
            String::from(
                "Velox solves 4-pass O(N) Flexbox & Grid layouts inside a transient ScratchArena reset every frame, combined with L2 subview isolation so unchanged panels never re-render.",
            )
        } else {
            format!(
                "Got it: \"{}\". Velox rendered your bubble with spring entry, ran the 3-dot wave indicator, and streamed this reply token-by-token before returning to 0% CPU idle!",
                prompt
            )
        }
    }

    /// 推进 AI 思考与流式打字机动画状态机；返回是否仍需要下一帧刷新。
    pub fn advance(&mut self, now_ms: u32) -> bool {
        self.last_now_ms = now_ms;
        if let Some(reply) = self.pending_trigger_reply.take() {
            self.phase = ChatStreamPhase::Thinking {
                start_ms: now_ms,
                reply,
            };
            return true;
        }
        match &mut self.phase {
            ChatStreamPhase::Idle => false,
            ChatStreamPhase::Thinking { start_ms, reply } => {
                let elapsed = now_ms.wrapping_sub(*start_ms);
                if elapsed >= 840 {
                    let full_reply = core::mem::take(reply);
                    self.phase = ChatStreamPhase::Streaming {
                        start_ms: now_ms,
                        full_reply,
                        revealed_chars: 1,
                    };
                }
                true
            }
            ChatStreamPhase::Streaming {
                start_ms,
                full_reply,
                revealed_chars,
            } => {
                let elapsed = now_ms.wrapping_sub(*start_ms);
                let total_chars = full_reply.chars().count();
                // 约 90 chars/sec 丝滑流式输出速度
                let target = ((elapsed as usize) / 14).saturating_add(1).min(total_chars);
                *revealed_chars = target;
                if target >= total_chars {
                    let finished = core::mem::take(full_reply);
                    let ai_id = self.next_msg_id;
                    self.next_msg_id = self.next_msg_id.saturating_add(1);
                    self.messages.push(ChatEntry {
                        id: ai_id,
                        role: MessageRole::Assistant,
                        author: "Velox AI",
                        avatar: "VX",
                        role_badge: "Velox-4o",
                        timestamp: "Now",
                        content: finished,
                        liked: false,
                        like_count: 1,
                        sparked: false,
                        spark_count: 1,
                    });
                    if self.messages.len() > 8 {
                        self.messages.remove(0);
                    }
                    if !self.scroller.follow_tail {
                        self.scroller.unread_count = self.scroller.unread_count.saturating_add(1);
                    }
                    self.phase = ChatStreamPhase::Idle;
                    false
                } else {
                    true
                }
            }
        }
    }

    /// 立即重放最后一条 AI 回复的思考 + 流式动画演示。
    pub fn replay_stream(&mut self) {
        self.pending_trigger_reply = Some(String::from(
            "Streaming replayed! Notice the 3-dot wave cadence -> live token stream with breathing cursor -> automatic 0% CPU idle settlement.",
        ));
    }

    /// 切换指定消息的表情回应计数。
    pub fn toggle_reaction(&mut self, msg_id: u32, react_idx: usize) {
        if let Some(m) = self.messages.iter_mut().find(|e| e.id == msg_id) {
            if react_idx == 0 {
                m.liked = !m.liked;
                if m.liked {
                    m.like_count = m.like_count.saturating_add(1);
                } else {
                    m.like_count = m.like_count.saturating_sub(1);
                }
            } else {
                m.sparked = !m.sparked;
                if m.sparked {
                    m.spark_count = m.spark_count.saturating_add(1);
                } else {
                    m.spark_count = m.spark_count.saturating_sub(1);
                }
            }
        }
    }
}

/// 按字符数安全截取 UTF-8 字符串前缀（零字节边界截断风险）。
fn utf8_char_prefix(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((byte_idx, _)) => &s[..byte_idx],
        None => s,
    }
}

impl GalleryApp {
    pub(crate) fn render_story_chat<'a>(
        &mut self,
        cx: &'a Scope<Self>,
        theme: CoreTheme,
    ) -> Div<'a> {
        let now_ms = cx.now_ms();
        if self.chat.advance(now_ms) {
            cx.request_frame();
        }

        let is_thinking = matches!(self.chat.phase, ChatStreamPhase::Thinking { .. });
        let is_streaming = matches!(self.chat.phase, ChatStreamPhase::Streaming { .. });
        let status_badge = if is_thinking {
            Badge::new("Thinking...").warning()
        } else if is_streaming {
            Badge::new("Streaming").info()
        } else {
            Badge::new("Idle (0% CPU)").success()
        };

        let on_send = cx.listener(|this, _: ClickEvent, _acx| {
            let txt = String::from(this.chat.input.as_str());
            if !txt.trim().is_empty() {
                this.chat.send_prompt(&txt);
                this.chat.input.clear();
            } else {
                this.chat.replay_stream();
            }
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_quick_spring = cx.listener(|this, _: ClickEvent, _acx| {
            this.chat.send_prompt("How does spring motion work?");
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_quick_gpu = cx.listener(|this, _: ClickEvent, _acx| {
            this.chat.send_prompt("Explain the GPU effects speedup!");
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_quick_layout = cx.listener(|this, _: ClickEvent, _acx| {
            this.chat.send_prompt("How does zero-alloc layout work?");
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_replay = cx.listener(|this, _: ClickEvent, _acx| {
            this.chat.replay_stream();
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_toggle_follow = cx.listener(|this, _: ClickEvent, _acx| {
            if this.chat.scroller.follow_tail {
                this.chat.scroller.follow_tail = false;
                this.chat.scroller.unread_count = this.chat.scroller.unread_count.max(2);
            } else {
                this.chat.scroller.scroll_to_bottom(0);
            }
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_reset = cx.listener(|this, _: ClickEvent, _acx| {
            this.chat = ChatDemoState::new();
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_input_focus = cx.action(|this, acx| {
            this.chat.input_focused = true;
            this.chat.input.reset_blink(acx.now_ms());
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_input_key = cx.listener(|this, key: KeyCode, acx| {
            this.chat.input_focused = true;
            if key == KeyCode::Enter {
                let txt = String::from(this.chat.input.as_str());
                if !txt.trim().is_empty() {
                    this.chat.send_prompt(&txt);
                    this.chat.input.clear();
                    this.event_count = this.event_count.saturating_add(1);
                }
            } else {
                let _ = this.chat.input.handle_key_with_mods(
                    key,
                    acx.modifiers(),
                    false,
                    acx.now_ms(),
                );
                this.event_count = this.event_count.saturating_add(1);
            }
        });
        let on_input_ptr = cx.listener(
            |this, (x, is_drag, now_ms, shift): (i16, bool, u32, bool), _acx| {
                this.chat.input_focused = true;
                this.chat
                    .input
                    .handle_pointer_action(x, is_drag, now_ms, shift, false, 13);
            },
        );
        let on_react_pair = cx.listener(|this, (msg_id, r_idx): (u32, usize), _acx| {
            this.chat.toggle_reaction(msg_id, r_idx);
            this.event_count = this.event_count.saturating_add(1);
        });
        let on_jump_bottom = cx.listener(|this, _: ClickEvent, _acx| {
            this.chat.scroller.scroll_to_bottom(0);
            this.event_count = this.event_count.saturating_add(1);
        });

        // 顶部工具条（状态徽章 + 回放 / 滚动状态切换 / 重置）
        let top_controls = div()
            .flex_row()
            .flex_wrap()
            .w_full()
            .gap(8)
            .items_center()
            .justify_between()
            .child(
                cx,
                div()
                    .flex_row()
                    .gap(8)
                    .items_center()
                    .child(cx, status_badge)
                    .child(
                        cx,
                        state_pill(
                            cx,
                            theme,
                            fmt!(cx, "Messages: {}", self.chat.messages.len()),
                        ),
                    ),
            )
            .child(
                cx,
                div()
                    .flex_row()
                    .gap(6)
                    .items_center()
                    .child(
                        cx,
                        Button::new(6001, "Replay Stream")
                            .primary()
                            .small()
                            .on_click(on_replay),
                    )
                    .child(
                        cx,
                        Button::new(
                            6002,
                            if self.chat.scroller.follow_tail {
                                "Simulate Scroll Up"
                            } else {
                                "Follow Tail"
                            },
                        )
                        .outline()
                        .small()
                        .on_click(on_toggle_follow),
                    )
                    .child(
                        cx,
                        Button::new(6003, "Reset")
                            .ghost()
                            .small()
                            .on_click(on_reset),
                    ),
            );

        // 对话消息流列表（取最近 4 条保证 1080×700 一屏完整呈现消息、流式动画与底部作曲栏）
        let mut thread_col = div()
            .id(6050)
            .flex_col()
            .w_full()
            .p(12)
            .gap(10)
            .rounded(10)
            .bg(theme.background)
            .border(1, theme.border);

        let visible_start = self.chat.messages.len().saturating_sub(4);
        for entry in &self.chat.messages[visible_start..] {
            let msg_id = entry.id;
            let like_lbl = fmt!(cx, "+1 {}", entry.like_count).as_str();
            let spark_lbl = fmt!(cx, "* {}", entry.spark_count).as_str();
            let reactions = cx.arena().alloc_iter([
                BubbleReaction::new(like_lbl, entry.like_count, entry.liked),
                BubbleReaction::new(spark_lbl, entry.spark_count, entry.sparked),
            ]);
            let on_react_cb = cx.arena().alloc(move |r_idx: usize, acx: &mut ActionScope| {
                on_react_pair((msg_id, r_idx), acx);
            });
            let content_copy = fmt!(cx, "{}", entry.content).as_str();
            let msg = Message::new(entry.id, entry.role, entry.author, content_copy)
                .avatar(entry.avatar)
                .role_badge(entry.role_badge)
                .timestamp(entry.timestamp)
                .reactions(reactions)
                .width(720)
                .on_reaction_click(on_react_cb);
            thread_col = thread_col.child(cx, msg);
        }

        // 若处于 AI 思考中 / 流式打字机生成中，实时渲染动画气泡
        match &self.chat.phase {
            ChatStreamPhase::Idle => {}
            ChatStreamPhase::Thinking { .. } => {
                thread_col = thread_col.child(
                    cx,
                    Message::new(6098, MessageRole::Assistant, "Velox AI", "Analyzing...")
                        .avatar("VX")
                        .role_badge("Velox-4o")
                        .timestamp("Typing")
                        .width(720)
                        .typing(true),
                );
            }
            ChatStreamPhase::Streaming {
                full_reply,
                revealed_chars,
                ..
            } => {
                let slice = utf8_char_prefix(full_reply.as_str(), *revealed_chars);
                let live_text = fmt!(cx, "{}", slice).as_str();
                thread_col = thread_col.child(
                    cx,
                    Message::new(6099, MessageRole::Assistant, "Velox AI", live_text)
                        .avatar("VX")
                        .role_badge("Velox-4o")
                        .timestamp("Live")
                        .width(720)
                        .streaming(true),
                );
            }
        }

        // 当模拟向上滚动离开底部时，展示 gpui-kit 风格弹性浮起的 “Jump to latest” 悬浮胶囊
        let jump_banner = if !self.chat.scroller.follow_tail {
            let jump_label = fmt!(
                cx,
                "v {} new · Jump to latest",
                self.chat.scroller.unread_count.max(1)
            );
            Some(
                div()
                    .w_full()
                    .flex_row()
                    .justify_center()
                    .child(
                        cx,
                        div()
                            .id(6095)
                            .enter_from(0, 0, 12, Duration::from_millis(220))
                            .ease(Easing::EaseOutBack)
                            .child(
                                cx,
                                Button::new(6096, jump_label)
                                    .primary()
                                    .small()
                                    .rounded(14)
                                    .on_click(on_jump_bottom),
                            ),
                    ),
            )
        } else {
            None
        };

        // 快捷 Prompt 芯片行
        let quick_prompts = div()
            .flex_row()
            .flex_wrap()
            .w_full()
            .gap(8)
            .items_center()
            .child(
                cx,
                text("Quick Prompts:")
                    .size(11)
                    .color(theme.muted_foreground),
            )
            .child(
                cx,
                Button::new(6011, "Spring Motion?")
                    .outline()
                    .small()
                    .rounded(14)
                    .on_click(on_quick_spring),
            )
            .child(
                cx,
                Button::new(6012, "GPU Effects Speedup?")
                    .outline()
                    .small()
                    .rounded(14)
                    .on_click(on_quick_gpu),
            )
            .child(
                cx,
                Button::new(6013, "Zero-Alloc Layout?")
                    .outline()
                    .small()
                    .rounded(14)
                    .on_click(on_quick_layout),
            );

        // 底部输入作曲栏（velox-component TextInput + Primary Send Button）
        let input_text = cx.arena().alloc_str(self.chat.input.as_str()).as_str();
        let composer = div()
            .flex_row()
            .w_full()
            .gap(8)
            .items_center()
            .child(
                cx,
                TextInput::new(6020, input_text, self.chat.input_focused)
                    .size(560, 34)
                    .placeholder("Ask Velox AI anything and press Enter...")
                    .cursor(self.chat.input.cursor())
                    .selection(self.chat.input.selected_range())
                    .blink_epoch(self.chat.input.blink_epoch_ms())
                    .on_focus(on_input_focus)
                    .on_key_input(on_input_key)
                    .on_pointer_action(on_input_ptr),
            )
            .child(
                cx,
                Button::new(6021, if is_thinking || is_streaming { "Streaming" } else { "Send" })
                    .primary()
                    .w(96)
                    .loading(is_thinking)
                    .on_click(on_send),
            );

        let mut chat_section = story_section(
            cx,
            theme,
            "Interactive AI Conversation (gpui-kit Animated Chat)",
            "Spring bubble entry • 3-dot wave typing • Live token streaming • Interactive reactions",
        )
        .child(cx, top_controls)
        .child(cx, thread_col);
        if let Some(banner) = jump_banner {
            chat_section = chat_section.child(cx, banner);
        }
        let chat_section = chat_section.child(cx, quick_prompts).child(cx, composer);

        div()
            .flex_col()
            .w_full()
            .gap(16)
            .child(
                cx,
                story_header(
                    cx,
                    theme,
                    "AI Chat & Streaming",
                    "gpui-kit inspired animated chat with spring bubbles, wave typing indicator & token streaming",
                    "velox-component::chat",
                ),
            )
            .child(cx, chat_section)
    }
}
