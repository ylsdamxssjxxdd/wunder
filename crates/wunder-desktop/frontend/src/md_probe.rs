//! Offscreen acceptance render for the Markdown surface.
//!
//! The unit tests assert the projection; only a render proves the layout agrees
//! with it (wrapping, block rhythm, table columns). `--markdown-probe <dir>`
//! writes three PNGs of one document that covers every block kind — the plain
//! render, a drag selection, and the right-click menu — so a change to
//! `ui/markdown-rich.slint` or to the vendored selection layer can be checked
//! without opening a chat.
use crate::message_blocks;
use slint::{ComponentHandle, ModelRc, VecModel};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

const SAMPLE: &str = r#"# Markdown 渲染验收

这一段用来检查正文的**粗体**、*斜体*、~~删除线~~、`行内代码` 与[链接](https://example.invalid/doc)在同一个段落里的基线是否对齐，顺便验证 CJK 与 Latin 混排的换行位置。

## 列表

- 无序项一，文字长一些以便观察换行后的悬挂缩进是否与项目符号对齐
- 无序项二
  - 嵌套项
  - 另一个嵌套项
- [x] 已完成的任务
- [ ] 未完成的任务

1. 有序项一
2. 有序项二

## 代码

```rust
fn main() {
    // 注释保持原色
    let value: i32 = 42;
    println!("value = {value}");
}
```

## 表格

| 名称 | 说明 | 数值 |
| :--- | :---: | ---: |
| alpha | 左对齐的说明文字 | 1 |
| beta | 居中的说明文字 | 22 |
| gamma | 右对齐的说明文字 | 333 |

> 引用段落：左侧竖线连续，并在多行时保持同一条导轨。
>
> 引用里的第二段。

---

普通段落结束。
"#;

thread_local! {
    static WINDOW: RefCell<slint::Weak<crate::MainWindow>> = RefCell::new(Default::default());
}

pub fn run(app: &crate::MainWindow, directory: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(&directory)?;
    // The shell gates the transcript behind its own boot state; the probe is a
    // pure layout check, so it opens the gate directly instead of booting the
    // runtime.
    app.set_chat_loading(false);
    app.set_session_loading(false);
    app.set_threads_ready(true);
    app.set_startup_note("".into());
    app.set_cloud_chat_active(false);
    // Keep the probe at the top of the document: the transcript otherwise
    // follows the newest row, which is the end of this sample.
    app.set_follow_output(false);
    app.set_timeline(rows());
    app.show()?;
    WINDOW.with(|window| *window.borrow_mut() = app.as_weak());

    // A scroll view delays forwarding a press by 100 ms while it decides
    // between a pan and a tap, so the events have to be paced like a person's
    // rather than fired in one burst.
    after(400, {
        let directory = directory.clone();
        move |app| {
            shoot(app, &directory, "markdown.png");
            moved(app, 400.0, 240.0);
            println!(
                "cursor@400,240={}",
                i_slint_core::cursor::last_mouse_cursor()
            );
            press(app, 400.0, 240.0);
        }
    });
    after(560, |app| {
        for (x, y) in [
            (470.0f32, 262.0f32),
            (560.0, 280.0),
            (650.0, 300.0),
            (760.0, 320.0),
        ] {
            moved(app, x, y);
        }
        released(app, 760.0, 320.0);
        println!(
            "selection: {:?}",
            i_slint_core::text_selection::selection_text()
        );
    });
    after(760, {
        let directory = directory.clone();
        move |app| shoot(app, &directory, "markdown-selection.png")
    });
    after(880, |app| right_click(app, 400.0, 240.0));
    after(1100, {
        let directory = directory.clone();
        move |app| {
            println!(
                "menu open: {}",
                app.global::<crate::TextMenuState>().get_open()
            );
            shoot(app, &directory, "markdown-menu.png");
            // Click the menu's 复制 row: it must hit, run the copy and close.
            click(app, 470.0, 320.0);
            println!(
                "menu open after click: {}",
                app.global::<crate::TextMenuState>().get_open()
            );
            let _ = slint::quit_event_loop();
        }
    });
    slint::run_event_loop()?;
    Ok(())
}

fn after<F>(delay_ms: u64, body: F)
where
    F: Fn(&crate::MainWindow) + 'static,
{
    let weak = WINDOW.with(|window| window.borrow().clone());
    slint::Timer::single_shot(Duration::from_millis(delay_ms), move || {
        if let Some(app) = weak.upgrade() {
            body(&app);
        }
    });
}

fn shoot(app: &crate::MainWindow, directory: &Path, name: &str) {
    let Ok(pixels) = app.window().take_snapshot() else {
        eprintln!("snapshot failed");
        return;
    };
    let path = directory.join(name);
    let Ok(file) = std::fs::File::create(&path) else {
        eprintln!("cannot create {}", path.display());
        return;
    };
    let mut encoder = png::Encoder::new(file, pixels.width(), pixels.height());
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let _ = encoder
        .write_header()
        .and_then(|mut writer| writer.write_image_data(pixels.as_bytes()));
    println!("{}", path.display());
}

fn press(app: &crate::MainWindow, x: f32, y: f32) {
    use slint::platform::{PointerEventButton, WindowEvent};
    let position = slint::LogicalPosition::new(x, y);
    let _ = app
        .window()
        .dispatch_event_with_result(WindowEvent::PointerMoved { position });
    let _ = app
        .window()
        .dispatch_event_with_result(WindowEvent::PointerPressed {
            position,
            button: PointerEventButton::Left,
        });
}

fn moved(app: &crate::MainWindow, x: f32, y: f32) {
    use slint::platform::WindowEvent;
    let _ = app
        .window()
        .dispatch_event_with_result(WindowEvent::PointerMoved {
            position: slint::LogicalPosition::new(x, y),
        });
}

fn released(app: &crate::MainWindow, x: f32, y: f32) {
    use slint::platform::{PointerEventButton, WindowEvent};
    let _ = app
        .window()
        .dispatch_event_with_result(WindowEvent::PointerReleased {
            position: slint::LogicalPosition::new(x, y),
            button: PointerEventButton::Left,
        });
}

fn right_click(app: &crate::MainWindow, x: f32, y: f32) {
    use slint::platform::{PointerEventButton, WindowEvent};
    let position = slint::LogicalPosition::new(x, y);
    let _ = app
        .window()
        .dispatch_event_with_result(WindowEvent::PointerMoved { position });
    let _ = app
        .window()
        .dispatch_event_with_result(WindowEvent::PointerPressed {
            position,
            button: PointerEventButton::Right,
        });
    let _ = app
        .window()
        .dispatch_event_with_result(WindowEvent::PointerReleased {
            position,
            button: PointerEventButton::Right,
        });
}

/// One user row and one answer row, built through the same projection the chat
/// uses, so the probe cannot drift from production.
fn rows() -> ModelRc<crate::TimelineRow> {
    let mut user = crate::TimelineRow::default();
    user.kind = crate::timeline::KIND_USER;
    user.id = "probe-user".into();
    user.visible = true;
    user.text = "渲染验收：请检查块间距、换行与表格列宽。".into();
    user.blocks = message_blocks::from_text(&user.text);
    // A row that belongs to no fold renders standalone; the struct default (0)
    // would claim membership of group 0 and the timeline would hide it.
    user.group_idx = -1;
    user.process_idx = -1;

    let mut body = crate::TimelineRow::default();
    body.kind = crate::timeline::KIND_BODY;
    body.id = "probe-body".into();
    body.visible = true;
    body.text = SAMPLE.into();
    body.blocks = message_blocks::from_text(SAMPLE);
    body.foldable = true;
    body.group_idx = -1;
    body.process_idx = -1;

    let rows: Vec<crate::TimelineRow> = vec![user, body];
    ModelRc::from(Rc::new(VecModel::from(rows)))
}

fn click(app: &crate::MainWindow, x: f32, y: f32) {
    press(app, x, y);
    released(app, x, y);
}
