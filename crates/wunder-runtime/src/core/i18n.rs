use std::collections::HashMap;
use std::future::Future;

use tokio::task_local;

pub use wunder_core::i18n::{
    configure_i18n, get_default_language, get_known_prefixes, get_language_aliases,
    get_supported_languages, normalize_language, resolve_language, t_in_language,
    t_with_params_in_language,
};

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

/// 输出 i18n 配置，供接口使用。
pub fn t(key: &str) -> String {
    t_with_params(key, &HashMap::new())
}

/// 翻译指定 key，并按占位符替换参数。
pub fn t_with_params(key: &str, params: &HashMap<String, String>) -> String {
    wunder_core::i18n::t_with_params_in_language(key, params, &get_language())
}
