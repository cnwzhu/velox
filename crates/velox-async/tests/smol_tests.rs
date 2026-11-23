#![cfg(feature = "smol")]

use core::time::Duration;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use velox_async::{SmolScheduler, VeloxSmolExt};
use velox_engine::prelude::*;

struct SmolDemoView {
    result: i32,
}

impl View for SmolDemoView {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        div()
            .id(1)
            .w(200)
            .h(80)
            .child(cx, text(fmt!(cx, "smol={}", self.result)))
            .into_element(cx)
    }
}

#[test]
fn test_velox_async_smol_scheduler_background_timer_and_application_ext() {
    let mut built = application()
        .with_smol()
        .build_headless(|cx| {
            init(cx);
            assert_eq!(cx.scheduler_name(), "smol");
            let _ = open_window(WindowOptions::new("Smol Test").size(200, 80), cx, |cx| {
                cx.new(|_| SmolDemoView { result: 0 })
            })
            .unwrap();
        })
        .expect("window built");

    assert_eq!(built.engine.scheduler_name(), "smol");
    let root = built.engine.mount_root(SmolDemoView { result: 0 });
    let _ = built.engine.step_frame();

    built
        .engine
        .spawn_for(root, async move |this: Handle<SmolDemoView>, mut cx: AsyncApp| {
            cx.timer(Duration::from_millis(12)).await;
            let val = cx
                .background_spawn(async move {
                    let _ = velox_async::smol::io::Timer::after(Duration::from_millis(12)).await;
                    256i32
                })
                .await;
            let _ = this.update(&mut cx, |v, _| v.result = val);
        })
        .detach();

    for _ in 0..80 {
        let _ = built.engine.step_frame();
        if built.engine.view(root).unwrap().result == 256 {
            break;
        }
        std::thread::sleep(Duration::from_millis(4));
    }
    assert_eq!(built.engine.view(root).unwrap().result, 256);
    assert_eq!(built.engine.active_async_task_count(), 0);
}

#[test]
fn test_velox_async_smol_task_drop_cancels_background_future() {
    let engine = VeloxEngine::new(160, 80).with_scheduler(SmolScheduler::global());
    let dropped = Arc::new(AtomicBool::new(false));
    struct Guard(Arc<AtomicBool>);
    impl Drop for Guard {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }

    let task = engine.background_spawn({
        let flag = Arc::clone(&dropped);
        async move {
            let _g = Guard(flag);
            let _ = velox_async::smol::io::Timer::after(Duration::from_secs(30)).await;
        }
    });
    std::thread::sleep(Duration::from_millis(10));
    drop(task);

    for _ in 0..50 {
        if dropped.load(Ordering::Acquire) {
            break;
        }
        std::thread::sleep(Duration::from_millis(4));
    }
    assert!(
        dropped.load(Ordering::Acquire),
        "dropping Task must promptly cancel and drop smol background future"
    );
}
