//! 图像生成工具的参数契约。
//!
//! 这里集中三件事，目的是让调用方不必靠试错去猜：
//!
//! 1. **稳定约束写成规则**：尺寸格式、对齐倍数、短边下限、上限、蒙版尺寸、
//!    `seed` / `strength` 取值范围，都在调用上游之前本地校验；
//! 2. **工具自己的改写必须可见**：被等比缩小、被补扩展名、蒙版全黑、参数已弃用
//!    一律进 `warnings`，不再 200 OK 静默发生；
//! 3. **失败统一成三段式**：`message` 说谁错了（哪个参数/哪个上游）+ 为什么，
//!    `hint` 说怎么改，两者不重复；同时给出机械可读的 `code` / `retryable`。

use crate::config::LlmModelConfig;
use crate::llm::normalize_provider;
use serde_json::Value;
use std::io::Cursor;

/// vllm-omni / Z-Image 一类服务要求宽高都被该单位整除。
pub(crate) const IMAGE_ALIGNMENT_UNIT: u32 = 16;
/// 对齐型模型能接受的最短边；小于它的请求上游必然失败，本地提前拦下。
pub(crate) const IMAGE_MIN_SHORT_EDGE: u32 = 32;
/// 未配置默认尺寸时的兜底值，同时也是未配置上限时的有效上限。
pub(crate) const IMAGE_DEFAULT_SIZE: &str = "1024x1024";
/// `seed` 上限：主流图像服务都按 32 位无符号整型接收。
pub(crate) const IMAGE_MAX_SEED: u64 = u32::MAX as u64;
/// 蒙版全黑/全白判定需要解码像素；超过该像素数只做尺寸校验，避免为大图付出解码成本。
const MASK_SCAN_PIXEL_LIMIT: u64 = 16_000_000;

const VLLM_OMNI_PROVIDER: &str = "vllm_omni";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageFormat {
    Png,
    Jpeg,
    Webp,
}

impl ImageFormat {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpeg",
            Self::Webp => "webp",
        }
    }

    pub(crate) fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
        }
    }

    pub(crate) fn from_extension(raw: &str) -> Option<Self> {
        match raw
            .trim()
            .trim_start_matches('.')
            .to_ascii_lowercase()
            .as_str()
        {
            "png" => Some(Self::Png),
            "jpg" | "jpeg" => Some(Self::Jpeg),
            "webp" => Some(Self::Webp),
            _ => None,
        }
    }

    pub(crate) fn from_content_type(content_type: &str) -> Option<Self> {
        match content_type
            .split(';')
            .next()
            .map(str::trim)
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "image/png" => Some(Self::Png),
            "image/jpeg" | "image/jpg" => Some(Self::Jpeg),
            "image/webp" => Some(Self::Webp),
            _ => None,
        }
    }
}

/// 当前图像模型对尺寸的硬约束。
#[derive(Debug, Clone, Copy)]
pub(crate) struct ImageLimits {
    pub(crate) default_size: (u32, u32),
    pub(crate) max_size: (u32, u32),
    /// 上限来自模型配置还是默认尺寸兜底；报错时要说清是哪一种。
    pub(crate) max_configured: bool,
    /// `Some(16)` 表示该提供方要求宽高按 16 对齐（vllm-omni 一类）。
    pub(crate) alignment: Option<u32>,
    /// 该提供方能接受的最短边；无对齐要求时不做本地限制。
    pub(crate) min_short_edge: u32,
}

impl ImageLimits {
    pub(crate) fn for_model(config: &LlmModelConfig) -> Self {
        let default_size = parse_size(
            config
                .image_default_size
                .as_deref()
                .unwrap_or(IMAGE_DEFAULT_SIZE),
        )
        .unwrap_or((1024, 1024));
        let configured_max = config.image_max_size.as_deref().and_then(parse_size);
        let max_size = configured_max.unwrap_or(default_size);
        let alignment = (normalize_provider(config.provider.as_deref()) == VLLM_OMNI_PROVIDER)
            .then_some(IMAGE_ALIGNMENT_UNIT);
        Self {
            default_size,
            max_size,
            max_configured: configured_max.is_some(),
            alignment,
            min_short_edge: if alignment.is_some() {
                IMAGE_MIN_SHORT_EDGE
            } else {
                1
            },
        }
    }

    pub(crate) fn max_label(&self) -> String {
        format!("{}x{}", self.max_size.0, self.max_size.1)
    }

    /// 供工具回执与错误提示复用的一段人话约束。
    pub(crate) fn describe(&self) -> String {
        let mut text = format!(
            "默认尺寸 {}x{}，尺寸上限 {}",
            self.default_size.0,
            self.default_size.1,
            self.max_label()
        );
        if !self.max_configured {
            text.push_str("（模型未配置 image_max_size，按默认尺寸兜底）");
        }
        if let Some(unit) = self.alignment {
            text.push_str(&format!(
                "，宽高须为 {unit} 的倍数，短边不小于 {}",
                self.min_short_edge
            ));
        }
        text
    }
}

/// 统一的图像工具失败：`message` 说「谁错了 + 为什么」，`hint` 说「怎么改」。
#[derive(Debug, Clone)]
pub(crate) struct ImageFailure {
    pub(crate) code: &'static str,
    pub(crate) retryable: bool,
    pub(crate) message: String,
    pub(crate) hint: String,
    pub(crate) suggested_size: Option<String>,
    pub(crate) field: Option<String>,
}

impl ImageFailure {
    pub(crate) fn new(
        code: &'static str,
        retryable: bool,
        message: impl Into<String>,
        hint: impl Into<String>,
    ) -> Self {
        Self {
            code,
            retryable,
            message: message.into(),
            hint: hint.into(),
            suggested_size: None,
            field: None,
        }
    }

    /// 参数本身不合法：调用方改参数就能继续。
    pub(crate) fn request(field: &str, reason: impl Into<String>, fix: impl Into<String>) -> Self {
        Self {
            code: "IMAGE_REQUEST_INVALID",
            retryable: false,
            // message 只说“谁错了 + 为什么”；“怎么改”只放在 hint。
            // 早前把 fix 同时拼进 message 和 hint，工具回执会把它显示两遍。
            message: format!("参数 `{}` 不合法：{}", field, reason.into()),
            hint: fix.into(),
            suggested_size: None,
            field: Some(field.to_string()),
        }
    }

    /// 输入文件（主图 / 蒙版 / 参考图）不合法：文件问题，不是模型参数问题。
    pub(crate) fn input(field: &str, reason: impl Into<String>, fix: impl Into<String>) -> Self {
        Self {
            code: "IMAGE_INPUT_INVALID",
            retryable: false,
            message: format!("输入图 `{}` 有问题：{}", field, reason.into()),
            hint: fix.into(),
            suggested_size: None,
            field: Some(field.to_string()),
        }
    }

    pub(crate) fn with_suggested_size(mut self, size: Option<String>) -> Self {
        self.suggested_size = size;
        self
    }
}

/// 尺寸决策结果：请求值、真正下发的值，以及这个值从哪来。
#[derive(Debug, Clone)]
pub(crate) struct SizeDecision {
    pub(crate) requested: Option<String>,
    pub(crate) effective: Option<String>,
    /// `requested` / `default` / `input_image` / `scaled`
    pub(crate) source: &'static str,
    pub(crate) warnings: Vec<String>,
}

pub(crate) fn parse_size(raw: &str) -> Option<(u32, u32)> {
    let cleaned = raw
        .trim()
        .to_ascii_lowercase()
        .replace(['*', '＊'], "x")
        .replace('×', "x");
    let (width, height) = cleaned.split_once('x')?;
    let width = width.trim().parse::<u32>().ok()?;
    let height = height.trim().parse::<u32>().ok()?;
    (width > 0 && height > 0).then_some((width, height))
}

/// 校验并决定真正下发的尺寸。
///
/// - 未传尺寸：文生图用模型默认尺寸，编辑保持原图（`auto`）；
/// - 显式尺寸先按稳定规则校验（对齐、短边），再按上限等比缩小并写进 warnings；
/// - 返回的 `effective` 就是最终写进请求的尺寸，调用方不必再猜。
pub(crate) fn decide_size(
    raw: Option<&str>,
    limits: &ImageLimits,
    has_input: bool,
) -> Result<SizeDecision, ImageFailure> {
    let requested = raw
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let Some(requested_text) = requested.clone() else {
        if has_input {
            return Ok(SizeDecision {
                requested: None,
                effective: None,
                source: "input_image",
                warnings: Vec::new(),
            });
        }
        let (width, height) = limits.default_size;
        return Ok(SizeDecision {
            requested: None,
            effective: Some(format!("{}x{}", width, height)),
            source: "default",
            warnings: Vec::new(),
        });
    };

    if requested_text.eq_ignore_ascii_case("auto") {
        if has_input {
            return Ok(SizeDecision {
                requested: Some(requested_text),
                effective: None,
                source: "input_image",
                warnings: Vec::new(),
            });
        }
        return Err(ImageFailure::request(
            "size",
            "文生图没有输入图可以继承尺寸，`auto` 无法解析",
            "删掉 size，或写成明确的宽高，例如 `1024x1024`。",
        ));
    }

    let (width, height) = parse_size(&requested_text).ok_or_else(|| {
        ImageFailure::request(
            "size",
            format!("无法识别 `{}`，格式必须是「宽x高」的整数对", requested_text),
            "改用 `宽x高` 写法，例如 `1024x1024`、`1344x768`。",
        )
    })?;
    validate_size_rule(width, height, limits)?;

    let (max_width, max_height) = limits.max_size;
    if width <= max_width && height <= max_height {
        return Ok(SizeDecision {
            requested: Some(requested_text),
            effective: Some(format!("{}x{}", width, height)),
            source: "requested",
            warnings: Vec::new(),
        });
    }

    let (scaled_width, scaled_height) = fit_within(width, height, max_width, max_height);
    let (scaled_width, scaled_height) = align_down(scaled_width, scaled_height, limits);
    if scaled_width < limits.min_short_edge || scaled_height < limits.min_short_edge {
        return Err(ImageFailure::request(
            "size",
            format!(
                "{}x{} 超出上限 {}x{}，等比缩小后短边会低于 {}",
                width, height, max_width, max_height, limits.min_short_edge
            ),
            format!(
                "改用不超过 {}x{} 的尺寸，例如 `{}`。",
                max_width,
                max_height,
                suggest_aligned_size(width.min(max_width), height.min(max_height), limits)
            ),
        ));
    }
    Ok(SizeDecision {
        requested: Some(requested_text),
        effective: Some(format!("{}x{}", scaled_width, scaled_height)),
        source: "scaled",
        warnings: vec![format!(
            "size 由 `{}x{}` 等比缩小为 `{}x{}`：超过当前模型上限 `{}x{}`（{}）。需要更大请先调高该模型的 image_max_size。",
            width,
            height,
            scaled_width,
            scaled_height,
            max_width,
            max_height,
            if limits.max_configured {
                "来自模型配置"
            } else {
                "未配置 image_max_size，按默认尺寸兜底"
            }
        )],
    })
}

fn validate_size_rule(width: u32, height: u32, limits: &ImageLimits) -> Result<(), ImageFailure> {
    if let Some(unit) = limits.alignment {
        if width % unit != 0 || height % unit != 0 {
            let suggestion = suggest_aligned_size(width, height, limits);
            return Err(ImageFailure {
                code: "IMAGE_SIZE_ALIGNMENT_INVALID",
                retryable: false,
                message: format!(
                    "参数 `size` 不合法：{}x{} 的宽高都必须能被 {} 整除（当前宽 {}、高 {}）。改用 `{}`，例如 `256x256`、`1024x1024` 或 `1344x768`。",
                    width, height, unit, width, height, suggestion
                ),
                hint: format!(
                    "改用 `{}` 这类宽高都能被 {} 整除的尺寸。",
                    suggestion, unit
                ),
                suggested_size: Some(suggestion),
                field: Some("size".to_string()),
            });
        }
    }
    if width < limits.min_short_edge || height < limits.min_short_edge {
        return Err(ImageFailure::request(
            "size",
            format!(
                "{}x{} 的短边小于 {}，该模型不接受更小的图",
                width, height, limits.min_short_edge
            ),
            format!(
                "改用短边不小于 {} 的尺寸，例如 `{}`。",
                limits.min_short_edge,
                suggest_aligned_size(
                    width.max(limits.min_short_edge),
                    height.max(limits.min_short_edge),
                    limits
                )
            ),
        ));
    }
    Ok(())
}

/// 优先向大对齐；对齐后会超上限时向小对齐，保证建议值一定可用。
pub(crate) fn suggest_aligned_size(width: u32, height: u32, limits: &ImageLimits) -> String {
    let (max_width, max_height) = limits.max_size;
    let (width, height) = (
        width.clamp(limits.min_short_edge.max(1), max_width),
        height.clamp(limits.min_short_edge.max(1), max_height),
    );
    let aligned_width = align_toward(width, limits.alignment, max_width, limits.min_short_edge);
    let aligned_height = align_toward(height, limits.alignment, max_height, limits.min_short_edge);
    format!("{}x{}", aligned_width, aligned_height)
}

fn align_toward(value: u32, unit: Option<u32>, max: u32, min: u32) -> u32 {
    let Some(unit) = unit else {
        return value;
    };
    let up = value.div_ceil(unit) * unit;
    if up <= max {
        return up.max(min);
    }
    (value / unit * unit).max(min)
}

fn align_down(width: u32, height: u32, limits: &ImageLimits) -> (u32, u32) {
    let Some(unit) = limits.alignment else {
        return (width, height);
    };
    (
        (width / unit * unit).max(unit),
        (height / unit * unit).max(unit),
    )
}

fn fit_within(width: u32, height: u32, max_width: u32, max_height: u32) -> (u32, u32) {
    if width <= max_width && height <= max_height {
        return (width, height);
    }
    let ratio =
        (max_width as f64 / width.max(1) as f64).min(max_height as f64 / height.max(1) as f64);
    (
        ((width as f64 * ratio).round() as u32).clamp(1, max_width),
        ((height as f64 * ratio).round() as u32).clamp(1, max_height),
    )
}

/// 输出格式：`output_format` 与 `path` 后缀必须一致，冲突直接报错而不是任选其一。
pub(crate) fn resolve_output_format(
    explicit: Option<&str>,
    path: Option<&str>,
) -> Result<ImageFormat, ImageFailure> {
    let explicit = explicit
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            ImageFormat::from_extension(value).ok_or_else(|| {
                ImageFailure::request(
                    "output_format",
                    format!("不支持 `{}`", value),
                    "只支持 `png`、`jpeg`、`webp`；留空则由 path 后缀决定，默认 png。",
                )
            })
        })
        .transpose()?;
    let path_format = path
        .and_then(|value| {
            std::path::Path::new(value.trim())
                .extension()
                .and_then(|value| value.to_str())
                .map(str::to_string)
        })
        .map(|ext| {
            ImageFormat::from_extension(&ext).ok_or_else(|| {
                ImageFailure::request(
                    "path",
                    format!("后缀 `.{ext}` 不是本工具能写出的格式"),
                    "把 path 改成 `.png`、`.jpg` 或 `.webp`，或删掉后缀让工具按 output_format 补全。",
                )
            })
        })
        .transpose()?;
    match (explicit, path_format) {
        (Some(explicit), Some(path_format)) if explicit != path_format => {
            Err(ImageFailure::request(
                "output_format",
                format!(
                    "与 path 后缀冲突：output_format 是 `{}`，path 用的是 `{}`",
                    explicit.as_str(),
                    path_format.as_str()
                ),
                format!(
                    "两者只留一个：删掉 output_format，或把 path 后缀改成 `{}`。",
                    explicit.extension()
                ),
            ))
        }
        (Some(explicit), _) => Ok(explicit),
        (None, Some(path_format)) => Ok(path_format),
        (None, None) => Ok(ImageFormat::Png),
    }
}

/// `seed` 接受非负整数与纯数字字符串；浮点、负数、超大值都给出可操作的说明。
pub(crate) fn parse_seed(raw: Option<&Value>) -> Result<Option<u64>, ImageFailure> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let fix = format!(
        "改用 0 到 {IMAGE_MAX_SEED} 之间的整数；只有需要复现同一张图时才固定 seed，正常生成可以省略（工具会自动生成并回显）。"
    );
    match raw {
        Value::Null => Ok(None),
        Value::Number(number) => {
            if let Some(value) = number.as_u64() {
                if value > IMAGE_MAX_SEED {
                    return Err(ImageFailure::request(
                        "seed",
                        format!("{} 超出上限 {}", value, IMAGE_MAX_SEED),
                        fix,
                    ));
                }
                return Ok(Some(value));
            }
            Err(ImageFailure::request(
                "seed",
                format!("需要整数，收到 {number}"),
                fix,
            ))
        }
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            match trimmed.parse::<u64>() {
                Ok(value) if value <= IMAGE_MAX_SEED => Ok(Some(value)),
                Ok(value) => Err(ImageFailure::request(
                    "seed",
                    format!("{} 超出上限 {}", value, IMAGE_MAX_SEED),
                    fix,
                )),
                Err(_) => Err(ImageFailure::request(
                    "seed",
                    format!(
                        "需要 0 到 {} 之间的整数，收到 `{}`",
                        IMAGE_MAX_SEED, trimmed
                    ),
                    fix,
                )),
            }
        }
        other => Err(ImageFailure::request(
            "seed",
            format!(
                "需要 0 到 {IMAGE_MAX_SEED} 之间的整数，收到 {}",
                json_type_name(other)
            ),
            fix,
        )),
    }
}

/// `strength` 只对编辑生效，取值范围 0..=1；越界直接报错，不给假旋钮。
pub(crate) fn parse_strength(raw: Option<&Value>) -> Result<Option<f32>, ImageFailure> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let value = match raw {
        Value::Null => return Ok(None),
        Value::Number(number) => number.as_f64().map(|value| value as f32),
        Value::String(text) if text.trim().is_empty() => return Ok(None),
        Value::String(text) => text.trim().parse::<f32>().ok(),
        _ => None,
    };
    let fix = "改用 0 到 1 之间的数值：0.2~0.4 只做轻微修改，0.6~0.9 改动更明显。";
    let Some(value) = value.filter(|value| value.is_finite()) else {
        return Err(ImageFailure::request(
            "strength",
            format!("需要 0 到 1 之间的数值，收到 {}", json_type_name(raw)),
            fix,
        ));
    };
    if !(0.0..=1.0).contains(&value) {
        return Err(ImageFailure::request(
            "strength",
            format!("必须在 0 到 1 之间，收到 {value}"),
            fix,
        ));
    }
    Ok(Some(value))
}

fn json_type_name(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(value) => format!("布尔值 {value}"),
        Value::Number(number) => number.to_string(),
        Value::String(text) => format!("字符串 `{}`", text),
        Value::Array(_) => "数组".to_string(),
        Value::Object(_) => "对象".to_string(),
    }
}

/// 收集输入图：`input_paths` 是唯一规范参数，`input_path` 是兼容别名。
///
/// 两个都给时报错而不是静默合并——顺序决定哪张是编辑主图，猜错了结果就错。
pub(crate) fn merge_input_paths(args: &Value) -> Result<(Vec<String>, Vec<String>), ImageFailure> {
    let single = args
        .get("input_path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let mut warnings = Vec::new();
    let list = match args.get("input_paths") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => {
            let mut paths = Vec::with_capacity(items.len());
            for item in items {
                let Some(text) = item.as_str() else {
                    return Err(ImageFailure::request(
                        "input_paths",
                        "数组元素必须是路径字符串",
                        "改成 `[\"a.png\", \"b.png\"]` 这样的字符串数组。",
                    ));
                };
                if let Some(cleaned) = Some(text.trim()).filter(|value| !value.is_empty()) {
                    paths.push(cleaned.to_string());
                }
            }
            if paths.is_empty() {
                // 空数组等价于没给输入图，但“填了空数组”和“根本没填”不是同一件事，
                // 静默按文生图处理会让调用方以为自己在改图。
                warnings.push(
                    "input_paths 是空数组（或全是空字符串），本次按文生图处理；要改图请给出至少一条路径。"
                        .to_string(),
                );
            }
            paths
        }
        Some(_) => {
            return Err(ImageFailure::request(
                "input_paths",
                "必须是字符串数组",
                "单图也写成数组：`input_paths: [\"a.png\"]`。",
            ))
        }
    };
    if single.is_some() && !list.is_empty() {
        return Err(ImageFailure::request(
            "input_paths",
            "同时给了 input_path 和 input_paths，无法判断以哪个为准",
            "只留 `input_paths`（单图也写成数组）；`input_path` 是旧参数，后续版本会移除。",
        ));
    }
    let mut paths = Vec::new();
    if let Some(single) = single {
        warnings.push(
            "input_path 已弃用，请改用 input_paths（单图写成数组）；本次仍按该图执行。".to_string(),
        );
        paths.push(single);
    }
    for path in list {
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    Ok((paths, warnings))
}

/// 蒙版像素统计：全黑/全白都是「没有局部可改」，必须在回执里说清楚。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MaskInfo {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) all_black: bool,
    pub(crate) all_white: bool,
    /// 像素过大时跳过扫描，此时全黑/全白恒为 false。
    pub(crate) scanned: bool,
}

pub(crate) fn inspect_mask(bytes: &[u8]) -> Option<MaskInfo> {
    let reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let (width, height) = reader.into_dimensions().ok()?;
    let mut info = MaskInfo {
        width,
        height,
        all_black: false,
        all_white: false,
        scanned: false,
    };
    if width as u64 * height as u64 > MASK_SCAN_PIXEL_LIMIT {
        return Some(info);
    }
    let image = image::load_from_memory(bytes).ok()?.to_luma8();
    let mut any_white = false;
    let mut any_black = false;
    for pixel in image.pixels() {
        if pixel[0] > 127 {
            any_white = true;
        } else {
            any_black = true;
        }
        if any_white && any_black {
            break;
        }
    }
    info.all_black = !any_white;
    info.all_white = !any_black;
    info.scanned = true;
    Some(info)
}

/// 只读图片头拿宽高，不解码整张图（用于校验蒙版是否与主图同尺寸）。
pub(crate) fn image_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn limits(provider: &str) -> ImageLimits {
        let mut config = LlmModelConfig {
            provider: Some(provider.to_string()),
            ..Default::default()
        };
        config.image_default_size = Some("1024x1024".to_string());
        config.image_max_size = Some("1440x1440".to_string());
        ImageLimits::for_model(&config)
    }

    #[test]
    fn size_format_is_normalized_and_validated() {
        assert_eq!(parse_size(" 1024 x 1024 "), Some((1024, 1024)));
        assert_eq!(parse_size("1024×768"), Some((1024, 768)));
        let failure = decide_size(Some("1024x1024.5"), &limits("vllm_omni"), false)
            .expect_err("malformed size must fail");
        assert_eq!(failure.code, "IMAGE_REQUEST_INVALID");
        assert!(failure.message.contains("宽x高"), "{}", failure.message);
    }

    #[test]
    fn alignment_and_short_edge_are_enforced_locally() {
        let limits = limits("vllm_omni");
        let failure = decide_size(Some("250x250"), &limits, false).expect_err("unaligned");
        assert_eq!(failure.code, "IMAGE_SIZE_ALIGNMENT_INVALID");
        assert_eq!(failure.suggested_size.as_deref(), Some("256x256"));
        assert!(failure.message.contains("16"), "{}", failure.message);

        let small = decide_size(Some("32x16"), &limits, false).expect_err("too small");
        assert_eq!(small.code, "IMAGE_REQUEST_INVALID");
        assert!(small.message.contains("短边"), "{}", small.message);
    }

    #[test]
    fn oversized_size_scales_down_with_a_warning() {
        let decision = decide_size(Some("4096x4096"), &limits("vllm_omni"), false).unwrap();
        assert_eq!(decision.effective.as_deref(), Some("1440x1440"));
        assert_eq!(decision.source, "scaled");
        assert_eq!(decision.warnings.len(), 1);
        assert!(decision.warnings[0].contains("1440x1440"));
    }

    #[test]
    fn edit_without_size_keeps_source_dimensions() {
        let decision = decide_size(None, &limits("vllm_omni"), true).unwrap();
        assert_eq!(decision.effective, None);
        assert_eq!(decision.source, "input_image");
        let text = decide_size(None, &limits("openai_compatible"), false).unwrap();
        assert_eq!(text.effective.as_deref(), Some("1024x1024"));
        assert_eq!(text.source, "default");
    }

    #[test]
    fn output_format_conflict_is_rejected() {
        let failure = resolve_output_format(Some("png"), Some("out.webp")).expect_err("conflict");
        assert_eq!(failure.code, "IMAGE_REQUEST_INVALID");
        assert!(failure.message.contains("webp"), "{}", failure.message);
        assert_eq!(
            resolve_output_format(None, Some("out.jpg")).unwrap(),
            ImageFormat::Jpeg
        );
        assert_eq!(resolve_output_format(None, None).unwrap(), ImageFormat::Png);
        assert!(resolve_output_format(Some("bmp"), None).is_err());
    }

    #[test]
    fn seed_errors_are_actionable() {
        let negative = parse_seed(Some(&json!(-1))).expect_err("negative seed");
        assert!(negative.message.contains("`seed`"), "{}", negative.message);
        assert!(!negative.message.contains("expected u64"));
        assert_eq!(parse_seed(Some(&json!(42))).unwrap(), Some(42));
        assert_eq!(parse_seed(Some(&json!("42"))).unwrap(), Some(42));
        assert_eq!(parse_seed(Some(&json!(null))).unwrap(), None);
        let float = parse_seed(Some(&json!(1.5))).expect_err("float seed");
        assert!(float.message.contains("整数"));
    }

    #[test]
    fn strength_out_of_range_is_rejected() {
        assert_eq!(parse_strength(Some(&json!(0.5))).unwrap(), Some(0.5));
        assert_eq!(parse_strength(Some(&json!(1))).unwrap(), Some(1.0));
        let failure = parse_strength(Some(&json!(1.5))).expect_err("out of range");
        assert_eq!(failure.code, "IMAGE_REQUEST_INVALID");
    }

    #[test]
    fn input_path_and_input_paths_conflict_instead_of_merging() {
        let failure = merge_input_paths(&json!({
            "input_path": "a.png",
            "input_paths": ["b.png", "c.png"]
        }))
        .expect_err("conflict");
        assert!(failure.message.contains("input_path"));
        let (paths, warnings) = merge_input_paths(&json!({ "input_path": "a.png" })).unwrap();
        assert_eq!(paths, vec!["a.png"]);
        assert_eq!(warnings.len(), 1);
        let (paths, _) =
            merge_input_paths(&json!({ "input_paths": ["b.png", "a.png", "b.png"] })).unwrap();
        assert_eq!(paths, vec!["b.png", "a.png"]);
    }

    #[test]
    fn empty_input_paths_array_is_reported_instead_of_silently_dropped() {
        let (paths, warnings) = merge_input_paths(&json!({ "input_paths": [] })).unwrap();
        assert!(paths.is_empty());
        assert_eq!(warnings.len(), 1, "空数组必须有一条说明");
        assert!(warnings[0].contains("空数组"), "{}", warnings[0]);
        // 全是空字符串等价于空数组，也要说。
        let (paths, warnings) =
            merge_input_paths(&json!({ "input_paths": ["  ", ""] })).unwrap();
        assert!(paths.is_empty());
        assert_eq!(warnings.len(), 1);
        // 完全没传则不提示，避免正常文生图也带噪音。
        let (_, warnings) = merge_input_paths(&json!({ "prompt": "a cat" })).unwrap();
        assert!(warnings.is_empty());
    }

    #[test]
    fn failure_message_never_repeats_the_hint() {
        // 回归：早前把 fix 同时拼进 message 和 hint，工具回执会把同一句显示两遍。
        let failure = parse_strength(Some(&json!(1.5))).expect_err("out of range");
        assert!(failure.message.contains("0 到 1"));
        assert!(failure.hint.contains("0 到 1"));
        assert!(
            !failure.message.contains(failure.hint.as_str()),
            "message 不应包含整句 hint：{}",
            failure.message
        );
        let failure = ImageFailure::request("path", "指向目录", "改成 `temp/out.png`。");
        assert_eq!(failure.message, "参数 `path` 不合法：指向目录");
        assert_eq!(failure.hint, "改成 `temp/out.png`。");
    }

    #[test]
    fn mask_inspection_detects_empty_edit_area() {
        let black = image::RgbImage::from_pixel(4, 4, image::Rgb([0, 0, 0]));
        let mut bytes = Vec::new();
        black
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        let info = inspect_mask(&bytes).expect("mask info");
        assert!(info.all_black && info.scanned);
        assert_eq!((info.width, info.height), (4, 4));
    }
}
