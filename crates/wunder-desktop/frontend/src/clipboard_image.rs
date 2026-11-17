use crate::{image_attachment, MainWindow};

pub fn paste(app: slint::Weak<MainWindow>) -> bool {
    let image = arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.get_image())
        .ok();
    let Some(image) = image else {
        return false;
    };
    let (Ok(width), Ok(height)) = (u32::try_from(image.width), u32::try_from(image.height)) else {
        report(&app, "剪贴板图片尺寸无效".into());
        return true;
    };
    let rgba = image.bytes.into_owned();
    if let Some(window) = app.upgrade() {
        window.set_status("正在读取剪贴板图片…".into());
    }
    std::thread::spawn(move || {
        let result = image_attachment::save(width, height, &rgba, "wunder-clipboard");
        let _ = slint::invoke_from_event_loop(move || match result {
            Ok((name, data_url)) => image_attachment::add(&app, name, data_url, "剪贴板图片"),
            Err(error) => report(&app, format!("无法添加剪贴板图片：{error}")),
        });
    });
    true
}

fn report(app: &slint::Weak<MainWindow>, message: String) {
    if let Some(app) = app.upgrade() {
        app.set_status(message.into());
    }
}
