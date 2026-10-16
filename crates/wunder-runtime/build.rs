use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let root = manifest.parent().unwrap().parent().unwrap().to_path_buf();
    let mut files = Vec::new();
    collect(&root.join("config"), &root, &mut files);
    let runner = root.join("scripts/skill_runner.py");
    if runner.is_file() {
        files.push(("scripts/skill_runner.py".to_string(), runner));
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let generated = out.join("embedded_local_assets.rs");
    let mut source = String::from("pub static EMBEDDED_LOCAL_ASSETS: &[(&str, &[u8])] = &[\n");
    for (relative, path) in &files {
        let include_path = path.to_string_lossy().replace('\\', "/");
        let target = if relative == "config/wunder-example.yaml" {
            "config/wunder.yaml"
        } else {
            relative.as_str()
        };
        source.push_str(&format!(
            "    ({:?}, include_bytes!(r#\"{}\"#)),\n",
            target, include_path
        ));
        println!("cargo:rerun-if-changed={}", path.display());
    }
    source.push_str("];\n");
    fs::write(generated, source).expect("write embedded local assets manifest");
    println!("cargo:rerun-if-changed={}", root.join("config").display());
}

fn collect(path: &Path, root: &Path, files: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // Do not follow links outside the release source tree, or package
        // generated caches and hidden deployment files from approved folders.
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if kind.is_symlink()
            || name.starts_with('.')
            || matches!(name.as_ref(), "__pycache__" | "node_modules" | "target")
        {
            continue;
        }
        if matches!(
            path.extension().and_then(|s| s.to_str()),
            Some("pyc" | "db" | "sqlite3" | "lock" | "log")
        ) {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        // Only deterministic, non-secret local-form assets are embedded. The
        // checked-out wunder.yaml/mcp files may contain deployment credentials;
        // the example YAML is released as the local seed under its canonical
        // target name. Databases, caches, fonts and generated data stay out of
        // both binaries.
        if path.is_dir() {
            let can_contain_assets = relative == "config"
                || relative.starts_with("config/prompts")
                || relative.starts_with("config/skills")
                || relative.starts_with("config/preset_worker_cards")
                || relative.starts_with("config/preset_worker_cards_example");
            if can_contain_assets {
                collect(&path, root, files);
            }
            continue;
        }
        let allowed = relative == "config/wunder-example.yaml"
            || relative == "config/i18n.messages.json"
            || relative.starts_with("config/prompts/")
            || relative.starts_with("config/skills/")
            || relative.starts_with("config/preset_worker_cards/")
            || relative.starts_with("config/preset_worker_cards_example/")
            || relative == "scripts/skill_runner.py";
        if !allowed {
            continue;
        }
        if path.is_file() {
            files.push((relative, path));
        }
    }
}
