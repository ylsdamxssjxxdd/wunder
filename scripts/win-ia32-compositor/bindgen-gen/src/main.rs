// 为 ffmpeg-sys 生成 32 位 Windows 的 bindings。
// Callbacks 实现来自 remotion-dev/rust-ffmpeg-sys 的 build.rs（原本用于官方生成流程）。
use bindgen::callbacks::{
    EnumVariantCustomBehavior, EnumVariantValue, IntKind, MacroParsingBehavior, ParseCallbacks,
};
#[derive(Debug)]
struct Callbacks;

impl ParseCallbacks for Callbacks {
    fn int_macro(&self, _name: &str, value: i64) -> Option<IntKind> {
        let ch_layout_prefix = "AV_CH_";
        let codec_cap_prefix = "AV_CODEC_CAP_";
        let codec_flag_prefix = "AV_CODEC_FLAG_";
        let error_max_size = "AV_ERROR_MAX_STRING_SIZE";

        if _name.starts_with(ch_layout_prefix) {
            Some(IntKind::ULongLong)
        } else if value >= i32::MIN as i64
            && value <= i32::MAX as i64
            && (_name.starts_with(codec_cap_prefix) || _name.starts_with(codec_flag_prefix))
        {
            Some(IntKind::UInt)
        } else if _name == error_max_size {
            Some(IntKind::Custom {
                name: "usize",
                is_signed: false,
            })
        } else if value >= i32::MIN as i64 && value <= i32::MAX as i64 {
            Some(IntKind::Int)
        } else {
            None
        }
    }

    fn enum_variant_behavior(
        &self,
        _enum_name: Option<&str>,
        original_variant_name: &str,
        _variant_value: EnumVariantValue,
    ) -> Option<EnumVariantCustomBehavior> {
        let dummy_codec_id_prefix = "AV_CODEC_ID_FIRST_";
        if original_variant_name.starts_with(dummy_codec_id_prefix) {
            Some(EnumVariantCustomBehavior::Constify)
        } else {
            None
        }
    }

    // https://github.com/rust-lang/rust-bindgen/issues/687#issuecomment-388277405
    fn will_parse_macro(&self, name: &str) -> MacroParsingBehavior {
        use MacroParsingBehavior::*;

        match name {
            "FP_INFINITE" => Ignore,
            "FP_NAN" => Ignore,
            "FP_NORMAL" => Ignore,
            "FP_SUBNORMAL" => Ignore,
            "FP_ZERO" => Ignore,
            _ => Default,
        }
    }
}


fn main() {
    let args: Vec<String> = std::env::args().collect();
    let header = &args[1];
    let out = &args[2];
    let mut builder = bindgen::Builder::default()
        .header(header)
        .parse_callbacks(Box::new(Callbacks))
        .ctypes_prefix("libc")
        .default_enum_style(bindgen::EnumVariation::Rust { non_exhaustive: false })
        .layout_tests(false)
        .derive_partialeq(true)
        .blocklist_item(".*_is_defined");
    // 需要迁就老工具链时（< 1.82 不认识 unsafe extern）设置 BINDGEN_RUST_MINOR，
    // 例如 BINDGEN_RUST_MINOR=77 生成 1.77 能吃的语法。
    if let Ok(minor) = std::env::var("BINDGEN_RUST_MINOR") {
        if let Ok(minor) = minor.parse::<u64>() {
            match bindgen::RustTarget::stable(1, minor) {
                Ok(target) => builder = builder.rust_target(target),
                Err(_) => panic!("unsupported BINDGEN_RUST_MINOR={}", minor),
            }
        }
    }
    if let Ok(target) = std::env::var("BINDGEN_TARGET") {
        builder = builder.clang_arg(format!("--target={}", target));
    }
    // BINDGEN_EXTRA：额外 clang 参数（空白分隔）。mingw 的 malloc.h 会引用只有
    // gcc 才带的 mm_malloc.h，clang 不搜 gcc 安装树，需调用方挂上 gcc 的 include。
    if let Ok(extra) = std::env::var("BINDGEN_EXTRA") {
        for arg in extra.split_whitespace() {
            builder = builder.clang_arg(arg);
        }
    }
    for inc in &args[3..] {
        builder = builder.clang_arg(format!("-I{}", inc));
    }
    let bindings = builder.generate().expect("bindgen failed");
    bindings.write_to_file(out).expect("write failed");
    println!("wrote {}", out);
}

