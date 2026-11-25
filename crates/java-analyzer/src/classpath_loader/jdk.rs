//! JDK indexing (`ct.sym`, `.jmod`, `rt.jar`), release cache keys, and `src.zip` lookup.

use std::{
    ffi::OsStr,
    fs::{self, File},
    io::{BufReader, Cursor, Read as _},
    num::NonZeroUsize,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    thread,
};

use ide::{BinaryOrigin, ClasspathIndex, ParsedClassfile, Release, SmolStr, parse_class_bytes};
use rustc_hash::{FxHashMap, FxHashSet};
use zip::{ZipArchive, result::ZipError};

use crate::workspace_config::probe_jdk_release;

use super::{archive, jimage};

pub(super) const JMOD_MAGIC: &[u8; 4] = b"JM\x01\x00";

pub(super) fn ct_sym_version_char(release: Release) -> Option<char> {
    char::from_digit(u32::from(release.number()), 36).map(|c| c.to_ascii_uppercase())
}

pub(super) fn load_jdk_into(index: &mut ClasspathIndex, jdk_home: &Path, target_release: Release) {
    let installed_release = probe_jdk_release(jdk_home);
    if installed_release.is_some_and(|installed| target_release < installed) {
        let ct_sym_path = jdk_home.join("lib/ct.sym");
        if ct_sym_path.is_file()
            && let Some(ct_index) = load_ct_sym(&ct_sym_path, target_release)
            && ct_index.type_count() > 0
        {
            index.merge(ct_index);
            return;
        }
    }

    let jmods_dir = jdk_home.join("jmods");
    if jmods_dir.is_dir() {
        load_jmods_dir(index, &jmods_dir);
        return;
    }

    let modules_file = jdk_home.join("lib/modules");
    if modules_file.is_file() {
        jimage::load_jimage_modules(index, &modules_file);
        return;
    }

    let rt_candidates = [jdk_home.join("jre/lib/rt.jar"), jdk_home.join("lib/rt.jar")];
    for rt_jar in rt_candidates {
        if rt_jar.is_file() {
            if let Some(partial) = load_jdk8_rt_jar(&rt_jar) {
                index.merge(partial);
            }
            return;
        }
    }
}

pub(super) struct ClassParseTask {
    pub(super) module_name: Option<SmolStr>,
    pub(super) entry_name: String,
    pub(super) bytes: Vec<u8>,
}

pub(super) fn parse_jdk_class_tasks_parallel(
    index: &mut ClasspathIndex,
    archive_path: &Path,
    archive_kind: &str,
    tasks: &[ClassParseTask],
) {
    if tasks.is_empty() {
        return;
    }

    let worker_count = thread::available_parallelism()
        .map_or(4, NonZeroUsize::get)
        .min(tasks.len())
        .max(1);
    let chunk_size = tasks.len().div_ceil(worker_count).max(1);

    let mut partials: Vec<ClasspathIndex> = thread::scope(|scope| {
        let mut handles = Vec::with_capacity(worker_count);
        for chunk in tasks.chunks(chunk_size) {
            handles.push(scope.spawn(move || {
                let mut local = ClasspathIndex::default();
                for task in chunk {
                    match parse_class_bytes(
                        &task.bytes,
                        BinaryOrigin::Jdk {
                            module_name: task.module_name.clone(),
                        },
                    ) {
                        Ok(Some(ParsedClassfile::Type(ty))) => local.insert_type(ty),
                        Ok(Some(ParsedClassfile::Package {
                            package,
                            nullness_scope,
                        })) => local.insert_package_nullness(package, nullness_scope),
                        Ok(Some(ParsedClassfile::Module(_)) | None) => {}
                        Err(err) => {
                            tracing::warn!(
                                path = %archive_path.display(),
                                entry = %task.entry_name,
                                kind = archive_kind,
                                error = %err,
                                "failed to parse classfile"
                            );
                        }
                    }
                }
                local
            }));
        }

        let mut collected = Vec::with_capacity(handles.len());
        for handle in handles {
            match handle.join() {
                Ok(partial) => collected.push(partial),
                Err(_) => {
                    tracing::warn!(
                        path = %archive_path.display(),
                        kind = archive_kind,
                        "worker thread panicked while indexing JDK archive"
                    );
                }
            }
        }
        collected
    });

    for partial in partials.drain(..) {
        index.merge(partial);
    }
}

fn load_ct_sym(ct_sym_path: &Path, target_release: Release) -> Option<ClasspathIndex> {
    let ver_char = ct_sym_version_char(target_release)?;
    let file = match File::open(ct_sym_path) {
        Ok(f) => f,
        Err(err) => {
            tracing::warn!(
                path = %ct_sym_path.display(),
                error = %err,
                "failed to open ct.sym"
            );
            return None;
        }
    };
    let mut archive = match ZipArchive::new(BufReader::new(file)) {
        Ok(a) => a,
        Err(err) => {
            tracing::warn!(
                path = %ct_sym_path.display(),
                error = %err,
                "failed to open ct.sym zip archive"
            );
            return None;
        }
    };

    let is_modular = target_release >= Release::JAVA_9;
    let mut partial = ClasspathIndex::default();
    let mut exported_by_module: FxHashMap<SmolStr, FxHashSet<String>> = FxHashMap::default();
    let mut known_modules: FxHashSet<SmolStr> = FxHashSet::default();
    let mut buf = Vec::new();
    let mut entry_name = String::new();

    // Pass 1: discover JPMS modules and exported packages for target_release >= JAVA_9.
    if is_modular {
        for i in 0..archive.len() {
            let mut entry = match archive.by_index(i) {
                Ok(e) => e,
                Err(err) => {
                    tracing::warn!(
                        path = %ct_sym_path.display(),
                        index = i,
                        error = %err,
                        "failed to read ct.sym entry"
                    );
                    continue;
                }
            };
            if entry.is_dir() {
                continue;
            }
            let name_cow = match entry.name() {
                Ok(n) => n,
                Err(err) => {
                    tracing::warn!(
                        path = %ct_sym_path.display(),
                        index = i,
                        error = %err,
                        "invalid entry name in ct.sym"
                    );
                    continue;
                }
            };
            let raw_name = name_cow.strip_prefix('/').unwrap_or(&name_cow);
            let Some((ver_set, rest)) = raw_name.split_once('/') else {
                continue;
            };
            if ver_set == "META-INF" || ver_set.contains('-') || !ver_set.contains(ver_char) {
                continue;
            }
            let Some(mod_dir) = rest.strip_suffix("/module-info.sig") else {
                continue;
            };
            if mod_dir.is_empty() || mod_dir.contains('/') {
                continue;
            }

            entry_name.clear();
            entry_name.push_str(raw_name);
            let mod_dir_owned = SmolStr::new(mod_dir);
            drop(name_cow);
            buf.clear();
            if let Err(err) = entry.read_to_end(&mut buf) {
                tracing::warn!(
                    path = %ct_sym_path.display(),
                    entry = %entry_name,
                    error = %err,
                    "failed to read module-info.sig in ct.sym"
                );
                continue;
            }
            drop(entry);

            match parse_class_bytes(&buf, BinaryOrigin::Jdk { module_name: None }) {
                Ok(Some(ParsedClassfile::Module(info))) => {
                    known_modules.insert(mod_dir_owned);
                    known_modules.insert(info.name.clone());
                    let exported: FxHashSet<String> = info.exports.iter().cloned().collect();
                    exported_by_module.insert(info.name.clone(), exported);
                    partial.insert_module(info);
                }
                Ok(_) => {
                    tracing::warn!(
                        path = %ct_sym_path.display(),
                        entry = %entry_name,
                        "unexpected non-module entry in ct.sym"
                    );
                }
                Err(err) => {
                    tracing::warn!(
                        path = %ct_sym_path.display(),
                        entry = %entry_name,
                        error = %err,
                        "failed to parse module-info.sig in ct.sym"
                    );
                }
            }
        }
    }

    // Pass 2: collect matching `.sig` class payloads and parse in parallel.
    let mut tasks: Vec<ClassParseTask> = Vec::new();
    for i in 0..archive.len() {
        let mut entry = match archive.by_index(i) {
            Ok(e) => e,
            Err(err) => {
                tracing::warn!(
                    path = %ct_sym_path.display(),
                    index = i,
                    error = %err,
                    "failed to read ct.sym entry"
                );
                continue;
            }
        };
        if entry.is_dir() {
            continue;
        }
        let name_cow = match entry.name() {
            Ok(n) => n,
            Err(err) => {
                tracing::warn!(
                    path = %ct_sym_path.display(),
                    index = i,
                    error = %err,
                    "invalid entry name in ct.sym"
                );
                continue;
            }
        };
        let raw_name = name_cow.strip_prefix('/').unwrap_or(&name_cow);
        let Some((ver_set, rest)) = raw_name.split_once('/') else {
            continue;
        };
        if ver_set == "META-INF" || ver_set.contains('-') || !ver_set.contains(ver_char) {
            continue;
        }
        let Some(rest_no_ext) = rest.strip_suffix(".sig") else {
            continue;
        };

        let (mod_dir_opt, rel_no_ext) = match rest_no_ext.split_once('/') {
            Some((first, tail)) if known_modules.contains(first) || first.contains('.') => {
                (Some(first), tail)
            }
            _ => (None, rest_no_ext),
        };

        if rel_no_ext == "module-info"
            || rel_no_ext == "package-info"
            || rel_no_ext.ends_with("/package-info")
        {
            continue;
        }
        let class_base = rel_no_ext.rsplit('/').next().unwrap_or(rel_no_ext);
        if is_anonymous_or_local_class_base(class_base) {
            continue;
        }

        let package = rel_no_ext
            .rsplit_once('/')
            .map_or_else(String::new, |(pkg, _)| pkg.replace('/', "."));

        if is_modular {
            if let Some(exported) = mod_dir_opt.and_then(|m| exported_by_module.get(m)) {
                if is_jdk_internal_package(&package) || !exported.contains(&package) {
                    continue;
                }
            } else if is_jdk8_internal_package(&package) {
                continue;
            }
        } else if is_jdk8_internal_package(&package) {
            continue;
        }

        let module_name = if is_modular {
            mod_dir_opt.map(SmolStr::new)
        } else {
            None
        };
        entry_name.clear();
        entry_name.push_str(raw_name);
        drop(name_cow);

        let mut bytes = Vec::new();
        if let Err(err) = entry.read_to_end(&mut bytes) {
            tracing::warn!(
                path = %ct_sym_path.display(),
                entry = %entry_name,
                error = %err,
                "failed to read classfile bytes in ct.sym"
            );
            continue;
        }
        drop(entry);

        tasks.push(ClassParseTask {
            module_name,
            entry_name: entry_name.clone(),
            bytes,
        });
    }

    parse_jdk_class_tasks_parallel(&mut partial, ct_sym_path, "ct.sym", &tasks);
    Some(partial)
}

pub(super) fn is_anonymous_or_local_class_base(base: &str) -> bool {
    let mut parts = base.split('$');
    let _outer = parts.next();
    parts.any(|seg| seg.is_empty() || seg.as_bytes().first().is_some_and(u8::is_ascii_digit))
}

fn load_jmods_dir(index: &mut ClasspathIndex, jmods_dir: &Path) {
    let entries = match fs::read_dir(jmods_dir) {
        Ok(entries) => entries,
        Err(err) => {
            tracing::warn!(path = %jmods_dir.display(), error = %err, "failed to read jmods directory");
            return;
        }
    };

    let mut jmod_paths = Vec::new();
    for entry in entries {
        match entry {
            Ok(de) => {
                let path = de.path();
                let is_jmod = path
                    .extension()
                    .and_then(OsStr::to_str)
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("jmod"));
                if is_jmod && path.is_file() {
                    jmod_paths.push(path);
                }
            }
            Err(err) => {
                tracing::warn!(path = %jmods_dir.display(), error = %err, "failed to read jmods entry");
            }
        }
    }
    jmod_paths.sort();

    if jmod_paths.is_empty() {
        return;
    }

    let worker_count = thread::available_parallelism()
        .map_or(4, NonZeroUsize::get)
        .min(jmod_paths.len())
        .max(1);
    let next_idx = AtomicUsize::new(0);

    let mut worker_results: Vec<Vec<(usize, ClasspathIndex)>> = thread::scope(|scope| {
        let mut handles = Vec::with_capacity(worker_count);
        for _ in 0..worker_count {
            let paths = &jmod_paths;
            let counter = &next_idx;
            handles.push(scope.spawn(move || {
                let mut local = Vec::new();
                loop {
                    let idx = counter.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = paths.get(idx) else {
                        break;
                    };
                    if let Some(partial) = load_jmod(path) {
                        local.push((idx, partial));
                    }
                }
                local
            }));
        }

        let mut collected = Vec::with_capacity(handles.len());
        for handle in handles {
            match handle.join() {
                Ok(res) => collected.push(res),
                Err(_) => {
                    tracing::warn!(path = %jmods_dir.display(), "worker thread panicked while indexing jmods");
                }
            }
        }
        collected
    });

    let mut ordered: Vec<(usize, ClasspathIndex)> = worker_results.drain(..).flatten().collect();
    ordered.sort_by_key(|(idx, _)| *idx);
    for (_, partial) in ordered {
        index.merge(partial);
    }
}

fn load_jmod(path: &Path) -> Option<ClasspathIndex> {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(err) => {
            tracing::warn!(path = %path.display(), error = %err, "failed to read jmod archive");
            return None;
        }
    };

    if !bytes.starts_with(JMOD_MAGIC) {
        tracing::warn!(path = %path.display(), "invalid jmod header magic");
        return None;
    }
    let Some(zip_bytes) = bytes.get(JMOD_MAGIC.len()..) else {
        tracing::warn!(path = %path.display(), "truncated jmod archive");
        return None;
    };

    let mut archive = match ZipArchive::new(Cursor::new(zip_bytes)) {
        Ok(a) => a,
        Err(err) => {
            tracing::warn!(path = %path.display(), error = %err, "failed to open jmod zip archive");
            return None;
        }
    };

    let mut partial = ClasspathIndex::default();
    let mut buf = Vec::new();
    let module_info = match archive.by_name("classes/module-info.class") {
        Ok(mut entry) => {
            if let Err(err) = entry.read_to_end(&mut buf) {
                tracing::warn!(
                    path = %path.display(),
                    entry = "classes/module-info.class",
                    error = %err,
                    "failed to read module-info.class in jmod"
                );
                return None;
            }
            match parse_class_bytes(&buf, BinaryOrigin::Jdk { module_name: None }) {
                Ok(Some(ParsedClassfile::Module(info))) => info,
                Ok(_) => {
                    tracing::warn!(
                        path = %path.display(),
                        entry = "classes/module-info.class",
                        "unexpected non-module entry in jmod"
                    );
                    return None;
                }
                Err(err) => {
                    tracing::warn!(
                        path = %path.display(),
                        entry = "classes/module-info.class",
                        error = %err,
                        "failed to parse module-info.class in jmod"
                    );
                    return None;
                }
            }
        }
        Err(ZipError::FileNotFound) => {
            tracing::warn!(path = %path.display(), "missing classes/module-info.class in jmod");
            return None;
        }
        Err(err) => {
            tracing::warn!(
                path = %path.display(),
                error = %err,
                "failed to look up module-info.class in jmod"
            );
            return None;
        }
    };

    let module_name = Some(module_info.name.clone());
    let exported_packages: FxHashSet<String> = module_info.exports.iter().cloned().collect();
    partial.insert_module(module_info);

    if exported_packages.is_empty() {
        return Some(partial);
    }

    let mut entry_name = String::new();
    for i in 0..archive.len() {
        let mut entry = match archive.by_index(i) {
            Ok(e) => e,
            Err(err) => {
                tracing::warn!(path = %path.display(), index = i, error = %err, "failed to read jmod entry");
                continue;
            }
        };
        if entry.is_dir() {
            continue;
        }
        let name_cow = match entry.name() {
            Ok(n) => n,
            Err(err) => {
                tracing::warn!(path = %path.display(), index = i, error = %err, "invalid entry name in jmod");
                continue;
            }
        };
        let Some(rel_no_ext) = name_cow
            .strip_prefix("classes/")
            .and_then(|s| s.strip_suffix(".class"))
        else {
            continue;
        };
        if rel_no_ext == "module-info"
            || rel_no_ext == "package-info"
            || rel_no_ext.ends_with("/package-info")
        {
            continue;
        }
        let package = rel_no_ext
            .rsplit_once('/')
            .map_or_else(String::new, |(pkg, _)| pkg.replace('/', "."));
        if is_jdk_internal_package(&package) || !exported_packages.contains(&package) {
            continue;
        }

        entry_name.clear();
        entry_name.push_str(&name_cow);
        drop(name_cow);
        buf.clear();
        if let Err(err) = entry.read_to_end(&mut buf) {
            tracing::warn!(
                path = %path.display(),
                entry = %entry_name,
                error = %err,
                "failed to read classfile bytes in jmod"
            );
            continue;
        }
        drop(entry);

        match parse_class_bytes(
            &buf,
            BinaryOrigin::Jdk {
                module_name: module_name.clone(),
            },
        ) {
            Ok(Some(ParsedClassfile::Type(ty))) => partial.insert_type(ty),
            Ok(Some(ParsedClassfile::Package {
                package,
                nullness_scope,
            })) => partial.insert_package_nullness(package, nullness_scope),
            Ok(Some(ParsedClassfile::Module(_)) | None) => {}
            Err(err) => {
                tracing::warn!(
                    path = %path.display(),
                    entry = %entry_name,
                    error = %err,
                    "failed to parse classfile in jmod"
                );
            }
        }
    }

    Some(partial)
}

fn load_jdk8_rt_jar(path: &Path) -> Option<ClasspathIndex> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(err) => {
            tracing::warn!(path = %path.display(), error = %err, "failed to open rt.jar");
            return None;
        }
    };
    let mut archive = match ZipArchive::new(BufReader::new(file)) {
        Ok(a) => a,
        Err(err) => {
            tracing::warn!(path = %path.display(), error = %err, "failed to read rt.jar zip archive");
            return None;
        }
    };

    let mut partial = ClasspathIndex::default();
    let mut buf = Vec::new();
    let mut entry_name = String::new();
    let origin = BinaryOrigin::Jdk { module_name: None };

    for i in 0..archive.len() {
        let mut entry = match archive.by_index(i) {
            Ok(e) => e,
            Err(err) => {
                tracing::warn!(path = %path.display(), index = i, error = %err, "failed to read rt.jar entry");
                continue;
            }
        };
        if entry.is_dir() {
            continue;
        }
        let name_cow = match entry.name() {
            Ok(n) => n,
            Err(err) => {
                tracing::warn!(path = %path.display(), index = i, error = %err, "invalid entry name in rt.jar");
                continue;
            }
        };
        let raw_name = name_cow.strip_prefix('/').unwrap_or(&name_cow);
        let Some(rel_no_ext) = raw_name.strip_suffix(".class") else {
            continue;
        };
        if rel_no_ext == "module-info"
            || rel_no_ext == "package-info"
            || rel_no_ext.ends_with("/package-info")
        {
            continue;
        }
        let package = rel_no_ext
            .rsplit_once('/')
            .map_or_else(String::new, |(pkg, _)| pkg.replace('/', "."));
        if is_jdk8_internal_package(&package) {
            continue;
        }

        entry_name.clear();
        entry_name.push_str(raw_name);
        drop(name_cow);
        buf.clear();
        if let Err(err) = entry.read_to_end(&mut buf) {
            tracing::warn!(
                path = %path.display(),
                entry = %entry_name,
                error = %err,
                "failed to read classfile bytes in rt.jar"
            );
            continue;
        }
        drop(entry);

        match parse_class_bytes(&buf, origin.clone()) {
            Ok(Some(ParsedClassfile::Type(ty))) => partial.insert_type(ty),
            Ok(Some(ParsedClassfile::Package {
                package,
                nullness_scope,
            })) => partial.insert_package_nullness(package, nullness_scope),
            Ok(Some(ParsedClassfile::Module(_)) | None) => {}
            Err(err) => {
                tracing::warn!(
                    path = %path.display(),
                    entry = %entry_name,
                    error = %err,
                    "failed to parse classfile in rt.jar"
                );
            }
        }
    }

    Some(partial)
}

pub(super) fn is_jdk_internal_package(package: &str) -> bool {
    package == "jdk.internal" || package.starts_with("jdk.internal.")
}

fn is_jdk8_internal_package(package: &str) -> bool {
    package == "sun"
        || package.starts_with("sun.")
        || package == "com.sun"
        || package.starts_with("com.sun.")
        || is_jdk_internal_package(package)
}

/// Computes a filesystem-safe cache key for a JDK home directory from `$JAVA_HOME/release`
/// (`JAVA_VERSION="..."`), falling back to the sanitized JDK directory name.
#[must_use]
pub fn jdk_version_key(jdk_home: &Path) -> String {
    let release_file = jdk_home.join("release");
    if let Ok(content) = fs::read_to_string(&release_file) {
        for line in content.lines() {
            if let Some(raw_val) = line.trim().strip_prefix("JAVA_VERSION=") {
                let unquoted = raw_val.trim().trim_matches('"').trim_matches('\'').trim();
                if !unquoted.is_empty() {
                    let sanitized = archive::sanitize_path_segment(unquoted);
                    if sanitized.starts_with("jdk-") {
                        return sanitized;
                    }
                    return format!("jdk-{sanitized}");
                }
            }
        }
    }

    jdk_home
        .file_name()
        .and_then(OsStr::to_str)
        .filter(|s| !s.is_empty())
        .map(archive::sanitize_path_segment)
        .unwrap_or_else(|| "jdk-unknown".to_string())
}

pub(super) fn find_jdk_src_zip(jdk_home: &Path) -> Option<PathBuf> {
    let lib_src = jdk_home.join("lib/src.zip");
    if lib_src.is_file() {
        return Some(lib_src);
    }
    let root_src = jdk_home.join("src.zip");
    if root_src.is_file() {
        return Some(root_src);
    }
    None
}
