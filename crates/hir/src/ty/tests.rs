use std::sync::Arc;

use crate::{
    FileId, RootDatabase,
    classpath::{BinaryOrigin, BinaryTypeSymbol, ClasspathIndex},
    item_tree::FileItemTree,
    symbols::{TypeIdentity, TypeKind, WorkspaceIndex},
    test_fixture::{db_with, db_with_files},
};

use super::*;

fn cls(fqn: &str) -> Ty {
    Ty::raw_class(fqn)
}

fn cls_with(fqn: &str, type_args: Vec<TypeArg>) -> Ty {
    Ty::class(fqn, type_args)
}

fn bin_type(
    fqn: &str,
    kind: TypeKind,
    type_params: &[&str],
    super_class: Option<&str>,
    super_class_generic: Option<&str>,
    interfaces: &[&str],
    interfaces_generic: &[&str],
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
            TypeIdentity::Unspecified
        } else {
            TypeIdentity::Identity
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
        interfaces_generic: interfaces_generic
            .iter()
            .map(|s| (*s).to_string())
            .collect(),
        permitted_subclasses: Vec::new(),
        members: Vec::new(),
        nested_types: Vec::new(),
        origin: BinaryOrigin::Jdk {
            module_name: Some(SmolStr::new("java.base")),
        },
    }
}

fn collection_classpath() -> ClasspathIndex {
    let mut cp = ClasspathIndex::default();
    cp.insert_type(bin_type(
        "java.lang.Object",
        TypeKind::Class,
        &[],
        None,
        None,
        &[],
        &[],
    ));
    cp.insert_type(bin_type(
        "java.lang.Number",
        TypeKind::Class,
        &[],
        Some("java.lang.Object"),
        None,
        &["java.io.Serializable"],
        &[],
    ));
    cp.insert_type(bin_type(
        "java.lang.Integer",
        TypeKind::Class,
        &[],
        Some("java.lang.Number"),
        None,
        &["java.lang.Comparable"],
        &["java.lang.Comparable<java.lang.Integer>"],
    ));
    cp.insert_type(bin_type(
        "java.lang.Double",
        TypeKind::Class,
        &[],
        Some("java.lang.Number"),
        None,
        &["java.lang.Comparable"],
        &["java.lang.Comparable<java.lang.Double>"],
    ));
    cp.insert_type(bin_type(
        "java.lang.String",
        TypeKind::Class,
        &[],
        Some("java.lang.Object"),
        None,
        &["java.io.Serializable", "java.lang.Comparable", "java.lang.CharSequence"],
        &[
            "java.io.Serializable",
            "java.lang.Comparable<java.lang.String>",
            "java.lang.CharSequence",
        ],
    ));
    cp.insert_type(bin_type(
        "java.lang.CharSequence",
        TypeKind::Interface,
        &[],
        None,
        None,
        &[],
        &[],
    ));
    cp.insert_type(bin_type(
        "java.lang.Comparable",
        TypeKind::Interface,
        &["T"],
        None,
        None,
        &[],
        &[],
    ));
    cp.insert_type(bin_type(
        "java.io.Serializable",
        TypeKind::Interface,
        &[],
        None,
        None,
        &[],
        &[],
    ));
    cp.insert_type(bin_type(
        "java.lang.Iterable",
        TypeKind::Interface,
        &["T"],
        None,
        None,
        &[],
        &[],
    ));
    cp.insert_type(bin_type(
        "java.util.Collection",
        TypeKind::Interface,
        &["E"],
        None,
        None,
        &["java.lang.Iterable"],
        &["java.lang.Iterable<E>"],
    ));
    cp.insert_type(bin_type(
        "java.util.List",
        TypeKind::Interface,
        &["E"],
        None,
        None,
        &["java.util.Collection"],
        &["java.util.Collection<E>"],
    ));
    cp.insert_type(bin_type(
        "java.util.AbstractList",
        TypeKind::Class,
        &["E"],
        Some("java.lang.Object"),
        Some("java.lang.Object"),
        &["java.util.List"],
        &["java.util.List<E>"],
    ));
    cp.insert_type(bin_type(
        "java.util.ArrayList",
        TypeKind::Class,
        &["E"],
        Some("java.util.AbstractList"),
        Some("java.util.AbstractList<E>"),
        &["java.util.List"],
        &["java.util.List<E>"],
    ));
    cp.insert_type(bin_type(
        "java.util.LinkedList",
        TypeKind::Class,
        &["E"],
        Some("java.util.AbstractList"),
        Some("java.util.AbstractList<E>"),
        &["java.util.List"],
        &["java.util.List<E>"],
    ));
    cp.finalize();
    cp
}

#[test]
fn parse_type_ref_handles_primitives_arrays_varargs_nested_wildcards_and_annotations() {
    let mut resolver = |segs: &[&str]| -> Option<Ty> {
        match segs {
            ["String"] => Some(Ty::string()),
            ["User"] => Some(cls("com.example.User")),
            ["Number"] => Some(cls("java.lang.Number")),
            ["List"] => Some(cls("java.util.List")),
            ["Map", "Entry"] => Some(cls("java.util.Map.Entry")),
            ["java", "util", "List"] => Some(cls("java.util.List")),
            ["com", "example", "User"] => Some(cls("com.example.User")),
            ["K"] => Some(Ty::type_var("K", Vec::new())),
            ["V"] => Some(Ty::type_var("V", Vec::new())),
            _ => None,
        }
    };

    assert_eq!(
        parse_type_ref("int", &mut resolver),
        Ty::Primitive(PrimitiveType::Int)
    );
    assert_eq!(
        parse_type_ref("boolean", &mut resolver),
        Ty::Primitive(PrimitiveType::Boolean)
    );
    assert_eq!(parse_type_ref("void", &mut resolver), Ty::Void);
    assert_eq!(parse_type_ref("var", &mut resolver), Ty::Unknown);
    assert_eq!(
        parse_type_ref("int[][]", &mut resolver),
        Ty::array(Ty::array(Ty::Primitive(PrimitiveType::Int)))
    );
    assert_eq!(
        parse_type_ref("@Nullable String...", &mut resolver),
        Ty::array(Ty::string())
    );
    assert_eq!(
        parse_type_ref("@com.ann.Foo(bar = \"a(b)\") int @NotNull []", &mut resolver),
        Ty::array(Ty::Primitive(PrimitiveType::Int))
    );
    assert_eq!(
        parse_type_ref("java.util.List<com.example.User>", &mut resolver),
        cls_with("java.util.List", vec![TypeArg::Type(cls("com.example.User"))])
    );
    assert_eq!(
        parse_type_ref("Map.Entry<K, ? extends V>", &mut resolver),
        cls_with(
            "java.util.Map.Entry",
            vec![
                TypeArg::Type(Ty::type_var("K", Vec::new())),
                TypeArg::Wildcard(WildcardBound::Extends(Box::new(Ty::type_var(
                    "V",
                    Vec::new(),
                )))),
            ]
        )
    );
    assert_eq!(
        parse_type_ref("List<?>", &mut resolver),
        cls_with(
            "java.util.List",
            vec![TypeArg::Wildcard(WildcardBound::Unbounded)]
        )
    );
    assert_eq!(
        parse_type_ref("List<? super User>", &mut resolver),
        cls_with(
            "java.util.List",
            vec![TypeArg::Wildcard(WildcardBound::Super(Box::new(cls(
                "com.example.User"
            ))))]
        )
    );
    assert_eq!(parse_type_ref("MissingType", &mut resolver), Ty::Unknown);
}

#[test]
fn boxing_widening_and_numeric_promotion() {
    assert_eq!(PrimitiveType::Int.boxed_fqn(), "java.lang.Integer");
    assert_eq!(
        PrimitiveType::from_boxed_fqn("java.lang.Character"),
        Some(PrimitiveType::Char)
    );
    assert!(PrimitiveType::Byte.widens_to(PrimitiveType::Short));
    assert!(PrimitiveType::Byte.widens_to(PrimitiveType::Double));
    assert!(PrimitiveType::Char.widens_to(PrimitiveType::Int));
    assert!(!PrimitiveType::Char.widens_to(PrimitiveType::Short));
    assert!(!PrimitiveType::Int.widens_to(PrimitiveType::Int));
    assert!(!PrimitiveType::Boolean.widens_to(PrimitiveType::Int));
    assert!(PrimitiveType::Char.is_integral());
    assert!(!PrimitiveType::Double.is_integral());
    assert!(!PrimitiveType::Boolean.is_numeric());

    assert_eq!(
        unary_numeric_promotion(&Ty::Primitive(PrimitiveType::Byte)),
        Some(PrimitiveType::Int)
    );
    assert_eq!(
        unary_numeric_promotion(&cls("java.lang.Character")),
        Some(PrimitiveType::Int)
    );
    assert_eq!(
        unary_numeric_promotion(&cls("java.lang.Double")),
        Some(PrimitiveType::Double)
    );
    assert_eq!(
        unary_numeric_promotion(&Ty::Primitive(PrimitiveType::Boolean)),
        None
    );

    assert_eq!(
        binary_numeric_promotion(
            &Ty::Primitive(PrimitiveType::Int),
            &cls("java.lang.Long")
        ),
        Some(PrimitiveType::Long)
    );
    assert_eq!(
        binary_numeric_promotion(
            &cls("java.lang.Float"),
            &Ty::Primitive(PrimitiveType::Double)
        ),
        Some(PrimitiveType::Double)
    );
    assert_eq!(
        binary_numeric_promotion(
            &Ty::Primitive(PrimitiveType::Short),
            &Ty::Primitive(PrimitiveType::Byte)
        ),
        Some(PrimitiveType::Int)
    );
}

#[test]
fn substitution_apply_covariant_and_cycle_depth_safety() {
    let user = cls("com.example.User");
    let e = Ty::type_var("E", Vec::new());

    let mut subst_ext = Substitution::default();
    subst_ext.map.insert(
        SmolStr::new("E"),
        TypeArg::Wildcard(WildcardBound::Extends(Box::new(user.clone()))),
    );
    assert_eq!(subst_ext.apply_covariant(&e), user);
    assert_eq!(
        subst_ext.apply_covariant(&cls_with("java.util.List", vec![TypeArg::Type(e.clone())])),
        cls_with(
            "java.util.List",
            vec![TypeArg::Wildcard(WildcardBound::Extends(Box::new(
                user.clone()
            )))]
        )
    );

    let mut subst_super = Substitution::default();
    subst_super.map.insert(
        SmolStr::new("E"),
        TypeArg::Wildcard(WildcardBound::Super(Box::new(user.clone()))),
    );
    assert_eq!(subst_super.apply_covariant(&e), Ty::object());

    let mut subst_unbounded = Substitution::default();
    subst_unbounded.map.insert(
        SmolStr::new("E"),
        TypeArg::Wildcard(WildcardBound::Unbounded),
    );
    assert_eq!(subst_unbounded.apply_covariant(&e), Ty::object());

    // Composition: {E -> T} composed with {T -> User} yields {E -> User, T -> User}.
    let mut inner = Substitution::default();
    inner.insert_ty(SmolStr::new("E"), Ty::type_var("T", Vec::new()));
    let mut outer = Substitution::default();
    outer.insert_ty(SmolStr::new("T"), user.clone());
    let composed = inner.compose(&outer);
    assert_eq!(composed.apply(&e), user);

    // Deeply nested type stops cleanly at MAX_SUBST_DEPTH without panicking.
    let mut deep = e;
    for _ in 0..(MAX_SUBST_DEPTH + 10) {
        deep = Ty::array(deep);
    }
    let _ = outer.apply(&deep);
}

#[test]
fn supertype_substitution_across_workspace_source_hierarchy() {
    let src = "\
package com.example;

class User {}
class BaseList<E> {}
class MyList<T> extends BaseList<T> {}
class UserList extends MyList<User> {}
";
    let (db, _) = db_with(src);
    let subst =
        supertype_substitution(&db, "com.example.UserList", &[], "com.example.BaseList")
            .expect("should resolve BaseList substitution");
    assert_eq!(
        subst.map.get("E"),
        Some(&TypeArg::Type(cls("com.example.User")))
    );
}

#[test]
fn supertype_substitution_across_mixed_source_and_binary_hierarchy() {
    let user_src = "\
package com.example;
import java.util.ArrayList;

public class User {}
public class UserList extends ArrayList<User> {}
";
    let mut db = RootDatabase::default();
    let fid = FileId(1);
    db.set_file_text(fid, Arc::new(user_src.to_string()));
    db.set_workspace_files(Arc::new(vec![fid]));
    db.set_classpath_index(Arc::new(collection_classpath()));

    let list_subst =
        supertype_substitution(&db, "com.example.UserList", &[], "java.util.List")
            .expect("should resolve java.util.List");
    assert_eq!(
        list_subst.map.get("E"),
        Some(&TypeArg::Type(cls("com.example.User")))
    );

    let iter_subst =
        supertype_substitution(&db, "com.example.UserList", &[], "java.lang.Iterable")
            .expect("should resolve java.lang.Iterable");
    assert_eq!(
        iter_subst.map.get("T"),
        Some(&TypeArg::Type(cls("com.example.User")))
    );
}

#[test]
fn subtyping_assignability_and_least_upper_bound() {
    let dog_src = "\
package p;
class Animal {}
class Dog extends Animal {}
class Cat extends Animal {}
";
    let mut db = db_with_files(&[(1, dog_src)]);
    db.set_classpath_index(Arc::new(collection_classpath()));

    let animal = cls("p.Animal");
    let dog = cls("p.Dog");
    let cat = cls("p.Cat");
    let num = cls("java.lang.Number");
    let int_box = cls("java.lang.Integer");
    let dbl_box = cls("java.lang.Double");

    assert!(is_subtype(&db, &dog, &animal));
    assert!(!is_subtype(&db, &animal, &dog));
    assert!(is_subtype(&db, &Ty::Null, &dog));
    assert!(!is_subtype(&db, &Ty::Null, &Ty::Primitive(PrimitiveType::Int)));
    assert!(!is_subtype(&db, &Ty::Unknown, &Ty::object()));

    // Array covariance and array superinterfaces.
    let dog_arr = Ty::array(dog.clone());
    let animal_arr = Ty::array(animal.clone());
    assert!(is_subtype(&db, &dog_arr, &animal_arr));
    assert!(is_subtype(&db, &dog_arr, &cls("java.lang.Cloneable")));
    assert!(is_subtype(&db, &dog_arr, &cls("java.io.Serializable")));
    assert!(!is_subtype(
        &db,
        &Ty::array(Ty::Primitive(PrimitiveType::Int)),
        &Ty::array(Ty::Primitive(PrimitiveType::Long))
    ));

    // Generics invariance, wildcards, and raw compatibility.
    let list_dog = cls_with("java.util.List", vec![TypeArg::Type(dog.clone())]);
    let arraylist_dog = cls_with("java.util.ArrayList", vec![TypeArg::Type(dog.clone())]);
    let list_animal = cls_with("java.util.List", vec![TypeArg::Type(animal.clone())]);
    let list_ext_animal = cls_with(
        "java.util.List",
        vec![TypeArg::Wildcard(WildcardBound::Extends(Box::new(
            animal.clone(),
        )))],
    );
    let list_sup_dog = cls_with(
        "java.util.List",
        vec![TypeArg::Wildcard(WildcardBound::Super(Box::new(
            dog.clone(),
        )))],
    );
    let list_q = cls_with(
        "java.util.List",
        vec![TypeArg::Wildcard(WildcardBound::Unbounded)],
    );
    assert!(is_subtype(&db, &arraylist_dog, &list_dog));
    assert!(!is_subtype(&db, &list_dog, &list_animal));
    assert!(is_subtype(&db, &arraylist_dog, &list_ext_animal));
    assert!(is_subtype(&db, &list_animal, &list_sup_dog));
    assert!(is_subtype(&db, &list_dog, &list_q));
    assert!(is_subtype(&db, &cls("java.util.ArrayList"), &list_dog));

    // Boxing / unboxing assignability.
    assert!(!is_subtype(&db, &Ty::Primitive(PrimitiveType::Int), &num));
    assert!(is_assignable(&db, &Ty::Primitive(PrimitiveType::Int), &int_box));
    assert!(is_assignable(&db, &Ty::Primitive(PrimitiveType::Int), &num));
    assert!(is_assignable(&db, &int_box, &Ty::Primitive(PrimitiveType::Int)));
    assert!(is_assignable(&db, &int_box, &Ty::Primitive(PrimitiveType::Double)));

    // TypeVar and Intersection subtyping.
    let tv = Ty::type_var("T", vec![num.clone()]);
    assert!(is_subtype(&db, &tv, &num));
    assert!(is_subtype(&db, &tv, &Ty::object()));
    let inter = Ty::Intersection(vec![animal.clone(), cls("java.io.Serializable")]);
    assert!(is_subtype(&db, &inter, &animal));
    assert!(!is_subtype(&db, &dog, &inter));

    // LUB.
    assert_eq!(least_upper_bound(&db, &dog, &cat), animal);
    assert_eq!(least_upper_bound(&db, &Ty::Null, &dog), dog);
    assert_eq!(
        least_upper_bound(
            &db,
            &Ty::Primitive(PrimitiveType::Int),
            &Ty::Primitive(PrimitiveType::Double)
        ),
        Ty::Primitive(PrimitiveType::Double)
    );
    assert_eq!(
        least_upper_bound(
            &db,
            &cls_with("java.util.ArrayList", vec![TypeArg::Type(int_box)]),
            &cls_with("java.util.LinkedList", vec![TypeArg::Type(dbl_box)]),
        ),
        cls_with(
            "java.util.AbstractList",
            vec![TypeArg::Wildcard(WildcardBound::Extends(Box::new(num)))]
        )
    );
}

#[test]
fn recursive_bounds_and_cyclic_inheritance_do_not_loop() {
    let src = "\
package cyc;
class A<T extends A<T>> {}
class Cyc1 extends Cyc2 {}
class Cyc2 extends Cyc1 {}
";
    let (db, fid) = db_with(src);
    let bounds = type_param_bounds_of_fqn(&db, "cyc.A");
    let ty = resolve_type_ref_in_file(&db, fid, "T", &bounds);
    assert_eq!(
        ty,
        Ty::type_var(
            "T",
            vec![cls_with(
                "cyc.A",
                vec![TypeArg::Type(Ty::type_var("T", Vec::new()))]
            )],
        )
    );

    assert_eq!(
        supertype_substitution(&db, "cyc.Cyc1", &[], "java.lang.String"),
        None
    );
    assert!(!is_subtype(&db, &cls("cyc.Cyc1"), &cls("java.lang.String")));
}

#[test]
fn formatting_short_and_qualified() {
    let map_entry = cls_with(
        "java.util.Map.Entry",
        vec![
            TypeArg::Type(Ty::string()),
            TypeArg::Wildcard(WildcardBound::Extends(Box::new(cls("java.lang.Number")))),
        ],
    );
    assert_eq!(map_entry.display_short(), "Map.Entry<String, ? extends Number>");
    assert_eq!(
        map_entry.display_qualified(),
        "java.util.Map.Entry<java.lang.String, ? extends java.lang.Number>"
    );

    let arr = Ty::array(Ty::Primitive(PrimitiveType::Int));
    assert_eq!(arr.display_short(), "int[]");
    assert_eq!(arr.display_qualified(), "int[]");
    assert_eq!(Ty::object().display_short(), "Object");
    assert_eq!(Ty::object().display_qualified(), "java.lang.Object");
    assert_eq!(
        TypeArg::Wildcard(WildcardBound::Unbounded).display_short(),
        "?"
    );

    assert_eq!(map_entry.erased_fqn(), Some("java.util.Map.Entry"));
    assert_eq!(arr.erased_fqn(), None);
    assert_eq!(
        Ty::type_var("T", vec![cls("java.lang.Number")]).erased_fqn(),
        Some("java.lang.Number")
    );
}

#[test]
fn resolve_binary_and_file_type_refs_and_inner_type_params() {
    let mut scope = BTreeMap::new();
    scope.insert(SmolStr::new("T"), vec!["Comparable<T>".to_string()]);
    let bin_ty = resolve_binary_type_ref("java.util.Map.Entry<String, T>", &scope);
    assert_eq!(
        bin_ty,
        cls_with(
            "java.util.Map.Entry",
            vec![
                TypeArg::Type(Ty::string()),
                TypeArg::Type(Ty::type_var(
                    "T",
                    vec![cls_with(
                        "java.lang.Comparable",
                        vec![TypeArg::Type(Ty::type_var("T", Vec::new()))]
                    )],
                )),
            ]
        )
    );
    assert_eq!(
        resolve_binary_type_ref("Character.Subset", &BTreeMap::new()),
        cls("java.lang.Character.Subset")
    );

    let src = "\
package app;
import java.util.List;
class Outer<K> {
    class Inner<V> {}
    static class StaticNested<S> {}
}
class Child extends java.util.ArrayList<String> {}
";
    let (db, fid) = db_with(src);
    assert_eq!(
        type_params_of_fqn(&db, "app.Outer"),
        vec![SmolStr::new("K")]
    );
    let inner_bounds = type_param_bounds_of_fqn(&db, "app.Outer.Inner");
    assert!(inner_bounds.contains_key("K"));
    assert!(inner_bounds.contains_key("V"));
    let static_bounds = type_param_bounds_of_fqn(&db, "app.Outer.StaticNested");
    assert!(!static_bounds.contains_key("K"));
    assert!(static_bounds.contains_key("S"));

    let resolved = resolve_type_ref_in_file(&db, fid, "List<? extends String>", &BTreeMap::new());
    assert_eq!(
        resolved,
        cls_with(
            "java.util.List",
            vec![TypeArg::Wildcard(WildcardBound::Extends(Box::new(
                Ty::string()
            )))]
        )
    );
}

#[test]
fn valhalla_nullability_parsing_and_formatting() {
    let mut resolver = |segs: &[&str]| -> Option<Ty> {
        match segs {
            ["Point"] => Some(cls("com.example.Point")),
            ["String"] => Some(Ty::string()),
            ["Number"] => Some(cls("java.lang.Number")),
            ["List"] => Some(cls("java.util.List")),
            ["Map"] => Some(cls("java.util.Map")),
            ["T"] => Some(Ty::type_var("T", Vec::new())),
            _ => None,
        }
    };

    let point_nn = parse_type_ref("Point!", &mut resolver);
    assert_eq!(point_nn.nullability(), Nullability::NonNull);
    assert!(!point_nn.is_nullable());
    assert_eq!(point_nn.display_short(), "Point!");
    assert_eq!(point_nn.display_qualified(), "com.example.Point!");

    let point_q = parse_type_ref("Point?", &mut resolver);
    assert_eq!(point_q.nullability(), Nullability::Nullable);
    assert!(point_q.is_nullable());
    assert_eq!(point_q.display_short(), "Point?");
    assert_eq!(point_q.display_qualified(), "com.example.Point?");

    let list_point_nn = parse_type_ref("List<Point!>", &mut resolver);
    assert_eq!(list_point_nn.display_short(), "List<Point!>");
    assert_eq!(
        list_point_nn.display_qualified(),
        "java.util.List<com.example.Point!>"
    );

    let map_ty = parse_type_ref("Map<String!, ? extends Number>", &mut resolver);
    assert_eq!(map_ty.display_short(), "Map<String!, ? extends Number>");
    assert_eq!(
        map_ty.display_qualified(),
        "java.util.Map<java.lang.String!, ? extends java.lang.Number>"
    );

    let str_arr_nn = parse_type_ref("String![]!", &mut resolver);
    assert_eq!(str_arr_nn.nullability(), Nullability::NonNull);
    assert!(!str_arr_nn.is_nullable());
    assert_eq!(str_arr_nn.display_short(), "String![]!");
    assert_eq!(str_arr_nn.display_qualified(), "java.lang.String![]!");

    let tvar_nn = parse_type_ref("T!", &mut resolver);
    assert_eq!(tvar_nn.nullability(), Nullability::NonNull);
    assert!(!tvar_nn.is_nullable());
    assert_eq!(tvar_nn.display_short(), "T!");

    // Primitives are always non-null and unaffected by `with_nullability`.
    let prim = Ty::Primitive(PrimitiveType::Int);
    assert_eq!(prim.nullability(), Nullability::NonNull);
    assert!(!prim.is_nullable());
    assert!(prim.is_primitive());
    assert_eq!(prim.clone().with_nullability(Nullability::Nullable), prim);
    assert_eq!(
        prim.boxed(),
        Ty::raw_class("java.lang.Integer").with_nullability(Nullability::NonNull)
    );
}

#[test]
fn valhalla_null_restricted_subtyping_assignability_and_lub() {
    let src = "\
package p;
class Point {}
";
    let mut db = db_with_files(&[(1, src)]);
    db.set_classpath_index(Arc::new(collection_classpath()));

    let point = cls("p.Point");
    let point_nn = point.clone().with_nullability(Nullability::NonNull);
    let point_q = point.clone().with_nullability(Nullability::Nullable);
    let tvar_nn = Ty::type_var("T", Vec::new()).with_nullability(Nullability::NonNull);
    let str_arr_nn = Ty::array(Ty::string()).with_nullability(Nullability::NonNull);

    // `null` is NOT a subtype of non-null types or primitives.
    assert!(!is_subtype(&db, &Ty::Null, &point_nn));
    assert!(!is_subtype(&db, &Ty::Null, &tvar_nn));
    assert!(!is_subtype(&db, &Ty::Null, &str_arr_nn));
    assert!(!is_subtype(&db, &Ty::Null, &Ty::Primitive(PrimitiveType::Int)));
    assert!(is_subtype(&db, &Ty::Null, &point_q));
    assert!(is_subtype(&db, &Ty::Null, &point));

    // Nullability subtyping lattice.
    assert!(is_subtype(&db, &point_nn, &point_q));
    assert!(is_subtype(&db, &point_nn, &point));
    assert!(is_subtype(&db, &point_q, &point));
    assert!(is_subtype(&db, &point, &point_q));
    assert!(!is_subtype(&db, &point_q, &point_nn));
    assert!(!is_subtype(&db, &point, &point_nn));

    // Resilient assignability: `Unspecified -> NonNull` allowed; `Null`/`Nullable -> NonNull` rejected.
    assert!(is_assignable(&db, &point, &point_nn));
    assert!(!is_assignable(&db, &point_q, &point_nn));
    assert!(!is_assignable(&db, &Ty::Null, &point_nn));

    // LUB with Null and mixed nullability.
    assert_eq!(least_upper_bound(&db, &point_nn, &Ty::Null), point_q);
    assert_eq!(least_upper_bound(&db, &Ty::Null, &point_nn), point_q);
    assert_eq!(least_upper_bound(&db, &point_nn, &point_q), point_q);
    assert_eq!(least_upper_bound(&db, &point_nn, &point_nn), point_nn);

    // Array covariance: `Point![] <: Object[]`, while `int[]` stays invariant.
    let point_nn_arr = Ty::array(point_nn);
    let obj_arr = Ty::array(Ty::object());
    let int_arr = Ty::array(Ty::Primitive(PrimitiveType::Int));
    assert!(is_subtype(&db, &point_nn_arr, &obj_arr));
    assert!(!is_subtype(&db, &int_arr, &obj_arr));
}

#[test]
fn valhalla_is_value_type_for_primitives_workspace_and_binary_types() {
    #[salsa::db]
    #[derive(Clone, Default)]
    struct ValueOverrideDb {
        storage: salsa::Storage<Self>,
        inner: RootDatabase,
        value_fqn: String,
    }

    #[salsa::db]
    impl salsa::Database for ValueOverrideDb {}

    impl crate::SourceDatabase for ValueOverrideDb {
        fn file_text(&self, file_id: FileId) -> Arc<String> {
            self.inner.file_text(file_id)
        }
        fn file_language_level(&self, file_id: FileId) -> syntax::LanguageLevel {
            self.inner.file_language_level(file_id)
        }
        fn parse(&self, file_id: FileId) -> syntax::Parse {
            self.inner.parse(file_id)
        }
        fn syntax_tree(&self, file_id: FileId) -> syntax::SyntaxNode {
            self.inner.syntax_tree(file_id)
        }
        fn syntax_errors(&self, file_id: FileId) -> Arc<Vec<syntax::SyntaxError>> {
            self.inner.syntax_errors(file_id)
        }
    }

    impl DefDatabase for ValueOverrideDb {
        fn workspace_files(&self) -> Arc<Vec<FileId>> {
            self.inner.workspace_files()
        }
        fn file_item_tree(&self, file_id: FileId) -> Arc<FileItemTree> {
            let mut tree = (*self.inner.file_item_tree(file_id)).clone();
            for (_, ty) in tree.types.iter_mut() {
                if ty.qualified_name == self.value_fqn {
                    ty.identity = TypeIdentity::Value;
                }
            }
            Arc::new(tree)
        }
        fn ast_id_map(&self, file_id: FileId) -> Arc<crate::AstIdMap> {
            self.inner.ast_id_map(file_id)
        }
        fn file_symbols(&self, file_id: FileId) -> Arc<crate::FileSymbols> {
            self.inner.file_symbols(file_id)
        }
        fn module_index(&self, module_id: crate::ModuleId) -> Arc<crate::ModuleIndex> {
            self.inner.module_index(module_id)
        }
        fn workspace_index(&self) -> Arc<WorkspaceIndex> {
            self.inner.workspace_index()
        }
        fn workspace_hierarchy_index(&self) -> Arc<crate::WorkspaceHierarchyIndex> {
            self.inner.workspace_hierarchy_index()
        }
        fn classpath_index(&self) -> Arc<ClasspathIndex> {
            self.inner.classpath_index()
        }
    }

    let src = "\
package p;
class WsValue {}
class WsIdentity {}
abstract class WsAbstract {}
interface WsInterface {}
";
    let mut inner = db_with_files(&[(1, src)]);
    let mut cp = collection_classpath();
    let mut bin_val = bin_type(
        "bin.BinValue",
        TypeKind::Class,
        &[],
        Some("java.lang.Object"),
        None,
        &[],
        &[],
    );
    bin_val.identity = TypeIdentity::Value;
    cp.insert_type(bin_val);
    cp.finalize();
    inner.set_classpath_index(Arc::new(cp));

    let db = ValueOverrideDb {
        storage: salsa::Storage::default(),
        inner,
        value_fqn: "p.WsValue".to_string(),
    };

    assert!(Ty::Primitive(PrimitiveType::Int).is_value_type(&db));
    assert!(cls("p.WsValue").is_value_type(&db));
    assert!(cls("bin.BinValue").is_value_type(&db));
    assert!(!cls("p.WsIdentity").is_value_type(&db));
    assert!(!cls("p.WsAbstract").is_value_type(&db));
    assert!(!cls("p.WsInterface").is_value_type(&db));
    assert!(!cls("java.lang.String").is_value_type(&db));
    assert!(!cls("java.util.List").is_value_type(&db));
    assert!(!Ty::array(cls("p.WsValue")).is_value_type(&db));
}

#[test]
fn valhalla_universal_generic_substitution_preserves_primitive_and_nullability() {
    let tvar_t = Ty::type_var("T", Vec::new());
    let tvar_t_nn = tvar_t.clone().with_nullability(Nullability::NonNull);
    let tvar_t_q = tvar_t.clone().with_nullability(Nullability::Nullable);
    let box_t = cls_with("p.Box", vec![TypeArg::Type(tvar_t.clone())]);

    // Box<int>
    let mut subst_int = Substitution::default();
    subst_int.insert_ty(SmolStr::new("T"), Ty::Primitive(PrimitiveType::Int));
    assert_eq!(subst_int.apply(&tvar_t), Ty::Primitive(PrimitiveType::Int));
    assert_eq!(
        subst_int.apply(&tvar_t_nn),
        Ty::Primitive(PrimitiveType::Int)
    );
    assert_eq!(
        subst_int.apply(&box_t),
        cls_with("p.Box", vec![TypeArg::Type(Ty::Primitive(PrimitiveType::Int))])
    );

    // Box<Point!>
    let point_nn = cls("p.Point").with_nullability(Nullability::NonNull);
    let mut subst_point_nn = Substitution::default();
    subst_point_nn.insert_ty(SmolStr::new("T"), point_nn.clone());
    assert_eq!(subst_point_nn.apply(&tvar_t), point_nn);
    assert_eq!(
        subst_point_nn.apply(&tvar_t_q),
        cls("p.Point").with_nullability(Nullability::Nullable)
    );
    assert_eq!(
        subst_point_nn.apply(&box_t),
        cls_with("p.Box", vec![TypeArg::Type(point_nn.clone())])
    );

    // T! substituted with unspecified `Point` produces `Point!`.
    let mut subst_point = Substitution::default();
    subst_point.insert_ty(SmolStr::new("T"), cls("p.Point"));
    assert_eq!(subst_point.apply(&tvar_t_nn), point_nn);
    assert_eq!(
        subst_point.apply_covariant(&cls_with("p.Box", vec![TypeArg::Type(tvar_t_nn)])),
        cls_with("p.Box", vec![TypeArg::Type(point_nn)])
    );
}

#[test]
fn effective_nullness_scope_nesting_across_module_package_types_and_members() {
    use crate::symbols::{NullabilitySource, NullnessScope};

    let mod_info = "\
import org.jspecify.annotations.NullMarked;
@NullMarked
module com.example.app {}
";
    let unmarked_pkg = "\
@NullUnmarked
package com.example.legacy;
import org.jspecify.annotations.NullUnmarked;
";
    let marked_cls = "\
package com.example.core;
import org.jspecify.annotations.*;

public class InheritedTop {
    public String fromMod;
    @NullUnmarked
    public static class UnmarkedInner {
        public String plain;
        @NullMarked
        public String reMarked() { return \"\"; }
    }
}
";
    let legacy_cls = "\
package com.example.legacy;
import org.jspecify.annotations.*;

public class Legacy {
    public String unmarkedFromPkg;
    @NullMarked
    public static class OptedIn {
        public String nonNull;
    }
}
";
    let db = db_with_files(&[
        (1, mod_info),
        (2, unmarked_pkg),
        (3, marked_cls),
        (4, legacy_cls),
    ]);

    let marked_scope = NullnessScope::NonNullByDefault(NullabilitySource::JSpecify);
    let unmarked_scope = NullnessScope::Unspecified(NullabilitySource::JSpecify);

    assert_eq!(
        effective_nullness_scope_for_file(&db, FileId(3)),
        marked_scope
    );
    assert_eq!(
        effective_nullness_scope_for_file(&db, FileId(4)),
        unmarked_scope
    );

    assert_eq!(
        effective_nullness_scope_for_type(&db, "com.example.core.InheritedTop"),
        marked_scope
    );
    assert_eq!(
        effective_nullness_scope_for_type(&db, "com.example.core.InheritedTop.UnmarkedInner"),
        unmarked_scope
    );
    assert_eq!(
        effective_nullness_scope_for_member(
            &db,
            "com.example.core.InheritedTop.UnmarkedInner",
            marked_scope,
        ),
        marked_scope
    );
    assert_eq!(
        effective_nullness_scope_for_type(&db, "com.example.legacy.Legacy"),
        unmarked_scope
    );
    assert_eq!(
        effective_nullness_scope_for_type(&db, "com.example.legacy.Legacy.OptedIn"),
        marked_scope
    );
}

#[test]
fn jspecify_defaulting_jls_9_7_4_and_typevar_projection() {
    use crate::symbols::{NullabilitySource, NullnessScope};

    let marked = NullnessScope::NonNullByDefault(NullabilitySource::JSpecify);

    // `apply_nullness_scope` with `is_local_root = false` vs `true`.
    let list_str = cls_with("java.util.List", vec![TypeArg::Type(Ty::string())]);
    assert_eq!(
        apply_nullness_scope(list_str.clone(), marked, false).display_short(),
        "List<String!>!"
    );
    assert_eq!(
        apply_nullness_scope(list_str, marked, true).display_short(),
        "List<String!>"
    );

    // JLS §9.7.4 array & nested type placement inside `@NullMarked`.
    let src = "\
package com.example;
import org.jspecify.annotations.*;

@NullMarked
class Outer {
    static class Inner {}
}

@NullMarked
class ArraysAndNested<T, U extends @Nullable Object> {
    @Nullable String[] elemNullableArr;
    String @Nullable [] outerNullableArr;
    Outer.@Nullable Inner nestedNullable;
    Outer.Inner nestedNonNull;
    T parametricT;
    @Nullable T nullableT;
    @NonNull T nonNullT;
    U unboundedNullableBound;
}
";
    let (db, fid) = db_with(src);
    let tree = db.file_item_tree(fid);
    let (_, cls_item) = tree.find_type_by_fqn("com.example.ArraysAndNested").unwrap();
    let tp_bounds = type_param_bounds_of_fqn(&db, "com.example.ArraysAndNested");

    let resolve_member = |idx: usize| {
        let m = &tree.members[cls_item.members[idx]];
        resolve_type_ref_in_file_for_member(
            &db,
            fid,
            Some("com.example.ArraysAndNested"),
            m.nullness_scope,
            m.type_ref.as_deref().unwrap(),
            &tp_bounds,
        )
    };

    assert_eq!(resolve_member(0).display_short(), "String?[]!");
    assert_eq!(resolve_member(1).display_short(), "String![]?");
    assert_eq!(resolve_member(2).display_short(), "Outer.Inner?");
    assert_eq!(resolve_member(3).display_short(), "Outer.Inner!");

    // Parametric TypeVar T: unannotated `T` stays `Unspecified` on the TypeVar itself,
    // with implicit upper bound `Object!`.
    let t_ty = resolve_member(4);
    assert_eq!(t_ty.nullability(), Nullability::Unspecified);
    assert_eq!(t_ty.display_short(), "T");
    assert_eq!(
        t_ty,
        Ty::type_var("T", vec![Ty::object().with_nullability(Nullability::NonNull)])
    );

    let t_nullable = resolve_member(5);
    assert_eq!(t_nullable.nullability(), Nullability::Nullable);
    assert_eq!(t_nullable.display_short(), "T?");

    let t_nonnull = resolve_member(6);
    assert_eq!(t_nonnull.nullability(), Nullability::NonNull);
    assert_eq!(t_nonnull.display_short(), "T!");

    let u_ty = resolve_member(7);
    assert_eq!(
        u_ty,
        Ty::type_var("U", vec![Ty::object().with_nullability(Nullability::Nullable)])
    );

    // Substitution & TypeVar projection.
    let str_q = Ty::string().with_nullability(Nullability::Nullable);
    let str_nn = Ty::string().with_nullability(Nullability::NonNull);
    let mut subst_q = Substitution::default();
    subst_q.insert_ty(SmolStr::new("T"), str_q.clone());
    assert_eq!(subst_q.apply(&t_ty), str_q);
    assert_eq!(subst_q.apply(&t_nonnull), str_nn);

    let mut subst_nn = Substitution::default();
    subst_nn.insert_ty(SmolStr::new("T"), str_nn.clone());
    assert_eq!(subst_nn.apply(&t_ty), str_nn);
    assert_eq!(subst_nn.apply(&t_nullable), str_q);

    // Wildcard `?` covariant projection respects upper bound (`Object!` vs `Object?` vs `Object`).
    let mut subst_wc = Substitution::default();
    subst_wc.map.insert(
        SmolStr::new("T"),
        TypeArg::Wildcard(WildcardBound::Unbounded),
    );
    subst_wc.map.insert(
        SmolStr::new("U"),
        TypeArg::Wildcard(WildcardBound::Unbounded),
    );
    assert_eq!(
        subst_wc.apply_covariant(&t_ty),
        Ty::object().with_nullability(Nullability::NonNull)
    );
    assert_eq!(
        subst_wc.apply_covariant(&u_ty),
        Ty::object().with_nullability(Nullability::Nullable)
    );
    assert_eq!(
        subst_wc.apply_covariant(&t_nullable),
        Ty::object().with_nullability(Nullability::Nullable)
    );
    assert_eq!(
        subst_wc.apply_covariant(&Ty::type_var("T", Vec::new())),
        Ty::object()
    );
}
