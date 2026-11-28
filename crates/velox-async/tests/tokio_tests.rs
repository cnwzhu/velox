#![cfg(feature = "tokio")]

use core::time::Duration;
use std::sync::mpsc::{SyncSender, sync_channel};

use velox_async::{TokioScheduler, VeloxTokioExt};
use velox_engine::prelude::*;

struct TokioDemoView {
    result: i32,
}

impl View for TokioDemoView {
    fn render<'a>(&mut self, cx: &'a mut Scope<Self>) -> Element<'a> {
        div()
            .id(1)
            .w(200)
            .h(80)
            .child(cx, text(fmt!(cx, "res={}", self.result)))
            .into_element(cx)
    }
}

#[test]
fn test_velox_async_tokio_scheduler_background_timer_and_application_ext() {
    let mut built = application()
        .with_tokio()
        .build_headless(|cx| {
            init(cx);
            assert_eq!(cx.scheduler_name(), "tokio");
            let _ = open_window(WindowOptions::new("Tokio Test").size(200, 80), cx, |cx| {
                cx.new(|_| TokioDemoView { result: 0 })
            })
            .unwrap();
        })
        .expect("window built");

    assert_eq!(built.engine.scheduler_name(), "tokio");

    let root = built.engine.mount_root(TokioDemoView { result: 0 });
    let _ = built.engine.step_frame();
    built
        .engine
        .spawn_for(root, async move |this: Handle<TokioDemoView>, mut cx: AsyncApp| {
            cx.timer(Duration::from_millis(12)).await;
            let val = cx
                .background_spawn(async move {
                    velox_async::tokio::runtime::time::sleep(Duration::from_millis(12)).await;
                    108i32
                })
                .await;
            let _ = this.update(&mut cx, |v, _| v.result = val);
        })
        .detach();

    for _ in 0..80 {
        let _ = built.engine.step_frame();
        if built.engine.view(root).unwrap().result == 108 {
            break;
        }
        std::thread::sleep(Duration::from_millis(4));
    }
    assert_eq!(built.engine.view(root).unwrap().result, 108);
    assert_eq!(built.engine.active_async_task_count(), 0);
}

#[test]
fn test_velox_async_tokio_task_drop_cancels_background_future() {
    let engine = VeloxEngine::new(160, 80).with_scheduler(TokioScheduler::global());
    let (started_tx, started_rx) = sync_channel(1);
    let (dropped_tx, dropped_rx) = sync_channel(1);
    struct Guard(SyncSender<()>);
    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = self.0.try_send(());
        }
    }

    let task = engine.background_spawn(async move {
        let _guard = Guard(dropped_tx);
        started_tx.send(()).expect("test must wait for task start");
        core::future::pending::<()>().await;
    });
    started_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("Tokio background task must start");
    drop(task);
    dropped_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("dropping Task must promptly cancel and drop Tokio background future");
}
