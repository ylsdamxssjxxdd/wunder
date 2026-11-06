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
    // Deterministic digest of every projected asset (path + bytes). Startup can
    // compare this single value against a home anchor and skip the per-file
    // pass entirely when the build and the released tree agree.
    let mut asset_set_id = Fnv128::new();
    for (relative, path) in &files {
        let include_path = path.to_string_lossy().replace('\\', "/");
        // The example YAML serves two roles: the create-if-absent seed for a new
        // home, and the versioned default template used to backfill missing keys
        // in an existing user config. Both targets embed the same source bytes.
        let mut targets: Vec<String> = Vec::new();
        if relative == "config/wunder-example.yaml" {
            targets.push("config/wunder.yaml".to_string());
            targets.push("config/wunder-example.yaml".to_string());
        } else if let Some(companion) = relative.strip_prefix("config/companions/") {
            // Source packages seed the writable companion library consumed by
            // WUNDER_COMPANIONS_ROOT on the first launch.
            targets.push(format!("config/data/companions/global/{companion}"));
        } else {
            targets.push(relative.clone());
        }
        let bytes = fs::read(path).unwrap_or_default();
        for target in targets {
            source.push_str(&format!(
                "    ({:?}, include_bytes!(r#\"{}\"#)),\n",
                target, include_path
            ));
            asset_set_id.write(target.as_bytes());
            asset_set_id.write(&[0u8]);
            asset_set_id.write(&(bytes.len() as u64).to_le_bytes());
            asset_set_id.write(&bytes);
        }
        println!("cargo:rerun-if-changed={}", path.display());
    }
    source.push_str("];\n");
    source.push_str(&format!(
        "pub const EMBEDDED_ASSET_SET_ID: &str = {:?};\n",
        asset_set_id.hex()
    ));
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
                || relative.starts_with("config/preset_worker_cards_example")
                || relative.starts_with("config/companions");
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
            || relative.starts_with("config/companions/")
            || relative == "scripts/skill_runner.py";
        if !allowed {
            continue;
        }
        if path.is_file() {
            files.push((relative, path));
        }
    }
}

/// FNV-1a 128-bit. Only needs to be deterministic across builds, which is why
/// the standard library's randomly seeded hasher is not used here.
struct Fnv128(u128);

impl Fnv128 {
    const OFFSET_BASIS: u128 = 0x6c62272e07bb014262b821756295c58d;
    const PRIME: u128 = 0x0000000001000000000000000000013b;

    fn new() -> Self {
        Self(Self::OFFSET_BASIS)
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= *byte as u128;
            self.0 = self.0.wrapping_mul(Self::PRIME);
        }
    }

    fn hex(&self) -> String {
        format!("{:032x}", self.0)
    }
}
