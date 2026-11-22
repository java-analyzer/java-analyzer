//! On-demand JDK (`.jmod` / `lib/modules` / `rt.jar`), local `.jar` (including JEP 238 MR-JARs), and
//! class-directory classpath indexing, source/stub materialization, and Javadoc lookup.
//!
//! See `docs/specs/0006-jdk-and-jar-classpath-indexing.md`.

use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsStr,
    fs::{self, File},
    io::{BufReader, Cursor, Read as _},
    num::NonZeroUsize,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    thread,
};

use ide::{
    BinaryOrigin, BinaryTarget, ClasspathIndex, ModuleDependency, ParsedClassfile, Release,
    SmolStr, TextRange, extract_javadoc_from_html, extract_javadoc_from_source,
    locate_binary_symbol_in_source, parse_class_bytes, parse_manifest_multi_release,
    select_mrjar_entries, synthesize_binary_stub,
};
use rustc_hash::{FxHashMap, FxHashSet};
use zip::{ZipArchive, result::ZipError};

use crate::{
    jimage::Jimage,
    workspace_config::{WorkspaceProbe, probe_jdk_release},
};

const JMOD_MAGIC: &[u8; 4] = b"JM\x01\x00";

/// Paths to optional companion source and Javadoc archives discovered alongside a `.jar`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JarCompanionPaths {
    pub sources_jar: Option<PathBuf>,
    pub javadoc_jar: Option<PathBuf>,
}

/// Mapping from `BinaryOrigin::Jar { label }` to discovered companion archives.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JarCompanions {
    by_label: BTreeMap<String, JarCompanionPaths>,
}

impl JarCompanions {
    #[must_use]
    pub fn get(&self, label: &str) -> Option<&JarCompanionPaths> {
        self.by_label.get(label)
    }

    pub fn insert(&mut self, label: impl Into<String>, paths: JarCompanionPaths) {
        self.by_label.insert(label.into(), paths);
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_label.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.by_label.len()
    }
}

/// Loads all binary types and JPMS modules from the probed JDK and workspace local dependencies.
#[must_use]
pub fn load_classpath(probe: &WorkspaceProbe) -> (ClasspathIndex, JarCompanions) {
    let mut index = ClasspathIndex::default();
    let mut companions = JarCompanions::default();

    let target_release = probe.resolved_level.release;

    if let Some(jdk_home) = &probe.resolved_jdk {
        load_jdk_into(&mut index, jdk_home, target_release);
    }
    let mut seen_paths: BTreeSet<PathBuf> = BTreeSet::new();

    for module in &probe.layout.modules {
        for dep in &module.dependencies {
            let ModuleDependency::Path { path, .. } = dep else {
                continue;
            };
            let raw = Path::new(path);
            let resolved = if raw.is_absolute() {
                raw.to_path_buf()
            } else {
                probe.workspace_dir.join(raw)
            };
            if !seen_paths.insert(resolved.clone()) {
                continue;
            }

            if resolved.is_file() {
                let is_jar = resolved
                    .extension()
                    .and_then(OsStr::to_str)
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("jar"));
                if is_jar {
                    if let Some(found) = discover_jar_companions(&resolved) {
                        companions.insert(path.clone(), found);
                    }
                    if let Some(partial) = load_jar(&resolved, path, target_release) {
                        index.merge(partial);
                    }
                }
            } else if resolved.is_dir() {
                if let Some(partial) = load_class_directory(&resolved, path) {
                    index.merge(partial);
                }
            } else {
                tracing::warn!(
                    path = %resolved.display(),
                    "configured classpath dependency path does not exist"
                );
            }
        }
    }

    index.finalize();
    (index, companions)
}

fn ct_sym_version_char(release: Release) -> Option<char> {
    char::from_digit(u32::from(release.number()), 36).map(|c| c.to_ascii_uppercase())
}

fn load_jdk_into(index: &mut ClasspathIndex, jdk_home: &Path, target_release: Release) {
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
        load_jimage_modules(index, &modules_file);
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

struct ClassParseTask {
    module_name: Option<SmolStr>,
    entry_name: String,
    bytes: Vec<u8>,
}

fn parse_jdk_class_tasks_parallel(
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

fn load_jimage_modules(index: &mut ClasspathIndex, modules_path: &Path) {
    let image = match Jimage::from_file(modules_path) {
        Ok(image) => image,
        Err(err) => {
            tracing::warn!(
                path = %modules_path.display(),
                error = %err,
                "failed to open jimage modules file"
            );
            return;
        }
    };

    // Pass 1: discover JPMS modules and their unqualified exported packages.
    let mut exported_by_module: FxHashMap<SmolStr, FxHashSet<String>> = FxHashMap::default();
    for (idx, entry) in image.entries().enumerate() {
        let resource = match entry {
            Ok(resource) => resource,
            Err(err) => {
                tracing::warn!(
                    path = %modules_path.display(),
                    index = idx,
                    error = %err,
                    "failed to read jimage entry"
                );
                continue;
            }
        };

        if resource.module.is_empty() || resource.module == "packages" || resource.module == "modules"
        {
            continue;
        }
        if !resource.parent.is_empty()
            || resource.base != "module-info"
            || resource.extension != "class"
        {
            continue;
        }

        let full_name = resource.full_name();
        if full_name.starts_with("/packages/") || full_name.starts_with("/modules/") {
            continue;
        }

        let bytes = match resource.uncompressed_data() {
            Ok(Some(bytes)) => bytes,
            Ok(None) => continue,
            Err(err) => {
                tracing::warn!(
                    path = %modules_path.display(),
                    entry = %full_name,
                    error = %err,
                    "failed to slice module-info.class in jimage"
                );
                continue;
            }
        };

        match parse_class_bytes(bytes, BinaryOrigin::Jdk { module_name: None }) {
            Ok(Some(ParsedClassfile::Module(info))) => {
                let exported: FxHashSet<String> = info.exports.iter().cloned().collect();
                exported_by_module.insert(info.name.clone(), exported);
                index.insert_module(info);
            }
            Ok(_) => {
                tracing::warn!(
                    path = %modules_path.display(),
                    entry = %full_name,
                    "unexpected non-module entry in jimage"
                );
            }
            Err(err) => {
                tracing::warn!(
                    path = %modules_path.display(),
                    entry = %full_name,
                    error = %err,
                    "failed to parse module-info.class in jimage"
                );
            }
        }
    }

    if exported_by_module.is_empty() {
        return;
    }

    // Pass 2: collect exported `.class` payloads, then parse in parallel and merge deterministically.
    let mut tasks: Vec<ClassParseTask> = Vec::new();
    for (idx, entry) in image.entries().enumerate() {
        let resource = match entry {
            Ok(resource) => resource,
            Err(err) => {
                tracing::warn!(
                    path = %modules_path.display(),
                    index = idx,
                    error = %err,
                    "failed to read jimage entry"
                );
                continue;
            }
        };

        if resource.module.is_empty() || resource.module == "packages" || resource.module == "modules"
        {
            continue;
        }
        if resource.extension != "class" {
            continue;
        }
        if resource.base == "module-info"
            || resource.base == "package-info"
            || is_anonymous_or_local_class_base(resource.base)
        {
            continue;
        }

        let Some((module_key, exported_packages)) = exported_by_module.get_key_value(resource.module)
        else {
            continue;
        };

        let package = resource.parent.replace('/', ".");
        if is_jdk_internal_package(&package) || !exported_packages.contains(&package) {
            continue;
        }

        let full_name = resource.full_name();
        let bytes = match resource.uncompressed_data() {
            Ok(Some(bytes)) => bytes,
            Ok(None) => continue,
            Err(err) => {
                tracing::warn!(
                    path = %modules_path.display(),
                    entry = %full_name,
                    error = %err,
                    "failed to slice classfile in jimage"
                );
                continue;
            }
        };

        tasks.push(ClassParseTask {
            module_name: Some(module_key.clone()),
            entry_name: full_name,
            bytes: bytes.to_vec(),
        });
    }

    parse_jdk_class_tasks_parallel(index, modules_path, "jimage", &tasks);
}

fn is_anonymous_or_local_class_base(base: &str) -> bool {
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

fn load_jar(path: &Path, label: &str, target_release: Release) -> Option<ClasspathIndex> {
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

fn load_class_directory(dir: &Path, label: &str) -> Option<ClasspathIndex> {
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

fn discover_jar_companions(jar_path: &Path) -> Option<JarCompanionPaths> {
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

fn is_jdk_internal_package(package: &str) -> bool {
    package == "jdk.internal" || package.starts_with("jdk.internal.")
}

fn is_jdk8_internal_package(package: &str) -> bool {
    package == "sun"
        || package.starts_with("sun.")
        || package == "com.sun"
        || package.starts_with("com.sun.")
        || is_jdk_internal_package(package)
}

/// Returns the user-scoped cache root directory for extracted JDK/JAR sources and stubs.
#[must_use]
pub fn cache_root() -> PathBuf {
    if let Some(xdg) = env::var_os("XDG_CACHE_HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(xdg).join("java-analyzer");
    }

    #[cfg(target_os = "macos")]
    if let Some(home) = env::var_os("HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(home).join("Library/Caches/java-analyzer");
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    if let Some(home) = env::var_os("HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(home).join(".cache/java-analyzer");
    }

    #[cfg(windows)]
    if let Some(local) = env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()) {
        return PathBuf::from(local).join("java-analyzer");
    }

    env::temp_dir().join("java-analyzer")
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
                    let sanitized = sanitize_path_segment(unquoted);
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
        .map(sanitize_path_segment)
        .unwrap_or_else(|| "jdk-unknown".to_string())
}

fn sanitize_path_segment(raw: &str) -> String {
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

fn jar_cache_key(label: &str) -> String {
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

fn safe_relative_subpath(rel: &str) -> Option<PathBuf> {
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

/// Materializes the `.java` source (from JDK `src.zip` or companion `*-sources.jar`) or a
/// synthesized `.java` stub on disk under [`cache_root()`], and locates `target` inside it.
#[must_use]
pub fn materialize_binary_location(
    classpath: &ClasspathIndex,
    jdk_home: Option<&Path>,
    companions: &JarCompanions,
    target: &BinaryTarget,
) -> Option<(PathBuf, String, Option<TextRange>)> {
    materialize_binary_location_in_cache_root(
        &cache_root(),
        classpath,
        jdk_home,
        companions,
        target,
    )
}

/// Same as [`materialize_binary_location`], using an explicit cache root (for isolated tests).
#[must_use]
pub fn materialize_binary_location_in_cache_root(
    cache_root: &Path,
    classpath: &ClasspathIndex,
    jdk_home: Option<&Path>,
    companions: &JarCompanions,
    target: &BinaryTarget,
) -> Option<(PathBuf, String, Option<TextRange>)> {
    let top_level_internal = resolve_top_level_internal_name(Some(classpath), target);
    let rel_java = format!("{top_level_internal}.java");
    let raw_rel_java = format!("{}.java", target.internal_name);

    let (stub_key, extracted) = match &target.origin {
        BinaryOrigin::Jdk { module_name } => {
            let jdk_key = jdk_home.map_or_else(|| "jdk-unknown".to_string(), jdk_version_key);
            let mut candidates = Vec::new();
            if let Some(m) = module_name {
                candidates.push(format!("{m}/{rel_java}"));
                if raw_rel_java != rel_java {
                    candidates.push(format!("{m}/{raw_rel_java}"));
                }
            }
            candidates.push(rel_java.clone());
            if raw_rel_java != rel_java {
                candidates.push(raw_rel_java.clone());
            }

            let src_dir = cache_root.join("jdk-src").join(&jdk_key);
            let materialized = find_cached_source(&src_dir, &candidates).or_else(|| {
                let src_zip = find_jdk_src_zip(jdk_home?)?;
                extract_and_cache_from_zip(&src_zip, &src_dir, &candidates)
            });
            (jdk_key, materialized)
        }
        BinaryOrigin::Jar { label } => {
            let jar_key = jar_cache_key(label);
            let mut candidates = vec![rel_java.clone()];
            if raw_rel_java != rel_java {
                candidates.push(raw_rel_java.clone());
            }

            let src_dir = cache_root.join("jar-src").join(&jar_key);
            let materialized = find_cached_source(&src_dir, &candidates).or_else(|| {
                let sources_jar = companions.get(label)?.sources_jar.as_deref()?;
                extract_and_cache_from_zip(sources_jar, &src_dir, &candidates)
            });
            (jar_key, materialized)
        }
    };

    let (path, source) = match extracted {
        Some(pair) => pair,
        None => {
            let stub = synthesize_binary_stub(classpath, target)?;
            let rel_subpath = safe_relative_subpath(&rel_java)?;
            let stub_path = cache_root.join("stubs").join(&stub_key).join(rel_subpath);
            if !write_cached_text_if_changed(&stub_path, &stub) {
                return None;
            }
            (stub_path, stub)
        }
    };

    let range = locate_binary_symbol_in_source(&source, target);
    Some((path, source, range))
}

fn resolve_top_level_internal_name(
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

fn split_top_level_internal_name(internal_name: &str) -> &str {
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

fn find_jdk_src_zip(jdk_home: &Path) -> Option<PathBuf> {
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

fn find_cached_source(base_dir: &Path, candidates: &[String]) -> Option<(PathBuf, String)> {
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

fn extract_and_cache_from_zip(
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

fn write_cached_text_if_changed(dest_path: &Path, text: &str) -> bool {
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

fn read_first_matching_zip_text(zip_path: &Path, candidates: &[String]) -> Option<(String, String)> {
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

/// Looks up Javadoc Markdown for `target` from JDK `src.zip`, companion `*-sources.jar`,
/// or companion `*-javadoc.jar` HTML.
#[must_use]
pub fn lookup_binary_javadoc(
    jdk_home: Option<&Path>,
    companions: &JarCompanions,
    target: &BinaryTarget,
) -> Option<String> {
    let top_level_internal = split_top_level_internal_name(&target.internal_name);
    let rel_java = format!("{top_level_internal}.java");
    let raw_rel_java = format!("{}.java", target.internal_name);

    match &target.origin {
        BinaryOrigin::Jdk { module_name } => {
            let src_zip = find_jdk_src_zip(jdk_home?)?;
            let mut candidates = Vec::new();
            if let Some(m) = module_name {
                candidates.push(format!("{m}/{rel_java}"));
                if raw_rel_java != rel_java {
                    candidates.push(format!("{m}/{raw_rel_java}"));
                }
            }
            candidates.push(rel_java);
            if let Some((_, source)) = read_first_matching_zip_text(&src_zip, &candidates) {
                return extract_javadoc_from_source(&source, target);
            }
            None
        }
        BinaryOrigin::Jar { label } => {
            let companion = companions.get(label)?;
            if let Some(sources_jar) = &companion.sources_jar {
                let mut candidates = Vec::with_capacity(2);
                let push_raw = raw_rel_java != rel_java;
                candidates.push(rel_java);
                if push_raw {
                    candidates.push(raw_rel_java);
                }
                if let Some((_, source)) = read_first_matching_zip_text(sources_jar, &candidates)
                    && let Some(doc) = extract_javadoc_from_source(&source, target)
                {
                    return Some(doc);
                }
            }
            if let Some(javadoc_jar) = &companion.javadoc_jar {
                let html_rel = html_entry_path_for_target(target);
                if let Some((_, html)) = read_first_matching_zip_text(javadoc_jar, &[html_rel]) {
                    return extract_javadoc_from_html(&html, target);
                }
            }
            None
        }
    }
}

fn html_entry_path_for_target(target: &BinaryTarget) -> String {
    match target.internal_name.rsplit_once('/') {
        Some((pkg_path, type_part)) => {
            let dotted_type = type_part.replace('$', ".");
            format!("{pkg_path}/{dotted_type}.html")
        }
        None => format!("{}.html", target.internal_name.replace('$', ".")),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::{Cursor, Write as _},
        path::{Path, PathBuf},
    };

    use ide::{BinaryOrigin, LanguageLevel, ModuleDependency, Release, RootScope};
    use zip::write::SimpleFileOptions;

    use super::*;
    use crate::{
        test_util::TestDir,
        workspace_config::{discover_jdks, probe_workspace},
    };

    // ---- Pure-Rust minimal .class / module-info.class byte builder (JVM-free) ----

    struct ClassBuilder {
        cp: Vec<CpEntry>,
    }

    enum CpEntry {
        Utf8(String),
        Class(u16),
        NameAndType(u16, u16),
        Module(u16),
        Package(u16),
    }

    impl ClassBuilder {
        fn new() -> Self {
            Self { cp: Vec::new() }
        }

        fn push_cp(&mut self, entry: CpEntry) -> u16 {
            self.cp.push(entry);
            u16::try_from(self.cp.len()).unwrap()
        }

        fn utf8(&mut self, s: &str) -> u16 {
            self.push_cp(CpEntry::Utf8(s.to_string()))
        }

        fn class(&mut self, internal_name: &str) -> u16 {
            let name_idx = self.utf8(internal_name);
            self.push_cp(CpEntry::Class(name_idx))
        }

        fn module_cp(&mut self, name: &str) -> u16 {
            let name_idx = self.utf8(name);
            self.push_cp(CpEntry::Module(name_idx))
        }

        fn package_cp(&mut self, internal_pkg: &str) -> u16 {
            let name_idx = self.utf8(internal_pkg);
            self.push_cp(CpEntry::Package(name_idx))
        }

        fn name_and_type(&mut self, name: &str, desc: &str) -> u16 {
            let n = self.utf8(name);
            let d = self.utf8(desc);
            self.push_cp(CpEntry::NameAndType(n, d))
        }

        fn finish_cp(&self, out: &mut Vec<u8>) {
            out.extend_from_slice(&u16::try_from(self.cp.len() + 1).unwrap().to_be_bytes());
            for entry in &self.cp {
                match entry {
                    CpEntry::Utf8(s) => {
                        out.push(1);
                        let bytes = s.as_bytes();
                        out.extend_from_slice(&u16::try_from(bytes.len()).unwrap().to_be_bytes());
                        out.extend_from_slice(bytes);
                    }
                    CpEntry::Class(idx) => {
                        out.push(7);
                        out.extend_from_slice(&idx.to_be_bytes());
                    }
                    CpEntry::NameAndType(n, d) => {
                        out.push(12);
                        out.extend_from_slice(&n.to_be_bytes());
                        out.extend_from_slice(&d.to_be_bytes());
                    }
                    CpEntry::Module(idx) => {
                        out.push(19);
                        out.extend_from_slice(&idx.to_be_bytes());
                    }
                    CpEntry::Package(idx) => {
                        out.push(20);
                        out.extend_from_slice(&idx.to_be_bytes());
                    }
                }
            }
        }
    }

    fn build_simple_class_bytes(
        major_version: u16,
        access_flags: u16,
        internal_name: &str,
        super_internal_name: &str,
        methods: &[(&str, &str, u16)],
        fields: &[(&str, &str, u16)],
    ) -> Vec<u8> {
        let mut b = ClassBuilder::new();
        let this_cls = b.class(internal_name);
        let super_cls = b.class(super_internal_name);

        let mut encoded_fields = Vec::with_capacity(fields.len());
        for &(name, desc, flags) in fields {
            let n = b.utf8(name);
            let d = b.utf8(desc);
            encoded_fields.push((flags, n, d));
        }

        let mut encoded_methods = Vec::with_capacity(methods.len());
        for &(name, desc, flags) in methods {
            let n = b.utf8(name);
            let d = b.utf8(desc);
            encoded_methods.push((flags, n, d));
        }

        let mut out = Vec::new();
        out.extend_from_slice(&[0xCA, 0xFE, 0xBA, 0xBE]);
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&major_version.to_be_bytes());
        b.finish_cp(&mut out);

        out.extend_from_slice(&access_flags.to_be_bytes());
        out.extend_from_slice(&this_cls.to_be_bytes());
        out.extend_from_slice(&super_cls.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes()); // interfaces_count

        out.extend_from_slice(&u16::try_from(encoded_fields.len()).unwrap().to_be_bytes());
        for (flags, n, d) in encoded_fields {
            out.extend_from_slice(&flags.to_be_bytes());
            out.extend_from_slice(&n.to_be_bytes());
            out.extend_from_slice(&d.to_be_bytes());
            out.extend_from_slice(&0u16.to_be_bytes()); // attributes_count
        }

        out.extend_from_slice(&u16::try_from(encoded_methods.len()).unwrap().to_be_bytes());
        for (flags, n, d) in encoded_methods {
            out.extend_from_slice(&flags.to_be_bytes());
            out.extend_from_slice(&n.to_be_bytes());
            out.extend_from_slice(&d.to_be_bytes());
            out.extend_from_slice(&0u16.to_be_bytes()); // attributes_count
        }

        out.extend_from_slice(&0u16.to_be_bytes()); // class attributes_count
        let _ = b.name_and_type("unused", "()V");
        out
    }

    fn build_module_info_bytes(
        module_name: &str,
        exports: &[&str],
        requires_transitive: &[&str],
    ) -> Vec<u8> {
        let mut b = ClassBuilder::new();
        let this_cls = b.class("module-info");
        let attr_name = b.utf8("Module");
        let mod_idx = b.module_cp(module_name);

        let req_indices: Vec<u16> = requires_transitive
            .iter()
            .map(|r| b.module_cp(r))
            .collect();
        let exp_indices: Vec<u16> = exports.iter().map(|e| b.package_cp(e)).collect();

        let mut mod_attr = Vec::new();
        mod_attr.extend_from_slice(&mod_idx.to_be_bytes());
        mod_attr.extend_from_slice(&0u16.to_be_bytes()); // module_flags
        mod_attr.extend_from_slice(&0u16.to_be_bytes()); // module_version_index

        mod_attr.extend_from_slice(&u16::try_from(req_indices.len()).unwrap().to_be_bytes());
        for r_idx in req_indices {
            mod_attr.extend_from_slice(&r_idx.to_be_bytes());
            mod_attr.extend_from_slice(&0x0020u16.to_be_bytes()); // ACC_TRANSITIVE
            mod_attr.extend_from_slice(&0u16.to_be_bytes());
        }

        mod_attr.extend_from_slice(&u16::try_from(exp_indices.len()).unwrap().to_be_bytes());
        for e_idx in exp_indices {
            mod_attr.extend_from_slice(&e_idx.to_be_bytes());
            mod_attr.extend_from_slice(&0u16.to_be_bytes());
            mod_attr.extend_from_slice(&0u16.to_be_bytes());
        }

        mod_attr.extend_from_slice(&0u16.to_be_bytes());
        mod_attr.extend_from_slice(&0u16.to_be_bytes());
        mod_attr.extend_from_slice(&0u16.to_be_bytes());

        let mut out = Vec::new();
        out.extend_from_slice(&[0xCA, 0xFE, 0xBA, 0xBE]);
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&55u16.to_be_bytes());
        b.finish_cp(&mut out);

        out.extend_from_slice(&0x8000u16.to_be_bytes());
        out.extend_from_slice(&this_cls.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&1u16.to_be_bytes());
        out.extend_from_slice(&attr_name.to_be_bytes());
        out.extend_from_slice(&u32::try_from(mod_attr.len()).unwrap().to_be_bytes());
        out.extend_from_slice(&mod_attr);
        out
    }

    fn write_zip_file(path: &Path, entries: &[(&str, &[u8])]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let file = fs::File::create(path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        for &(name, bytes) in entries {
            writer.start_file(name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap();
    }

    fn write_jmod_file(path: &Path, entries: &[(&str, &[u8])]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut cursor);
            let options = SimpleFileOptions::default();
            for &(name, bytes) in entries {
                writer.start_file(name, options).unwrap();
                writer.write_all(bytes).unwrap();
            }
            writer.finish().unwrap();
        }
        let mut jmod_bytes = JMOD_MAGIC.to_vec();
        jmod_bytes.extend_from_slice(&cursor.into_inner());
        fs::write(path, jmod_bytes).unwrap();
    }

    fn probe_with_deps(
        dir: &TestDir,
        jdk: Option<PathBuf>,
        release: Release,
        deps: Vec<ModuleDependency>,
    ) -> WorkspaceProbe {
        dir.write("src/Main.java", "class Main {}\n");
        let mut probe = probe_workspace(dir.path());
        probe.resolved_jdk = jdk;
        probe.resolved_level = LanguageLevel::new(release, false);
        probe.layout.modules[0].dependencies = deps;
        probe
    }

    #[test]
    fn jdk_version_key_parses_release_file_and_falls_back_to_dir_name() {
        let dir = TestDir::new();
        let jdk_with_release = dir.path().join("temurin-25");
        fs::create_dir_all(&jdk_with_release).unwrap();
        fs::write(
            jdk_with_release.join("release"),
            "IMPLEMENTOR=\"Eclipse Adoptium\"\nJAVA_VERSION=\"25.0.1\"\n",
        )
        .unwrap();
        assert_eq!(jdk_version_key(&jdk_with_release), "jdk-25.0.1");

        let jdk_without_release = dir.path().join("custom jdk@21");
        fs::create_dir_all(&jdk_without_release).unwrap();
        assert_eq!(jdk_version_key(&jdk_without_release), "custom_jdk_21");
    }

    #[test]
    fn load_classpath_indexes_jmods_filters_unexported_and_handles_corrupt_entries() {
        let dir = TestDir::new();
        let jdk_home = dir.path().join("fake-jdk");
        let base_mod = build_module_info_bytes("java.base", &["java/lang"], &[]);
        let pub_str = build_simple_class_bytes(
            65,
            0x0001,
            "java/lang/FakeString",
            "java/lang/Object",
            &[("length", "()I", 0x0001)],
            &[],
        );
        let unexported = build_simple_class_bytes(
            65,
            0x0001,
            "sun/nio/Secret",
            "java/lang/Object",
            &[],
            &[],
        );
        let internal = build_simple_class_bytes(
            65,
            0x0001,
            "jdk/internal/misc/Unsafe",
            "java/lang/Object",
            &[],
            &[],
        );

        write_jmod_file(
            &jdk_home.join("jmods/java.base.jmod"),
            &[
                ("classes/module-info.class", &base_mod),
                ("classes/java/lang/FakeString.class", &pub_str),
                ("classes/java/lang/Corrupt.class", b"not-a-classfile"),
                ("classes/sun/nio/Secret.class", &unexported),
                ("classes/jdk/internal/misc/Unsafe.class", &internal),
            ],
        );
        fs::write(jdk_home.join("jmods/corrupt.jmod"), b"bad-magic").unwrap();

        let probe = probe_with_deps(&dir, Some(jdk_home), Release::JAVA_21, vec![]);
        let (index, _) = load_classpath(&probe);

        assert!(index.modules.contains_key("java.base"));
        assert!(index.type_by_fqn("java.lang.FakeString").is_some());
        assert!(index.type_by_fqn("sun.nio.Secret").is_none());
        assert!(index.type_by_fqn("jdk.internal.misc.Unsafe").is_none());
    }

    #[test]
    fn load_classpath_indexes_jdk8_rt_jar_skipping_internal_packages() {
        let dir = TestDir::new();
        let jdk_home = dir.path().join("jdk8");
        let obj =
            build_simple_class_bytes(52, 0x0001, "java/lang/FakeObj", "java/lang/Object", &[], &[]);
        let sun_cls =
            build_simple_class_bytes(52, 0x0001, "sun/misc/Hidden", "java/lang/Object", &[], &[]);
        let com_sun_cls =
            build_simple_class_bytes(52, 0x0001, "com/sun/Hidden2", "java/lang/Object", &[], &[]);

        write_zip_file(
            &jdk_home.join("jre/lib/rt.jar"),
            &[
                ("java/lang/FakeObj.class", &obj),
                ("sun/misc/Hidden.class", &sun_cls),
                ("com/sun/Hidden2.class", &com_sun_cls),
            ],
        );

        let probe = probe_with_deps(&dir, Some(jdk_home), Release::JAVA_8, vec![]);
        let (index, _) = load_classpath(&probe);

        let ty = index
            .type_by_fqn("java.lang.FakeObj")
            .expect("FakeObj indexed");
        assert_eq!(ty.origin, BinaryOrigin::Jdk { module_name: None });
        assert!(index.type_by_fqn("sun.misc.Hidden").is_none());
        assert!(index.type_by_fqn("com.sun.Hidden2").is_none());
    }

    #[test]
    fn load_classpath_indexes_mrjar_class_dir_and_discovers_companions() {
        let dir = TestDir::new();
        let base_cls = build_simple_class_bytes(
            52,
            0x0001,
            "com/acme/Widget",
            "java/lang/Object",
            &[("legacy", "()V", 0x0001)],
            &[],
        );
        let v17_cls = build_simple_class_bytes(
            61,
            0x0001,
            "com/acme/Widget",
            "java/lang/Object",
            &[("modern", "()I", 0x0001)],
            &[],
        );
        let v25_cls = build_simple_class_bytes(
            69,
            0x0001,
            "com/acme/Widget",
            "java/lang/Object",
            &[("future", "()V", 0x0001)],
            &[],
        );

        let jar_path = dir.path().join("libs/acme-1.0.jar");
        write_zip_file(
            &jar_path,
            &[
                (
                    "META-INF/MANIFEST.MF",
                    b"Manifest-Version: 1.0\r\nMulti-Release: true\r\n",
                ),
                ("com/acme/Widget.class", &base_cls),
                ("META-INF/versions/17/com/acme/Widget.class", &v17_cls),
                ("META-INF/versions/25/com/acme/Widget.class", &v25_cls),
            ],
        );
        let sources_jar = dir.path().join("libs/acme-1.0-sources.jar");
        write_zip_file(
            &sources_jar,
            &[(
                "com/acme/Widget.java",
                b"package com.acme;\n/** Widget doc. */\npublic class Widget { public int modern() { return 1; } }\n",
            )],
        );
        let javadoc_jar = dir.path().join("libs/acme-1.0-javadoc.jar");
        write_zip_file(&javadoc_jar, &[]);

        let dir_cls = build_simple_class_bytes(
            61,
            0x0001,
            "com/local/Helper",
            "java/lang/Object",
            &[("ping", "()V", 0x0001)],
            &[],
        );
        let classes_dir = dir.path().join("build/classes");
        fs::create_dir_all(classes_dir.join("com/local")).unwrap();
        fs::write(classes_dir.join("com/local/Helper.class"), &dir_cls).unwrap();

        let probe = probe_with_deps(
            &dir,
            None,
            Release::JAVA_21,
            vec![
                ModuleDependency::Path {
                    path: "libs/acme-1.0.jar".to_string(),
                    scope: RootScope::Main,
                },
                ModuleDependency::Path {
                    path: "build/classes".to_string(),
                    scope: RootScope::Main,
                },
            ],
        );

        let (index, companions) = load_classpath(&probe);
        let widget = index
            .type_by_fqn("com.acme.Widget")
            .expect("Widget indexed");
        assert!(widget.members.iter().any(|m| m.name == "modern"));
        assert!(!widget.members.iter().any(|m| m.name == "legacy"));
        assert!(!widget.members.iter().any(|m| m.name == "future"));
        assert_eq!(
            widget.origin,
            BinaryOrigin::Jar {
                label: "libs/acme-1.0.jar".to_string()
            }
        );

        let comp = companions
            .get("libs/acme-1.0.jar")
            .expect("companions discovered");
        assert_eq!(comp.sources_jar.as_deref(), Some(sources_jar.as_path()));
        assert_eq!(comp.javadoc_jar.as_deref(), Some(javadoc_jar.as_path()));

        let helper = index
            .type_by_fqn("com.local.Helper")
            .expect("Helper from class dir");
        assert!(helper.members.iter().any(|m| m.name == "ping"));
    }

    #[test]
    fn materialize_binary_location_prefers_sources_and_falls_back_to_stub() {
        let dir = TestDir::new();
        let cache = dir.path().join("cache");

        // 1. JAR with companion sources.jar
        let jar_cls = build_simple_class_bytes(
            61,
            0x0001,
            "com/acme/Greeter",
            "java/lang/Object",
            &[("greet", "()V", 0x0001)],
            &[],
        );
        let jar_path = dir.path().join("libs/greeter.jar");
        write_zip_file(&jar_path, &[("com/acme/Greeter.class", &jar_cls)]);
        let sources_path = dir.path().join("libs/greeter-sources.jar");
        let greeter_src = "package com.acme;\npublic class Greeter {\n    public void greet() {}\n}\n";
        write_zip_file(&sources_path, &[("com/acme/Greeter.java", greeter_src.as_bytes())]);

        // 2. JAR without companion sources -> stub fallback
        let stub_cls = build_simple_class_bytes(
            61,
            0x0001,
            "com/acme/StubOnly",
            "java/lang/Object",
            &[("run", "()I", 0x0001)],
            &[],
        );
        let stub_jar = dir.path().join("libs/stubonly.jar");
        write_zip_file(&stub_jar, &[("com/acme/StubOnly.class", &stub_cls)]);

        // 3. JDK with src.zip
        let jdk_home = dir.path().join("jdk-21");
        let jdk_mod = build_module_info_bytes("java.base", &["java/lang"], &[]);
        let jdk_cls = build_simple_class_bytes(
            65,
            0x0001,
            "java/lang/FakeJdkType",
            "java/lang/Object",
            &[("value", "()I", 0x0001)],
            &[],
        );
        write_jmod_file(
            &jdk_home.join("jmods/java.base.jmod"),
            &[
                ("classes/module-info.class", &jdk_mod),
                ("classes/java/lang/FakeJdkType.class", &jdk_cls),
            ],
        );
        fs::write(jdk_home.join("release"), "JAVA_VERSION=\"21.0.2\"\n").unwrap();
        let jdk_src_text = "package java.lang;\npublic class FakeJdkType {\n    public int value() { return 0; }\n}\n";
        write_zip_file(
            &jdk_home.join("lib/src.zip"),
            &[("java.base/java/lang/FakeJdkType.java", jdk_src_text.as_bytes())],
        );

        let probe = probe_with_deps(
            &dir,
            Some(jdk_home.clone()),
            Release::JAVA_21,
            vec![
                ModuleDependency::Path {
                    path: "libs/greeter.jar".to_string(),
                    scope: RootScope::Main,
                },
                ModuleDependency::Path {
                    path: "libs/stubonly.jar".to_string(),
                    scope: RootScope::Main,
                },
            ],
        );
        let (index, companions) = load_classpath(&probe);

        let greeter_target = BinaryTarget {
            origin: BinaryOrigin::Jar {
                label: "libs/greeter.jar".to_string(),
            },
            internal_name: "com/acme/Greeter".to_string(),
            qualified_name: "com.acme.Greeter".to_string(),
            member_name: Some(ide::SmolStr::new("greet")),
        };
        let (path, src, range) = materialize_binary_location_in_cache_root(
            &cache,
            &index,
            Some(&jdk_home),
            &companions,
            &greeter_target,
        )
        .expect("materialized from sources.jar");
        assert!(path.starts_with(cache.join("jar-src")));
        assert!(path.is_file());
        assert_eq!(src, greeter_src);
        let range = range.expect("greet range");
        assert_eq!(
            &src[usize::from(range.start())..usize::from(range.end())],
            "greet"
        );

        let stub_target = BinaryTarget {
            origin: BinaryOrigin::Jar {
                label: "libs/stubonly.jar".to_string(),
            },
            internal_name: "com/acme/StubOnly".to_string(),
            qualified_name: "com.acme.StubOnly".to_string(),
            member_name: Some(ide::SmolStr::new("run")),
        };
        let (stub_path, stub_src, stub_range) = materialize_binary_location_in_cache_root(
            &cache,
            &index,
            Some(&jdk_home),
            &companions,
            &stub_target,
        )
        .expect("materialized stub fallback");
        assert!(stub_path.starts_with(cache.join("stubs")));
        assert!(stub_path.is_file());
        assert!(stub_src.contains("public int run()"));
        let stub_range = stub_range.expect("run range");
        assert_eq!(
            &stub_src[usize::from(stub_range.start())..usize::from(stub_range.end())],
            "run"
        );

        let jdk_target = BinaryTarget {
            origin: BinaryOrigin::Jdk {
                module_name: Some(ide::SmolStr::new("java.base")),
            },
            internal_name: "java/lang/FakeJdkType".to_string(),
            qualified_name: "java.lang.FakeJdkType".to_string(),
            member_name: Some(ide::SmolStr::new("value")),
        };
        let (jdk_path, jdk_src, jdk_range) = materialize_binary_location_in_cache_root(
            &cache,
            &index,
            Some(&jdk_home),
            &companions,
            &jdk_target,
        )
        .expect("materialized from jdk src.zip");
        assert!(jdk_path.starts_with(cache.join("jdk-src/jdk-21.0.2")));
        assert_eq!(jdk_src, jdk_src_text);
        let jdk_range = jdk_range.expect("value range");
        assert_eq!(
            &jdk_src[usize::from(jdk_range.start())..usize::from(jdk_range.end())],
            "value"
        );

        // Removing src.zip still reuses cached file on disk.
        fs::remove_file(jdk_home.join("lib/src.zip")).unwrap();
        let (reused_path, reused_src, _) = materialize_binary_location_in_cache_root(
            &cache,
            &index,
            Some(&jdk_home),
            &companions,
            &jdk_target,
        )
        .expect("reused cached jdk source");
        assert_eq!(reused_path, jdk_path);
        assert_eq!(reused_src, jdk_src_text);
    }

    #[test]
    fn lookup_binary_javadoc_uses_sources_jar_and_falls_back_to_javadoc_jar_html() {
        let dir = TestDir::new();
        let src_jar = dir.path().join("libs/doclib-sources.jar");
        write_zip_file(
            &src_jar,
            &[(
                "com/acme/FromSrc.java",
                b"package com.acme;\n/** Source javadoc for {@code FromSrc}. */\npublic class FromSrc {}\n",
            )],
        );

        let html_jar = dir.path().join("libs/htmlonly-javadoc.jar");
        write_zip_file(
            &html_jar,
            &[(
                "com/acme/FromHtml.html",
                br#"<html><body>
<section class="class-description" id="class-description">
<div class="block">HTML javadoc for <code>FromHtml</code>.</div>
</section>
<section class="detail" id="compute()">
<h3>compute</h3>
<div class="block">Computes a result.</div>
</section>
</body></html>"#,
            )],
        );

        let mut companions = JarCompanions::default();
        companions.insert(
            "libs/doclib.jar",
            JarCompanionPaths {
                sources_jar: Some(src_jar),
                javadoc_jar: None,
            },
        );
        companions.insert(
            "libs/htmlonly.jar",
            JarCompanionPaths {
                sources_jar: None,
                javadoc_jar: Some(html_jar),
            },
        );

        let src_target = BinaryTarget {
            origin: BinaryOrigin::Jar {
                label: "libs/doclib.jar".to_string(),
            },
            internal_name: "com/acme/FromSrc".to_string(),
            qualified_name: "com.acme.FromSrc".to_string(),
            member_name: None,
        };
        assert_eq!(
            lookup_binary_javadoc(None, &companions, &src_target).as_deref(),
            Some("Source javadoc for `FromSrc`.")
        );

        let html_target = BinaryTarget {
            origin: BinaryOrigin::Jar {
                label: "libs/htmlonly.jar".to_string(),
            },
            internal_name: "com/acme/FromHtml".to_string(),
            qualified_name: "com.acme.FromHtml".to_string(),
            member_name: Some(ide::SmolStr::new("compute")),
        };
        assert_eq!(
            lookup_binary_javadoc(None, &companions, &html_target).as_deref(),
            Some("Computes a result.")
        );
    }

    fn write_jimage_file(path: &Path, entries: &[(&str, &str, &str, &str, &[u8])]) {
        write_jimage_file_with_options(path, 0, false, false, entries);
    }

    fn write_jimage_file_with_options(
        path: &Path,
        minor_version: u16,
        little_endian: bool,
        include_v1_1_attr_and_compressed: bool,
        entries: &[(&str, &str, &str, &str, &[u8])],
    ) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut strings = vec![0u8];
        let mut intern = |s: &str| -> usize {
            if s.is_empty() {
                return 0;
            }
            let off = strings.len();
            strings.extend_from_slice(s.as_bytes());
            strings.push(0);
            off
        };

        let mut attr_offsets: Vec<i32> = Vec::with_capacity(entries.len());
        let mut attr_data = Vec::new();
        let mut data_section = Vec::new();

        let push_attr = |buf: &mut Vec<u8>, kind: u8, val: usize| {
            let be = (val as u64).to_be_bytes();
            let first_nonzero = be.iter().position(|&b| b != 0).unwrap_or(7);
            let used = &be[first_nonzero..];
            let len_code = u8::try_from(used.len() - 1).unwrap();
            buf.push((kind << 3) | len_code);
            buf.extend_from_slice(used);
        };

        for (idx, &(module, parent_pkg, base, ext, payload)) in entries.iter().enumerate() {
            let mod_off = intern(module);
            let par_off = intern(parent_pkg);
            let base_off = intern(base);
            let ext_off = intern(ext);
            let data_off = data_section.len();
            data_section.extend_from_slice(payload);

            attr_offsets.push(i32::try_from(attr_data.len()).unwrap());
            if mod_off != 0 {
                push_attr(&mut attr_data, 1, mod_off);
            }
            if par_off != 0 {
                push_attr(&mut attr_data, 2, par_off);
            }
            if base_off != 0 {
                push_attr(&mut attr_data, 3, base_off);
            }
            if ext_off != 0 {
                push_attr(&mut attr_data, 4, ext_off);
            }
            if include_v1_1_attr_and_compressed {
                // Attribute kind 8 (e.g. preview flags in v1.1) should be skipped transparently.
                push_attr(&mut attr_data, 8, 1);
                if idx + 1 == entries.len() {
                    // Mark last entry as compressed so reader skips it cleanly.
                    push_attr(&mut attr_data, 6, payload.len().max(1));
                }
            }
            push_attr(&mut attr_data, 5, data_off);
            push_attr(&mut attr_data, 7, payload.len());
            attr_data.push(0);
        }

        let encode_u32 = |v: u32| -> [u8; 4] {
            if little_endian {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            }
        };
        let encode_i32 = |v: i32| -> [u8; 4] {
            if little_endian {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            }
        };

        let count = u32::try_from(entries.len()).unwrap();
        let version = 0x0001_0000u32 | u32::from(minor_version);
        let mut out = Vec::new();
        out.extend_from_slice(&encode_u32(0xCAFE_DADA));
        out.extend_from_slice(&encode_u32(version));
        out.extend_from_slice(&encode_u32(0)); // flags
        out.extend_from_slice(&encode_u32(count)); // resource_count
        out.extend_from_slice(&encode_u32(count)); // table_length
        out.extend_from_slice(&encode_u32(u32::try_from(attr_data.len()).unwrap()));
        out.extend_from_slice(&encode_u32(u32::try_from(strings.len()).unwrap()));
        for i in 0..entries.len() {
            let redirect = -1 - i32::try_from(i).unwrap();
            out.extend_from_slice(&encode_i32(redirect));
        }
        for off in attr_offsets {
            out.extend_from_slice(&encode_i32(off));
        }
        out.extend_from_slice(&attr_data);
        out.extend_from_slice(&strings);
        out.extend_from_slice(&data_section);
        fs::write(path, out).unwrap();
    }

    #[test]
    fn load_classpath_indexes_jimage_v1_1_little_endian_with_unknown_attrs_and_compressed() {
        let dir = TestDir::new();
        let jdk_home = dir.path().join("fake-jimage-v11-jdk");
        let base_mod = build_module_info_bytes("java.base", &["java/lang"], &[]);
        let str_cls = build_simple_class_bytes(
            69,
            0x0001,
            "java/lang/PreviewString",
            "java/lang/Object",
            &[("isEmpty", "()Z", 0x0001)],
            &[],
        );
        let skipped_cls = build_simple_class_bytes(
            69,
            0x0001,
            "java/lang/CompressedType",
            "java/lang/Object",
            &[],
            &[],
        );

        write_jimage_file_with_options(
            &jdk_home.join("lib/modules"),
            1,
            true,
            true,
            &[
                ("java.base", "", "module-info", "class", &base_mod),
                ("java.base", "java/lang", "PreviewString", "class", &str_cls),
                (
                    "java.base",
                    "java/lang",
                    "CompressedType",
                    "class",
                    &skipped_cls,
                ),
            ],
        );

        let probe = probe_with_deps(&dir, Some(jdk_home), Release::JAVA_25, vec![]);
        let (index, _) = load_classpath(&probe);

        assert!(index.modules.contains_key("java.base"));
        let ty = index
            .type_by_fqn("java.lang.PreviewString")
            .expect("PreviewString indexed from v1.1 LE jimage");
        assert!(ty.members.iter().any(|m| m.name == "isEmpty"));
        assert!(index.type_by_fqn("java.lang.CompressedType").is_none());
    }

    #[test]
    fn load_classpath_indexes_jimage_modules_and_filters_entries() {
        let dir = TestDir::new();
        let jdk_home = dir.path().join("fake-jimage-jdk");
        let base_mod = build_module_info_bytes("java.base", &["java/lang", "java/util"], &[]);
        let str_cls = build_simple_class_bytes(
            65,
            0x0001,
            "java/lang/FakeString",
            "java/lang/Object",
            &[("length", "()I", 0x0001)],
            &[],
        );
        let entry_cls = build_simple_class_bytes(
            65,
            0x0001,
            "java/util/Map$Entry",
            "java/lang/Object",
            &[],
            &[],
        );
        let anon_cls = build_simple_class_bytes(
            65,
            0x0001,
            "java/lang/FakeString$1",
            "java/lang/Object",
            &[],
            &[],
        );
        let pkg_info = build_simple_class_bytes(
            65,
            0x1600,
            "java/lang/package-info",
            "java/lang/Object",
            &[],
            &[],
        );
        let unexported = build_simple_class_bytes(
            65,
            0x0001,
            "sun/nio/Secret",
            "java/lang/Object",
            &[],
            &[],
        );
        let internal = build_simple_class_bytes(
            65,
            0x0001,
            "jdk/internal/misc/Unsafe",
            "java/lang/Object",
            &[],
            &[],
        );

        write_jimage_file(
            &jdk_home.join("lib/modules"),
            &[
                ("packages", "java.lang", "java.base", "", b""),
                ("modules", "java.base", "java.lang", "", b""),
                ("java.base", "", "module-info", "class", &base_mod),
                ("java.base", "java/lang", "FakeString", "class", &str_cls),
                ("java.base", "java/util", "Map$Entry", "class", &entry_cls),
                ("java.base", "java/lang", "FakeString$1", "class", &anon_cls),
                ("java.base", "java/lang", "package-info", "class", &pkg_info),
                ("java.base", "java/lang", "Corrupt", "class", b"not-a-classfile"),
                ("java.base", "sun/nio", "Secret", "class", &unexported),
                ("java.base", "jdk/internal/misc", "Unsafe", "class", &internal),
            ],
        );

        let probe = probe_with_deps(&dir, Some(jdk_home), Release::JAVA_21, vec![]);
        let (index, _) = load_classpath(&probe);

        let base = index.modules.get("java.base").expect("java.base module");
        assert!(base.exports.iter().any(|p| p == "java.lang"));
        assert!(base.exports.iter().any(|p| p == "java.util"));

        let fake_str = index
            .type_by_fqn("java.lang.FakeString")
            .expect("FakeString indexed");
        assert_eq!(
            fake_str.origin,
            BinaryOrigin::Jdk {
                module_name: Some(SmolStr::new("java.base"))
            }
        );
        assert!(fake_str.members.iter().any(|m| m.name == "length"));
        assert!(index.type_by_fqn("java.util.Map$Entry").is_some());
        assert!(index.type_by_fqn("java.lang.FakeString$1").is_none());
        assert!(index.type_by_fqn("java.lang.package-info").is_none());
        assert!(index.type_by_fqn("sun.nio.Secret").is_none());
        assert!(index.type_by_fqn("jdk.internal.misc.Unsafe").is_none());
    }

    #[test]
    fn load_classpath_handles_corrupt_jimage_modules_gracefully() {
        let dir = TestDir::new();
        let jdk_home = dir.path().join("corrupt-jimage-jdk");
        fs::create_dir_all(jdk_home.join("lib")).unwrap();
        fs::write(jdk_home.join("lib/modules"), b"not-a-jimage").unwrap();

        let probe = probe_with_deps(&dir, Some(jdk_home), Release::JAVA_21, vec![]);
        let (index, _) = load_classpath(&probe);
        assert_eq!(index.type_count(), 0);
        assert_eq!(index.module_count(), 0);
    }

    #[test]
    fn ct_sym_version_char_encodes_base36_uppercase() {
        assert_eq!(ct_sym_version_char(Release::JAVA_8), Some('8'));
        assert_eq!(ct_sym_version_char(Release::JAVA_9), Some('9'));
        assert_eq!(ct_sym_version_char(Release::JAVA_10), Some('A'));
        assert_eq!(ct_sym_version_char(Release::JAVA_17), Some('H'));
        assert_eq!(ct_sym_version_char(Release::JAVA_21), Some('L'));
        assert_eq!(ct_sym_version_char(Release::JAVA_26), Some('Q'));
        assert_eq!(ct_sym_version_char(Release::JAVA_27), Some('R'));
    }

    #[test]
    fn load_classpath_uses_ct_sym_for_historical_release_and_falls_back_when_absent() {
        let dir = TestDir::new();
        let jdk_home = dir.path().join("jdk-21");
        fs::create_dir_all(&jdk_home).unwrap();
        fs::write(jdk_home.join("release"), "JAVA_VERSION=\"21.0.2\"\n").unwrap();

        // Fallback JDK 21 jimage contains `FutureOnly` (not in ct.sym for 8/11/17).
        let jimage_mod = build_module_info_bytes("java.base", &["java/lang"], &[]);
        let jimage_future = build_simple_class_bytes(
            65,
            0x0001,
            "java/lang/FutureOnly",
            "java/lang/Object",
            &[],
            &[],
        );
        write_jimage_file(
            &jdk_home.join("lib/modules"),
            &[
                ("java.base", "", "module-info", "class", &jimage_mod),
                ("java.base", "java/lang", "FutureOnly", "class", &jimage_future),
            ],
        );

        let str_v8 = build_simple_class_bytes(
            52,
            0x0001,
            "java/lang/FakeString",
            "java/lang/Object",
            &[("length", "()I", 0x0001)],
            &[],
        );
        let str_v11 = build_simple_class_bytes(
            55,
            0x0001,
            "java/lang/FakeString",
            "java/lang/Object",
            &[("length", "()I", 0x0001), ("isBlank", "()Z", 0x0001)],
            &[],
        );
        let obj_shared = build_simple_class_bytes(
            52,
            0x0001,
            "java/lang/FakeObject",
            "java/lang/Object",
            &[],
            &[],
        );
        let sun_cls = build_simple_class_bytes(
            55,
            0x0001,
            "sun/nio/Hidden",
            "java/lang/Object",
            &[],
            &[],
        );
        let mod_info_9b = build_module_info_bytes("java.base", &["java/lang"], &[]);

        write_zip_file(
            &jdk_home.join("lib/ct.sym"),
            &[
                ("89A/java.base/java/lang/FakeString.sig", &str_v8),
                ("BC/java.base/java/lang/FakeString.sig", &str_v11),
                ("89ABC/java.base/java/lang/FakeObject.sig", &obj_shared),
                ("BC/java.base/sun/nio/Hidden.sig", &sun_cls),
                ("9ABC/java.base/module-info.sig", &mod_info_9b),
                ("B-modules/java.base/system-modules", b"ignored"),
                ("META-INF/MANIFEST.MF", b"Manifest-Version: 1.0\n"),
            ],
        );

        // Target Java 8 -> matches 89A + 89ABC, module_name = None, no isBlank.
        let probe_8 = probe_with_deps(&dir, Some(jdk_home.clone()), Release::JAVA_8, vec![]);
        let (idx_8, _) = load_classpath(&probe_8);
        let s8 = idx_8
            .type_by_fqn("java.lang.FakeString")
            .expect("FakeString at Java 8");
        assert_eq!(s8.origin, BinaryOrigin::Jdk { module_name: None });
        assert!(s8.members.iter().any(|m| m.name == "length"));
        assert!(!s8.members.iter().any(|m| m.name == "isBlank"));
        assert!(idx_8.type_by_fqn("java.lang.FakeObject").is_some());
        assert!(idx_8.type_by_fqn("java.lang.FutureOnly").is_none());
        assert_eq!(idx_8.module_count(), 0);

        // Target Java 11 ('B') -> matches BC + 89ABC + 9ABC module-info, has isBlank & java.base.
        let probe_11 = probe_with_deps(&dir, Some(jdk_home.clone()), Release::JAVA_11, vec![]);
        let (idx_11, _) = load_classpath(&probe_11);
        let s11 = idx_11
            .type_by_fqn("java.lang.FakeString")
            .expect("FakeString at Java 11");
        assert_eq!(
            s11.origin,
            BinaryOrigin::Jdk {
                module_name: Some(SmolStr::new("java.base"))
            }
        );
        assert!(s11.members.iter().any(|m| m.name == "isBlank"));
        assert!(idx_11.type_by_fqn("sun.nio.Hidden").is_none());
        assert!(idx_11.type_by_fqn("java.lang.FutureOnly").is_none());
        let base_11 = idx_11.modules.get("java.base").expect("java.base module");
        assert!(base_11.exports.iter().any(|p| p == "java.lang"));

        // Target Java 17 ('H') is absent from synthetic ct.sym -> falls back to lib/modules.
        let probe_17 = probe_with_deps(&dir, Some(jdk_home), Release::JAVA_17, vec![]);
        let (idx_17, _) = load_classpath(&probe_17);
        assert!(idx_17.type_by_fqn("java.lang.FutureOnly").is_some());
        assert!(idx_17.type_by_fqn("java.lang.FakeString").is_none());
    }

    #[test]
    fn real_local_jdk_ct_sym_smoke_test_if_installed() {
        let jdks = discover_jdks(None);
        let Some(jdk) = jdks.into_iter().find(|j| {
            j.home.join("lib/ct.sym").is_file()
                && probe_jdk_release(&j.home).is_some_and(|r| r >= Release::JAVA_18)
        }) else {
            return;
        };

        let dir = TestDir::new();
        let probe_8 = probe_with_deps(&dir, Some(jdk.home.clone()), Release::JAVA_8, vec![]);
        let (idx_8, _) = load_classpath(&probe_8);
        eprintln!(
            "real_local_jdk_ct_sym_smoke_test ({}, JAVA_8): types={}, modules={}",
            jdk.home.display(),
            idx_8.type_count(),
            idx_8.module_count()
        );
        let str_8 = idx_8
            .type_by_fqn("java.lang.String")
            .expect("expected java.lang.String in ct.sym at JAVA_8");
        assert!(str_8.members.iter().any(|m| m.name == "length"));
        assert!(
            !str_8.members.iter().any(|m| m.name == "isBlank"),
            "java.lang.String#isBlank should not exist at JAVA_8"
        );
        assert!(
            idx_8.type_by_fqn("java.lang.Record").is_none(),
            "java.lang.Record should not exist at JAVA_8"
        );

        let probe_17 = probe_with_deps(&dir, Some(jdk.home.clone()), Release::JAVA_17, vec![]);
        let (idx_17, _) = load_classpath(&probe_17);
        eprintln!(
            "real_local_jdk_ct_sym_smoke_test ({}, JAVA_17): types={}, modules={}",
            jdk.home.display(),
            idx_17.type_count(),
            idx_17.module_count()
        );
        let str_17 = idx_17
            .type_by_fqn("java.lang.String")
            .expect("expected java.lang.String in ct.sym at JAVA_17");
        assert!(
            str_17.members.iter().any(|m| m.name == "isBlank"),
            "expected java.lang.String#isBlank in ct.sym at JAVA_17"
        );
        assert!(
            idx_17.type_by_fqn("java.lang.Record").is_some(),
            "expected java.lang.Record in ct.sym at JAVA_17"
        );
        assert!(idx_17.modules.contains_key("java.base"));
    }

    #[test]
    fn real_local_jdk_smoke_test_if_installed() {
        let jdks = discover_jdks(None);
        let Some(jdk) = jdks.first() else {
            return;
        };
        let has_jdk_archive = jdk.home.join("jmods").is_dir()
            || jdk.home.join("lib/modules").is_file()
            || jdk.home.join("jre/lib/rt.jar").is_file()
            || jdk.home.join("lib/rt.jar").is_file();
        if !has_jdk_archive {
            return;
        }

        let dir = TestDir::new();
        let probe = probe_with_deps(
            &dir,
            Some(jdk.home.clone()),
            jdk.release.unwrap_or(Release::JAVA_21),
            vec![],
        );
        let (index, _) = load_classpath(&probe);
        eprintln!(
            "real_local_jdk_smoke_test_if_installed ({}): types={}, modules={}",
            jdk.home.display(),
            index.type_count(),
            index.module_count()
        );

        let string_ty = index
            .type_by_fqn("java.lang.String")
            .unwrap_or_else(|| panic!("expected java.lang.String in real JDK {:?}", jdk.home));
        assert!(
            string_ty.members.iter().any(|m| m.name == "length"),
            "expected java.lang.String#length in real JDK {:?}",
            jdk.home
        );
        assert!(
            index.type_by_fqn("java.util.List").is_some(),
            "expected java.util.List in real JDK {:?}",
            jdk.home
        );
        assert!(
            index.type_by_fqn("java.util.Map.Entry").is_some(),
            "expected java.util.Map.Entry in real JDK {:?}",
            jdk.home
        );
        assert!(
            index.type_count() > 1000,
            "expected > 1000 types in real JDK {:?}, got {}",
            jdk.home,
            index.type_count()
        );
        if jdk.home.join("jmods").is_dir() || jdk.home.join("lib/modules").is_file() {
            assert!(
                index.module_count() >= 1,
                "expected >= 1 module in real JDK {:?}",
                jdk.home
            );
            let base = index
                .modules
                .get("java.base")
                .expect("expected java.base module");
            assert!(base.exports.iter().any(|p| p == "java.lang"));
            assert!(base.exports.iter().any(|p| p == "java.util"));
        }
    }
}
