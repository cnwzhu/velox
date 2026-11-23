//! 极简 Velox 桌面应用示例（参照 `gpui-kit/examples/hello_world/src/main.rs` 精简启动范式）。

use velox_kit::prelude::*;

pub struct HelloWorld {
    pub clicks: u32,
}

impl View for HelloWorld {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        let on_click = cx.listener(|this, _: ClickEvent, _| {
            this.clicks += 1;
            println!("Clicked! ({})", this.clicks);
        });

        div()
            .flex_col()
            .gap(12)
            .size_full()
            .items_center()
            .justify_center()
            .bg(cx.theme().background)
            .child(
                cx,
                text(fmt!(cx, "Hello, World! ({})", self.clicks))
                    .size(18)
                    .color(cx.theme().foreground),
            )
            .child(
                cx,
                Button::new(1, "Let's Go!")
                    .primary()
                    .medium()
                    .on_click(on_click),
            )
            .into_element(cx)
    }
}

fn main() {
    velox_kit::application().run(|cx| {
        velox_kit::init(cx);
        velox_kit::open_window(
            WindowOptions::new("Hello Velox").size(480, 320),
            cx,
            |cx| cx.new(|_| HelloWorld { clicks: 0 }),
        )
        .expect("Failed to open window");
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hello_world_minimal_application_startup_and_click() {
        let mut root_handle = None;
        let mut built = velox_kit::application()
            .build_headless(|cx| {
                velox_kit::init(cx);
                let win = velox_kit::open_window(
                    WindowOptions::new("Hello Velox").size(480, 320),
                    cx,
                    |cx| cx.new(|_| HelloWorld { clicks: 0 }),
                )
                .expect("Failed to open window");
                root_handle = Some(win.root());
            })
            .expect("primary window must be built");

        let root = root_handle.unwrap();
        assert_eq!(built.options.title, "Hello Velox");
        assert_eq!((built.options.width, built.options.height), (480, 320));

        let _ = built.engine.step_frame();
        let btn = built
            .engine
            .cached_draw_ops()
            .iter()
            .find(|op| op.id == Some(1))
            .expect("primary button id=1 must render")
            .bounds;
        assert!(built
            .engine
            .dispatch_click(btn.x + btn.w / 2, btn.y + btn.h / 2));
        assert_eq!(built.engine.view(root).unwrap().clicks, 1);
    }
}
