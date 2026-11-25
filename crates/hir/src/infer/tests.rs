use std::{collections::BTreeMap, sync::Arc};

use smol_str::SmolStr;
use syntax::TextSize;

use crate::{
    DefDatabase, FileId, SourceDatabase,
    classpath::{BinaryMemberSymbol, BinaryOrigin, BinaryTypeSymbol, ClasspathIndex},
    resolve::{ResolvedSymbolKind, SymbolDetail, resolve_at_offset, resolve_type_of_symbol_at_offset},
    symbols::{MemberKind, ParamSummary, TypeKind},
    test_fixture::{cursor, db_with, db_with_files},
    ty::{Ty, TypeArg},
};

use super::*;

#[expect(
    clippy::too_many_arguments,
    reason = "concise test fixture helper for constructing BinaryTypeSymbol"
)]
fn bin_cls(
    fqn: &str,
    kind: TypeKind,
    type_params: &[&str],
    super_class: Option<&str>,
    super_class_generic: Option<&str>,
    interfaces: &[&str],
    interfaces_generic: &[&str],
    members: Vec<BinaryMemberSymbol>,
) -> BinaryTypeSymbol {
    let (pkg, name) = match fqn.rsplit_once('.') {
        Some((p, n)) => (p.to_string(), SmolStr::new(n)),
        None => (String::new(), SmolStr::new(fqn)),
    };
    BinaryTypeSymbol {
        name,
        qualified_name: fqn.to_string(),
        internal_name: fqn.replace('.', "/"),
        package: pkg,
        enclosing_type: None,
        kind,
        identity: if matches!(kind, TypeKind::Interface | TypeKind::Annotation) {
            crate::TypeIdentity::Unspecified
        } else {
            crate::TypeIdentity::Identity
        },
        is_public: true,
        is_static: false,
        is_final: false,
        is_abstract: kind == TypeKind::Interface,
        nullness_scope: crate::NullnessScope::Inherited,
        type_params: type_params.iter().map(|s| SmolStr::new(*s)).collect(),
        type_param_bounds: BTreeMap::new(),
        super_class: super_class.map(str::to_string),
        super_class_generic: super_class_generic.map(str::to_string),
        interfaces: interfaces.iter().map(|s| (*s).to_string()).collect(),
        interfaces_generic: interfaces_generic.iter().map(|s| (*s).to_string()).collect(),
        permitted_subclasses: Vec::new(),
        members,
        nested_types: Vec::new(),
        origin: BinaryOrigin::Jdk {
            module_name: Some(SmolStr::new("java.base")),
        },
    }
}

fn bin_method(
    owner: &str,
    name: &str,
    is_static: bool,
    is_abstract: bool,
    type_params: &[&str],
    ret: &str,
    params: &[(&str, &str, bool)],
) -> BinaryMemberSymbol {
    BinaryMemberSymbol {
        owner_type: owner.to_string(),
        name: SmolStr::new(name),
        kind: MemberKind::Method,
        is_public: true,
        is_static,
        is_abstract,
        nullness_scope: crate::NullnessScope::Inherited,
        decl_nullability: None,
        type_params: type_params.iter().map(|s| SmolStr::new(*s)).collect(),
        type_param_bounds: BTreeMap::new(),
        type_ref: Some(ret.to_string()),
        params: params
            .iter()
            .map(|(n, ty, va)| ParamSummary {
                name: Some(SmolStr::new(*n)),
                type_ref: (*ty).to_string(),
                is_varargs: *va,
                decl_nullability: None,
            })
            .collect(),
        is_varargs: params.last().is_some_and(|(_, _, va)| *va),
    }
}

fn bin_ctor(owner: &str, params: &[(&str, &str, bool)]) -> BinaryMemberSymbol {
    let simple = owner.rsplit('.').next().unwrap_or(owner);
    BinaryMemberSymbol {
        owner_type: owner.to_string(),
        name: SmolStr::new(simple),
        kind: MemberKind::Constructor,
        is_public: true,
        is_static: false,
        is_abstract: false,
        nullness_scope: crate::NullnessScope::Inherited,
        decl_nullability: None,
        type_params: Vec::new(),
        type_param_bounds: BTreeMap::new(),
        type_ref: None,
        params: params
            .iter()
            .map(|(n, ty, va)| ParamSummary {
                name: Some(SmolStr::new(*n)),
                type_ref: (*ty).to_string(),
                is_varargs: *va,
                decl_nullability: None,
            })
            .collect(),
        is_varargs: params.last().is_some_and(|(_, _, va)| *va),
    }
}

fn rich_jdk_classpath() -> ClasspathIndex {
    let mut cp = ClasspathIndex::default();
    cp.insert_type(bin_cls(
        "java.lang.Object",
        TypeKind::Class,
        &[],
        None,
        None,
        &[],
        &[],
        vec![
            bin_method("java.lang.Object", "toString", false, false, &[], "java.lang.String", &[]),
            bin_method(
                "java.lang.Object",
                "equals",
                false,
                false,
                &[],
                "boolean",
                &[("obj", "java.lang.Object", false)],
            ),
            bin_method("java.lang.Object", "hashCode", false, false, &[], "int", &[]),
        ],
    ));
    cp.insert_type(bin_cls(
        "java.lang.String",
        TypeKind::Class,
        &[],
        Some("java.lang.Object"),
        None,
        &["java.lang.CharSequence"],
        &[],
        vec![bin_method("java.lang.String", "length", false, false, &[], "int", &[])],
    ));
    cp.insert_type(bin_cls(
        "java.lang.CharSequence",
        TypeKind::Interface,
        &[],
        None,
        None,
        &[],
        &[],
        vec![],
    ));
    cp.insert_type(bin_cls(
        "java.lang.Number",
        TypeKind::Class,
        &[],
        Some("java.lang.Object"),
        None,
        &[],
        &[],
        vec![],
    ));
    cp.insert_type(bin_cls(
        "java.lang.Integer",
        TypeKind::Class,
        &[],
        Some("java.lang.Number"),
        None,
        &[],
        &[],
        vec![bin_method(
            "java.lang.Integer",
            "valueOf",
            true,
            false,
            &[],
            "java.lang.Integer",
            &[("i", "int", false)],
        )],
    ));
    cp.insert_type(bin_cls(
        "java.lang.AutoCloseable",
        TypeKind::Interface,
        &[],
        None,
        None,
        &[],
        &[],
        vec![bin_method("java.lang.AutoCloseable", "close", false, true, &[], "void", &[])],
    ));
    cp.insert_type(bin_cls(
        "java.util.function.Function",
        TypeKind::Interface,
        &["T", "R"],
        None,
        None,
        &[],
        &[],
        vec![
            bin_method("java.util.function.Function", "apply", false, true, &[], "R", &[("t", "T", false)]),
            bin_method(
                "java.util.function.Function",
                "compose",
                false,
                false,
                &["V"],
                "java.util.function.Function<V, R>",
                &[("before", "java.util.function.Function<? super V, ? extends T>", false)],
            ),
            bin_method(
                "java.util.function.Function",
                "identity",
                true,
                false,
                &["T"],
                "java.util.function.Function<T, T>",
                &[],
            ),
        ],
    ));
    cp.insert_type(bin_cls(
        "java.util.function.Consumer",
        TypeKind::Interface,
        &["T"],
        None,
        None,
        &[],
        &[],
        vec![
            bin_method("java.util.function.Consumer", "accept", false, true, &[], "void", &[("t", "T", false)]),
            bin_method(
                "java.util.function.Consumer",
                "andThen",
                false,
                false,
                &[],
                "java.util.function.Consumer<T>",
                &[("after", "java.util.function.Consumer<? super T>", false)],
            ),
        ],
    ));
    cp.insert_type(bin_cls(
        "java.util.stream.Stream",
        TypeKind::Interface,
        &["T"],
        None,
        None,
        &["java.lang.AutoCloseable"],
        &[],
        vec![
            bin_method(
                "java.util.stream.Stream",
                "map",
                false,
                true,
                &["R"],
                "java.util.stream.Stream<R>",
                &[("mapper", "java.util.function.Function<? super T, ? extends R>", false)],
            ),
            bin_method(
                "java.util.stream.Stream",
                "toList",
                false,
                false,
                &[],
                "java.util.List<T>",
                &[],
            ),
            bin_method(
                "java.util.stream.Stream",
                "forEach",
                false,
                true,
                &[],
                "void",
                &[("action", "java.util.function.Consumer<? super T>", false)],
            ),
        ],
    ));
    cp.insert_type(bin_cls(
        "java.lang.Iterable",
        TypeKind::Interface,
        &["T"],
        None,
        None,
        &[],
        &[],
        vec![bin_method(
            "java.lang.Iterable",
            "forEach",
            false,
            false,
            &[],
            "void",
            &[("action", "java.util.function.Consumer<? super T>", false)],
        )],
    ));
    cp.insert_type(bin_cls(
        "java.util.Collection",
        TypeKind::Interface,
        &["E"],
        None,
        None,
        &["java.lang.Iterable"],
        &["java.lang.Iterable<E>"],
        vec![
            bin_method("java.util.Collection", "size", false, true, &[], "int", &[]),
            bin_method(
                "java.util.Collection",
                "stream",
                false,
                false,
                &[],
                "java.util.stream.Stream<E>",
                &[],
            ),
        ],
    ));
    cp.insert_type(bin_cls(
        "java.util.List",
        TypeKind::Interface,
        &["E"],
        None,
        None,
        &["java.util.Collection"],
        &["java.util.Collection<E>"],
        vec![
            bin_method("java.util.List", "get", false, true, &[], "E", &[("index", "int", false)]),
            bin_method("java.util.List", "remove", false, true, &[], "E", &[("index", "int", false)]),
            bin_method(
                "java.util.List",
                "remove",
                false,
                true,
                &[],
                "boolean",
                &[("o", "java.lang.Object", false)],
            ),
            bin_method(
                "java.util.List",
                "of",
                true,
                false,
                &["E"],
                "java.util.List<E>",
                &[("e1", "E", false)],
            ),
        ],
    ));
    cp.insert_type(bin_cls(
        "java.util.ArrayList",
        TypeKind::Class,
        &["E"],
        Some("java.lang.Object"),
        Some("java.lang.Object"),
        &["java.util.List"],
        &["java.util.List<E>"],
        vec![
            bin_ctor("java.util.ArrayList", &[]),
            bin_ctor(
                "java.util.ArrayList",
                &[("c", "java.util.Collection<? extends E>", false)],
            ),
        ],
    ));
    cp.insert_type(bin_cls(
        "java.util.Collections",
        TypeKind::Class,
        &[],
        Some("java.lang.Object"),
        None,
        &[],
        &[],
        vec![bin_method(
            "java.util.Collections",
            "emptyList",
            true,
            false,
            &["T"],
            "java.util.List<T>",
            &[],
        )],
    ));
    cp.insert_type(bin_cls(
        "java.util.Optional",
        TypeKind::Class,
        &["T"],
        Some("java.lang.Object"),
        None,
        &[],
        &[],
        vec![
            bin_method(
                "java.util.Optional",
                "of",
                true,
                false,
                &["T"],
                "java.util.Optional<T>",
                &[("value", "T", false)],
            ),
            bin_method("java.util.Optional", "get", false, false, &[], "T", &[]),
        ],
    ));
    cp.finalize();
    cp
}

fn cls(fqn: &str) -> Ty {
    Ty::raw_class(fqn)
}

fn cls_with(fqn: &str, args: Vec<Ty>) -> Ty {
    Ty::class(fqn, args.into_iter().map(TypeArg::Type).collect())
}

#[test]
fn early_cutoff_preserves_item_tree_and_hierarchy_on_body_edits() {
    let initial = "\
package p;
import java.util.List;
class User { String getName() { return \"a\"; } }
class App {
    void run(List<User> users) {
        var u = users.get(0);
    }
}
";
    let edited = "\
package p;
import java.util.List;
class User { String getName() { return \"a\"; } }
class App {
    void run(List<User> users) {
        var names = users.stream().map(u -> u.getName()).toList();
        var x = \"hello\" + 42;
    }
}
";
    let (mut db, fid) = db_with(initial);
    let tree_before = db.file_item_tree(fid);
    let ws_before = db.workspace_index();
    let hier_before = db.workspace_hierarchy_index();

    db.set_file_text(fid, Arc::new(edited.to_string()));
    assert_eq!(tree_before, db.file_item_tree(fid));
    assert!(Arc::ptr_eq(&ws_before, &db.workspace_index()));
    assert!(Arc::ptr_eq(&hier_before, &db.workspace_hierarchy_index()));
}

#[test]
fn cycle_and_malformed_code_safety() {
    let src = "\
package cyc;
class A<T extends A<T>> {}
class Demo {
    void broken() {
        var /*self*/x = x;
        var /*mut_a*/a = b;
        var b = a;
        var /*empty*/z = ;
        var /*call*/c = unknownObj.missing(
        A<?> rec = null;
        var r = rec;
    }
}
";
    let (db, fid) = db_with(src);
    let root = db.syntax_tree(fid);

    for marker in ["/*self*/", "/*mut_a*/", "/*empty*/"] {
        let off = TextSize::from(cursor(src, marker));
        let tok = root.token_at_offset(off).right_biased().unwrap();
        let decl = tok.parent().unwrap().parent().unwrap();
        assert_eq!(infer_local_ty(&db, fid, &decl), Ty::Unknown);
        let _ = resolve_at_offset(&db, fid, off.into());
        let _ = resolve_type_of_symbol_at_offset(&db, fid, off);
    }
    let call_off = TextSize::from(cursor(src, "/*call*/"));
    let call_tok = root.token_at_offset(call_off).right_biased().unwrap();
    let call_decl = call_tok.parent().unwrap().parent().unwrap();
    let _ = infer_local_ty(&db, fid, &call_decl);
    let _ = resolve_at_offset(&db, fid, call_off.into());
    let _ = resolve_type_of_symbol_at_offset(&db, fid, call_off);
}

#[test]
fn generics_inheritance_and_wildcards_substitution() {
    let src = "\
package com.example;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Optional;

public class User {
    public String getName() { return \"\"; }
}
class UserList extends ArrayList<User> {}
class Demo {
    void test(
        List<User> users,
        List<? extends User> extUsers,
        List<? super User> supUsers
    ) {
        users./*list_get*/get(0)./*get_name*/getName();
        new UserList().get(0)./*userlist_get_name*/getName();
        for (var /*foreach_ul*/ulItem : new UserList()) {
            ulItem./*ul_item_name*/getName();
        }
        var /*ext_var*/eu = extUsers.get(0);
        var /*sup_var*/su = supUsers.get(0);
        var /*col_empty*/empties = Collections.<String>emptyList();
        var /*list_of*/ofUsers = List.of(new User());
        var /*opt_of*/optUser = Optional.of(new User());
        List<String> /*target_empty*/targetList = Collections.emptyList();
        /*target_empty_use*/targetList.get(0);
    }
}
";
    let mut db = db_with_files(&[(1, src)]);
    db.set_classpath_index(Arc::new(rich_jdk_classpath()));
    let fid = FileId(1);

    let list_get = resolve_at_offset(&db, fid, cursor(src, "/*list_get*/")).unwrap();
    assert_eq!(
        list_get.detail,
        Some(SymbolDetail::Method {
            return_type: Some("User".to_string()),
            params: "(int index)".to_string(),
        })
    );

    let get_name = resolve_at_offset(&db, fid, cursor(src, "/*get_name*/")).unwrap();
    assert_eq!(get_name.qualified_name, "com.example.User.getName");

    let ul_get_name = resolve_at_offset(&db, fid, cursor(src, "/*userlist_get_name*/")).unwrap();
    assert_eq!(ul_get_name.qualified_name, "com.example.User.getName");

    let foreach_ul = resolve_at_offset(&db, fid, cursor(src, "/*foreach_ul*/")).unwrap();
    assert_eq!(foreach_ul.detail, Some(SymbolDetail::TypeRef("User".to_string())));

    let ul_item_name = resolve_at_offset(&db, fid, cursor(src, "/*ul_item_name*/")).unwrap();
    assert_eq!(ul_item_name.qualified_name, "com.example.User.getName");

    let ext_var = resolve_at_offset(&db, fid, cursor(src, "/*ext_var*/")).unwrap();
    assert_eq!(ext_var.detail, Some(SymbolDetail::TypeRef("User".to_string())));

    let sup_var = resolve_at_offset(&db, fid, cursor(src, "/*sup_var*/")).unwrap();
    assert_eq!(sup_var.detail, Some(SymbolDetail::TypeRef("Object".to_string())));

    let col_empty = resolve_at_offset(&db, fid, cursor(src, "/*col_empty*/")).unwrap();
    assert_eq!(col_empty.detail, Some(SymbolDetail::TypeRef("List<String>".to_string())));

    let list_of = resolve_at_offset(&db, fid, cursor(src, "/*list_of*/")).unwrap();
    assert_eq!(list_of.detail, Some(SymbolDetail::TypeRef("List<User>".to_string())));

    let opt_of = resolve_at_offset(&db, fid, cursor(src, "/*opt_of*/")).unwrap();
    assert_eq!(opt_of.detail, Some(SymbolDetail::TypeRef("Optional<User>".to_string())));

    let root = db.syntax_tree(fid);
    let target_decl = root
        .token_at_offset(TextSize::from(cursor(src, "/*target_empty*/")))
        .right_biased()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let init_expr = target_decl.children().last().unwrap();
    let expected_list_str = cls_with("java.util.List", vec![Ty::string()]);
    assert_eq!(
        infer_expr_ty(&db, fid, &init_expr, Some(&expected_list_str)),
        expected_list_str
    );
}

#[test]
fn var_and_all_expression_forms() {
    let src = "\
package com.example;
import java.util.ArrayList;
import java.util.List;

class Animal {}
class Dog extends Animal {}
class Cat extends Animal {}
class Box<T> { Box(T val) {} }
record Pair<A, B>(A first, B second) {}
class Res implements AutoCloseable {
    public void close() {}
    public int read() { return 1; }
}

class Demo {
    void run(boolean cond, int code, Object obj, String[] strArr, Dog dog, Cat cat) {
        var /*v_int*/n = 42;
        var /*v_str*/s = \"x\" + 1;
        var /*v_cls*/c = Demo.class;
        var /*v_pcls*/pc = int.class;
        List<Dog> /*v_diam*/dogs = new ArrayList<>();
        var /*v_diam_arg*/box = new Box<>(dog);
        var /*v_arr*/arr = new Dog[3];
        var /*v_arr_len*/len = arr.length;
        var /*v_arr_elem*/first = arr[0];
        var /*v_tern_num*/num = cond ? 1 : 2.5;
        var /*v_tern_ref*/pet = cond ? dog : cat;
        var /*v_sw*/sw = switch (code) {
            case 1 -> dog;
            default -> { yield cat; }
        };
        for (var /*v_for_arr*/item : strArr) {
            item./*arr_item_len*/length();
        }
        try (var /*v_res*/r = new Res()) {
            r./*res_read*/read();
        }
        if (obj instanceof Pair<String, Dog>(var /*v_pat_a*/pa, var /*v_pat_b*/pb)) {
            pa./*pat_a_len*/length();
        }
    }
}
";
    let mut db = db_with_files(&[(1, src)]);
    db.set_classpath_index(Arc::new(rich_jdk_classpath()));
    let fid = FileId(1);

    let check_var = |marker: &str, expected: &str| {
        let res = resolve_at_offset(&db, fid, cursor(src, marker))
            .unwrap_or_else(|| panic!("missing resolved symbol at {marker}"));
        assert_eq!(
            res.detail,
            Some(SymbolDetail::TypeRef(expected.to_string())),
            "mismatch at {marker}"
        );
    };

    check_var("/*v_int*/", "int");
    check_var("/*v_str*/", "String");
    check_var("/*v_cls*/", "Class<Demo>");
    check_var("/*v_pcls*/", "Class<Integer>");
    check_var("/*v_diam_arg*/", "Box<Dog>");
    check_var("/*v_arr*/", "Dog[]");
    check_var("/*v_arr_len*/", "int");
    check_var("/*v_arr_elem*/", "Dog");
    check_var("/*v_tern_num*/", "double");
    check_var("/*v_tern_ref*/", "Animal");
    check_var("/*v_sw*/", "Animal");
    check_var("/*v_for_arr*/", "String");
    check_var("/*v_res*/", "Res");
    check_var("/*v_pat_a*/", "String");
    check_var("/*v_pat_b*/", "Dog");

    // Verify diamond `new ArrayList<>()` with expected `List<Dog>` resolves to `ArrayList<Dog>`.
    let root = db.syntax_tree(fid);
    let diam_decl = root
        .token_at_offset(TextSize::from(cursor(src, "/*v_diam*/")))
        .right_biased()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let new_expr = diam_decl.children().last().unwrap();
    let exp = cls_with("java.util.List", vec![cls("com.example.Dog")]);
    assert_eq!(
        infer_expr_ty(&db, fid, &new_expr, Some(&exp)),
        cls_with("java.util.ArrayList", vec![cls("com.example.Dog")])
    );

    assert_eq!(
        resolve_at_offset(&db, fid, cursor(src, "/*arr_item_len*/"))
            .unwrap()
            .qualified_name,
        "java.lang.String.length"
    );
    assert_eq!(
        resolve_at_offset(&db, fid, cursor(src, "/*res_read*/"))
            .unwrap()
            .qualified_name,
        "com.example.Res.read"
    );
    assert_eq!(
        resolve_at_offset(&db, fid, cursor(src, "/*pat_a_len*/"))
            .unwrap()
            .qualified_name,
        "java.lang.String.length"
    );
}

#[test]
fn lambdas_method_references_and_sam_resolution() {
    let src = "\
package com.example;
import java.util.List;
import java.util.function.Function;

interface ExtraBase {
    void go();
}
interface NotSam extends ExtraBase {
    void stop();
}
interface ObjectOverrideSam {
    boolean equals(Object other);
    String toString();
    int hashCode();
    void run(String s);
    default void helper() {}
    static void util() {}
}

public class User {
    public String getName() { return \"\"; }
    public void ping() {}
}

class Pipeline {
    void run(List<User> users) {
        var /*res_list*/names = users.stream()./*map_call*/map(/*lparam*/u -> /*luse*/u./*get_name*/getName())./*to_list*/toList();
        users.forEach(User::/*mref_ping*/ping);
        Function<User, String> fn = /*fn_lparam*/u -> u.getName();
    }
}
";
    let mut db = db_with_files(&[(1, src)]);
    db.set_classpath_index(Arc::new(rich_jdk_classpath()));
    let fid = FileId(1);

    // SAM identification tests.
    let func_ty = cls_with(
        "java.util.function.Function",
        vec![cls("com.example.User"), Ty::string()],
    );
    let func_sam = find_functional_interface_sam(&db, &func_ty).unwrap();
    assert_eq!(func_sam.method_name, "apply");
    assert_eq!(func_sam.param_types, vec![cls("com.example.User")]);
    assert_eq!(func_sam.return_type, Ty::string());

    assert!(find_functional_interface_sam(&db, &cls("com.example.NotSam")).is_none());
    let obj_sam = find_functional_interface_sam(&db, &cls("com.example.ObjectOverrideSam")).unwrap();
    assert_eq!(obj_sam.method_name, "run");
    assert_eq!(obj_sam.param_types, vec![Ty::string()]);
    assert_eq!(obj_sam.return_type, Ty::Void);

    // Stream pipeline inference.
    let names = resolve_at_offset(&db, fid, cursor(src, "/*res_list*/")).unwrap();
    assert_eq!(names.detail, Some(SymbolDetail::TypeRef("List<String>".to_string())));

    let map_call = resolve_at_offset(&db, fid, cursor(src, "/*map_call*/")).unwrap();
    assert_eq!(map_call.qualified_name, "java.util.stream.Stream.map");
    assert_eq!(
        map_call.detail,
        Some(SymbolDetail::Method {
            return_type: Some("Stream<String>".to_string()),
            params: "(Function<? super User, ? extends String> mapper)".to_string(),
        })
    );

    let lparam = resolve_at_offset(&db, fid, cursor(src, "/*lparam*/")).unwrap();
    assert_eq!(lparam.kind, ResolvedSymbolKind::LambdaParam);
    assert_eq!(lparam.detail, Some(SymbolDetail::TypeRef("User".to_string())));

    let luse = resolve_at_offset(&db, fid, cursor(src, "/*luse*/")).unwrap();
    assert_eq!(luse.detail, Some(SymbolDetail::TypeRef("User".to_string())));

    let get_name = resolve_at_offset(&db, fid, cursor(src, "/*get_name*/")).unwrap();
    assert_eq!(get_name.qualified_name, "com.example.User.getName");

    let to_list = resolve_at_offset(&db, fid, cursor(src, "/*to_list*/")).unwrap();
    assert_eq!(
        to_list.detail,
        Some(SymbolDetail::Method {
            return_type: Some("List<String>".to_string()),
            params: "()".to_string(),
        })
    );

    let mref_ping = resolve_at_offset(&db, fid, cursor(src, "/*mref_ping*/")).unwrap();
    assert_eq!(mref_ping.qualified_name, "com.example.User.ping");

    let fn_lparam = resolve_at_offset(&db, fid, cursor(src, "/*fn_lparam*/")).unwrap();
    assert_eq!(fn_lparam.detail, Some(SymbolDetail::TypeRef("User".to_string())));
}

#[test]
fn type_based_overload_resolution_jls_phases_and_specificity() {
    let src = "\
package com.example;
import java.util.List;

class Base {
    void cross(String s) {}
}

class Overload extends Base {
    void cross(int x) {}

    void foo(int x) {}
    void foo(String s) {}

    void numOrInt(Number n) {}
    void numOrInt(Integer i) {}

    void objOrStr(Object o) {}
    void objOrStr(String s) {}

    void widenOrBox(long l) {}
    void widenOrBox(Integer i) {}

    void varargsTest(int a) {}
    void varargsTest(int a, int... rest) {}

    void test(List<String> xs, Integer boxedInt) {
        /*foo_int*/foo(1);
        /*foo_str*/foo(\"hi\");
        /*num_int*/numOrInt(boxedInt);
        /*num_prim*/numOrInt(1);
        /*obj_str*/objOrStr(\"hi\");
        /*widen_box*/widenOrBox(1);
        /*va_1*/varargsTest(1);
        /*va_2*/varargsTest(1, 2, 3);
        /*cross_str*/cross(\"hello\");
        /*cross_int*/cross(5);
        xs./*rem_idx*/remove(0);
        xs./*rem_obj*/remove(\"item\");
    }
}
";
    let mut db = db_with_files(&[(1, src)]);
    db.set_classpath_index(Arc::new(rich_jdk_classpath()));
    let fid = FileId(1);

    let check_params = |marker: &str, expected_params: &str| {
        let res = resolve_at_offset(&db, fid, cursor(src, marker)).unwrap();
        assert_eq!(
            res.detail,
            Some(SymbolDetail::Method {
                return_type: Some("void".to_string()),
                params: expected_params.to_string(),
            }),
            "at {marker}"
        );
    };

    check_params("/*foo_int*/", "(int x)");
    check_params("/*foo_str*/", "(String s)");
    check_params("/*num_int*/", "(Integer i)");
    check_params("/*num_prim*/", "(Integer i)");
    check_params("/*obj_str*/", "(String s)");
    check_params("/*widen_box*/", "(long l)");
    check_params("/*va_1*/", "(int a)");
    check_params("/*va_2*/", "(int a, int... rest)");
    check_params("/*cross_str*/", "(String s)");
    check_params("/*cross_int*/", "(int x)");

    let rem_idx = resolve_at_offset(&db, fid, cursor(src, "/*rem_idx*/")).unwrap();
    assert_eq!(
        rem_idx.detail,
        Some(SymbolDetail::Method {
            return_type: Some("String".to_string()),
            params: "(int index)".to_string(),
        })
    );

    let rem_obj = resolve_at_offset(&db, fid, cursor(src, "/*rem_obj*/")).unwrap();
    assert_eq!(
        rem_obj.detail,
        Some(SymbolDetail::Method {
            return_type: Some("boolean".to_string()),
            params: "(java.lang.Object o)".to_string(),
        })
    );
}

#[test]
fn type_definition_on_var_lambda_param_and_arrays() {
    let src = "\
package com.example;
import java.util.List;

public class User {
    public String getName() { return \"\"; }
}

class Nav {
    void demo(List<User> users, User[] arr) {
        var /*var_decl*/u = users.get(0);
        /*var_use*/u.getName();
        users.stream().map(/*lparam*/x -> /*lparam_use*/x.getName());
        var /*arr_decl*/copy = arr;
        /*arr_use*/copy[0].getName();
        users./*call_get*/get(0);
    }
}
";
    let mut db = db_with_files(&[(1, src)]);
    db.set_classpath_index(Arc::new(rich_jdk_classpath()));
    let fid = FileId(1);

    for marker in [
        "/*var_decl*/",
        "/*var_use*/",
        "/*lparam*/",
        "/*lparam_use*/",
        "/*arr_decl*/",
        "/*arr_use*/",
        "/*call_get*/",
    ] {
        let td = resolve_type_of_symbol_at_offset(&db, fid, TextSize::from(cursor(src, marker)))
            .unwrap_or_else(|| panic!("expected typeDefinition at {marker}"));
        assert_eq!(td.qualified_name, "com.example.User", "at {marker}");
        assert_eq!(td.file_id, Some(fid));
    }
}

#[test]
fn nullability_var_lub_local_root_and_binary_calls() {
    let user_src = "\
package com.example;
import org.jspecify.annotations.*;

@NullMarked
public class User {
    public String getName() { return \"\"; }
}
";
    let app_src = "\
package com.example;
import org.jspecify.annotations.*;
import java.util.List;

enum Role { ADMIN, GUEST }

@NullMarked
class MarkedApp {
    void run(boolean flag, List<String> /*param_list*/xs, BinaryRepo repo) {
        var /*v_user*/u = new User();
        var /*v_lub*/maybe = flag ? u : null;
        var /*v_str*/s = \"hello\";
        var /*v_cls*/c = User.class;
        var /*v_this*/self = this;
        var /*v_enum*/role = Role.ADMIN;
        String /*loc_plain*/plainLocal = \"x\";
        @Nullable String /*loc_q*/nullableLocal = null;
        List<String> /*loc_list*/listLocal = xs;
        var /*v_bin_find*/found = repo./*call_find*/findUser(\"1\");
        var /*v_bin_req*/req = repo./*call_req*/requireUser(\"1\");
    }

    @NullUnmarked
    void legacyMethod(User /*unmarked_param*/u) {
        var /*unmarked_var*/x = new User();
    }
}

class UnmarkedCaller {
    void call(BinaryRepo repo) {
        var /*ext_find*/f = repo.findUser(\"1\");
        var /*ext_req*/r = repo.requireUser(\"1\");
        var /*ext_user*/u = new User();
    }
}
";
    let mut cp = rich_jdk_classpath();
    let mut bin_repo = bin_cls(
        "com.example.BinaryRepo",
        TypeKind::Interface,
        &[],
        None,
        None,
        &[],
        &[],
        vec![
            bin_method(
                "com.example.BinaryRepo",
                "findUser",
                false,
                true,
                &[],
                "com.example.User?",
                &[("id", "java.lang.String", false)],
            ),
            bin_method(
                "com.example.BinaryRepo",
                "requireUser",
                false,
                true,
                &[],
                "com.example.User",
                &[("id", "java.lang.String", false)],
            ),
        ],
    );
    bin_repo.nullness_scope =
        crate::NullnessScope::NonNullByDefault(crate::NullabilitySource::JSpecify);
    cp.insert_type(bin_repo);
    cp.finalize();

    let mut db = db_with_files(&[(1, user_src), (2, app_src)]);
    db.set_classpath_index(Arc::new(cp));
    let fid = FileId(2);

    let check_var = |marker: &str, expected: &str| {
        let res = resolve_at_offset(&db, fid, cursor(app_src, marker))
            .unwrap_or_else(|| panic!("missing symbol at {marker}"));
        assert_eq!(
            res.detail,
            Some(SymbolDetail::TypeRef(expected.to_string())),
            "mismatch at {marker}"
        );
    };

    check_var("/*param_list*/", "List<String!>!");
    check_var("/*v_user*/", "User!");
    check_var("/*v_lub*/", "User?");
    check_var("/*v_str*/", "String!");
    check_var("/*v_cls*/", "Class<User!>!");
    check_var("/*v_this*/", "MarkedApp!");
    check_var("/*v_enum*/", "Role!");
    // JSpecify §3.2.5: local variable root is NOT forced NonNull, nested type args ARE.
    check_var("/*loc_plain*/", "String");
    check_var("/*loc_q*/", "String?");
    check_var("/*loc_list*/", "List<String!>");
    check_var("/*v_bin_find*/", "User?");
    check_var("/*v_bin_req*/", "User!");

    // `@NullUnmarked` method inside `@NullMarked` class resets defaulting.
    check_var("/*unmarked_param*/", "User");
    check_var("/*unmarked_var*/", "User");

    // Unmarked caller still receives `User?` / `User!` from `@NullMarked` binary JAR methods,
    // while its own `new User()` stays `User` (zero noise on unmarked code).
    check_var("/*ext_find*/", "User?");
    check_var("/*ext_req*/", "User!");
    check_var("/*ext_user*/", "User");

    let call_find = resolve_at_offset(&db, fid, cursor(app_src, "/*call_find*/")).unwrap();
    assert_eq!(
        call_find.detail,
        Some(SymbolDetail::Method {
            return_type: Some("User?".to_string()),
            params: "(String! id)".to_string(),
        })
    );
    let call_req = resolve_at_offset(&db, fid, cursor(app_src, "/*call_req*/")).unwrap();
    assert_eq!(
        call_req.detail,
        Some(SymbolDetail::Method {
            return_type: Some("User!".to_string()),
            params: "(String! id)".to_string(),
        })
    );
}
