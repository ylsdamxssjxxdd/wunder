use std::collections::HashMap;
use std::future::Future;

use tokio::task_local;

pub use wunder_core::i18n::{
    configure_i18n, get_default_language, get_known_prefixes, get_language_aliases,
    get_supported_languages, normalize_language, resolve_language, t_in_language,
    t_with_params_in_language,
};

/// 面向模型的文本语言。
///
/// 系统提示词、工具定义与工具结果是模型读的内容，只维护英文一套：
/// 用户界面语言（[`get_language`]）仍可切换中英，但不影响这里。
pub const PROMPT_LANGUAGE: &str = "en-US";

task_local! {
    static CURRENT_LANGUAGE: String;
}

/// 在当前任务上下文中设置语言，返回可等待的执行结果。
pub async fn with_language<F, R>(language: String, fut: F) -> R
where
    F: Future<Output = R>,
{
    CURRENT_LANGUAGE.scope(language, fut).await
}

/// 获取当前上下文语言，未设置时回退默认值。
pub fn get_language() -> String {
    if let Ok(value) = CURRENT_LANGUAGE.try_with(Clone::clone) {
        return value;
    }
    get_default_language()
}

/// 面向模型的文本语言，恒为 [`PROMPT_LANGUAGE`]。
pub fn prompt_language() -> &'static str {
    PROMPT_LANGUAGE
}

/// 输出 i18n 配置，供接口使用。
pub fn t(key: &str) -> String {
    t_with_params(key, &HashMap::new())
}

/// 翻译指定 key，并按占位符替换参数。
pub fn t_with_params(key: &str, params: &HashMap<String, String>) -> String {
    wunder_core::i18n::t_with_params_in_language(key, params, &get_language())
}

/// 翻译面向模型的 key，恒用英文。
pub fn prompt_t(key: &str) -> String {
    t_in_language(key, PROMPT_LANGUAGE)
}

/// 翻译面向模型的 key 并替换参数，恒用英文。
pub fn prompt_t_with_params(key: &str, params: &HashMap<String, String>) -> String {
    t_with_params_in_language(key, params, PROMPT_LANGUAGE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_text_ignores_ui_language() {
        assert_eq!(prompt_language(), "en-US");
        assert_eq!(
            prompt_t("tool.plan.invalid_shape"),
            t_in_language("tool.plan.invalid_shape", "en-US")
        );
        let params = HashMap::from([("max_depth".to_string(), "5".to_string())]);
        assert_eq!(
            prompt_t_with_params("tool.a2a.depth_exceeded", &params),
            t_with_params_in_language("tool.a2a.depth_exceeded", &params, "en-US")
        );
    }
}
