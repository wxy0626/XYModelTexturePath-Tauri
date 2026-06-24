#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use encoding_rs::GBK;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Serialize)]
struct ModelFile {
    path: String,
    name: String,
}

#[derive(Debug, Serialize, Clone)]
struct TextureChange {
    file: String,
    model: String,
    old_path: String,
    new_path: String,
    filtered: bool,
}

#[derive(Debug, Serialize)]
struct PreviewResult {
    changes: Vec<TextureChange>,
    warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ApplyResult {
    changed_files: usize,
    changes: Vec<TextureChange>,
    warnings: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Options {
    prefix: String,
    append_slash: bool,
    skip_builtin: bool,
    auto_backup: bool,
    trim_resource: bool,
    reset: bool,
}

struct FileResult {
    changed: bool,
    changes: Vec<TextureChange>,
    warnings: Vec<String>,
}

#[tauri::command]
fn collect_models(paths: Vec<String>) -> Result<Vec<ModelFile>, String> {
    let mut out = Vec::new();
    for raw in paths {
        let path = PathBuf::from(raw.trim_matches('"'));
        if path.is_dir() {
            for entry in WalkDir::new(&path).into_iter().filter_map(Result::ok) {
                let p = entry.path();
                if p.is_file() && is_model_file(p) {
                    out.push(model_file(p));
                }
            }
        } else if path.is_file() && is_model_file(&path) {
            out.push(model_file(&path));
        }
    }
    Ok(out)
}

#[tauri::command]
fn preview_paths(files: Vec<String>, opts: Options) -> Result<PreviewResult, String> {
    let mut changes = Vec::new();
    let mut warnings = Vec::new();
    for file in files {
        let result = process_file(Path::new(&file), &opts, false)?;
        changes.extend(result.changes);
        warnings.extend(result.warnings);
    }
    Ok(PreviewResult { changes, warnings })
}

#[tauri::command]
fn apply_paths(files: Vec<String>, opts: Options) -> Result<ApplyResult, String> {
    let mut changed_files = 0;
    let mut changes = Vec::new();
    let mut warnings = Vec::new();
    for file in files {
        let result = process_file(Path::new(&file), &opts, true)?;
        if result.changed {
            changed_files += 1;
        }
        changes.extend(result.changes);
        warnings.extend(result.warnings);
    }
    Ok(ApplyResult {
        changed_files,
        changes,
        warnings,
    })
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            collect_models,
            preview_paths,
            apply_paths
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn model_file(path: &Path) -> ModelFile {
    ModelFile {
        path: path.to_string_lossy().to_string(),
        name: path
            .file_name()
            .map(|item| item.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string()),
    }
}

fn is_model_file(path: &Path) -> bool {
    path.extension()
        .and_then(|item| item.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("mdx") || ext.eq_ignore_ascii_case("mdl"))
        .unwrap_or(false)
}

fn process_file(path: &Path, opts: &Options, write: bool) -> Result<FileResult, String> {
    let ext = path
        .extension()
        .and_then(|item| item.to_str())
        .unwrap_or_default();
    if ext.eq_ignore_ascii_case("mdx") {
        process_mdx(path, opts, write)
    } else {
        process_mdl(path, opts, write)
    }
}

fn process_mdx(path: &Path, opts: &Options, write: bool) -> Result<FileResult, String> {
    let mut bytes = fs::read(path).map_err(|err| err.to_string())?;
    let mut result = FileResult {
        changed: false,
        changes: Vec::new(),
        warnings: Vec::new(),
    };

    if bytes.len() < 12 || &bytes[0..4] != b"MDLX" {
        result
            .warnings
            .push(format!("{}：不是标准 MDX 文件", path.display()));
        return Ok(result);
    }

    let mut offset = 4usize;
    while offset + 8 <= bytes.len() {
        let chunk = &bytes[offset..offset + 4];
        let size = u32::from_le_bytes([
            bytes[offset + 4],
            bytes[offset + 5],
            bytes[offset + 6],
            bytes[offset + 7],
        ]) as usize;
        let start = offset + 8;
        let end = start.saturating_add(size);
        if end > bytes.len() {
            result
                .warnings
                .push(format!("{}：MDX 区块长度异常，已停止解析", path.display()));
            break;
        }

        if chunk == b"TEXS" {
            let entry_size = 268usize;
            let count = size / entry_size;
            for entry in 0..count {
                let path_offset = start + entry * entry_size + 4;
                let max_len = 260usize;
                let mut len = 0usize;
                while len < max_len && bytes[path_offset + len] != 0 {
                    len += 1;
                }
                if len == 0 {
                    continue;
                }

                let old_path = decode_text(&bytes[path_offset..path_offset + len]);
                if !is_texture_path(&old_path) {
                    continue;
                }
                if opts.skip_builtin
                    && is_builtin_texture(&old_path)
                    && !resource_rule_applies(path, opts)
                {
                    result
                        .changes
                        .push(texture_change(path, &old_path, &old_path, true));
                    continue;
                }

                let new_path = build_new_path(&old_path, path, opts);
                let change = texture_change(path, &old_path, &new_path, false);
                if old_path != new_path {
                    let encoded = encode_text(&new_path);
                    if encoded.len() > 259 {
                        result
                            .warnings
                            .push(format!("路径过长，跳过：{}", new_path));
                    } else {
                        if write {
                            for index in 0..max_len {
                                bytes[path_offset + index] = 0;
                            }
                            bytes[path_offset..path_offset + encoded.len()]
                                .copy_from_slice(&encoded);
                        }
                        result.changed = true;
                    }
                }
                result.changes.push(change);
            }
        }

        offset = end;
    }

    if write && result.changed {
        if opts.auto_backup {
            result
                .warnings
                .push(format!("已备份：{}", backup_file(path)?));
        }
        fs::write(path, bytes).map_err(|err| err.to_string())?;
    }

    Ok(result)
}

fn process_mdl(path: &Path, opts: &Options, write: bool) -> Result<FileResult, String> {
    let bytes = fs::read(path).map_err(|err| err.to_string())?;
    let text = decode_text(&bytes);
    let pattern =
        Regex::new(r#"(?i)Image\s+"([^"]+\.(?:blp|tga))""#).map_err(|err| err.to_string())?;
    let mut result = FileResult {
        changed: false,
        changes: Vec::new(),
        warnings: Vec::new(),
    };

    let replaced = pattern.replace_all(&text, |caps: &regex::Captures| {
        let old_path = caps.get(1).map(|item| item.as_str()).unwrap_or_default();
        if opts.skip_builtin && is_builtin_texture(old_path) && !resource_rule_applies(path, opts) {
            result
                .changes
                .push(texture_change(path, old_path, old_path, true));
            return caps.get(0).unwrap().as_str().to_string();
        }

        let new_path = build_new_path(old_path, path, opts);
        result
            .changes
            .push(texture_change(path, old_path, &new_path, false));
        if old_path != new_path {
            result.changed = true;
            caps.get(0).unwrap().as_str().replace(old_path, &new_path)
        } else {
            caps.get(0).unwrap().as_str().to_string()
        }
    });

    if write && result.changed {
        if opts.auto_backup {
            result
                .warnings
                .push(format!("已备份：{}", backup_file(path)?));
        }
        fs::write(path, encode_text(&replaced)).map_err(|err| err.to_string())?;
    }

    Ok(result)
}

fn texture_change(path: &Path, old_path: &str, new_path: &str, filtered: bool) -> TextureChange {
    TextureChange {
        file: path.to_string_lossy().to_string(),
        model: path
            .file_name()
            .map(|item| item.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string()),
        old_path: old_path.to_string(),
        new_path: new_path.to_string(),
        filtered,
    }
}

fn build_new_path(old_path: &str, model_path: &Path, opts: &Options) -> String {
    if opts.reset {
        return texture_basename(old_path);
    }

    if opts.trim_resource {
        if let Some(relative_dir) = resource_relative_dir(model_path) {
            return format!("{}{}", relative_dir, texture_basename(old_path));
        }
    }

    let prefix = opts.prefix.as_str();
    let model_name = model_path
        .file_stem()
        .map(|item| item.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut expanded = prefix.replace("($fileName)", &model_name);

    if expanded.trim().is_empty() {
        return texture_basename(old_path);
    }

    if opts.append_slash && !expanded.ends_with('\\') && !expanded.ends_with('/') {
        expanded.push('\\');
    }

    format!("{}{}", expanded, texture_basename(old_path))
}

fn resource_rule_applies(model_path: &Path, opts: &Options) -> bool {
    !opts.reset && opts.trim_resource && resource_relative_dir(model_path).is_some()
}

fn is_texture_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".blp") || lower.ends_with(".tga")
}

fn is_builtin_texture(path: &str) -> bool {
    let lower = normalize_slash(path)
        .trim_start_matches('\\')
        .to_ascii_lowercase()
        .to_string();
    [
        "replaceabletextures\\",
        "textures\\",
        "units\\",
        "abilities\\",
        "ui\\",
        "buildings\\",
        "doodads\\",
        "object\\",
        "objects\\",
        "terrainart\\",
    ]
    .iter()
    .any(|prefix| lower.starts_with(prefix))
}

fn resource_relative_dir(model_path: &Path) -> Option<String> {
    let normalized = normalize_slash(&model_path.to_string_lossy());
    let lower = normalized.to_ascii_lowercase();
    let markers = ["\\resource\\", "\\resources\\"];

    for marker in markers {
        if let Some(index) = lower.find(marker) {
            let tail_start = index + marker.len();
            let tail = &normalized[tail_start..];
            if let Some(last_slash) = tail.rfind('\\') {
                let directory = tail[..last_slash].trim_matches('\\');
                if !directory.is_empty() {
                    return Some(format!("{}\\", directory));
                }
            }
        }
    }
    None
}

fn texture_basename(path: &str) -> String {
    let normalized = normalize_slash(path);
    normalized
        .rsplit('\\')
        .next()
        .unwrap_or(normalized.as_str())
        .to_string()
}

fn normalize_slash(path: &str) -> String {
    path.replace('/', "\\")
}

fn decode_text(bytes: &[u8]) -> String {
    let (text, _, _) = GBK.decode(bytes);
    text.to_string()
}

fn encode_text(text: &str) -> Vec<u8> {
    let (bytes, _, _) = GBK.encode(text);
    match bytes {
        Cow::Borrowed(items) => items.to_vec(),
        Cow::Owned(items) => items,
    }
}

fn backup_file(path: &Path) -> Result<String, String> {
    let backup = next_backup_path(path);
    fs::copy(path, &backup).map_err(|err| err.to_string())?;
    Ok(backup.to_string_lossy().to_string())
}

fn next_backup_path(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let stem = path
        .file_stem()
        .map(|item| item.to_string_lossy().to_string())
        .unwrap_or_else(|| "model".to_string());
    let extension = path
        .extension()
        .map(|item| format!(".{}", item.to_string_lossy()))
        .unwrap_or_default();

    for counter in 1.. {
        let candidate = parent.join(format!("{}back{}{}", stem, counter, extension));
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_directory_comes_from_model_location() {
        let path = Path::new(r"E:\maps\NewMap\resource\effect\tips\range\001\技能范围_静态.mdx");
        assert_eq!(
            resource_relative_dir(path),
            Some(r"effect\tips\range\001\".to_string())
        );
    }

    #[test]
    fn builtin_texture_roots_are_filtered_case_insensitively() {
        for path in [
            r"Textures\foo.blp",
            r"units\human\footman.blp",
            r"Abilities\Spells\test.blp",
            r"UI\Console\test.blp",
            r"buildings\human\test.blp",
            r"Doodads\Cityscape\test.blp",
            r"Object\test.blp",
            r"Objects\InventoryItems\BundleofLumber\WoodItem.blp",
            r"TerrainArt\Village\Village_GrassThick.blp",
            r"ReplaceableTextures\TeamColor\TeamColor00.blp",
        ] {
            assert!(is_builtin_texture(path), "{} should be builtin", path);
        }
        assert!(!is_builtin_texture(r"effect\custom.blp"));
    }

    #[test]
    fn resource_rule_overrides_builtin_filter_for_resource_models() {
        let path = Path::new(r"E:\maps\NewMap\resource\effect\buff\debuff\dizziness\dizziness.mdx");
        let opts = Options {
            prefix: String::new(),
            append_slash: true,
            skip_builtin: true,
            auto_backup: false,
            trim_resource: true,
            reset: false,
        };

        assert!(resource_rule_applies(path, &opts));
        assert_eq!(
            build_new_path(r"Textures\zapblue2.blp", path, &opts),
            r"effect\buff\debuff\dizziness\zapblue2.blp"
        );
    }

    #[test]
    fn mdx_resource_preview_returns_the_expected_path() {
        let root = std::env::temp_dir().join(format!(
            "xy_texture_path_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let model_dir = root.join(r"resource\effect\buff\debuff\dizziness");
        fs::create_dir_all(&model_dir).unwrap();
        let model_path = model_dir.join("dizziness.mdx");

        let texture = b"Textures\\zapblue2.blp";
        let mut entry = vec![0u8; 268];
        entry[4..4 + texture.len()].copy_from_slice(texture);
        let mut mdx = b"MDLX".to_vec();
        mdx.extend_from_slice(b"TEXS");
        mdx.extend_from_slice(&(268u32).to_le_bytes());
        mdx.extend_from_slice(&entry);
        fs::write(&model_path, mdx).unwrap();

        let opts = Options {
            prefix: String::new(),
            append_slash: true,
            skip_builtin: true,
            auto_backup: false,
            trim_resource: true,
            reset: false,
        };
        let result = preview_paths(vec![model_path.to_string_lossy().to_string()], opts).unwrap();

        assert_eq!(result.changes.len(), 1);
        assert_eq!(
            result.changes[0].new_path,
            r"effect\buff\debuff\dizziness\zapblue2.blp"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reset_removes_custom_prefix_but_respects_builtin_filter() {
        let path = Path::new(r"E:\maps\NewMap\resource\effect\buff\debuff\dizziness\dizziness.mdx");
        let opts = Options {
            prefix: r"custom\path".to_string(),
            append_slash: true,
            skip_builtin: true,
            auto_backup: false,
            trim_resource: true,
            reset: true,
        };

        assert_eq!(
            build_new_path(r"effect\buff\debuff\dizziness\ZapBlue2.blp", path, &opts,),
            "ZapBlue2.blp"
        );
        assert!(!resource_rule_applies(path, &opts));
        assert!(opts.skip_builtin);
        assert!(is_builtin_texture(
            r"TerrainArt\Village\Village_GrassThick.blp"
        ));
    }

    #[test]
    fn reset_preview_keeps_filtered_builtin_texture_unchanged() {
        let root = std::env::temp_dir().join(format!(
            "xy_reset_filter_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let model_dir = root.join(r"resource\effect\test");
        fs::create_dir_all(&model_dir).unwrap();
        let model_path = model_dir.join("test.mdl");
        fs::write(
            &model_path,
            br#"Textures 1 {
    Bitmap {
        Image "TerrainArt\Village\Village_GrassThick.blp",
    }
}"#,
        )
        .unwrap();

        let opts = Options {
            prefix: String::new(),
            append_slash: true,
            skip_builtin: true,
            auto_backup: false,
            trim_resource: true,
            reset: true,
        };
        let result = preview_paths(vec![model_path.to_string_lossy().to_string()], opts).unwrap();

        assert_eq!(result.changes.len(), 1);
        assert!(result.changes[0].filtered);
        assert_eq!(
            result.changes[0].new_path,
            r"TerrainArt\Village\Village_GrassThick.blp"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
