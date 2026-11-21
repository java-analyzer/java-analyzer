pub mod resolve;
pub mod symbols;
#[cfg(any(test, feature = "test-utils"))]
pub mod test_fixture;
pub mod workspace;

use std::sync::Arc;

use rustc_hash::FxHashMap;
use salsa::Setter as _;

pub use resolve::{resolve_at_offset, LocalKind, ResolvedSymbol, ResolvedSymbolKind, SymbolDetail};
pub use smol_str::SmolStr;
pub use symbols::{
    build_workspace_index, extract_file_symbols, FileSymbols, ImportDecl, ImportKind, MemberKind,
    MemberSymbol, ParamSummary, TypeKind, TypeSymbol, WorkspaceIndex,
};
pub use syntax::{
    parse_shebang_level, LanguageLevel, Parse, Release, SyntaxError, SyntaxNode, TextRange,
    TextSize,
};
pub use workspace::{
    extract_file_header, infer_workspace_layout, infer_workspace_layout_from_sources,
    is_path_within, normalize_rel_path, DeltaSet, FileHeader, ModuleData, ModuleDelta,
    ModuleDependency, ModuleHeader, ModuleId, ModuleTarget, PackageMismatch, ResourceRoot,
    ResourceRootId, RootScope, SourceRoot, SourceRootId, WorkspaceDelta, WorkspaceLayout,
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
pub(crate) struct WorkspaceFiles {
    pub(crate) files: Arc<Vec<SourceFile>>,
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
fn extract_symbols(db: &dyn salsa::Database, file: SourceFile) -> Arc<FileSymbols> {
    let root = parse_file(db, file).syntax_node();
    Arc::new(symbols::extract_file_symbols(*file.file_id(db), &root))
}

#[salsa::tracked]
fn build_index(db: &dyn salsa::Database, workspace: WorkspaceFiles) -> Arc<WorkspaceIndex> {
    let files = workspace.files(db);
    let per_file: Vec<Arc<FileSymbols>> =
        files.iter().map(|&f| extract_symbols(db, f).clone()).collect();
    Arc::new(symbols::build_workspace_index(&per_file))
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

    /// Declared package, imports, types, and members extracted from a single file.
    fn file_symbols(&self, file_id: FileId) -> Arc<FileSymbols>;

    /// Aggregated workspace-wide lookup index over `workspace_files`.
    fn workspace_index(&self) -> Arc<WorkspaceIndex>;
}

#[salsa::db]
#[derive(Default)]
pub struct RootDatabase {
    storage: salsa::Storage<Self>,
    files: FxHashMap<FileId, SourceFile>,
    workspace: Option<WorkspaceFiles>,
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
        let files: Arc<Vec<SourceFile>> = Arc::new(
            file_ids
                .iter()
                .map(|&id| self.ensure_source_file(id))
                .collect(),
        );
        if let Some(ws) = self.workspace {
            ws.set_files(self).to(files);
        } else {
            self.workspace = Some(WorkspaceFiles::new(self, files));
        }
    }

    /// Removes `file_id` from the tracked file map. Does nothing if `file_id` is absent.
    pub fn remove_file(&mut self, file_id: FileId) {
        self.files.remove(&file_id);
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
        match self.workspace {
            Some(ws) => Arc::new(ws.files(self).iter().map(|&f| *f.file_id(self)).collect()),
            None => Arc::new(Vec::new()),
        }
    }

    fn file_symbols(&self, file_id: FileId) -> Arc<FileSymbols> {
        extract_symbols(self, self.source_file(file_id)).clone()
    }

    fn workspace_index(&self) -> Arc<WorkspaceIndex> {
        match self.workspace {
            Some(ws) => build_index(self, ws).clone(),
            None => Arc::new(WorkspaceIndex::default()),
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
        let fqns: Vec<&str> = syms.types.iter().map(|t| t.qualified_name.as_str()).collect();
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
        assert_eq!(
            qual_stat.qualified_name,
            "com.model.Item.Nested.NESTED_VAL"
        );
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

        db.set_workspace_files(Arc::new(vec![f1]));
        db.remove_file(f2);
        assert!(!db.files.contains_key(&f2));
        assert!(db.files.contains_key(&f1));

        // Removing an absent FileId does not panic.
        db.remove_file(f2);
        db.remove_file(FileId(999));
    }
}
