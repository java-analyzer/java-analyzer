mod binary;
mod diagnostics;
mod document_symbols;
mod hover;
mod navigation;
pub mod diag_snapshot;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use hir::{DefDatabase, SourceDatabase};

pub use binary::{
    append_javadoc_to_hover, extract_javadoc_from_html, extract_javadoc_from_source,
    locate_binary_symbol_in_source, synthesize_binary_stub,
};
pub use diagnostics::{
    Diagnostic, RelatedInformation, Severity, classify_syntax_error, diagnostics,
    diagnostics_with_layout,
};
pub use document_symbols::{StructureNode, SymbolKind, document_symbols};
pub use hir::{
    BinaryMemberSymbol, BinaryModuleInfo, BinaryOrigin, BinaryTarget, BinaryTypeSymbol,
    Cancellable, Cancelled, ClassfileError, ClasspathIndex, DeltaSet, FileId, LanguageLevel,
    MemberKind, ModuleData, ModuleDelta, ModuleDependency, ModuleId, ModuleTarget, Nullability,
    NullabilitySource, NullnessScope, ParamSummary, ParsedClassfile, Release, ResourceRoot,
    RootScope, SmolStr, SourceRoot, TypeKind, WorkspaceDelta, WorkspaceLayout,
    infer_workspace_layout_from_sources, is_path_within, normalize_rel_path, parse_class_bytes,
    parse_manifest_multi_release, parse_shebang_level, select_mrjar_entries,
};
pub use hover::{HoverResult, hover};
pub use navigation::{
    NavigationTarget, TypeHierarchyNode, goto_definition, goto_implementation,
    goto_type_definition, prepare_type_hierarchy, type_hierarchy_subtypes,
    type_hierarchy_supertypes,
};
pub use syntax::TextRange;

#[derive(Default)]
pub struct AnalysisHost {
    db: hir::RootDatabase,
}

impl AnalysisHost {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn analysis(&self) -> Analysis {
        Analysis {
            db: self.db.clone(),
        }
    }

    pub fn set_file_text(&mut self, file_id: FileId, text: Arc<String>) {
        self.db.set_file_text(file_id, text);
    }

    pub fn set_file_language_level(&mut self, file_id: FileId, level: LanguageLevel) {
        self.db.set_file_language_level(file_id, level);
    }

    pub fn set_workspace_files(&mut self, file_ids: Arc<Vec<FileId>>) {
        self.db.set_workspace_files(file_ids);
    }

    pub fn set_workspace_modules(&mut self, modules: Vec<(ModuleId, Arc<Vec<FileId>>)>) {
        self.db.set_workspace_modules(modules);
    }

    pub fn set_classpath_index(&mut self, index: Arc<ClasspathIndex>) {
        self.db.set_classpath_index(index);
    }

    pub fn remove_file(&mut self, file_id: FileId) {
        self.db.remove_file(file_id);
    }
}

pub struct Analysis {
    db: hir::RootDatabase,
}

impl Analysis {
    pub fn file_text(&self, file_id: FileId) -> Cancellable<Arc<String>> {
        hir::catch_cancelled(|| self.db.file_text(file_id))
    }

    pub fn hover(&self, file_id: FileId, offset: u32) -> Cancellable<Option<HoverResult>> {
        hir::catch_cancelled(|| hover(&self.db, file_id, offset))
    }

    pub fn goto_definition(
        &self,
        file_id: FileId,
        offset: u32,
    ) -> Cancellable<Option<NavigationTarget>> {
        hir::catch_cancelled(|| goto_definition(&self.db, file_id, offset))
    }

    pub fn goto_implementation(
        &self,
        file_id: FileId,
        offset: u32,
    ) -> Cancellable<Vec<NavigationTarget>> {
        hir::catch_cancelled(|| goto_implementation(&self.db, file_id, offset))
    }

    pub fn goto_type_definition(
        &self,
        file_id: FileId,
        offset: u32,
    ) -> Cancellable<Option<NavigationTarget>> {
        hir::catch_cancelled(|| goto_type_definition(&self.db, file_id, offset))
    }

    pub fn prepare_type_hierarchy(
        &self,
        file_id: FileId,
        offset: u32,
    ) -> Cancellable<Option<TypeHierarchyNode>> {
        hir::catch_cancelled(|| prepare_type_hierarchy(&self.db, file_id, offset))
    }

    pub fn type_hierarchy_supertypes(&self, fqn: &str) -> Cancellable<Vec<TypeHierarchyNode>> {
        hir::catch_cancelled(|| type_hierarchy_supertypes(&self.db, fqn))
    }

    pub fn type_hierarchy_subtypes(&self, fqn: &str) -> Cancellable<Vec<TypeHierarchyNode>> {
        hir::catch_cancelled(|| type_hierarchy_subtypes(&self.db, fqn))
    }

    pub fn document_symbols(&self, file_id: FileId) -> Cancellable<Vec<StructureNode>> {
        hir::catch_cancelled(|| document_symbols(&self.db, file_id))
    }

    pub fn diagnostics(&self, file_id: FileId) -> Cancellable<Vec<Diagnostic>> {
        hir::catch_cancelled(|| diagnostics(&self.db, file_id))
    }

    pub fn diagnostics_with_layout(
        &self,
        file_id: FileId,
        layout: Option<(&WorkspaceLayout, &str)>,
    ) -> Cancellable<Vec<Diagnostic>> {
        hir::catch_cancelled(|| diagnostics_with_layout(&self.db, file_id, layout))
    }

    pub fn workspace_index_stats(&self) -> Cancellable<(usize, usize)> {
        hir::catch_cancelled(|| {
            let index = self.db.workspace_index();
            (index.package_count(), index.type_count())
        })
    }

    pub fn classpath_index_stats(&self) -> Cancellable<(usize, usize)> {
        hir::catch_cancelled(|| {
            let index = self.db.classpath_index();
            (index.type_count(), index.module_count())
        })
    }
}
