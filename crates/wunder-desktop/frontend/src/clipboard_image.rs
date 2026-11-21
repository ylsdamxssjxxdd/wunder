//! System clipboard image support for the native desktop shell.
//!
//! Two directions live here: turning a clipboard bitmap into a composer
//! attachment ([`paste`]), and putting a captured image on the clipboard so it
//! can be pasted into other programs ([`publish`]).
//!
//! On X11 and Wayland the clipboard is *hosted by the writing process*: arboard
//! serves the data from its own selection window until another program takes
//! the selection over. Dropping the last `Clipboard` instance hands the data to
//! the clipboard manager (if one is running) and tears that window down, so a
//! write from a short-lived `Clipboard` reports success and the picture is gone
//! before the user can paste it anywhere. Every image write therefore goes
//! through one process-wide owner thread that keeps the instance alive for the
//! whole session. The same single owner also serialises access, which Windows
//! needs because the native clipboard can only be opened by one thread at a
//! time.

use crate::{image_attachment, MainWindow};
use std::{
    borrow::Cow,
    sync::{mpsc, Mutex, MutexGuard},
    time::Duration,
};

/// A stuck clipboard (occupied by a monitor, or a desktop that stopped
/// answering) must not strand the capture thread.
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
/// The first write pays for connecting to the desktop clipboard service; a
/// desktop without one must fail fast instead of stalling the caller.
const INIT_TIMEOUT: Duration = Duration::from_secs(2);

struct Publish {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    reply: mpsc::Sender<Result<(), String>>,
}

static OWNER: Mutex<Option<mpsc::Sender<Publish>>> = Mutex::new(None);

/// Put one RGBA image on the system clipboard, blocking until the clipboard has
/// accepted it. Call it from a worker thread only: connecting to the desktop
/// clipboard and encoding a full-screen bitmap are both far too slow for the UI
/// thread.
pub fn publish(width: u32, height: u32, rgba: Vec<u8>) -> Result<(), String> {
    // 32-bit builds (Win7 x86) must not wrap the expected length on a huge
    // buffer; a wrong size would otherwise reach arboard as a readable image.
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4));
    if width == 0 || height == 0 || expected != Some(rgba.len()) {
        return Err("图像数据无效".into());
    }
    let sender = owner()?;
    let (reply, answer) = mpsc::channel();
    if sender
        .send(Publish {
            width,
            height,
            rgba,
            reply,
        })
        .is_err()
    {
        // The owner thread is gone; let the next write start a fresh one.
        *owner_slot() = None;
        return Err("剪贴板通道已关闭".into());
    }
    match answer.recv_timeout(WRITE_TIMEOUT) {
        Ok(result) => result,
        Err(_) => Err("写入剪贴板超时".into()),
    }
}

/// Read one image from the clipboard into the composer. Returns whether an
/// image was found: `false` keeps the caller's normal text paste path intact.
pub fn paste(app: slint::Weak<MainWindow>) -> bool {
    // A failed read is the ordinary "the clipboard holds text" answer here and
    // must stay silent: `false` hands the paste back to the text path.
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
        let result = image_attachment::save(width, height, &rgba, "clipboard");
        let _ = slint::invoke_from_event_loop(move || match result {
            Ok((name, data_url)) => image_attachment::add(&app, name, data_url, "剪贴板图片"),
            Err(error) => report(&app, format!("无法添加剪贴板图片：{}", error)),
        });
    });
    true
}

fn owner_slot() -> MutexGuard<'static, Option<mpsc::Sender<Publish>>> {
    OWNER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn owner() -> Result<mpsc::Sender<Publish>, String> {
    // Serialise startup: two captures finishing at once must not create two
    // owners, the second of which would silently replace the first.
    let mut slot = owner_slot();
    if let Some(sender) = slot.as_ref() {
        return Ok(sender.clone());
    }
    let sender = start_owner()?;
    *slot = Some(sender.clone());
    Ok(sender)
}

/// Start the one thread that owns the clipboard for this process. It stays
/// parked on the channel for the whole session, which is exactly what keeps the
/// published image alive on X11 and Wayland.
fn start_owner() -> Result<mpsc::Sender<Publish>, String> {
    let (sender, receiver) = mpsc::channel::<Publish>();
    let (ready, ready_rx) = mpsc::channel::<Result<(), String>>();
    std::thread::Builder::new()
        .name("clipboard-owner".into())
        .spawn(move || {
            let mut clipboard = match arboard::Clipboard::new() {
                Ok(clipboard) => {
                    let _ = ready.send(Ok(()));
                    clipboard
                }
                Err(error) => {
                    let _ = ready.send(Err(describe(&error)));
                    return;
                }
            };
            for request in receiver {
                let image = arboard::ImageData {
                    width: request.width as usize,
                    height: request.height as usize,
                    bytes: Cow::Owned(request.rgba),
                };
                // A rejected write only affects this capture; the owner must
                // stay alive so the next screenshot can try again.
                let _ = request
                    .reply
                    .send(clipboard.set_image(image).map_err(|error| describe(&error)));
            }
            // Leaving the loop drops the last instance, which lets arboard hand
            // the data over to the clipboard manager before the window closes.
        })
        .map_err(|error| format!("无法启动剪贴板线程：{}", error))?;
    match ready_rx.recv_timeout(INIT_TIMEOUT) {
        Ok(Ok(())) => Ok(sender),
        Ok(Err(error)) => Err(error),
        Err(_) => Err("剪贴板初始化超时".into()),
    }
}

fn describe(error: &arboard::Error) -> String {
    match error {
        arboard::Error::ClipboardNotSupported => "当前桌面环境不支持剪贴板".into(),
        arboard::Error::ClipboardOccupied => "剪贴板被其他程序占用".into(),
        arboard::Error::ContentNotAvailable => "剪贴板中没有图片".into(),
        arboard::Error::ConversionFailure => "剪贴板图片格式无法转换".into(),
        arboard::Error::Unknown { description } => format!("剪贴板不可用（{}）", description),
        _ => "剪贴板操作失败".into(),
    }
}

fn report(app: &slint::Weak<MainWindow>, message: String) {
    if let Some(app) = app.upgrade() {
        app.set_status(message.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(width: u32, height: u32) -> Vec<u8> {
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                rgba.extend_from_slice(&[(x * 40) as u8, (y * 40) as u8, 128, 255]);
            }
        }
        rgba
    }

    #[test]
    fn rejects_malformed_image_data() {
        assert!(publish(2, 2, vec![0; 3]).is_err());
        assert!(publish(0, 0, Vec::new()).is_err());
    }

    /// Round-trip through the OS clipboard. Headless sessions (CI, no desktop)
    /// have no clipboard at all, so an unavailable clipboard is skipped rather
    /// than failed.
    ///
    /// The write is also the only cross-process check we have: with
    /// `WUNDER_CLIPBOARD_PROBE_HOLD_MS=8000` the owner stays alive long enough
    /// for another program (`xclip -o -t image/png`, an image editor, a chat
    /// client) to paste it while the test waits.
    #[test]
    fn publishes_an_image_other_programs_can_read() {
        let (width, height) = (4, 3);
        let rgba = sample(width, height);
        if let Err(error) = publish(width, height, rgba) {
            eprintln!("跳过剪贴板写入检查：{}", error);
            return;
        }
        let read = arboard::Clipboard::new()
            .and_then(|mut clipboard| clipboard.get_image())
            .expect("published image must be readable back");
        assert_eq!((read.width, read.height), (width as usize, height as usize));
        assert_eq!(read.bytes.len(), (width * height * 4) as usize);
        let hold = std::env::var("WUNDER_CLIPBOARD_PROBE_HOLD_MS")
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(0);
        if hold > 0 {
            std::thread::sleep(Duration::from_millis(hold));
        }
    }
}
