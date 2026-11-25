//! JDK 9+ `lib/modules` (`jimage`) module and exported-class indexing.

use std::path::Path;

use ide::{BinaryOrigin, ClasspathIndex, ParsedClassfile, SmolStr, parse_class_bytes};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::jimage::Jimage;

use super::jdk::{
    ClassParseTask, is_anonymous_or_local_class_base, is_jdk_internal_package,
    parse_jdk_class_tasks_parallel,
};

pub(super) fn load_jimage_modules(index: &mut ClasspathIndex, modules_path: &Path) {
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
