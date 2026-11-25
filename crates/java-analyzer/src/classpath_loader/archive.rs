//! `.jar` / MR-JAR, class-directory, and `src.zip` / `-sources.jar` / `-javadoc.jar` readers.

use std::{
    collections::BTreeSet,
    ffi::OsStr,
    fs::{self, File},
    io::{BufReader, Read as _},
    path::{Component, Path, PathBuf},
};

use ide::{
    BinaryOrigin, BinaryTarget, ClasspathIndex, ParsedClassfile, Release, parse_class_bytes,
    parse_manifest_multi_release, select_mrjar_entries,
};
use zip::{ZipArchive, result::ZipError};

use super::JarCompanionPaths;

pub(super) fn load_jar(path: &Path, label: &str, target_release: Release) -> Option<ClasspathIndex> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(err) => {
            tracing::warn!(path = %path.display(), error = %err, "failed to open jar archive");
            return None;
        }
    };
    let mut archive = match ZipArchive::new(BufReader::new(file)) {
        Ok(a) => a,
        Err(err) => {
            tracing::warn!(path = %path.display(), error = %err, "failed to open jar zip archive");
            return None;
        }
    };

    let is_multi_release = match archive.by_name("META-INF/MANIFEST.MF") {
        Ok(mut mf) => {
            let mut mf_bytes = Vec::new();
            if let Err(err) = mf.read_to_end(&mut mf_bytes) {
                tracing::warn!(
                    path = %path.display(),
                    entry = "META-INF/MANIFEST.MF",
                    error = %err,
                    "failed to read jar manifest"
                );
                false
            } else {
                let mf_text = String::from_utf8_lossy(&mf_bytes);
                parse_manifest_multi_release(&mf_text)
            }
        }
        Err(ZipError::FileNotFound) => false,
        Err(err) => {
            tracing::warn!(
                path = %path.display(),
                entry = "META-INF/MANIFEST.MF",
                error = %err,
                "failed to inspect jar manifest"
            );
            false
        }
    };

    let mut raw_entries: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..archive.len() {
        let mut entry = match archive.by_index(i) {
            Ok(e) => e,
            Err(err) => {
                tracing::warn!(path = %path.display(), index = i, error = %err, "failed to read jar entry");
                continue;
            }
        };
        if entry.is_dir() {
            continue;
        }
        let name_cow = match entry.name() {
            Ok(n) => n,
            Err(err) => {
                tracing::warn!(path = %path.display(), index = i, error = %err, "invalid entry name in jar");
                continue;
            }
        };
        let name = name_cow.strip_prefix('/').unwrap_or(&name_cow);
        if !name.ends_with(".class") {
            continue;
        }
        if !is_multi_release && name.starts_with("META-INF/") {
            continue;
        }
        let name_owned = name.to_string();
        drop(name_cow);
        let mut bytes = Vec::new();
        if let Err(err) = entry.read_to_end(&mut bytes) {
            tracing::warn!(
                path = %path.display(),
                entry = %name_owned,
                error = %err,
                "failed to read classfile bytes in jar"
            );
            continue;
        }
        raw_entries.push((name_owned, bytes));
    }

    let entry_refs: Vec<(&str, &[u8])> = raw_entries
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();
    let selected = select_mrjar_entries(&entry_refs, is_multi_release, target_release);

    let mut partial = ClasspathIndex::default();
    let origin = BinaryOrigin::Jar {
        label: label.to_string(),
    };
    for (rel_path, class_bytes) in selected {
        match parse_class_bytes(class_bytes, origin.clone()) {
            Ok(Some(ParsedClassfile::Type(ty))) => partial.insert_type(ty),
            Ok(Some(ParsedClassfile::Module(module))) => partial.insert_module(module),
            Ok(Some(ParsedClassfile::Package {
                package,
                nullness_scope,
            })) => partial.insert_package_nullness(package, nullness_scope),
            Ok(None) => {}
            Err(err) => {
                tracing::warn!(
                    path = %path.display(),
                    entry = %rel_path,
                    error = %err,
                    "failed to parse classfile in jar"
                );
            }
        }
    }

    Some(partial)
}

pub(super) fn load_class_directory(dir: &Path, label: &str) -> Option<ClasspathIndex> {
    let mut class_files = Vec::new();
    collect_class_files_recursive(dir, &mut class_files);
    class_files.sort();

    let mut partial = ClasspathIndex::default();
    let origin = BinaryOrigin::Jar {
        label: label.to_string(),
    };

    for class_path in class_files {
        let bytes = match fs::read(&class_path) {
            Ok(b) => b,
            Err(err) => {
                tracing::warn!(path = %class_path.display(), error = %err, "failed to read class file");
                continue;
            }
        };
        match parse_class_bytes(&bytes, origin.clone()) {
            Ok(Some(ParsedClassfile::Type(ty))) => partial.insert_type(ty),
            Ok(Some(ParsedClassfile::Module(module))) => partial.insert_module(module),
            Ok(Some(ParsedClassfile::Package {
                package,
                nullness_scope,
            })) => partial.insert_package_nullness(package, nullness_scope),
            Ok(None) => {}
            Err(err) => {
                tracing::warn!(path = %class_path.display(), error = %err, "failed to parse class file");
            }
        }
    }

    Some(partial)
}

fn collect_class_files_recursive(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(err) => {
            tracing::warn!(path = %dir.display(), error = %err, "failed to read class directory");
            return;
        }
    };
    for entry in entries {
        let de = match entry {
            Ok(de) => de,
            Err(err) => {
                tracing::warn!(path = %dir.display(), error = %err, "failed to read class directory entry");
                continue;
            }
        };
        let path = de.path();
        if path.is_dir() {
            collect_class_files_recursive(&path, out);
        } else if path.is_file()
            && path
                .extension()
                .and_then(OsStr::to_str)
                .is_some_and(|ext| ext.eq_ignore_ascii_case("class"))
        {
            out.push(path);
        }
    }
}

pub(super) fn discover_jar_companions(jar_path: &Path) -> Option<JarCompanionPaths> {
    let parent = jar_path.parent()?;
    let stem = jar_path.file_stem()?.to_str()?;

    let sources_jar = ["-sources.jar", "-src.jar", "-source.jar"]
        .into_iter()
        .map(|suffix| parent.join(format!("{stem}{suffix}")))
        .find(|p| p.is_file());

    let javadoc_jar = ["-javadoc.jar", "-docs.jar"]
        .into_iter()
        .map(|suffix| parent.join(format!("{stem}{suffix}")))
        .find(|p| p.is_file());

    if sources_jar.is_some() || javadoc_jar.is_some() {
        Some(JarCompanionPaths {
            sources_jar,
            javadoc_jar,
        })
    } else {
        None
    }
}

pub(super) fn sanitize_path_segment(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | '+') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() || out == "." || out == ".." {
        "unknown".to_string()
    } else {
        out
    }
}

pub(super) fn jar_cache_key(label: &str) -> String {
    let stem = Path::new(label)
        .file_stem()
        .and_then(OsStr::to_str)
        .unwrap_or("jar");
    let sanitized = sanitize_path_segment(stem);
    let hash = fnv1a_64(label.as_bytes());
    format!("{sanitized}-{hash:016x}")
}

fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

pub(super) fn safe_relative_subpath(rel: &str) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for segment in rel.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." || segment.contains(['\\', ':'])
        {
            return None;
        }
        out.push(segment);
    }
    if out.components().all(|c| matches!(c, Component::Normal(_))) && !out.as_os_str().is_empty() {
        Some(out)
    } else {
        None
    }
}

pub(super) fn resolve_top_level_internal_name(
    classpath: Option<&ClasspathIndex>,
    target: &BinaryTarget,
) -> String {
    if let Some(index) = classpath
        && let Some(mut current) = index.type_by_fqn(&target.qualified_name)
    {
        let mut visited = BTreeSet::new();
        visited.insert(current.qualified_name.as_str());
        while let Some(enclosing_fqn) = &current.enclosing_type {
            if !visited.insert(enclosing_fqn.as_str()) {
                break;
            }
            let Some(parent) = index.type_by_fqn(enclosing_fqn) else {
                break;
            };
            current = parent;
        }
        return current.internal_name.clone();
    }

    split_top_level_internal_name(&target.internal_name).to_string()
}

pub(super) fn split_top_level_internal_name(internal_name: &str) -> &str {
    let last_slash = internal_name.rfind('/').map_or(0, |idx| idx + 1);
    if let Some(tail) = internal_name.get(last_slash..)
        && let Some(dollar_rel) = tail.find('$')
    {
        return internal_name
            .get(..last_slash + dollar_rel)
            .unwrap_or(internal_name);
    }
    internal_name
}

pub(super) fn find_cached_source(base_dir: &Path, candidates: &[String]) -> Option<(PathBuf, String)> {
    for rel in candidates {
        let Some(subpath) = safe_relative_subpath(rel) else {
            continue;
        };
        let full = base_dir.join(subpath);
        if full.is_file()
            && let Ok(text) = fs::read_to_string(&full)
        {
            return Some((full, text));
        }
    }
    None
}

pub(super) fn extract_and_cache_from_zip(
    zip_path: &Path,
    dest_root: &Path,
    candidates: &[String],
) -> Option<(PathBuf, String)> {
    let (rel, text) = read_first_matching_zip_text(zip_path, candidates)?;
    let subpath = safe_relative_subpath(&rel)?;
    let dest_path = dest_root.join(subpath);
    if !write_cached_text_if_changed(&dest_path, &text) {
        return None;
    }
    Some((dest_path, text))
}

pub(super) fn write_cached_text_if_changed(dest_path: &Path, text: &str) -> bool {
    if let Ok(existing) = fs::read_to_string(dest_path)
        && existing == text
    {
        return true;
    }
    if let Some(parent) = dest_path.parent()
        && let Err(err) = fs::create_dir_all(parent)
    {
        tracing::warn!(
            path = %parent.display(),
            error = %err,
            "failed to create cache directory"
        );
        return false;
    }
    if let Err(err) = fs::write(dest_path, text) {
        tracing::warn!(
            path = %dest_path.display(),
            error = %err,
            "failed to write cached source file"
        );
        return false;
    }
    true
}

pub(super) fn read_first_matching_zip_text(
    zip_path: &Path,
    candidates: &[String],
) -> Option<(String, String)> {
    let file = match File::open(zip_path) {
        Ok(f) => f,
        Err(err) => {
            tracing::warn!(path = %zip_path.display(), error = %err, "failed to open archive");
            return None;
        }
    };
    let mut archive = match ZipArchive::new(BufReader::new(file)) {
        Ok(a) => a,
        Err(err) => {
            tracing::warn!(path = %zip_path.display(), error = %err, "failed to read zip archive");
            return None;
        }
    };

    for candidate in candidates {
        match archive.by_name(candidate) {
            Ok(mut entry) => {
                let mut bytes = Vec::new();
                if let Err(err) = entry.read_to_end(&mut bytes) {
                    tracing::warn!(
                        path = %zip_path.display(),
                        entry = %candidate,
                        error = %err,
                        "failed to read zip entry"
                    );
                    return None;
                }
                return Some((
                    candidate.clone(),
                    String::from_utf8_lossy(&bytes).into_owned(),
                ));
            }
            Err(ZipError::FileNotFound) => continue,
            Err(err) => {
                tracing::warn!(
                    path = %zip_path.display(),
                    entry = %candidate,
                    error = %err,
                    "failed to query zip entry"
                );
                return None;
            }
        }
    }

    None
}

pub(super) fn html_entry_path_for_target(target: &BinaryTarget) -> String {
    match target.internal_name.rsplit_once('/') {
        Some((pkg_path, type_part)) => {
            let dotted_type = type_part.replace('$', ".");
            format!("{pkg_path}/{dotted_type}.html")
        }
        None => format!("{}.html", target.internal_name.replace('$', ".")),
    }
}
