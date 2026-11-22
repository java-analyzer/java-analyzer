pub mod classfile;
pub mod classpath;
pub mod hierarchy;
pub(crate) mod item_tree;
pub mod resolve;
pub(crate) mod symbols;
#[cfg(any(test, feature = "test-utils"))]
pub mod test_fixture;
pub mod workspace;

use std::sync::Arc;

use rustc_hash::FxHashMap;
use salsa::Setter as _;

pub use classfile::{ClassfileError, ParsedClassfile, parse_class_bytes};
pub use classpath::{
    BinaryMemberSymbol, BinaryModuleInfo, BinaryOrigin, BinaryTarget, BinaryTypeSymbol,
    ClasspathIndex, parse_manifest_multi_release, select_mrjar_entries,
};
pub use hierarchy::{
    DirectSupertypes, WorkspaceHierarchyIndex, all_subtypes, direct_subtypes, direct_supertypes,
    find_overridden_method, find_overriding_methods, linearized_supertypes,
};
pub use item_tree::{
    AstId, AstIdMap, AstRange, FileItemTree, ImportId, ImportTree, MemberId, MemberItem, TypeId,
    TypeItem,
};
pub use resolve::{
    LocalKind, ResolvedSymbol, ResolvedSymbolKind, SymbolDetail, resolve_at_offset,
    resolve_type_of_symbol_at_offset,
};
pub use smol_str::SmolStr;
pub use symbols::{
    FileSymbols, ImportDecl, ImportKind, MemberKind, MemberSymbol, ModuleIndex, ParamSummary,
    TypeKind, TypeSymbol, WorkspaceIndex,
};
pub use syntax::{
    LanguageLevel, Parse, Release, SyntaxError, SyntaxNode, TextRange, TextSize,
    parse_shebang_level,
};
pub use workspace::{
    DeltaSet, FileHeader, ModuleData, ModuleDelta, ModuleDependency, ModuleHeader, ModuleId,
    ModuleTarget, PackageMismatch, ResourceRoot, ResourceRootId, RootScope, SourceRoot,
    SourceRootId, WorkspaceDelta, WorkspaceLayout, extract_file_header, infer_workspace_layout,
    infer_workspace_layout_from_sources, is_path_within, normalize_rel_path,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileId(pub u32);

#[salsa::input]
pub(crate) struct SourceFile {
    pub(crate) file_id: FileId,
    pub(crate) text: Arc<String>,
    pub(crate) language_level: LanguageLevel,
}

#[salsa::input]
pub(crate) struct ModuleFiles {
    pub(crate) module_id: ModuleId,
    pub(crate) files: Arc<Vec<SourceFile>>,
}

#[salsa::input]
pub(crate) struct WorkspaceModules {
    pub(crate) modules: Arc<Vec<ModuleFiles>>,
}

#[salsa::input]
pub(crate) struct ClasspathInput {
    pub(crate) index: Arc<ClasspathIndex>,
}

#[salsa::tracked]
fn parse_file(db: &dyn salsa::Database, file: SourceFile) -> Parse {
    syntax::parse_with_level(file.text(db), *file.language_level(db))
}

#[salsa::tracked]
fn file_syntax_errors(db: &dyn salsa::Database, file: SourceFile) -> Arc<Vec<SyntaxError>> {
    Arc::new(parse_file(db, file).errors().to_vec())
}

#[salsa::tracked]
fn file_item_tree(db: &dyn salsa::Database, file: SourceFile) -> Arc<FileItemTree> {
    let root = parse_file(db, file).syntax_node();
    let (tree, _map) = item_tree::lower_file_item_tree(*file.file_id(db), &root);
    Arc::new(tree)
}

#[salsa::tracked]
fn file_ast_id_map(db: &dyn salsa::Database, file: SourceFile) -> Arc<AstIdMap> {
    let root = parse_file(db, file).syntax_node();
    let (_tree, map) = item_tree::lower_file_item_tree(*file.file_id(db), &root);
    Arc::new(map)
}

#[salsa::tracked]
fn extract_symbols(db: &dyn salsa::Database, file: SourceFile) -> Arc<FileSymbols> {
    let tree = file_item_tree(db, file);
    let map = file_ast_id_map(db, file);
    Arc::new(item_tree::file_symbols_from_item_tree(tree, map))
}

#[salsa::tracked]
fn build_module_index_query(db: &dyn salsa::Database, module: ModuleFiles) -> Arc<ModuleIndex> {
    let files = module.files(db);
    let per_file: Vec<Arc<FileItemTree>> = files
        .iter()
        .map(|&f| file_item_tree(db, f).clone())
        .collect();
    Arc::new(symbols::build_module_index(&per_file))
}

#[salsa::tracked]
fn build_workspace_index_query(
    db: &dyn salsa::Database,
    workspace: WorkspaceModules,
) -> Arc<WorkspaceIndex> {
    let modules = workspace.modules(db);
    let per_module: Vec<Arc<ModuleIndex>> = modules
        .iter()
        .map(|&m| build_module_index_query(db, m).clone())
        .collect();
    Arc::new(symbols::merge_module_indexes(&per_module))
}

#[salsa::tracked]
fn build_workspace_hierarchy_query(
    db: &dyn salsa::Database,
    workspace: WorkspaceModules,
    classpath: Option<ClasspathInput>,
) -> Arc<WorkspaceHierarchyIndex> {
    let workspace_index = build_workspace_index_query(db, workspace);
    let modules = workspace.modules(db);
    let mut per_file = Vec::new();
    for &module in modules.iter() {
        for &file in module.files(db).iter() {
            per_file.push(file_item_tree(db, file).clone());
        }
    }
    let empty_cp = ClasspathIndex::default();
    let cp_ref = match classpath {
        Some(cp) => cp.index(db).as_ref(),
        None => &empty_cp,
    };
    Arc::new(hierarchy::build_workspace_hierarchy_index(
        &per_file,
        workspace_index,
        cp_ref,
    ))
}

pub trait SourceDatabase: salsa::Database {
    fn file_text(&self, file_id: FileId) -> Arc<String>;

    fn file_language_level(&self, file_id: FileId) -> LanguageLevel;

    /// The parse result of a file. Cheap to clone; recomputed only when the file text or language
    /// level changes.
    fn parse(&self, file_id: FileId) -> Parse;

    /// Root of the syntax tree of a file.
    fn syntax_tree(&self, file_id: FileId) -> SyntaxNode;

    /// Lexical and syntax errors of a file, ordered by position.
    fn syntax_errors(&self, file_id: FileId) -> Arc<Vec<SyntaxError>>;
}

pub trait DefDatabase: SourceDatabase {
    /// Ordered list of all `.java` files currently in the workspace.
    fn workspace_files(&self) -> Arc<Vec<FileId>>;

    /// Position-independent structural item tree for a single file.
    fn file_item_tree(&self, file_id: FileId) -> Arc<FileItemTree>;

    /// Source byte ranges keyed by [`AstId`] for a single file.
    fn ast_id_map(&self, file_id: FileId) -> Arc<AstIdMap>;

    /// Declared package, imports, types, and members extracted from a single file.
    fn file_symbols(&self, file_id: FileId) -> Arc<FileSymbols>;

    /// Position-independent lookup index for a single workspace module.
    fn module_index(&self, module_id: ModuleId) -> Arc<ModuleIndex>;

    /// Aggregated workspace-wide lookup index over `workspace_files`.
    fn workspace_index(&self) -> Arc<WorkspaceIndex>;

    /// Position-independent workspace type hierarchy index.
    fn workspace_hierarchy_index(&self) -> Arc<WorkspaceHierarchyIndex>;

    /// Indexed JDK and external JAR binary symbols.
    fn classpath_index(&self) -> Arc<ClasspathIndex>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("query cancelled")
    }
}

impl std::error::Error for Cancelled {}

pub type Cancellable<T> = Result<T, Cancelled>;

pub fn catch_cancelled<F, T>(f: F) -> Cancellable<T>
where
    F: FnOnce() -> T,
{
    salsa::Cancelled::catch(std::panic::AssertUnwindSafe(f)).map_err(|_| Cancelled)
}

#[salsa::db]
#[derive(Clone, Default)]
pub struct RootDatabase {
    storage: salsa::Storage<Self>,
    files: FxHashMap<FileId, SourceFile>,
    module_files: FxHashMap<ModuleId, ModuleFiles>,
    workspace_modules: Option<WorkspaceModules>,
    classpath_input: Option<ClasspathInput>,
}

#[salsa::db]
impl salsa::Database for RootDatabase {}

impl RootDatabase {
    fn source_file(&self, file_id: FileId) -> SourceFile {
        *self
            .files
            .get(&file_id)
            .unwrap_or_else(|| panic!("unknown file id: {file_id:?}"))
    }

    fn ensure_source_file(&mut self, file_id: FileId) -> SourceFile {
        if let Some(&file) = self.files.get(&file_id) {
            return file;
        }
        let file = SourceFile::new(
            self,
            file_id,
            Arc::new(String::new()),
            LanguageLevel::latest(),
        );
        self.files.insert(file_id, file);
        file
    }

    pub fn set_file_text(&mut self, file_id: FileId, text: Arc<String>) {
        if let Some(&file) = self.files.get(&file_id) {
            file.set_text(self).to(text);
        } else {
            let file = SourceFile::new(self, file_id, text, LanguageLevel::latest());
            self.files.insert(file_id, file);
        }
    }

    pub fn set_file_language_level(&mut self, file_id: FileId, level: LanguageLevel) {
        if let Some(&file) = self.files.get(&file_id) {
            file.set_language_level(self).to(level);
        } else {
            let file = SourceFile::new(self, file_id, Arc::new(String::new()), level);
            self.files.insert(file_id, file);
        }
    }

    pub fn set_workspace_files(&mut self, file_ids: Arc<Vec<FileId>>) {
        self.set_workspace_modules(vec![(ModuleId(0), file_ids)]);
    }

    pub fn set_workspace_modules(&mut self, modules: Vec<(ModuleId, Arc<Vec<FileId>>)>) {
        let mut active_ids = Vec::with_capacity(modules.len());
        let mut mod_inputs = Vec::with_capacity(modules.len());

        for (module_id, file_ids) in modules {
            active_ids.push(module_id);
            let files: Vec<SourceFile> = file_ids
                .iter()
                .map(|&id| self.ensure_source_file(id))
                .collect();

            let mod_files = if let Some(&existing) = self.module_files.get(&module_id) {
                if existing.files(self).as_slice() != files.as_slice() {
                    existing.set_files(self).to(Arc::new(files));
                }
                existing
            } else {
                let created = ModuleFiles::new(self, module_id, Arc::new(files));
                self.module_files.insert(module_id, created);
                created
            };
            mod_inputs.push(mod_files);
        }

        self.module_files.retain(|id, _| active_ids.contains(id));

        if let Some(ws) = self.workspace_modules {
            if ws.modules(self).as_slice() != mod_inputs.as_slice() {
                ws.set_modules(self).to(Arc::new(mod_inputs));
            }
        } else {
            self.workspace_modules = Some(WorkspaceModules::new(self, Arc::new(mod_inputs)));
        }
    }

    /// Replaces the indexed binary JDK and JAR classpath symbols, preserving Salsa early-cutoff
    /// when the new [`ClasspathIndex`] equals the existing one.
    pub fn set_classpath_index(&mut self, index: Arc<ClasspathIndex>) {
        if let Some(existing) = self.classpath_input {
            if existing.index(self).as_ref() != index.as_ref() {
                existing.set_index(self).to(index);
            }
        } else {
            self.classpath_input = Some(ClasspathInput::new(self, index));
        }
    }

    /// Removes `file_id` from the tracked file map and prunes it from any module file lists.
    /// Does nothing if `file_id` is absent.
    pub fn remove_file(&mut self, file_id: FileId) {
        let Some(removed) = self.files.remove(&file_id) else {
            return;
        };
        let module_inputs: Vec<ModuleFiles> = self.module_files.values().copied().collect();
        for module in module_inputs {
            let current = module.files(self);
            if current.contains(&removed) {
                let filtered: Vec<SourceFile> =
                    current.iter().copied().filter(|&f| f != removed).collect();
                module.set_files(self).to(Arc::new(filtered));
            }
        }
    }
}

impl SourceDatabase for RootDatabase {
    fn file_text(&self, file_id: FileId) -> Arc<String> {
        self.source_file(file_id).text(self).clone()
    }

    fn file_language_level(&self, file_id: FileId) -> LanguageLevel {
        *self.source_file(file_id).language_level(self)
    }

    fn parse(&self, file_id: FileId) -> Parse {
        parse_file(self, self.source_file(file_id)).clone()
    }

    fn syntax_tree(&self, file_id: FileId) -> SyntaxNode {
        self.parse(file_id).syntax_node()
    }

    fn syntax_errors(&self, file_id: FileId) -> Arc<Vec<SyntaxError>> {
        file_syntax_errors(self, self.source_file(file_id)).clone()
    }
}

impl DefDatabase for RootDatabase {
    fn workspace_files(&self) -> Arc<Vec<FileId>> {
        match self.workspace_modules {
            Some(ws) => {
                let mut ids = Vec::new();
                for &module in ws.modules(self).iter() {
                    for &file in module.files(self).iter() {
                        ids.push(*file.file_id(self));
                    }
                }
                Arc::new(ids)
            }
            None => Arc::new(Vec::new()),
        }
    }

    fn file_item_tree(&self, file_id: FileId) -> Arc<FileItemTree> {
        file_item_tree(self, self.source_file(file_id)).clone()
    }

    fn ast_id_map(&self, file_id: FileId) -> Arc<AstIdMap> {
        file_ast_id_map(self, self.source_file(file_id)).clone()
    }

    fn file_symbols(&self, file_id: FileId) -> Arc<FileSymbols> {
        extract_symbols(self, self.source_file(file_id)).clone()
    }

    fn module_index(&self, module_id: ModuleId) -> Arc<ModuleIndex> {
        match self.module_files.get(&module_id) {
            Some(&module) => build_module_index_query(self, module).clone(),
            None => Arc::new(ModuleIndex::default()),
        }
    }

    fn workspace_index(&self) -> Arc<WorkspaceIndex> {
        match self.workspace_modules {
            Some(ws) => build_workspace_index_query(self, ws).clone(),
            None => Arc::new(WorkspaceIndex::default()),
        }
    }

    fn workspace_hierarchy_index(&self) -> Arc<WorkspaceHierarchyIndex> {
        match self.workspace_modules {
            Some(ws) => build_workspace_hierarchy_query(self, ws, self.classpath_input).clone(),
            None => Arc::new(WorkspaceHierarchyIndex::default()),
        }
    }

    fn classpath_index(&self) -> Arc<ClasspathIndex> {
        match self.classpath_input {
            Some(input) => input.index(self).clone(),
            None => Arc::new(ClasspathIndex::default()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixture::{cursor, db_with, db_with_files, slice};
    use syntax::syntax_kind::SyntaxKind;

    #[test]
    fn syntax_tree_of_a_file() {
        let (db, file_id) = db_with("class Main {}");
        assert_eq!(db.syntax_tree(file_id).kind(), SyntaxKind::SOURCE_FILE);
        assert!(db.syntax_errors(file_id).is_empty());
    }

    #[test]
    fn syntax_errors_of_a_broken_file() {
        let (db, file_id) = db_with("class Main {");
        let errors = db.syntax_errors(file_id);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].message, "expected '}'");
    }

    #[test]
    fn results_follow_text_changes() {
        let (mut db, file_id) = db_with("class Main {");
        assert_eq!(db.syntax_errors(file_id).len(), 1);

        db.set_file_text(file_id, Arc::new("class Main {}".to_string()));
        assert!(db.syntax_errors(file_id).is_empty());
    }

    #[test]
    fn results_follow_language_level_changes() {
        let (mut db, file_id) = db_with("record R(int x) {}");
        assert!(db.syntax_errors(file_id).is_empty());

        db.set_file_language_level(file_id, LanguageLevel::new(Release::JAVA_11, false));
        let errors = db.syntax_errors(file_id);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].message, "records require language level 16");
    }

    #[test]
    fn extract_all_type_kinds_imports_and_members() {
        let text = "\
package com.example;

import a.b.Foo;
import a.b.*;
import static a.b.Foo.BAR;
import static a.b.Foo.*;
import module java.base;

public class Outer<T> {
    private int x = 1, y[];
    Outer(int x) {}
    public <U> String greet(U u, int... nums) {
        class LocalClass { int hidden; }
        int localVar = 42;
        return \"\";
    }
    interface NestedIface {
        int CONST = 1;
        void run();
        class InsideIface {}
    }
    enum Color {
        RED,
        BLUE(1) { void extra() {} };
        Color(int code) {}
    }
    record Pair(int first, String second) {}
    @interface Marker {
        String value() default \"\";
    }
    { int initLocal = 0; }
}
";
        let (db, file_id) = db_with(text);
        let syms = db.file_symbols(file_id);

        assert_eq!(syms.package.as_deref(), Some("com.example"));
        assert_eq!(syms.imports.len(), 5);
        assert_eq!(syms.imports[0].kind, ImportKind::SingleType);
        assert_eq!(syms.imports[0].path, vec!["a", "b", "Foo"]);
        assert_eq!(syms.imports[1].kind, ImportKind::TypeOnDemand);
        assert_eq!(syms.imports[1].path, vec!["a", "b"]);
        assert_eq!(syms.imports[2].kind, ImportKind::StaticSingle);
        assert_eq!(syms.imports[2].path, vec!["a", "b", "Foo", "BAR"]);
        assert_eq!(syms.imports[3].kind, ImportKind::StaticOnDemand);
        assert_eq!(syms.imports[3].path, vec!["a", "b", "Foo"]);
        assert_eq!(syms.imports[4].kind, ImportKind::Module);
        assert_eq!(syms.imports[4].path, vec!["java", "base"]);

        // LocalClass inside `greet` must be excluded from `syms.types`.
        let fqns: Vec<&str> = syms
            .types
            .iter()
            .map(|t| t.qualified_name.as_str())
            .collect();
        assert_eq!(
            fqns,
            vec![
                "com.example.Outer",
                "com.example.Outer.NestedIface",
                "com.example.Outer.NestedIface.InsideIface",
                "com.example.Outer.Color",
                "com.example.Outer.Pair",
                "com.example.Outer.Marker",
            ]
        );

        let outer = &syms.types[0];
        assert_eq!(outer.kind, TypeKind::Class);
        assert!(!outer.is_static);
        assert_eq!(outer.type_params, vec!["T"]);
        assert_eq!(slice(text, outer.selection_range), "Outer");
        assert!(slice(text, outer.full_range).starts_with("public class Outer<T>"));
        assert_eq!(
            outer.nested_types,
            vec!["NestedIface", "Color", "Pair", "Marker"]
        );

        assert_eq!(outer.members.len(), 4);
        assert_eq!(outer.members[0].name, "x");
        assert_eq!(outer.members[0].kind, MemberKind::Field);
        assert_eq!(outer.members[0].type_ref.as_deref(), Some("int"));
        assert_eq!(outer.members[1].name, "y");
        assert_eq!(outer.members[1].type_ref.as_deref(), Some("int[]"));

        assert_eq!(outer.members[2].name, "Outer");
        assert_eq!(outer.members[2].kind, MemberKind::Constructor);
        assert_eq!(outer.members[2].params.len(), 1);

        let greet = &outer.members[3];
        assert_eq!(greet.name, "greet");
        assert_eq!(greet.kind, MemberKind::Method);
        assert_eq!(greet.type_ref.as_deref(), Some("String"));
        assert_eq!(greet.params.len(), 2);
        assert!(!greet.params[0].is_varargs);
        assert!(greet.params[1].is_varargs);
        assert_eq!(greet.params_signature(), "(U u, int... nums)");

        // Implicit static rules.
        let nested_iface = &syms.types[1];
        assert_eq!(nested_iface.kind, TypeKind::Interface);
        assert!(nested_iface.is_static);
        assert!(nested_iface.members[0].is_static); // CONST inside interface

        let inside_iface = &syms.types[2];
        assert_eq!(inside_iface.kind, TypeKind::Class);
        assert!(inside_iface.is_static); // class nested inside interface

        let color = &syms.types[3];
        assert_eq!(color.kind, TypeKind::Enum);
        assert!(color.is_static);
        assert_eq!(color.members[0].name, "RED");
        assert_eq!(color.members[0].kind, MemberKind::EnumConstant);
        assert!(color.members[0].is_static);
        assert_eq!(color.members[1].name, "BLUE");
        assert_eq!(color.members[2].kind, MemberKind::Constructor);

        let pair = &syms.types[4];
        assert_eq!(pair.kind, TypeKind::Record);
        assert!(pair.is_static);
        assert_eq!(pair.members.len(), 2);
        assert_eq!(pair.members[0].kind, MemberKind::RecordComponent);
        assert_eq!(pair.members[0].name, "first");
        assert_eq!(pair.members[0].type_ref.as_deref(), Some("int"));
        assert_eq!(pair.members[1].name, "second");

        let marker = &syms.types[5];
        assert_eq!(marker.kind, TypeKind::Annotation);
        assert!(marker.is_static);
        assert_eq!(marker.members[0].name, "value");
    }

    #[test]
    fn extract_compact_source_file_members() {
        let text = "\
#!/usr/bin/env java --source 25
int count = 1;
void main(String[] args) {
    int localOnly = 2;
}
";
        let (db, file_id) = db_with(text);
        let syms = db.file_symbols(file_id);
        assert!(syms.types.is_empty());
        assert_eq!(syms.top_level_members.len(), 2);
        assert_eq!(syms.top_level_members[0].name, "count");
        assert_eq!(syms.top_level_members[0].kind, MemberKind::Field);
        assert_eq!(syms.top_level_members[0].owner_type, None);
        assert_eq!(syms.top_level_members[1].name, "main");
        assert_eq!(syms.top_level_members[1].kind, MemberKind::Method);
    }

    #[test]
    fn file_symbols_body_edit_invariance_and_incremental_index() {
        let initial = "\
package p;
class A {
    int f() {
        int x = 1;
        return x;
    }
}
";
        let same_len_body_edit = "\
package p;
class A {
    int f() {
        int y = 9;
        return y;
    }
}
";
        let (mut db, file_id) = db_with(initial);
        let syms_before = db.file_symbols(file_id);
        let idx_before = db.workspace_index();
        assert!(idx_before.contains_type("p.A"));
        assert_eq!(
            idx_before.package_types("p"),
            Some(["p.A".to_string()].as_slice())
        );

        db.set_file_text(file_id, Arc::new(same_len_body_edit.to_string()));
        let syms_after = db.file_symbols(file_id);
        assert_eq!(syms_before, syms_after);
        assert_eq!(idx_before, db.workspace_index());

        // Structural edit updates workspace_index incrementally.
        db.set_file_text(file_id, Arc::new("package p; class B {}".to_string()));
        let idx_updated = db.workspace_index();
        assert!(!idx_updated.contains_type("p.A"));
        assert!(idx_updated.contains_type("p.B"));
    }

    #[test]
    fn resolve_cross_file_imports_and_precedence() {
        let model = "\
package com.model;
public class Item {
    public int value;
    public static int COUNT = 5;
    public static class Nested {
        public static int NESTED_VAL = 9;
    }
}
";
        let same_pkg = "\
package com.app;
public class SamePkg {
    public void ping() {}
}
";
        let wildcard_pkg = "\
package com.wild;
public class WildOnly {}
class SamePkg {}
class Item {}
";
        let static_src = "\
package com.util;
public class Constants {
    public static int MAGIC = 42;
    public static int helper() { return 1; }
    public static int helper(int x) { return x; }
    public static class InnerType {}
}
";
        let app = "\
package com.app;
import /*imp_pkg*/com./*imp_sub*/model./*imp_cls*/Item;
import com.wild.*;
import static com.util.Constants./*imp_static*/MAGIC;
import static com.util.Constants.*;

public class App {
    class LocalShadow {}
    void test() {
        /*single_imp*/Item a = new /*new_cls*/Item();
        /*same_pkg*/SamePkg b = new SamePkg();
        /*wild_imp*/WildOnly w = null;
        /*qual*/com.model.Item q = null;
        /*nested*/Item.Nested n = null;
        int x = /*stat_single*/MAGIC;
        int y = /*stat_wild*/helper(1);
        /*stat_nested*/InnerType it = null;
        int z = /*qual_static*/com.model.Item.Nested.NESTED_VAL;
    }
}
";
        let db = db_with_files(&[
            (1, model),
            (2, same_pkg),
            (3, wildcard_pkg),
            (4, static_src),
            (5, app),
        ]);
        let app_id = FileId(5);

        // Import declaration targets: package, subpackage, type, static member.
        let imp_pkg = resolve_at_offset(&db, app_id, cursor(app, "/*imp_pkg*/")).unwrap();
        assert_eq!(imp_pkg.kind, ResolvedSymbolKind::Package);
        assert_eq!(imp_pkg.qualified_name, "com");
        assert!(imp_pkg.location().is_none());

        let imp_sub = resolve_at_offset(&db, app_id, cursor(app, "/*imp_sub*/")).unwrap();
        assert_eq!(imp_sub.kind, ResolvedSymbolKind::Package);
        assert_eq!(imp_sub.qualified_name, "com.model");

        let imp_cls = resolve_at_offset(&db, app_id, cursor(app, "/*imp_cls*/")).unwrap();
        assert_eq!(imp_cls.kind, ResolvedSymbolKind::Class);
        assert_eq!(imp_cls.qualified_name, "com.model.Item");
        assert_eq!(imp_cls.file_id, Some(FileId(1)));

        let imp_stat = resolve_at_offset(&db, app_id, cursor(app, "/*imp_static*/")).unwrap();
        assert_eq!(imp_stat.kind, ResolvedSymbolKind::Field);
        assert_eq!(imp_stat.qualified_name, "com.util.Constants.MAGIC");
        assert_eq!(imp_stat.file_id, Some(FileId(4)));

        // Single-type import shadows wildcard import Item.
        let single_imp = resolve_at_offset(&db, app_id, cursor(app, "/*single_imp*/")).unwrap();
        assert_eq!(single_imp.qualified_name, "com.model.Item");
        assert_eq!(single_imp.file_id, Some(FileId(1)));

        // `new Item()` resolves `Item` to TypeSymbol.
        let new_cls = resolve_at_offset(&db, app_id, cursor(app, "/*new_cls*/")).unwrap();
        assert_eq!(new_cls.qualified_name, "com.model.Item");

        // Same-package shadows wildcard import SamePkg.
        let same = resolve_at_offset(&db, app_id, cursor(app, "/*same_pkg*/")).unwrap();
        assert_eq!(same.qualified_name, "com.app.SamePkg");
        assert_eq!(same.file_id, Some(FileId(2)));

        // Wildcard import.
        let wild = resolve_at_offset(&db, app_id, cursor(app, "/*wild_imp*/")).unwrap();
        assert_eq!(wild.qualified_name, "com.wild.WildOnly");
        assert_eq!(wild.file_id, Some(FileId(3)));

        // Qualified type path (`com.model.Item`) clicking `Item`.
        let qual_off = cursor(app, "/*qual*/com.model.");
        let qual = resolve_at_offset(&db, app_id, qual_off).unwrap();
        assert_eq!(qual.qualified_name, "com.model.Item");

        // Nested type (`Item.Nested`) clicking `Nested`.
        let nested_off = cursor(app, "/*nested*/Item.");
        let nested = resolve_at_offset(&db, app_id, nested_off).unwrap();
        assert_eq!(nested.qualified_name, "com.model.Item.Nested");

        // Static single import.
        let stat_single = resolve_at_offset(&db, app_id, cursor(app, "/*stat_single*/")).unwrap();
        assert_eq!(stat_single.kind, ResolvedSymbolKind::Field);
        assert_eq!(stat_single.qualified_name, "com.util.Constants.MAGIC");

        // Static wildcard import method with overload arity.
        let stat_wild = resolve_at_offset(&db, app_id, cursor(app, "/*stat_wild*/")).unwrap();
        assert_eq!(stat_wild.kind, ResolvedSymbolKind::Method);
        assert_eq!(stat_wild.qualified_name, "com.util.Constants.helper");
        assert_eq!(
            stat_wild.detail,
            Some(SymbolDetail::Method {
                return_type: Some("int".to_string()),
                params: "(int x)".to_string(),
            })
        );

        // Static wildcard import nested type.
        let stat_nested = resolve_at_offset(&db, app_id, cursor(app, "/*stat_nested*/")).unwrap();
        assert_eq!(stat_nested.kind, ResolvedSymbolKind::Class);
        assert_eq!(stat_nested.qualified_name, "com.util.Constants.InnerType");

        // Qualified static field access `com.model.Item.Nested.NESTED_VAL`.
        let qual_stat_off = cursor(app, "/*qual_static*/com.model.Item.Nested.");
        let qual_stat = resolve_at_offset(&db, app_id, qual_stat_off).unwrap();
        assert_eq!(qual_stat.kind, ResolvedSymbolKind::Field);
        assert_eq!(qual_stat.qualified_name, "com.model.Item.Nested.NESTED_VAL");
    }

    #[test]
    fn resolve_enclosing_shadows_single_import() {
        let other = "package p2; public class Foo {}";
        let main = "\
package p1;
import p2.Foo;
public class Outer {
    static class Foo {}
    /*ref*/Foo field;
}
";
        let db = db_with_files(&[(1, other), (2, main)]);
        let res = resolve_at_offset(&db, FileId(2), cursor(main, "/*ref*/")).unwrap();
        assert_eq!(res.qualified_name, "p1.Outer.Foo");
    }

    #[test]
    fn resolve_local_scopes_shadowing_and_forward_exclusion() {
        let text = "\
class Scopes {
    int x = 10;
    void demo(int /*decl_p*/p, List<String> items) {
        int a = /*forward*/b;
        int b = /*self_init*/b + 1;
        int c = 1, d = /*seq_decl*/c;
        {
            int /*inner_x*/x = 20;
            int y = /*use_inner_x*/x;
        }
        int z = /*use_field_x*/x;

        for (int i = 0; i < /*for_cond*/i; i++) {
            int k = /*for_body*/i;
        }

        for (String item : /*foreach_iter*/items) {
            int len = /*foreach_body*/item.length();
        }

        try (AutoCloseable r1 = open(); AutoCloseable r2 = wrap(/*res_next*/r1)) {
            /*res_body*/r2.close();
        } catch (Exception ex) {
            /*catch_body*/ex.getMessage();
        }

        Object obj = null;
        if (obj instanceof String s && /*and_rhs*/s.length() > 0) {
            /*if_then*/s.trim();
        } else {
            /*if_else*/s.trim();
        }

        Runnable r = (int /*lparam_decl*/lp) -> { int v = /*lparam_use*/lp; };
        Function<String, Integer> fn = u -> /*untyped_lparam*/u.length();

        int _ = 0;
        int after = /*underscore*/_;
    }
    AutoCloseable open() { return null; }
    AutoCloseable wrap(AutoCloseable a) { return a; }
}
";
        let (db, file_id) = db_with(text);

        // Declaration site resolves to itself.
        let decl_p = resolve_at_offset(&db, file_id, cursor(text, "/*decl_p*/")).unwrap();
        assert_eq!(decl_p.kind, ResolvedSymbolKind::Param);
        assert_eq!(decl_p.name, "p");

        // Forward reference & self-initializer do not resolve.
        assert!(resolve_at_offset(&db, file_id, cursor(text, "/*forward*/")).is_none());
        assert!(resolve_at_offset(&db, file_id, cursor(text, "/*self_init*/")).is_none());

        // Multi-declarator sequential visibility.
        let seq = resolve_at_offset(&db, file_id, cursor(text, "/*seq_decl*/")).unwrap();
        assert_eq!(seq.kind, ResolvedSymbolKind::LocalVar);
        assert_eq!(seq.name, "c");

        // Block shadowing vs outer field.
        let inner_x_decl = resolve_at_offset(&db, file_id, cursor(text, "/*inner_x*/")).unwrap();
        let use_inner = resolve_at_offset(&db, file_id, cursor(text, "/*use_inner_x*/")).unwrap();
        assert_eq!(use_inner.kind, ResolvedSymbolKind::LocalVar);
        assert_eq!(use_inner.focus_range, inner_x_decl.focus_range);

        let use_field = resolve_at_offset(&db, file_id, cursor(text, "/*use_field_x*/")).unwrap();
        assert_eq!(use_field.kind, ResolvedSymbolKind::Field);
        assert_eq!(use_field.qualified_name, "Scopes.x");

        // `for` loop header variable.
        let for_cond = resolve_at_offset(&db, file_id, cursor(text, "/*for_cond*/")).unwrap();
        assert_eq!(for_cond.kind, ResolvedSymbolKind::LocalVar);
        assert_eq!(for_cond.name, "i");
        let for_body = resolve_at_offset(&db, file_id, cursor(text, "/*for_body*/")).unwrap();
        assert_eq!(for_body.focus_range, for_cond.focus_range);

        // `foreach` variable.
        let fe_iter = resolve_at_offset(&db, file_id, cursor(text, "/*foreach_iter*/")).unwrap();
        assert_eq!(fe_iter.kind, ResolvedSymbolKind::Param);
        let fe_body = resolve_at_offset(&db, file_id, cursor(text, "/*foreach_body*/")).unwrap();
        assert_eq!(fe_body.kind, ResolvedSymbolKind::ForEachVar);
        assert_eq!(fe_body.name, "item");

        // Try-with-resources & catch.
        let res_next = resolve_at_offset(&db, file_id, cursor(text, "/*res_next*/")).unwrap();
        assert_eq!(res_next.kind, ResolvedSymbolKind::ResourceVar);
        assert_eq!(res_next.name, "r1");
        let res_body = resolve_at_offset(&db, file_id, cursor(text, "/*res_body*/")).unwrap();
        assert_eq!(res_body.kind, ResolvedSymbolKind::ResourceVar);
        assert_eq!(res_body.name, "r2");
        let catch_body = resolve_at_offset(&db, file_id, cursor(text, "/*catch_body*/")).unwrap();
        assert_eq!(catch_body.kind, ResolvedSymbolKind::CatchParam);
        assert_eq!(catch_body.name, "ex");

        // Pattern variables in `&&` RHS and `if` then-branch, absent in `else`.
        let and_rhs = resolve_at_offset(&db, file_id, cursor(text, "/*and_rhs*/")).unwrap();
        assert_eq!(and_rhs.kind, ResolvedSymbolKind::PatternVar);
        assert_eq!(and_rhs.name, "s");
        let if_then = resolve_at_offset(&db, file_id, cursor(text, "/*if_then*/")).unwrap();
        assert_eq!(if_then.kind, ResolvedSymbolKind::PatternVar);
        assert!(resolve_at_offset(&db, file_id, cursor(text, "/*if_else*/")).is_none());

        // Lambda params (typed and untyped).
        let lp = resolve_at_offset(&db, file_id, cursor(text, "/*lparam_use*/")).unwrap();
        assert_eq!(lp.kind, ResolvedSymbolKind::LambdaParam);
        assert_eq!(lp.detail, Some(SymbolDetail::TypeRef("int".to_string())));
        let ulp = resolve_at_offset(&db, file_id, cursor(text, "/*untyped_lparam*/")).unwrap();
        assert_eq!(ulp.kind, ResolvedSymbolKind::LambdaParam);
        assert_eq!(ulp.detail, None);

        // Unnamed `_` is never bound.
        assert!(resolve_at_offset(&db, file_id, cursor(text, "/*underscore*/")).is_none());
    }

    #[test]
    fn resolve_member_access_overload_arity_this_and_type_params() {
        let box_file = "\
package com.data;
public class Box<T> {
    public T item;
    public int size;
    public void put() {}
    public void put(T val) {}
    public void put(T a, T b) {}
}
";
        let user_file = "\
package com.app;
import com.data.Box;
public class User<E extends Number> {
    private Box<String> myBox;
    private E num;
    private int count;
    record Point(int x, int y) {}
    enum Mode { FAST, SLOW }

    void run(Box<E> paramBox) {
        int c = this./*this_field*/count;
        int s = /*local_field*/paramBox.size;
        /*field_method0*/myBox.put();
        /*field_method2*/myBox.put(\"a\", \"b\");
        int px = new Point(1, 2)./*rec_comp*/x;
        Mode m = Mode./*enum_const*/FAST;
        /*tp_ref*/E localE = num;
        class LocalHelper { int helperVal; }
        LocalHelper lh = new LocalHelper();
        int hv = lh./*local_cls_field*/helperVal;
    }
}
";
        let db = db_with_files(&[(1, box_file), (2, user_file)]);
        let user_id = FileId(2);

        let this_f = resolve_at_offset(&db, user_id, cursor(user_file, "/*this_field*/")).unwrap();
        assert_eq!(this_f.kind, ResolvedSymbolKind::Field);
        assert_eq!(this_f.qualified_name, "com.app.User.count");

        let param_s_off = cursor(user_file, "/*local_field*/paramBox.");
        let param_s = resolve_at_offset(&db, user_id, param_s_off).unwrap();
        assert_eq!(param_s.kind, ResolvedSymbolKind::Field);
        assert_eq!(param_s.qualified_name, "com.data.Box.size");
        assert_eq!(param_s.file_id, Some(FileId(1)));

        let put0_off = cursor(user_file, "/*field_method0*/myBox.");
        let put0 = resolve_at_offset(&db, user_id, put0_off).unwrap();
        assert_eq!(put0.kind, ResolvedSymbolKind::Method);
        assert_eq!(
            put0.detail,
            Some(SymbolDetail::Method {
                return_type: Some("void".to_string()),
                params: "()".to_string(),
            })
        );

        let put2_off = cursor(user_file, "/*field_method2*/myBox.");
        let put2 = resolve_at_offset(&db, user_id, put2_off).unwrap();
        assert_eq!(put2.kind, ResolvedSymbolKind::Method);
        assert_eq!(
            put2.detail,
            Some(SymbolDetail::Method {
                return_type: Some("void".to_string()),
                params: "(T a, T b)".to_string(),
            })
        );

        let rec_x = resolve_at_offset(&db, user_id, cursor(user_file, "/*rec_comp*/")).unwrap();
        assert_eq!(rec_x.kind, ResolvedSymbolKind::RecordComponent);
        assert_eq!(rec_x.qualified_name, "com.app.User.Point.x");

        let enum_c = resolve_at_offset(&db, user_id, cursor(user_file, "/*enum_const*/")).unwrap();
        assert_eq!(enum_c.kind, ResolvedSymbolKind::EnumConstant);
        assert_eq!(enum_c.qualified_name, "com.app.User.Mode.FAST");

        let tp = resolve_at_offset(&db, user_id, cursor(user_file, "/*tp_ref*/")).unwrap();
        assert_eq!(tp.kind, ResolvedSymbolKind::TypeParam);
        assert_eq!(tp.name, "E");
        assert_eq!(
            tp.detail,
            Some(SymbolDetail::Bounds(vec!["Number".to_string()]))
        );

        let lcf =
            resolve_at_offset(&db, user_id, cursor(user_file, "/*local_cls_field*/")).unwrap();
        assert_eq!(lcf.kind, ResolvedSymbolKind::Field);
        assert_eq!(lcf.name, "helperVal");
    }

    #[test]
    fn resolve_method_reference() {
        let text = "\
class Refs {
    static int parse(String s) { return 0; }
    void run() {
        Function<String, Integer> f = Refs::/*mref*/parse;
    }
}
";
        let (db, file_id) = db_with(text);
        let res = resolve_at_offset(&db, file_id, cursor(text, "/*mref*/")).unwrap();
        assert_eq!(res.kind, ResolvedSymbolKind::Method);
        assert_eq!(res.qualified_name, "Refs.parse");
    }

    #[test]
    fn resolve_record_pattern_and_switch_guard() {
        let text = "\
class Patterns {
    record Pair(int x, int y) {}
    int test(Object o) {
        return switch (o) {
            case Pair(int a, int b) when /*guard*/a > 0 -> /*body*/b;
            default -> 0;
        };
    }
}
";
        let (db, file_id) = db_with(text);
        let guard_a = resolve_at_offset(&db, file_id, cursor(text, "/*guard*/")).unwrap();
        assert_eq!(guard_a.kind, ResolvedSymbolKind::PatternVar);
        assert_eq!(guard_a.name, "a");

        let body_b = resolve_at_offset(&db, file_id, cursor(text, "/*body*/")).unwrap();
        assert_eq!(body_b.kind, ResolvedSymbolKind::PatternVar);
        assert_eq!(body_b.name, "b");
    }

    #[test]
    fn resolve_type_param_shadowing_and_static_boundary() {
        let text = "\
class Outer<T> {
    void useOuter(/*outer_tp*/T p) {
        class T {}
        /*local_shadow*/T x;
    }
    static void staticMethod() {
        /*static_method_tp*/T bad;
    }
    class NonStaticInner {
        /*inner_tp*/T ok;
    }
    static class Inner<U> {
        /*static_inner_own_tp*/U own;
        /*static_inner_outer_cls*/Outer o;
        /*static_inner_outer_tp*/T missing;
    }
}
";
        let (db, file_id) = db_with(text);

        let outer_tp = resolve_at_offset(&db, file_id, cursor(text, "/*outer_tp*/")).unwrap();
        assert_eq!(outer_tp.kind, ResolvedSymbolKind::TypeParam);
        assert_eq!(outer_tp.name, "T");

        let local_shadow =
            resolve_at_offset(&db, file_id, cursor(text, "/*local_shadow*/")).unwrap();
        assert_eq!(local_shadow.kind, ResolvedSymbolKind::Class);
        assert_eq!(local_shadow.name, "T");

        assert!(resolve_at_offset(&db, file_id, cursor(text, "/*static_method_tp*/")).is_none());

        let inner_tp = resolve_at_offset(&db, file_id, cursor(text, "/*inner_tp*/")).unwrap();
        assert_eq!(inner_tp.kind, ResolvedSymbolKind::TypeParam);
        assert_eq!(inner_tp.name, "T");

        let own_tp =
            resolve_at_offset(&db, file_id, cursor(text, "/*static_inner_own_tp*/")).unwrap();
        assert_eq!(own_tp.kind, ResolvedSymbolKind::TypeParam);
        assert_eq!(own_tp.name, "U");

        let outer_cls =
            resolve_at_offset(&db, file_id, cursor(text, "/*static_inner_outer_cls*/")).unwrap();
        assert_eq!(outer_cls.kind, ResolvedSymbolKind::Class);
        assert_eq!(outer_cls.qualified_name, "Outer");

        assert!(
            resolve_at_offset(&db, file_id, cursor(text, "/*static_inner_outer_tp*/")).is_none()
        );
    }

    #[test]
    fn resolve_local_and_anonymous_class_member_declaration_sites() {
        let text = "\
class LocalDecls {
    void m() {
        class Local {
            int /*local_field*/val = 1;
            String /*local_method*/greet(int x) { return \"\"; }
            /*local_ctor*/Local() {}
        }
        record LocalRec(int /*local_rec_comp*/coord) {}
        Runnable r = new Runnable() {
            int /*anon_field*/count = 0;
            public void /*anon_method*/run() {}
        };
    }
}
";
        let (db, file_id) = db_with(text);

        let lf = resolve_at_offset(&db, file_id, cursor(text, "/*local_field*/")).unwrap();
        assert_eq!(lf.kind, ResolvedSymbolKind::Field);
        assert_eq!(lf.qualified_name, "Local.val");
        assert_eq!(lf.detail, Some(SymbolDetail::TypeRef("int".to_string())));

        let lm = resolve_at_offset(&db, file_id, cursor(text, "/*local_method*/")).unwrap();
        assert_eq!(lm.kind, ResolvedSymbolKind::Method);
        assert_eq!(lm.qualified_name, "Local.greet");
        assert_eq!(
            lm.detail,
            Some(SymbolDetail::Method {
                return_type: Some("String".to_string()),
                params: "(int x)".to_string(),
            })
        );

        let lc = resolve_at_offset(&db, file_id, cursor(text, "/*local_ctor*/")).unwrap();
        assert_eq!(lc.kind, ResolvedSymbolKind::Constructor);
        assert_eq!(lc.qualified_name, "Local.Local");
        assert_eq!(
            lc.detail,
            Some(SymbolDetail::Constructor {
                params: "()".to_string(),
            })
        );

        let lrc = resolve_at_offset(&db, file_id, cursor(text, "/*local_rec_comp*/")).unwrap();
        assert_eq!(lrc.kind, ResolvedSymbolKind::RecordComponent);
        assert_eq!(lrc.qualified_name, "LocalRec.coord");
        assert_eq!(lrc.detail, Some(SymbolDetail::TypeRef("int".to_string())));

        let af = resolve_at_offset(&db, file_id, cursor(text, "/*anon_field*/")).unwrap();
        assert_eq!(af.kind, ResolvedSymbolKind::Field);
        assert_eq!(af.qualified_name, "count");
        assert_eq!(af.detail, Some(SymbolDetail::TypeRef("int".to_string())));

        let am = resolve_at_offset(&db, file_id, cursor(text, "/*anon_method*/")).unwrap();
        assert_eq!(am.kind, ResolvedSymbolKind::Method);
        assert_eq!(am.qualified_name, "run");
        assert_eq!(
            am.detail,
            Some(SymbolDetail::Method {
                return_type: Some("void".to_string()),
                params: "()".to_string(),
            })
        );
    }

    #[test]
    fn default_root_database_does_not_panic_on_workspace_queries() {
        let mut db = RootDatabase::default();
        assert!(db.workspace_files().is_empty());

        let file_id = FileId(1);
        let text = "class Solo { int x; int y = /*r*/x; }";
        db.set_file_text(file_id, Arc::new(text.to_string()));
        db.set_file_language_level(file_id, LanguageLevel::latest());

        assert!(db.workspace_index().is_empty());
        let res = resolve_at_offset(&db, file_id, cursor(text, "/*r*/")).unwrap();
        assert_eq!(res.kind, ResolvedSymbolKind::Field);
        assert_eq!(res.name, "x");
    }

    #[test]
    fn workspace_files_updates_and_level_before_text() {
        let mut db = RootDatabase::default();
        let f1 = FileId(1);
        let f2 = FileId(2);

        db.set_file_language_level(f1, LanguageLevel::new(Release::JAVA_11, false));
        db.set_file_text(f1, Arc::new("record R(int x) {}".to_string()));
        assert_eq!(db.syntax_errors(f1).len(), 1);
        assert_eq!(
            db.file_language_level(f1),
            LanguageLevel::new(Release::JAVA_11, false)
        );

        db.set_file_language_level(f2, LanguageLevel::latest());
        db.set_file_text(f2, Arc::new("package p; class Keep {}".to_string()));

        db.set_workspace_files(Arc::new(vec![f1, f2]));
        assert_eq!(db.workspace_files().as_slice(), &[f1, f2]);
        assert!(db.workspace_index().contains_type("p.Keep"));

        db.set_workspace_files(Arc::new(vec![f1]));
        assert_eq!(db.workspace_files().as_slice(), &[f1]);
        assert!(!db.workspace_index().contains_type("p.Keep"));
    }

    #[test]
    fn remove_file_cleans_up_tracked_entry_and_ignores_missing() {
        let mut db = RootDatabase::default();
        let f1 = FileId(1);
        let f2 = FileId(2);

        db.set_file_text(f1, Arc::new("package p; class One {}".to_string()));
        db.set_file_text(f2, Arc::new("package p; class Two {}".to_string()));
        db.set_workspace_files(Arc::new(vec![f1, f2]));

        // Removing a file directly without updating workspace_files first prunes it from modules
        // so workspace_index never retains a dangling FileId.
        db.remove_file(f2);
        assert!(!db.files.contains_key(&f2));
        assert!(db.files.contains_key(&f1));
        assert_eq!(db.workspace_files().as_slice(), &[f1]);
        assert!(!db.workspace_index().contains_type("p.Two"));
        assert!(db.workspace_index().contains_type("p.One"));

        // Removing an absent FileId does not panic.
        db.remove_file(f2);
        db.remove_file(FileId(999));
    }

    #[test]
    fn variable_length_body_edit_preserves_item_tree_and_index_with_shifted_ranges() {
        let initial = "\
package p;
class First {
    int compute() {
        return 1;
    }
}
class Second {
    int field;
}
class Caller {
    Second ref = null;
}
";
        let edited = "\
package p;
class First {
    int compute() {
        int a = 10;
        int b = 20;
        int c = a + b;
        return c;
    }
}
class Second {
    int field;
}
class Caller {
    /*ref*/Second ref = null;
}
";
        let (mut db, file_id) = db_with(initial);
        let tree_before = db.file_item_tree(file_id);
        let mod_before = db.module_index(ModuleId(0));
        let idx_before = db.workspace_index();

        let expected_before_offset = initial.find("Second {\n").unwrap();
        let caller_ref_before = initial.find("Second ref").unwrap() as u32;
        let resolved_before = resolve_at_offset(&db, file_id, caller_ref_before).unwrap();
        assert_eq!(
            usize::from(resolved_before.focus_range.unwrap().start()),
            expected_before_offset
        );

        db.set_file_text(file_id, Arc::new(edited.to_string()));
        let tree_after = db.file_item_tree(file_id);
        let mod_after = db.module_index(ModuleId(0));
        let idx_after = db.workspace_index();

        assert_eq!(
            tree_before, tree_after,
            "variable-length body edit must keep FileItemTree structurally identical"
        );
        assert!(
            Arc::ptr_eq(&mod_before, &mod_after),
            "variable-length body edit must trigger Salsa early-cutoff for ModuleIndex"
        );
        assert!(
            Arc::ptr_eq(&idx_before, &idx_after),
            "variable-length body edit must trigger Salsa early-cutoff for WorkspaceIndex"
        );

        let expected_after_offset = edited.find("Second {\n").unwrap();
        assert_ne!(expected_before_offset, expected_after_offset);
        let resolved_after = resolve_at_offset(&db, file_id, cursor(edited, "/*ref*/")).unwrap();
        assert_eq!(resolved_after.qualified_name, "p.Second");
        assert_eq!(
            usize::from(resolved_after.focus_range.unwrap().start()),
            expected_after_offset
        );
    }

    #[test]
    fn signature_edit_in_one_module_does_not_invalidate_other_modules() {
        let mut db = RootDatabase::default();
        let f0 = FileId(1);
        let f1 = FileId(2);

        db.set_file_text(
            f0,
            Arc::new("package m0; public class Alpha {}".to_string()),
        );
        db.set_file_text(f1, Arc::new("package m1; public class Beta {}".to_string()));
        db.set_workspace_modules(vec![
            (ModuleId(0), Arc::new(vec![f0])),
            (ModuleId(1), Arc::new(vec![f1])),
        ]);

        let mod0_before = db.module_index(ModuleId(0));
        let mod1_before = db.module_index(ModuleId(1));
        let ws_before = db.workspace_index();
        assert!(mod0_before.contains_type("m0.Alpha"));
        assert!(mod1_before.contains_type("m1.Beta"));
        assert!(ws_before.contains_type("m0.Alpha"));
        assert!(ws_before.contains_type("m1.Beta"));

        // Edit signature in Module 0.
        db.set_file_text(
            f0,
            Arc::new("package m0; public class AlphaRenamed { int x; }".to_string()),
        );

        let mod0_after = db.module_index(ModuleId(0));
        let mod1_after = db.module_index(ModuleId(1));
        let ws_after = db.workspace_index();

        assert!(
            !Arc::ptr_eq(&mod0_before, &mod0_after),
            "Module 0 index must update after signature edit"
        );
        assert!(
            Arc::ptr_eq(&mod1_before, &mod1_after),
            "Module 1 index must remain pointer-identical after Module 0 edit"
        );
        assert!(!Arc::ptr_eq(&ws_before, &ws_after));
        assert!(!ws_after.contains_type("m0.Alpha"));
        assert!(ws_after.contains_type("m0.AlphaRenamed"));
        assert!(ws_after.contains_type("m1.Beta"));
    }

    fn sample_binary_classpath() -> ClasspathIndex {
        let base_origin = BinaryOrigin::Jdk {
            module_name: Some(SmolStr::new("java.base")),
        };
        let sql_origin = BinaryOrigin::Jdk {
            module_name: Some(SmolStr::new("java.sql")),
        };

        let mut cp = ClasspathIndex::default();
        cp.insert_module(BinaryModuleInfo {
            name: SmolStr::new("java.base"),
            exports: vec![
                "java.lang".to_string(),
                "java.util".to_string(),
                "java.io".to_string(),
            ],
            requires_transitive: vec![],
        });
        cp.insert_module(BinaryModuleInfo {
            name: SmolStr::new("java.sql"),
            exports: vec!["java.sql".to_string()],
            requires_transitive: vec![SmolStr::new("java.base")],
        });

        cp.insert_type(BinaryTypeSymbol {
            name: SmolStr::new("String"),
            qualified_name: "java.lang.String".to_string(),
            internal_name: "java/lang/String".to_string(),
            package: "java.lang".to_string(),
            enclosing_type: None,
            kind: TypeKind::Class,
            is_public: true,
            is_static: false,
            is_final: true,
            is_abstract: false,
            type_params: vec![],
            super_class: Some("java.lang.Object".to_string()),
            interfaces: vec![],
            permitted_subclasses: vec![],
            members: vec![
                BinaryMemberSymbol {
                    owner_type: "java.lang.String".to_string(),
                    name: SmolStr::new("length"),
                    kind: MemberKind::Method,
                    is_public: true,
                    is_static: false,
                    type_ref: Some("int".to_string()),
                    params: vec![],
                    is_varargs: false,
                },
                BinaryMemberSymbol {
                    owner_type: "java.lang.String".to_string(),
                    name: SmolStr::new("valueOf"),
                    kind: MemberKind::Method,
                    is_public: true,
                    is_static: true,
                    type_ref: Some("String".to_string()),
                    params: vec![ParamSummary {
                        name: Some(SmolStr::new("i")),
                        type_ref: "int".to_string(),
                        is_varargs: false,
                    }],
                    is_varargs: false,
                },
            ],
            nested_types: vec![],
            origin: base_origin.clone(),
        });

        cp.insert_type(BinaryTypeSymbol {
            name: SmolStr::new("PrintStream"),
            qualified_name: "java.io.PrintStream".to_string(),
            internal_name: "java/io/PrintStream".to_string(),
            package: "java.io".to_string(),
            enclosing_type: None,
            kind: TypeKind::Class,
            is_public: true,
            is_static: false,
            is_final: false,
            is_abstract: false,
            type_params: vec![],
            super_class: Some("java.lang.Object".to_string()),
            interfaces: vec![],
            permitted_subclasses: vec![],
            members: vec![BinaryMemberSymbol {
                owner_type: "java.io.PrintStream".to_string(),
                name: SmolStr::new("println"),
                kind: MemberKind::Method,
                is_public: true,
                is_static: false,
                type_ref: Some("void".to_string()),
                params: vec![ParamSummary {
                    name: Some(SmolStr::new("x")),
                    type_ref: "String".to_string(),
                    is_varargs: false,
                }],
                is_varargs: false,
            }],
            nested_types: vec![],
            origin: base_origin.clone(),
        });

        cp.insert_type(BinaryTypeSymbol {
            name: SmolStr::new("System"),
            qualified_name: "java.lang.System".to_string(),
            internal_name: "java/lang/System".to_string(),
            package: "java.lang".to_string(),
            enclosing_type: None,
            kind: TypeKind::Class,
            is_public: true,
            is_static: false,
            is_final: true,
            is_abstract: false,
            type_params: vec![],
            super_class: Some("java.lang.Object".to_string()),
            interfaces: vec![],
            permitted_subclasses: vec![],
            members: vec![BinaryMemberSymbol {
                owner_type: "java.lang.System".to_string(),
                name: SmolStr::new("out"),
                kind: MemberKind::Field,
                is_public: true,
                is_static: true,
                type_ref: Some("java.io.PrintStream".to_string()),
                params: vec![],
                is_varargs: false,
            }],
            nested_types: vec![],
            origin: base_origin.clone(),
        });

        cp.insert_type(BinaryTypeSymbol {
            name: SmolStr::new("List"),
            qualified_name: "java.util.List".to_string(),
            internal_name: "java/util/List".to_string(),
            package: "java.util".to_string(),
            enclosing_type: None,
            kind: TypeKind::Interface,
            is_public: true,
            is_static: false,
            is_final: false,
            is_abstract: true,
            type_params: vec![SmolStr::new("E")],
            super_class: Some("java.lang.Object".to_string()),
            interfaces: vec![],
            permitted_subclasses: vec![],
            members: vec![],
            nested_types: vec![],
            origin: base_origin.clone(),
        });

        cp.insert_type(BinaryTypeSymbol {
            name: SmolStr::new("Map"),
            qualified_name: "java.util.Map".to_string(),
            internal_name: "java/util/Map".to_string(),
            package: "java.util".to_string(),
            enclosing_type: None,
            kind: TypeKind::Interface,
            is_public: true,
            is_static: false,
            is_final: false,
            is_abstract: true,
            type_params: vec![SmolStr::new("K"), SmolStr::new("V")],
            super_class: Some("java.lang.Object".to_string()),
            interfaces: vec![],
            permitted_subclasses: vec![],
            members: vec![],
            nested_types: vec![],
            origin: base_origin.clone(),
        });

        cp.insert_type(BinaryTypeSymbol {
            name: SmolStr::new("Entry"),
            qualified_name: "java.util.Map.Entry".to_string(),
            internal_name: "java/util/Map$Entry".to_string(),
            package: "java.util".to_string(),
            enclosing_type: Some("java.util.Map".to_string()),
            kind: TypeKind::Interface,
            is_public: true,
            is_static: true,
            is_final: false,
            is_abstract: true,
            type_params: vec![SmolStr::new("K"), SmolStr::new("V")],
            super_class: Some("java.lang.Object".to_string()),
            interfaces: vec![],
            permitted_subclasses: vec![],
            members: vec![],
            nested_types: vec![],
            origin: base_origin.clone(),
        });

        cp.insert_type(BinaryTypeSymbol {
            name: SmolStr::new("Objects"),
            qualified_name: "java.util.Objects".to_string(),
            internal_name: "java/util/Objects".to_string(),
            package: "java.util".to_string(),
            enclosing_type: None,
            kind: TypeKind::Class,
            is_public: true,
            is_static: false,
            is_final: true,
            is_abstract: false,
            type_params: vec![],
            super_class: Some("java.lang.Object".to_string()),
            interfaces: vec![],
            permitted_subclasses: vec![],
            members: vec![BinaryMemberSymbol {
                owner_type: "java.util.Objects".to_string(),
                name: SmolStr::new("requireNonNull"),
                kind: MemberKind::Method,
                is_public: true,
                is_static: true,
                type_ref: Some("T".to_string()),
                params: vec![ParamSummary {
                    name: Some(SmolStr::new("obj")),
                    type_ref: "T".to_string(),
                    is_varargs: false,
                }],
                is_varargs: false,
            }],
            nested_types: vec![],
            origin: base_origin,
        });

        cp.insert_type(BinaryTypeSymbol {
            name: SmolStr::new("Connection"),
            qualified_name: "java.sql.Connection".to_string(),
            internal_name: "java/sql/Connection".to_string(),
            package: "java.sql".to_string(),
            enclosing_type: None,
            kind: TypeKind::Interface,
            is_public: true,
            is_static: false,
            is_final: false,
            is_abstract: true,
            type_params: vec![],
            super_class: Some("java.lang.Object".to_string()),
            interfaces: vec![],
            permitted_subclasses: vec![],
            members: vec![],
            nested_types: vec![],
            origin: sql_origin,
        });

        cp.finalize();
        cp
    }

    #[test]
    fn resolve_binary_jdk_types_members_imports_and_module_imports() {
        let app = "\
package com.app;
import /*imp_pkg*/java./*imp_sub*/util./*imp_list*/List;
import static java.util.Objects./*imp_static*/requireNonNull;

class Demo {
    void run(/*str_param*/String s, /*list_ref*/List<String> items, /*qual_entry*/java.util.Map.Entry<String, String> entry) {
        int n = s./*str_len*/length();
        System./*sys_out*/out./*ps_println*/println(s);
        /*stat_req*/requireNonNull(s);
        Object fn = String::/*mref_valof*/valueOf;
    }
}
";
        let mod_user = "\
package com.mod;
import module java.sql;

class SqlClient {
    /*conn*/Connection c;
    /*trans_list*/List<String> xs;
}
";
        let mut db = db_with_files(&[(1, app), (2, mod_user)]);
        assert!(db.classpath_index().types_by_fqn.is_empty());

        let cp = Arc::new(sample_binary_classpath());
        db.set_classpath_index(Arc::clone(&cp));
        // Re-setting an equal ClasspathIndex preserves Salsa pointer identity.
        db.set_classpath_index(Arc::new(sample_binary_classpath()));
        assert_eq!(db.classpath_index(), cp);

        let f1 = FileId(1);
        let imp_pkg = resolve_at_offset(&db, f1, cursor(app, "/*imp_pkg*/")).unwrap();
        assert_eq!(imp_pkg.kind, ResolvedSymbolKind::Package);
        assert_eq!(imp_pkg.qualified_name, "java");
        assert!(imp_pkg.binary_target.is_none());

        let imp_sub = resolve_at_offset(&db, f1, cursor(app, "/*imp_sub*/")).unwrap();
        assert_eq!(imp_sub.kind, ResolvedSymbolKind::Package);
        assert_eq!(imp_sub.qualified_name, "java.util");

        let imp_list = resolve_at_offset(&db, f1, cursor(app, "/*imp_list*/")).unwrap();
        assert_eq!(imp_list.kind, ResolvedSymbolKind::Interface);
        assert_eq!(imp_list.qualified_name, "java.util.List");
        assert_eq!(
            imp_list.binary_target.as_ref().map(|t| t.internal_name.as_str()),
            Some("java/util/List")
        );

        let imp_stat = resolve_at_offset(&db, f1, cursor(app, "/*imp_static*/")).unwrap();
        assert_eq!(imp_stat.kind, ResolvedSymbolKind::Method);
        assert_eq!(imp_stat.qualified_name, "java.util.Objects.requireNonNull");

        let str_param = resolve_at_offset(&db, f1, cursor(app, "/*str_param*/")).unwrap();
        assert_eq!(str_param.kind, ResolvedSymbolKind::Class);
        assert_eq!(str_param.qualified_name, "java.lang.String");
        assert_eq!(
            str_param.binary_target,
            Some(BinaryTarget {
                origin: BinaryOrigin::Jdk {
                    module_name: Some(SmolStr::new("java.base")),
                },
                internal_name: "java/lang/String".to_string(),
                qualified_name: "java.lang.String".to_string(),
                member_name: None,
            })
        );

        let list_ref = resolve_at_offset(&db, f1, cursor(app, "/*list_ref*/")).unwrap();
        assert_eq!(list_ref.qualified_name, "java.util.List");

        let qual_entry_off = cursor(app, "/*qual_entry*/java.util.Map.");
        let qual_entry = resolve_at_offset(&db, f1, qual_entry_off).unwrap();
        assert_eq!(qual_entry.kind, ResolvedSymbolKind::Interface);
        assert_eq!(qual_entry.qualified_name, "java.util.Map.Entry");
        assert_eq!(
            qual_entry.binary_target.as_ref().map(|t| t.internal_name.as_str()),
            Some("java/util/Map$Entry")
        );

        let str_len = resolve_at_offset(&db, f1, cursor(app, "/*str_len*/")).unwrap();
        assert_eq!(str_len.kind, ResolvedSymbolKind::Method);
        assert_eq!(str_len.qualified_name, "java.lang.String.length");
        assert_eq!(
            str_len.binary_target.as_ref().and_then(|t| t.member_name.as_deref()),
            Some("length")
        );

        let sys_out = resolve_at_offset(&db, f1, cursor(app, "/*sys_out*/")).unwrap();
        assert_eq!(sys_out.kind, ResolvedSymbolKind::Field);
        assert_eq!(sys_out.qualified_name, "java.lang.System.out");

        let ps_println = resolve_at_offset(&db, f1, cursor(app, "/*ps_println*/")).unwrap();
        assert_eq!(ps_println.kind, ResolvedSymbolKind::Method);
        assert_eq!(ps_println.qualified_name, "java.io.PrintStream.println");

        let stat_req = resolve_at_offset(&db, f1, cursor(app, "/*stat_req*/")).unwrap();
        assert_eq!(stat_req.kind, ResolvedSymbolKind::Method);
        assert_eq!(stat_req.qualified_name, "java.util.Objects.requireNonNull");

        let mref = resolve_at_offset(&db, f1, cursor(app, "/*mref_valof*/")).unwrap();
        assert_eq!(mref.kind, ResolvedSymbolKind::Method);
        assert_eq!(mref.qualified_name, "java.lang.String.valueOf");

        // Module import `import module java.sql;` brings in `java.sql.Connection` and
        // transitively `java.util.List` via `requires transitive java.base`.
        let f2 = FileId(2);
        let conn = resolve_at_offset(&db, f2, cursor(mod_user, "/*conn*/")).unwrap();
        assert_eq!(conn.qualified_name, "java.sql.Connection");
        let trans_list = resolve_at_offset(&db, f2, cursor(mod_user, "/*trans_list*/")).unwrap();
        assert_eq!(trans_list.qualified_name, "java.util.List");
    }

    #[test]
    fn workspace_source_type_shadows_binary_classpath_type() {
        let ws_list = "package java.util; public interface List<T> {}";
        let app = "\
package com.app;
import java.util.List;
class Use {
    /*ref*/List<String> xs;
}
";
        let mut db = db_with_files(&[(1, ws_list), (2, app)]);
        db.set_classpath_index(Arc::new(sample_binary_classpath()));

        let res = resolve_at_offset(&db, FileId(2), cursor(app, "/*ref*/")).unwrap();
        assert_eq!(res.qualified_name, "java.util.List");
        assert_eq!(res.file_id, Some(FileId(1)));
        assert!(res.binary_target.is_none());

        // Also check implicit java.lang shadowing by same-package workspace class.
        let ws_string = "package com.app; public class String {}";
        let app2 = "\
package com.app;
class Use2 {
    /*ref*/String s;
}
";
        let mut db2 = db_with_files(&[(1, ws_string), (2, app2)]);
        db2.set_classpath_index(Arc::new(sample_binary_classpath()));

        let res2 = resolve_at_offset(&db2, FileId(2), cursor(app2, "/*ref*/")).unwrap();
        assert_eq!(res2.qualified_name, "com.app.String");
        assert_eq!(res2.file_id, Some(FileId(1)));
        assert!(res2.binary_target.is_none());
    }

    #[test]
    fn variable_length_body_edit_preserves_workspace_hierarchy_index_via_early_cutoff() {
        let initial = "\
package p;
interface Greet {
    void greet(String name);
}
class Base implements Greet {
    @Override
    public void greet(String name) {
        int x = 1;
    }
}
class Sub extends Base {
    @Override
    public void greet(String name) {
        int y = 2;
    }
}
";
        let edited = "\
package p;
interface Greet {
    void greet(String name);
}
class Base implements Greet {
    @Override
    public void greet(String name) {
        int x = 1;
        int z = x + 42;
        String msg = \"much longer method body\";
    }
}
class Sub extends Base {
    @Override
    public void greet(String name) {
        int y = 2;
        int w = y * 10;
    }
}
";
        let (mut db, file_id) = db_with(initial);
        let hier_before = db.workspace_hierarchy_index();
        assert_eq!(
            direct_subtypes(&db, "p.Base"),
            vec!["p.Sub".to_string()]
        );

        db.set_file_text(file_id, Arc::new(edited.to_string()));
        let hier_after = db.workspace_hierarchy_index();
        assert!(
            Arc::ptr_eq(&hier_before, &hier_after),
            "variable-length body edit must preserve WorkspaceHierarchyIndex via Salsa early-cutoff"
        );
    }

    #[test]
    fn resolve_inherited_members_interface_methods_super_and_type_definition() {
        let base_src = "\
package com.example;

public interface Greeter {
    int GREET_CODE = 7;
    default String defaultGreet() { return \"hi\"; }
    static void helper() {}
}

public class Base implements Greeter {
    public int baseField = 10;
    public Base() {}
    public Base(int x) {}
    public Base(String... parts) {}
    public void greet(String who) {}
    public void overloaded(int a) {}
    public static class NestedInBase {}
}
";
        let sub_src = "\
package com.example;
import java.util.List;

public class Sub extends Base implements List<String> {
    public int baseField = 99;
    record Holder(Base owner) {}
    private Base myBase;

    public Sub() {
        /*sup_ctor_0*/super();
    }
    public Sub(int x) {
        /*sup_ctor_1*/super(x);
    }
    public Sub(String a, String b) {
        /*sup_ctor_varargs*/super(a, b);
    }

    @Override
    public void /*decl_greet*/greet(String who) {
        int f = /*sup_field*/super.baseField;
        /*sup_method*/super.greet(who);
        /*iface_sup_method*/Greeter.super.defaultGreet();
        /*unqual_base_method*/overloaded(1);
        int c = /*unqual_iface_const*/GREET_CODE;
        /*nested_type_ref*/NestedInBase nb = null;
    }

    public void overloaded() {}

    public Base getBase() { return myBase; }

    void demo(/*param_list*/List<String> items, Holder h) {
        /*local_sub*/Sub s = new Sub();
        int a = s./*inh_field*/baseField;
        s./*call_greet*/greet(\"world\");
        s./*call_overloaded_base*/overloaded(1);
        s./*call_overloaded_sub*/overloaded();
        String dg = s./*call_default*/defaultGreet();
        s./*call_iface_static_on_sub*/helper();
        Greeter./*call_iface_static_on_iface*/helper();
        int sz = /*var_items*/items./*call_bin_iface*/size();
        String ts = s./*call_obj_to_string*/toString();
        /*call_get_base*/getBase();
        Base fRef = this./*field_my_base*/myBase;
        Base rComp = h./*rec_comp_owner*/owner;
    }
}
";
        let mut db = db_with_files(&[(1, base_src), (2, sub_src)]);

        let mut cp = ClasspathIndex::default();
        let jdk_origin = BinaryOrigin::Jdk {
            module_name: Some(SmolStr::new("java.base")),
        };
        cp.insert_type(BinaryTypeSymbol {
            name: SmolStr::new("Object"),
            qualified_name: "java.lang.Object".to_string(),
            internal_name: "java/lang/Object".to_string(),
            package: "java.lang".to_string(),
            enclosing_type: None,
            kind: TypeKind::Class,
            is_public: true,
            is_static: false,
            is_final: false,
            is_abstract: false,
            type_params: vec![],
            super_class: None,
            interfaces: vec![],
            permitted_subclasses: vec![],
            members: vec![BinaryMemberSymbol {
                owner_type: "java.lang.Object".to_string(),
                name: SmolStr::new("toString"),
                kind: MemberKind::Method,
                is_public: true,
                is_static: false,
                type_ref: Some("java.lang.String".to_string()),
                params: vec![],
                is_varargs: false,
            }],
            nested_types: vec![],
            origin: jdk_origin.clone(),
        });
        cp.insert_type(BinaryTypeSymbol {
            name: SmolStr::new("String"),
            qualified_name: "java.lang.String".to_string(),
            internal_name: "java/lang/String".to_string(),
            package: "java.lang".to_string(),
            enclosing_type: None,
            kind: TypeKind::Class,
            is_public: true,
            is_static: false,
            is_final: true,
            is_abstract: false,
            type_params: vec![],
            super_class: Some("java.lang.Object".to_string()),
            interfaces: vec![],
            permitted_subclasses: vec![],
            members: vec![],
            nested_types: vec![],
            origin: jdk_origin.clone(),
        });
        cp.insert_type(BinaryTypeSymbol {
            name: SmolStr::new("Collection"),
            qualified_name: "java.util.Collection".to_string(),
            internal_name: "java/util/Collection".to_string(),
            package: "java.util".to_string(),
            enclosing_type: None,
            kind: TypeKind::Interface,
            is_public: true,
            is_static: false,
            is_final: false,
            is_abstract: true,
            type_params: vec![SmolStr::new("E")],
            super_class: Some("java.lang.Object".to_string()),
            interfaces: vec![],
            permitted_subclasses: vec![],
            members: vec![BinaryMemberSymbol {
                owner_type: "java.util.Collection".to_string(),
                name: SmolStr::new("size"),
                kind: MemberKind::Method,
                is_public: true,
                is_static: false,
                type_ref: Some("int".to_string()),
                params: vec![],
                is_varargs: false,
            }],
            nested_types: vec![],
            origin: jdk_origin.clone(),
        });
        cp.insert_type(BinaryTypeSymbol {
            name: SmolStr::new("List"),
            qualified_name: "java.util.List".to_string(),
            internal_name: "java/util/List".to_string(),
            package: "java.util".to_string(),
            enclosing_type: None,
            kind: TypeKind::Interface,
            is_public: true,
            is_static: false,
            is_final: false,
            is_abstract: true,
            type_params: vec![SmolStr::new("E")],
            super_class: Some("java.lang.Object".to_string()),
            interfaces: vec!["java.util.Collection".to_string()],
            permitted_subclasses: vec![],
            members: vec![],
            nested_types: vec![],
            origin: jdk_origin,
        });
        cp.finalize();
        db.set_classpath_index(Arc::new(cp));

        let sub_id = FileId(2);

        // super(...) constructor resolution by arity & varargs.
        let sup0 = resolve_at_offset(&db, sub_id, cursor(sub_src, "/*sup_ctor_0*/")).unwrap();
        assert_eq!(sup0.kind, ResolvedSymbolKind::Constructor);
        assert_eq!(sup0.qualified_name, "com.example.Base.Base");
        assert_eq!(
            sup0.detail,
            Some(SymbolDetail::Constructor {
                params: "()".to_string()
            })
        );

        let sup1 = resolve_at_offset(&db, sub_id, cursor(sub_src, "/*sup_ctor_1*/")).unwrap();
        assert_eq!(sup1.kind, ResolvedSymbolKind::Constructor);
        assert_eq!(
            sup1.detail,
            Some(SymbolDetail::Constructor {
                params: "(int x)".to_string()
            })
        );

        let sup_v =
            resolve_at_offset(&db, sub_id, cursor(sub_src, "/*sup_ctor_varargs*/")).unwrap();
        assert_eq!(sup_v.kind, ResolvedSymbolKind::Constructor);
        assert_eq!(
            sup_v.detail,
            Some(SymbolDetail::Constructor {
                params: "(String... parts)".to_string()
            })
        );

        // Overriding method declaration and call site carry `overrides`.
        let decl_greet = resolve_at_offset(&db, sub_id, cursor(sub_src, "/*decl_greet*/")).unwrap();
        assert_eq!(
            decl_greet.overrides.as_deref(),
            Some("com.example.Base.greet(String)")
        );
        let call_greet = resolve_at_offset(&db, sub_id, cursor(sub_src, "/*call_greet*/")).unwrap();
        assert_eq!(call_greet.qualified_name, "com.example.Sub.greet");
        assert_eq!(
            call_greet.overrides.as_deref(),
            Some("com.example.Base.greet(String)")
        );

        // `super.field`, `super.method(...)`, and `Interface.super.defaultMethod()`.
        let sup_field_off = cursor(sub_src, "/*sup_field*/super.");
        let sup_field = resolve_at_offset(&db, sub_id, sup_field_off).unwrap();
        assert_eq!(sup_field.qualified_name, "com.example.Base.baseField");

        let sup_method_off = cursor(sub_src, "/*sup_method*/super.");
        let sup_method = resolve_at_offset(&db, sub_id, sup_method_off).unwrap();
        assert_eq!(sup_method.qualified_name, "com.example.Base.greet");
        assert!(sup_method.overrides.is_none());

        let iface_sup_off = cursor(sub_src, "/*iface_sup_method*/Greeter.super.");
        let iface_sup = resolve_at_offset(&db, sub_id, iface_sup_off).unwrap();
        assert_eq!(iface_sup.qualified_name, "com.example.Greeter.defaultGreet");

        // Unqualified inherited members & nested types inside Sub.
        let unq_m =
            resolve_at_offset(&db, sub_id, cursor(sub_src, "/*unqual_base_method*/")).unwrap();
        assert_eq!(unq_m.qualified_name, "com.example.Base.overloaded");
        let unq_c =
            resolve_at_offset(&db, sub_id, cursor(sub_src, "/*unqual_iface_const*/")).unwrap();
        assert_eq!(unq_c.qualified_name, "com.example.Greeter.GREET_CODE");
        let nested_ref =
            resolve_at_offset(&db, sub_id, cursor(sub_src, "/*nested_type_ref*/")).unwrap();
        assert_eq!(nested_ref.qualified_name, "com.example.Base.NestedInBase");

        // Subclass own field shadows Base field.
        let inh_f = resolve_at_offset(&db, sub_id, cursor(sub_src, "/*inh_field*/")).unwrap();
        assert_eq!(inh_f.qualified_name, "com.example.Sub.baseField");

        // Overload arity preserved across hierarchy.
        let ov_base =
            resolve_at_offset(&db, sub_id, cursor(sub_src, "/*call_overloaded_base*/")).unwrap();
        assert_eq!(ov_base.qualified_name, "com.example.Base.overloaded");
        let ov_sub =
            resolve_at_offset(&db, sub_id, cursor(sub_src, "/*call_overloaded_sub*/")).unwrap();
        assert_eq!(ov_sub.qualified_name, "com.example.Sub.overloaded");

        // Interface default method inherited; interface static method NOT inherited (JLS §8.4.8).
        let def_g = resolve_at_offset(&db, sub_id, cursor(sub_src, "/*call_default*/")).unwrap();
        assert_eq!(def_g.qualified_name, "com.example.Greeter.defaultGreet");
        assert!(
            resolve_at_offset(&db, sub_id, cursor(sub_src, "/*call_iface_static_on_sub*/"))
                .is_none()
        );
        let iface_stat = resolve_at_offset(
            &db,
            sub_id,
            cursor(sub_src, "/*call_iface_static_on_iface*/"),
        )
        .unwrap();
        assert_eq!(iface_stat.qualified_name, "com.example.Greeter.helper");

        // Inherited binary interface method (List -> Collection.size) and Object.toString().
        let bin_size =
            resolve_at_offset(&db, sub_id, cursor(sub_src, "/*call_bin_iface*/")).unwrap();
        assert_eq!(bin_size.qualified_name, "java.util.Collection.size");
        let obj_ts =
            resolve_at_offset(&db, sub_id, cursor(sub_src, "/*call_obj_to_string*/")).unwrap();
        assert_eq!(obj_ts.qualified_name, "java.lang.Object.toString");

        // `resolve_type_of_symbol_at_offset` for local var, param, field, record component, method call.
        let td_local = resolve_type_of_symbol_at_offset(
            &db,
            sub_id,
            TextSize::from(cursor(sub_src, "/*local_sub*/Sub ")),
        )
        .unwrap();
        assert_eq!(td_local.qualified_name, "com.example.Sub");
        assert_eq!(td_local.file_id, Some(FileId(2)));

        let td_param = resolve_type_of_symbol_at_offset(
            &db,
            sub_id,
            TextSize::from(cursor(sub_src, "/*var_items*/")),
        )
        .unwrap();
        assert_eq!(td_param.qualified_name, "java.util.List");
        assert!(td_param.binary_target.is_some());

        let td_field = resolve_type_of_symbol_at_offset(
            &db,
            sub_id,
            TextSize::from(cursor(sub_src, "/*field_my_base*/")),
        )
        .unwrap();
        assert_eq!(td_field.qualified_name, "com.example.Base");

        let td_rec = resolve_type_of_symbol_at_offset(
            &db,
            sub_id,
            TextSize::from(cursor(sub_src, "/*rec_comp_owner*/")),
        )
        .unwrap();
        assert_eq!(td_rec.qualified_name, "com.example.Base");

        let td_method = resolve_type_of_symbol_at_offset(
            &db,
            sub_id,
            TextSize::from(cursor(sub_src, "/*call_get_base*/")),
        )
        .unwrap();
        assert_eq!(td_method.qualified_name, "com.example.Base");

        let td_this = resolve_type_of_symbol_at_offset(
            &db,
            sub_id,
            TextSize::from(sub_src.find("this./*field_my_base*/").unwrap() as u32),
        )
        .unwrap();
        assert_eq!(td_this.qualified_name, "com.example.Sub");

        let td_sup_ctor = resolve_type_of_symbol_at_offset(
            &db,
            sub_id,
            TextSize::from(cursor(sub_src, "/*sup_ctor_0*/")),
        )
        .unwrap();
        assert_eq!(td_sup_ctor.qualified_name, "com.example.Base");

        // Primitive local variable returns `None` for type definition.
        assert!(
            resolve_type_of_symbol_at_offset(
                &db,
                sub_id,
                TextSize::from(sub_src.find("a = s./*inh_field*/").unwrap() as u32),
            )
            .is_none()
        );
    }
}
