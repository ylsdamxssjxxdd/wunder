//! UI language: the one place that flips `I18n.zh` from the runtime language.
//!
//! The shell derives `ui-zh` from `runtime-language` (`zh-CN` / `en-US`) and
//! reports it through `ui-language-changed`; Slint components cannot assign to a
//! global, so the assignment happens here. 桌面版 and 安卓版 share this module
//! (安卓版 feeds its own `settings-language` into the same callback), which keeps
//! the two shells on one rule: any language string starting with `en` renders
//! English, everything else renders Chinese.

use crate::{I18n, MainWindow};
use slint::ComponentHandle;

/// Install the callback. Called by both the native shell and the in-memory
/// preview, so a preview never disagrees with the shipped window.
pub fn install(app: &MainWindow) {
    let weak = app.as_weak();
    app.on_ui_language_changed(move |zh| {
        let Some(app) = weak.upgrade() else { return };
        apply_zh(&app, zh);
    });
}

/// Push the derived flag into `I18n.zh` once. `apply_settings` calls this after
/// projecting `runtime-language`, so a settings save is effective immediately
/// even if the binding's `changed` notification is coalesced.
pub fn apply(app: &MainWindow) {
    let zh = app.get_ui_zh();
    apply_zh(app, zh);
}

fn apply_zh(app: &MainWindow, zh: bool) {
    let i18n = app.global::<I18n>();
    if i18n.get_zh() == zh {
        return;
    }
    i18n.set_zh(zh);
}

#[cfg(test)]
mod tests {
    /// The rule the shell binding encodes: only an `en*` language is English.
    fn zh_for(language: &str) -> bool {
        !language.starts_with("en")
    }

    #[test]
    fn english_only_for_en_prefix() {
        assert!(zh_for("zh-CN"));
        assert!(!zh_for("en-US"));
        assert!(!zh_for("en"));
        // An empty value (fresh settings before the runtime answers) stays on the
        // shipped default instead of flashing an English shell.
        assert!(zh_for(""));
    }
}

pub(crate) mod probe {
    //! Opt-in screenshot probe: `WUNDER_LANG_PROBE=<dir> wunder-desktop --demo`
    //! renders the shell once with the runtime language forced to `en-US`, so the
    //! English projection can be reviewed without a runtime or a settings round
    //! trip. Exits the event loop after writing the PNG.
    use super::*;
    use slint::ComponentHandle;

    pub fn run(app: &MainWindow) -> bool {
        let Some(directory) = std::env::var_os("WUNDER_LANG_PROBE").map(std::path::PathBuf::from)
        else {
            return false;
        };
        let language =
            std::env::var("WUNDER_LANG_PROBE_LANGUAGE").unwrap_or_else(|_| "en-US".into());
        app.set_runtime_language(language.into());
        apply(app);
        let weak = app.as_weak();
        slint::Timer::single_shot(std::time::Duration::from_millis(400), move || {
            if let Some(app) = weak.upgrade() {
                let _ = std::fs::create_dir_all(&directory);
                let _ = crate::smoke::snapshot(&app, &directory.join("ui-language.png"));
            }
            let _ = slint::quit_event_loop();
        });
        true
    }
}
