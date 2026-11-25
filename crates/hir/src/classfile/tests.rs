use ristretto_classfile::{
    BaseType, ClassAccessFlags, ClassFile, ConstantPool, Field, FieldAccessFlags, FieldType,
    JAVA_17, JAVA_21, JAVA_25, Method, MethodAccessFlags, Version,
    attributes::{
        Annotation, Attribute, Exports, ExportsFlags, InnerClass, MethodParameter,
        MethodParameterAccessFlags, ModuleAccessFlags, ModuleAttribute, NestedClassAccessFlags,
        Record as RecordAttr, Requires, RequiresFlags, TargetPath, TargetType, TypeAnnotation,
    },
};

use crate::{symbols::NullabilitySource, ty::Nullability};

use super::*;

fn jdk_base_origin() -> BinaryOrigin {
    BinaryOrigin::Jdk {
        module_name: Some(SmolStr::new("java.base")),
    }
}

fn serialize(cf: &ClassFile<'_>) -> Vec<u8> {
    let mut bytes = Vec::new();
    cf.to_bytes(&mut bytes).expect("serialize synthetic classfile");
    bytes
}

#[test]
fn parses_java_27_class_with_generics_methods_and_method_parameters() {
    let mut cp = ConstantPool::default();
    let this_class = cp.add_class("com/example/Box").unwrap();
    let super_class = cp.add_class("java/lang/Object").unwrap();
    let iface = cp.add_class("java/io/Serializable").unwrap();

    let sig_name = cp.add_utf8("Signature").unwrap();
    let class_sig = cp.add_utf8("<T:Ljava/lang/Object;>Ljava/lang/Object;Ljava/io/Serializable;").unwrap();

    let field_name = cp.add_utf8("value").unwrap();
    let field_desc = cp.add_utf8("Ljava/lang/Object;").unwrap();
    let field_sig = cp.add_utf8("TT;").unwrap();

    let ctor_name = cp.add_utf8("<init>").unwrap();
    let ctor_desc = cp.add_utf8("(Ljava/lang/Object;)V").unwrap();
    let ctor_sig = cp.add_utf8("(TT;)V").unwrap();

    let method_name = cp.add_utf8("formatAll").unwrap();
    let method_desc = cp.add_utf8("(I[Ljava/lang/String;)Ljava/util/List;").unwrap();
    let method_sig = cp.add_utf8("(I[Ljava/lang/String;)Ljava/util/List<Ljava/lang/String;>;").unwrap();
    let mp_attr_name = cp.add_utf8("MethodParameters").unwrap();
    let p0_name = cp.add_utf8("limit").unwrap();
    let p1_name = cp.add_utf8("items").unwrap();

    let cf = ClassFile {
        version: JAVA_25,
        access_flags: ClassAccessFlags::PUBLIC | ClassAccessFlags::SUPER,
        this_class,
        super_class,
        interfaces: vec![iface],
        fields: vec![Field {
            access_flags: FieldAccessFlags::PUBLIC,
            name_index: field_name,
            descriptor_index: field_desc,
            field_type: FieldType::Object("java/lang/Object".into()),
            attributes: vec![Attribute::Signature {
                name_index: sig_name,
                signature_index: field_sig,
            }],
        }],
        methods: vec![
            Method {
                access_flags: MethodAccessFlags::PUBLIC,
                name_index: ctor_name,
                descriptor_index: ctor_desc,
                attributes: vec![Attribute::Signature {
                    name_index: sig_name,
                    signature_index: ctor_sig,
                }],
            },
            Method {
                access_flags: MethodAccessFlags::PUBLIC
                    | MethodAccessFlags::STATIC
                    | MethodAccessFlags::VARARGS,
                name_index: method_name,
                descriptor_index: method_desc,
                attributes: vec![
                    Attribute::Signature {
                        name_index: sig_name,
                        signature_index: method_sig,
                    },
                    Attribute::MethodParameters {
                        name_index: mp_attr_name,
                        parameters: vec![
                            MethodParameter {
                                name_index: p0_name,
                                access_flags: MethodParameterAccessFlags::FINAL,
                            },
                            MethodParameter {
                                name_index: p1_name,
                                access_flags: MethodParameterAccessFlags::empty(),
                            },
                        ],
                    },
                ],
            },
        ],
        attributes: vec![Attribute::Signature {
            name_index: sig_name,
            signature_index: class_sig,
        }],
        constant_pool: cp,
        ..Default::default()
    };

    let mut bytes = serialize(&cf);
    // Stamp Java 27 major version (71) into the classfile header to verify JDK 27 support.
    bytes[6..8].copy_from_slice(&71u16.to_be_bytes());
    assert_eq!(u16::from_be_bytes([bytes[6], bytes[7]]), 71);
    let Some(ParsedClassfile::Type(ty)) = parse_class_bytes(&bytes, jdk_base_origin()).unwrap()
    else {
        panic!("expected ParsedClassfile::Type");
    };

    assert_eq!(ty.name, "Box");
    assert_eq!(ty.qualified_name, "com.example.Box");
    assert_eq!(ty.package, "com.example");
    assert_eq!(ty.kind, TypeKind::Class);
    assert_eq!(ty.type_params, vec![SmolStr::new("T")]);
    assert_eq!(ty.super_class.as_deref(), Some("java.lang.Object"));
    assert_eq!(ty.interfaces, vec!["java.io.Serializable".to_string()]);

    assert_eq!(ty.members.len(), 3);
    let field = &ty.members[0];
    assert_eq!(field.kind, MemberKind::Field);
    assert_eq!(field.name, "value");
    assert_eq!(field.type_ref.as_deref(), Some("T"));

    let ctor = &ty.members[1];
    assert_eq!(ctor.kind, MemberKind::Constructor);
    assert_eq!(ctor.name, "Box");
    assert_eq!(ctor.type_ref, None);
    assert_eq!(ctor.params[0].type_ref, "T");
    assert_eq!(ctor.params[0].name.as_deref(), Some("arg0"));

    let method = &ty.members[2];
    assert_eq!(method.kind, MemberKind::Method);
    assert_eq!(method.name, "formatAll");
    assert!(method.is_static);
    assert!(method.is_varargs);
    assert_eq!(method.type_ref.as_deref(), Some("java.util.List<String>"));
    assert_eq!(method.params.len(), 2);
    assert_eq!(method.params[0].name.as_deref(), Some("limit"));
    assert_eq!(method.params[0].type_ref, "int");
    assert!(!method.params[0].is_varargs);
    assert_eq!(method.params[1].name.as_deref(), Some("items"));
    assert_eq!(method.params[1].type_ref, "String");
    assert!(method.params[1].is_varargs);
}

#[test]
fn parses_interface_enum_record_annotation_and_sealed_permits() {
    // 1. Sealed interface with PermittedSubclasses & InnerClasses
    let mut cp = ConstantPool::default();
    let this_class = cp.add_class("java/util/Map").unwrap();
    let super_class = cp.add_class("java/lang/Object").unwrap();
    let entry_class = cp.add_class("java/util/Map$Entry").unwrap();
    let entry_name = cp.add_utf8("Entry").unwrap();
    let ic_attr = cp.add_utf8("InnerClasses").unwrap();
    let permits_attr = cp.add_utf8("PermittedSubclasses").unwrap();
    let sub_class = cp.add_class("java/util/HashMap").unwrap();

    let iface_cf = ClassFile {
        version: JAVA_17,
        access_flags: ClassAccessFlags::PUBLIC
            | ClassAccessFlags::INTERFACE
            | ClassAccessFlags::ABSTRACT,
        this_class,
        super_class,
        attributes: vec![
            Attribute::InnerClasses {
                name_index: ic_attr,
                classes: vec![InnerClass {
                    class_info_index: entry_class,
                    outer_class_info_index: this_class,
                    name_index: entry_name,
                    access_flags: NestedClassAccessFlags::PUBLIC
                        | NestedClassAccessFlags::STATIC
                        | NestedClassAccessFlags::INTERFACE
                        | NestedClassAccessFlags::ABSTRACT,
                }],
            },
            Attribute::PermittedSubclasses {
                name_index: permits_attr,
                class_indexes: vec![sub_class],
            },
        ],
        constant_pool: cp,
        ..Default::default()
    };
    let Some(ParsedClassfile::Type(iface_ty)) =
        parse_class_bytes(&serialize(&iface_cf), jdk_base_origin()).unwrap()
    else {
        panic!("expected interface");
    };
    assert_eq!(iface_ty.kind, TypeKind::Interface);
    assert_eq!(iface_ty.nested_types, vec![SmolStr::new("Entry")]);
    assert_eq!(
        iface_ty.permitted_subclasses,
        vec!["java.util.HashMap".to_string()]
    );

    // 2. Nested Map$Entry classfile
    let mut cp = ConstantPool::default();
    let entry_cls = cp.add_class("java/util/Map$Entry").unwrap();
    let map_cls = cp.add_class("java/util/Map").unwrap();
    let obj_cls = cp.add_class("java/lang/Object").unwrap();
    let entry_utf = cp.add_utf8("Entry").unwrap();
    let ic_attr = cp.add_utf8("InnerClasses").unwrap();
    let entry_cf = ClassFile {
        version: JAVA_21,
        access_flags: ClassAccessFlags::INTERFACE | ClassAccessFlags::ABSTRACT,
        this_class: entry_cls,
        super_class: obj_cls,
        attributes: vec![Attribute::InnerClasses {
            name_index: ic_attr,
            classes: vec![InnerClass {
                class_info_index: entry_cls,
                outer_class_info_index: map_cls,
                name_index: entry_utf,
                access_flags: NestedClassAccessFlags::PUBLIC
                    | NestedClassAccessFlags::STATIC
                    | NestedClassAccessFlags::INTERFACE,
            }],
        }],
        constant_pool: cp,
        ..Default::default()
    };
    let Some(ParsedClassfile::Type(entry_ty)) =
        parse_class_bytes(&serialize(&entry_cf), jdk_base_origin()).unwrap()
    else {
        panic!("expected Map.Entry");
    };
    assert_eq!(entry_ty.name, "Entry");
    assert_eq!(entry_ty.qualified_name, "java.util.Map.Entry");
    assert_eq!(entry_ty.internal_name, "java/util/Map$Entry");
    assert_eq!(entry_ty.enclosing_type.as_deref(), Some("java.util.Map"));
    assert!(entry_ty.is_public);
    assert!(entry_ty.is_static);

    // 3. Enum
    let mut cp = ConstantPool::default();
    let enum_cls = cp.add_class("p/Color").unwrap();
    let enum_super = cp.add_class("java/lang/Enum").unwrap();
    let red_name = cp.add_utf8("RED").unwrap();
    let red_desc = cp.add_utf8("Lp/Color;").unwrap();
    let enum_cf = ClassFile {
        version: JAVA_21,
        access_flags: ClassAccessFlags::PUBLIC
            | ClassAccessFlags::FINAL
            | ClassAccessFlags::ENUM,
        this_class: enum_cls,
        super_class: enum_super,
        fields: vec![Field {
            access_flags: FieldAccessFlags::PUBLIC
                | FieldAccessFlags::STATIC
                | FieldAccessFlags::FINAL
                | FieldAccessFlags::ENUM,
            name_index: red_name,
            descriptor_index: red_desc,
            field_type: FieldType::Object("p/Color".into()),
            attributes: vec![],
        }],
        constant_pool: cp,
        ..Default::default()
    };
    let Some(ParsedClassfile::Type(enum_ty)) =
        parse_class_bytes(&serialize(&enum_cf), jdk_base_origin()).unwrap()
    else {
        panic!("expected enum");
    };
    assert_eq!(enum_ty.kind, TypeKind::Enum);
    assert_eq!(enum_ty.members[0].kind, MemberKind::EnumConstant);
    assert_eq!(enum_ty.members[0].name, "RED");

    // 4. Record
    let mut cp = ConstantPool::default();
    let rec_cls = cp.add_class("p/Point").unwrap();
    let rec_super = cp.add_class("java/lang/Record").unwrap();
    let rec_attr = cp.add_utf8("Record").unwrap();
    let x_name = cp.add_utf8("x").unwrap();
    let i_desc = cp.add_utf8("I").unwrap();
    let rec_cf = ClassFile {
        version: JAVA_21,
        access_flags: ClassAccessFlags::PUBLIC | ClassAccessFlags::FINAL,
        this_class: rec_cls,
        super_class: rec_super,
        attributes: vec![Attribute::Record {
            name_index: rec_attr,
            records: vec![RecordAttr {
                name_index: x_name,
                descriptor_index: i_desc,
                attributes: vec![],
            }],
        }],
        constant_pool: cp,
        ..Default::default()
    };
    let Some(ParsedClassfile::Type(rec_ty)) =
        parse_class_bytes(&serialize(&rec_cf), jdk_base_origin()).unwrap()
    else {
        panic!("expected record");
    };
    assert_eq!(rec_ty.kind, TypeKind::Record);
    assert_eq!(rec_ty.members[0].kind, MemberKind::RecordComponent);
    assert_eq!(rec_ty.members[0].name, "x");
    assert_eq!(rec_ty.members[0].type_ref.as_deref(), Some("int"));

    // 5. Annotation
    let mut cp = ConstantPool::default();
    let ann_cls = cp.add_class("p/Tag").unwrap();
    let obj_cls = cp.add_class("java/lang/Object").unwrap();
    let ann_cf = ClassFile {
        version: JAVA_21,
        access_flags: ClassAccessFlags::PUBLIC
            | ClassAccessFlags::INTERFACE
            | ClassAccessFlags::ABSTRACT
            | ClassAccessFlags::ANNOTATION,
        this_class: ann_cls,
        super_class: obj_cls,
        constant_pool: cp,
        ..Default::default()
    };
    let Some(ParsedClassfile::Type(ann_ty)) =
        parse_class_bytes(&serialize(&ann_cf), jdk_base_origin()).unwrap()
    else {
        panic!("expected annotation");
    };
    assert_eq!(ann_ty.kind, TypeKind::Annotation);
}

#[test]
fn parses_module_info_with_unqualified_exports_and_requires_transitive() {
    let mut cp = ConstantPool::default();
    let this_class = cp.add_class("module-info").unwrap();
    let mod_attr_name = cp.add_utf8("Module").unwrap();
    let self_mod = cp.add_module("java.sql").unwrap();
    let dep_trans = cp.add_module("java.xml").unwrap();
    let dep_priv = cp.add_module("java.logging").unwrap();
    let friend_mod = cp.add_module("some.friend").unwrap();
    let pub_pkg = cp.add_package("java/sql").unwrap();
    let qual_pkg = cp.add_package("com/sun/sql/internal").unwrap();

    let mod_cf = ClassFile {
        version: Version::Java21 { minor: 0 },
        access_flags: ClassAccessFlags::MODULE,
        this_class,
        super_class: 0,
        attributes: vec![Attribute::Module {
            name_index: mod_attr_name,
            module: Box::new(ModuleAttribute {
                module_name_index: self_mod,
                flags: ModuleAccessFlags::empty(),
                version_index: 0,
                requires: vec![
                    Requires {
                        index: dep_trans,
                        flags: RequiresFlags::TRANSITIVE,
                        version_index: 0,
                    },
                    Requires {
                        index: dep_priv,
                        flags: RequiresFlags::empty(),
                        version_index: 0,
                    },
                ],
                exports: vec![
                    Exports {
                        index: pub_pkg,
                        flags: ExportsFlags::empty(),
                        to_index: vec![],
                    },
                    Exports {
                        index: qual_pkg,
                        flags: ExportsFlags::empty(),
                        to_index: vec![friend_mod],
                    },
                ],
                opens: vec![],
                uses: vec![],
                provides: vec![],
            }),
        }],
        constant_pool: cp,
        ..Default::default()
    };

    let Some(ParsedClassfile::Module(m)) =
        parse_class_bytes(&serialize(&mod_cf), jdk_base_origin()).unwrap()
    else {
        panic!("expected ParsedClassfile::Module");
    };
    assert_eq!(m.name, "java.sql");
    assert_eq!(m.exports, vec!["java.sql".to_string()]);
    assert_eq!(m.requires_transitive, vec![SmolStr::new("java.xml")]);
}

#[test]
fn filters_synthetic_bridge_private_and_anonymous_local_entries() {
    // Package-private top-level class -> None
    let mut cp = ConstantPool::default();
    let pkg_priv_cls = cp.add_class("p/Hidden").unwrap();
    let obj_cls = cp.add_class("java/lang/Object").unwrap();
    let hidden_cf = ClassFile {
        version: JAVA_21,
        access_flags: ClassAccessFlags::SUPER,
        this_class: pkg_priv_cls,
        super_class: obj_cls,
        constant_pool: cp,
        ..Default::default()
    };
    assert_eq!(
        parse_class_bytes(&serialize(&hidden_cf), jdk_base_origin()).unwrap(),
        None
    );

    // Anonymous class ($1) -> None
    let mut cp = ConstantPool::default();
    let anon_cls = cp.add_class("p/Outer$1").unwrap();
    let obj_cls = cp.add_class("java/lang/Object").unwrap();
    let anon_cf = ClassFile {
        version: JAVA_21,
        access_flags: ClassAccessFlags::PUBLIC | ClassAccessFlags::SUPER,
        this_class: anon_cls,
        super_class: obj_cls,
        constant_pool: cp,
        ..Default::default()
    };
    assert_eq!(
        parse_class_bytes(&serialize(&anon_cf), jdk_base_origin()).unwrap(),
        None
    );

    // Local class ($1Local) -> None
    let mut cp = ConstantPool::default();
    let local_cls = cp.add_class("p/Outer$1Local").unwrap();
    let obj_cls = cp.add_class("java/lang/Object").unwrap();
    let local_cf = ClassFile {
        version: JAVA_21,
        access_flags: ClassAccessFlags::PUBLIC | ClassAccessFlags::SUPER,
        this_class: local_cls,
        super_class: obj_cls,
        constant_pool: cp,
        ..Default::default()
    };
    assert_eq!(
        parse_class_bytes(&serialize(&local_cf), jdk_base_origin()).unwrap(),
        None
    );

    // InnerClasses without outer_class_info_index -> None
    let mut cp = ConstantPool::default();
    let inner_no_outer = cp.add_class("p/Outer$NamedLocal").unwrap();
    let obj_cls = cp.add_class("java/lang/Object").unwrap();
    let ic_attr = cp.add_utf8("InnerClasses").unwrap();
    let local_name = cp.add_utf8("NamedLocal").unwrap();
    let no_outer_cf = ClassFile {
        version: JAVA_21,
        access_flags: ClassAccessFlags::PUBLIC | ClassAccessFlags::SUPER,
        this_class: inner_no_outer,
        super_class: obj_cls,
        attributes: vec![Attribute::InnerClasses {
            name_index: ic_attr,
            classes: vec![InnerClass {
                class_info_index: inner_no_outer,
                outer_class_info_index: 0,
                name_index: local_name,
                access_flags: NestedClassAccessFlags::PUBLIC,
            }],
        }],
        constant_pool: cp,
        ..Default::default()
    };
    assert_eq!(
        parse_class_bytes(&serialize(&no_outer_cf), jdk_base_origin()).unwrap(),
        None
    );

    // Public class with synthetic/bridge/private members + malformed generic signature fallback.
    let mut cp = ConstantPool::default();
    let pub_cls = cp.add_class("p/Api").unwrap();
    let obj_cls = cp.add_class("java/lang/Object").unwrap();
    let f_priv = cp.add_utf8("secret").unwrap();
    let f_synth = cp.add_utf8("synth").unwrap();
    let f_prot = cp.add_utf8("prot").unwrap();
    let desc_i = cp.add_utf8("I").unwrap();
    let desc_obj = cp.add_utf8("Ljava/lang/Object;").unwrap();
    let m_bridge = cp.add_utf8("compareTo").unwrap();
    let m_clinit = cp.add_utf8("<clinit>").unwrap();
    let m_ok = cp.add_utf8("ok").unwrap();
    let desc_m = cp.add_utf8("()Ljava/lang/String;").unwrap();
    let desc_v = cp.add_utf8("()V").unwrap();
    let sig_attr = cp.add_utf8("Signature").unwrap();
    let bad_sig = cp.add_utf8("<<<BROKEN").unwrap();

    let api_cf = ClassFile {
        version: JAVA_21,
        access_flags: ClassAccessFlags::PUBLIC | ClassAccessFlags::SUPER,
        this_class: pub_cls,
        super_class: obj_cls,
        fields: vec![
            Field {
                access_flags: FieldAccessFlags::PRIVATE,
                name_index: f_priv,
                descriptor_index: desc_i,
                field_type: FieldType::Base(BaseType::Int),
                attributes: vec![],
            },
            Field {
                access_flags: FieldAccessFlags::PUBLIC | FieldAccessFlags::SYNTHETIC,
                name_index: f_synth,
                descriptor_index: desc_i,
                field_type: FieldType::Base(BaseType::Int),
                attributes: vec![],
            },
            Field {
                access_flags: FieldAccessFlags::PROTECTED,
                name_index: f_prot,
                descriptor_index: desc_obj,
                field_type: FieldType::Object("java/lang/Object".into()),
                attributes: vec![Attribute::Signature {
                    name_index: sig_attr,
                    signature_index: bad_sig,
                }],
            },
        ],
        methods: vec![
            Method {
                access_flags: MethodAccessFlags::PUBLIC | MethodAccessFlags::BRIDGE,
                name_index: m_bridge,
                descriptor_index: desc_v,
                attributes: vec![],
            },
            Method {
                access_flags: MethodAccessFlags::PUBLIC | MethodAccessFlags::STATIC,
                name_index: m_clinit,
                descriptor_index: desc_v,
                attributes: vec![],
            },
            Method {
                access_flags: MethodAccessFlags::PUBLIC,
                name_index: m_ok,
                descriptor_index: desc_m,
                attributes: vec![Attribute::Signature {
                    name_index: sig_attr,
                    signature_index: bad_sig,
                }],
            },
        ],
        constant_pool: cp,
        ..Default::default()
    };

    let Some(ParsedClassfile::Type(api_ty)) =
        parse_class_bytes(&serialize(&api_cf), jdk_base_origin()).unwrap()
    else {
        panic!("expected Api type");
    };
    assert_eq!(api_ty.members.len(), 2);
    assert_eq!(api_ty.members[0].name, "prot");
    assert!(!api_ty.members[0].is_public);
    assert_eq!(api_ty.members[0].type_ref.as_deref(), Some("Object"));
    assert_eq!(api_ty.members[1].name, "ok");
    assert_eq!(api_ty.members[1].type_ref.as_deref(), Some("String"));
}

#[test]
fn returns_typed_error_on_corrupt_class_bytes() {
    let err = parse_class_bytes(b"not a classfile", jdk_base_origin()).unwrap_err();
    assert!(!err.to_string().is_empty());
}

#[test]
fn parses_class_and_method_generic_signatures_bounds_and_supertypes() {
    let mut cp = ConstantPool::default();
    let this_class = cp.add_class("com/example/MyList").unwrap();
    let super_class = cp.add_class("java/util/AbstractList").unwrap();
    let iface_list = cp.add_class("java/util/List").unwrap();
    let iface_ra = cp.add_class("java/util/RandomAccess").unwrap();
    let iface_comp = cp.add_class("java/util/Comparator").unwrap();

    let sig_attr = cp.add_utf8("Signature").unwrap();
    let class_sig = cp
        .add_utf8(
            "<T:Ljava/lang/Number;:Ljava/lang/Comparable<TT;>;U:Ljava/lang/Object;V::Ljava/io/Serializable;>\
            Ljava/util/AbstractList<Ljava/util/Map<TK;TV;>.Entry<TK;TV;>;>;\
            Ljava/util/List<+Ljava/lang/Number;>;\
            Ljava/util/RandomAccess;\
            Ljava/util/Comparator<-TT;>;",
        )
        .unwrap();

    let ctor_name = cp.add_utf8("<init>").unwrap();
    let ctor_desc = cp.add_utf8("(Ljava/lang/Exception;)V").unwrap();
    let ctor_sig = cp.add_utf8("<E:Ljava/lang/Exception;>(TE;)V").unwrap();

    let method_name = cp.add_utf8("max").unwrap();
    let method_desc = cp
        .add_utf8("(Ljava/lang/Comparable;Ljava/lang/Object;)Ljava/lang/Comparable;")
        .unwrap();
    let method_sig = cp
        .add_utf8("<T:Ljava/lang/Comparable<TT;>;U:Ljava/lang/Object;>(TT;TU;)TT;")
        .unwrap();

    let plain_name = cp.add_utf8("plain").unwrap();
    let plain_desc = cp.add_utf8("()I").unwrap();

    let cf = ClassFile {
        version: JAVA_21,
        access_flags: ClassAccessFlags::PUBLIC | ClassAccessFlags::SUPER,
        this_class,
        super_class,
        interfaces: vec![iface_list, iface_ra, iface_comp],
        methods: vec![
            Method {
                access_flags: MethodAccessFlags::PUBLIC,
                name_index: ctor_name,
                descriptor_index: ctor_desc,
                attributes: vec![Attribute::Signature {
                    name_index: sig_attr,
                    signature_index: ctor_sig,
                }],
            },
            Method {
                access_flags: MethodAccessFlags::PUBLIC | MethodAccessFlags::STATIC,
                name_index: method_name,
                descriptor_index: method_desc,
                attributes: vec![Attribute::Signature {
                    name_index: sig_attr,
                    signature_index: method_sig,
                }],
            },
            Method {
                access_flags: MethodAccessFlags::PUBLIC,
                name_index: plain_name,
                descriptor_index: plain_desc,
                attributes: vec![],
            },
        ],
        attributes: vec![Attribute::Signature {
            name_index: sig_attr,
            signature_index: class_sig,
        }],
        constant_pool: cp,
        ..Default::default()
    };

    let Some(ParsedClassfile::Type(ty)) =
        parse_class_bytes(&serialize(&cf), jdk_base_origin()).unwrap()
    else {
        panic!("expected ParsedClassfile::Type");
    };

    assert_eq!(
        ty.type_params,
        vec![SmolStr::new("T"), SmolStr::new("U"), SmolStr::new("V")]
    );
    assert_eq!(
        ty.type_param_bounds,
        BTreeMap::from([
            (
                SmolStr::new("T"),
                vec![
                    "java.lang.Number".to_string(),
                    "java.lang.Comparable<T>".to_string()
                ]
            ),
            (
                SmolStr::new("V"),
                vec!["java.io.Serializable".to_string()]
            ),
        ])
    );
    assert_eq!(
        ty.super_class_generic.as_deref(),
        Some("java.util.AbstractList<java.util.Map.Entry<K, V>>")
    );
    assert_eq!(
        ty.interfaces_generic,
        vec![
            "java.util.List<? extends java.lang.Number>".to_string(),
            "java.util.RandomAccess".to_string(),
            "java.util.Comparator<? super T>".to_string(),
        ]
    );

    let ctor = &ty.members[0];
    assert_eq!(ctor.type_params, vec![SmolStr::new("E")]);
    assert_eq!(
        ctor.type_param_bounds,
        BTreeMap::from([(SmolStr::new("E"), vec!["java.lang.Exception".to_string()])])
    );
    assert_eq!(ctor.type_ref, None);

    let max = &ty.members[1];
    assert_eq!(max.type_params, vec![SmolStr::new("T"), SmolStr::new("U")]);
    assert_eq!(
        max.type_param_bounds,
        BTreeMap::from([(
            SmolStr::new("T"),
            vec!["java.lang.Comparable<T>".to_string()]
        )])
    );
    assert_eq!(max.type_ref.as_deref(), Some("T"));
    assert_eq!(max.params[0].type_ref, "T");
    assert_eq!(max.params[1].type_ref, "U");

    let plain = &ty.members[2];
    assert!(plain.type_params.is_empty());
    assert!(plain.type_param_bounds.is_empty());
}

#[test]
fn parses_non_generic_class_extending_generic_superclass_and_wildcard_star() {
    let mut cp = ConstantPool::default();
    let this_class = cp.add_class("com/example/StringList").unwrap();
    let super_class = cp.add_class("java/util/ArrayList").unwrap();
    let iface = cp.add_class("java/util/Collection").unwrap();
    let sig_attr = cp.add_utf8("Signature").unwrap();
    let class_sig = cp
        .add_utf8("Ljava/util/ArrayList<Ljava/lang/String;>;Ljava/util/Collection<*>;")
        .unwrap();

    let cf = ClassFile {
        version: JAVA_21,
        access_flags: ClassAccessFlags::PUBLIC | ClassAccessFlags::SUPER,
        this_class,
        super_class,
        interfaces: vec![iface],
        attributes: vec![Attribute::Signature {
            name_index: sig_attr,
            signature_index: class_sig,
        }],
        constant_pool: cp,
        ..Default::default()
    };

    let Some(ParsedClassfile::Type(ty)) =
        parse_class_bytes(&serialize(&cf), jdk_base_origin()).unwrap()
    else {
        panic!("expected ParsedClassfile::Type");
    };

    assert!(ty.type_params.is_empty());
    assert!(ty.type_param_bounds.is_empty());
    assert_eq!(
        ty.super_class_generic.as_deref(),
        Some("java.util.ArrayList<java.lang.String>")
    );
    assert_eq!(
        ty.interfaces_generic,
        vec!["java.util.Collection<?>".to_string()]
    );

    // Class without Signature attribute leaves generic fields empty/None.
    let mut cp2 = ConstantPool::default();
    let plain_cls = cp2.add_class("com/example/Plain").unwrap();
    let obj_cls = cp2.add_class("java/lang/Object").unwrap();
    let plain_cf = ClassFile {
        version: JAVA_21,
        access_flags: ClassAccessFlags::PUBLIC | ClassAccessFlags::SUPER,
        this_class: plain_cls,
        super_class: obj_cls,
        constant_pool: cp2,
        ..Default::default()
    };
    let Some(ParsedClassfile::Type(plain_ty)) =
        parse_class_bytes(&serialize(&plain_cf), jdk_base_origin()).unwrap()
    else {
        panic!("expected ParsedClassfile::Type");
    };
    assert_eq!(plain_ty.super_class_generic, None);
    assert!(plain_ty.interfaces_generic.is_empty());
}

#[test]
fn parses_package_info_and_module_info_nullness_scopes() {
    // 1. package-info.class (marked ACC_SYNTHETIC by javac!) with @NullMarked
    let mut cp = ConstantPool::default();
    let this_cls = cp.add_class("com/example/package-info").unwrap();
    let obj_cls = cp.add_class("java/lang/Object").unwrap();
    let rva_attr = cp.add_utf8("RuntimeVisibleAnnotations").unwrap();
    let null_marked_desc = cp.add_utf8("Lorg/jspecify/annotations/NullMarked;").unwrap();

    let pkg_cf = ClassFile {
        version: JAVA_21,
        access_flags: ClassAccessFlags::INTERFACE
            | ClassAccessFlags::ABSTRACT
            | ClassAccessFlags::SYNTHETIC,
        this_class: this_cls,
        super_class: obj_cls,
        attributes: vec![Attribute::RuntimeVisibleAnnotations {
            name_index: rva_attr,
            annotations: vec![Annotation {
                type_index: null_marked_desc,
                elements: vec![],
            }],
        }],
        constant_pool: cp,
        ..Default::default()
    };
    let Some(ParsedClassfile::Package {
        package,
        nullness_scope,
    }) = parse_class_bytes(&serialize(&pkg_cf), jdk_base_origin()).unwrap()
    else {
        panic!("expected ParsedClassfile::Package");
    };
    assert_eq!(package, "com.example");
    assert_eq!(
        nullness_scope,
        NullnessScope::NonNullByDefault(NullabilitySource::JSpecify)
    );

    // 2. module-info.class with @NullMarked
    let mut cp = ConstantPool::default();
    let this_cls = cp.add_class("module-info").unwrap();
    let mod_attr = cp.add_utf8("Module").unwrap();
    let self_mod = cp.add_module("com.example.mod").unwrap();
    let rva_attr = cp.add_utf8("RuntimeVisibleAnnotations").unwrap();
    let null_marked_desc = cp.add_utf8("Lorg/jspecify/annotations/NullMarked;").unwrap();

    let mod_cf = ClassFile {
        version: JAVA_21,
        access_flags: ClassAccessFlags::MODULE,
        this_class: this_cls,
        super_class: 0,
        attributes: vec![
            Attribute::Module {
                name_index: mod_attr,
                module: Box::new(ModuleAttribute {
                    module_name_index: self_mod,
                    flags: ModuleAccessFlags::empty(),
                    version_index: 0,
                    requires: vec![],
                    exports: vec![],
                    opens: vec![],
                    uses: vec![],
                    provides: vec![],
                }),
            },
            Attribute::RuntimeVisibleAnnotations {
                name_index: rva_attr,
                annotations: vec![Annotation {
                    type_index: null_marked_desc,
                    elements: vec![],
                }],
            },
        ],
        constant_pool: cp,
        ..Default::default()
    };
    let Some(ParsedClassfile::Module(m)) =
        parse_class_bytes(&serialize(&mod_cf), jdk_base_origin()).unwrap()
    else {
        panic!("expected ParsedClassfile::Module");
    };
    assert_eq!(
        m.nullness_scope,
        NullnessScope::NonNullByDefault(NullabilitySource::JSpecify)
    );
}

#[test]
fn decodes_visible_and_invisible_annotations_and_type_paths() {
    let mut cp = ConstantPool::default();
    let this_cls = cp.add_class("com/example/NullApi").unwrap();
    let super_cls = cp.add_class("java/util/ArrayList").unwrap();

    let sig_attr = cp.add_utf8("Signature").unwrap();
    let rva_attr = cp.add_utf8("RuntimeVisibleAnnotations").unwrap();
    let ria_attr = cp.add_utf8("RuntimeInvisibleAnnotations").unwrap();
    let rvpa_attr = cp.add_utf8("RuntimeVisibleParameterAnnotations").unwrap();
    let rvta_attr = cp.add_utf8("RuntimeVisibleTypeAnnotations").unwrap();
    let rita_attr = cp.add_utf8("RuntimeInvisibleTypeAnnotations").unwrap();

    let jspec_marked = cp.add_utf8("Lorg/jspecify/annotations/NullMarked;").unwrap();
    let jspec_unmarked = cp
        .add_utf8("Lorg/jspecify/annotations/NullUnmarked;")
        .unwrap();
    let jspec_nullable = cp.add_utf8("Lorg/jspecify/annotations/Nullable;").unwrap();
    let jspec_nonnull = cp.add_utf8("Lorg/jspecify/annotations/NonNull;").unwrap();
    let jb_nullable = cp
        .add_utf8("Lorg/jetbrains/annotations/Nullable;")
        .unwrap();
    let javax_nullable = cp.add_utf8("Ljavax/annotation/Nullable;").unwrap();

    // Class signature: `<T:Ljava/lang/Object;>Ljava/util/ArrayList<Ljava/lang/String;>;`
    // with `<T extends @Nullable Object>` and `extends ArrayList<@Nullable String>`.
    let class_sig = cp
        .add_utf8("<T:Ljava/lang/Object;>Ljava/util/ArrayList<Ljava/lang/String;>;")
        .unwrap();

    // Field 1: `List<@Nullable String>`
    let f_list = cp.add_utf8("list").unwrap();
    let f_list_desc = cp.add_utf8("Ljava/util/List;").unwrap();
    let f_list_sig = cp.add_utf8("Ljava/util/List<Ljava/lang/String;>;").unwrap();

    // Field 2: `String @Nullable []` (type_path = [])
    let f_arr_outer = cp.add_utf8("arrOuter").unwrap();
    let f_arr_desc = cp.add_utf8("[Ljava/lang/String;").unwrap();

    // Field 3: `@org.jetbrains.annotations.Nullable String[]` (CLASS-retention invisible!)
    let f_arr_elem = cp.add_utf8("arrElem").unwrap();

    // Field 4: `Map<String, @Nullable List<@NonNull User>>`
    let f_map = cp.add_utf8("nestedMap").unwrap();
    let f_map_desc = cp.add_utf8("Ljava/util/Map;").unwrap();
    let f_map_sig = cp
        .add_utf8("Ljava/util/Map<Ljava/lang/String;Ljava/util/List<Lcom/example/User;>;>;")
        .unwrap();

    // Method: `@NullUnmarked List<? extends @Nullable String> transform(@javax.annotation.Nullable String a, @Nullable String[] b)`
    let m_name = cp.add_utf8("transform").unwrap();
    let m_desc = cp
        .add_utf8("(Ljava/lang/String;[Ljava/lang/String;)Ljava/util/List;")
        .unwrap();
    let m_sig = cp
        .add_utf8("(Ljava/lang/String;[Ljava/lang/String;)Ljava/util/List<+Ljava/lang/String;>;")
        .unwrap();

    let cf = ClassFile {
        version: JAVA_21,
        access_flags: ClassAccessFlags::PUBLIC | ClassAccessFlags::SUPER,
        this_class: this_cls,
        super_class: super_cls,
        fields: vec![
            Field {
                access_flags: FieldAccessFlags::PUBLIC,
                name_index: f_list,
                descriptor_index: f_list_desc,
                field_type: FieldType::Object("java/util/List".into()),
                attributes: vec![
                    Attribute::Signature {
                        name_index: sig_attr,
                        signature_index: f_list_sig,
                    },
                    Attribute::RuntimeVisibleTypeAnnotations {
                        name_index: rvta_attr,
                        type_annotations: vec![TypeAnnotation {
                            target_type: TargetType::Empty { target_type: 0x13 },
                            type_path: vec![TargetPath {
                                type_path_kind: 3,
                                type_argument_index: 0,
                            }],
                            type_index: jspec_nullable,
                            elements: vec![],
                        }],
                    },
                ],
            },
            Field {
                access_flags: FieldAccessFlags::PUBLIC,
                name_index: f_arr_outer,
                descriptor_index: f_arr_desc,
                field_type: FieldType::Array(Box::new(FieldType::Object(
                    "java/lang/String".into(),
                ))),
                attributes: vec![Attribute::RuntimeVisibleTypeAnnotations {
                    name_index: rvta_attr,
                    type_annotations: vec![TypeAnnotation {
                        target_type: TargetType::Empty { target_type: 0x13 },
                        type_path: vec![],
                        type_index: jspec_nullable,
                        elements: vec![],
                    }],
                }],
            },
            Field {
                access_flags: FieldAccessFlags::PUBLIC,
                name_index: f_arr_elem,
                descriptor_index: f_arr_desc,
                field_type: FieldType::Array(Box::new(FieldType::Object(
                    "java/lang/String".into(),
                ))),
                attributes: vec![
                    // JetBrains emits both RuntimeInvisibleAnnotations and RuntimeInvisibleTypeAnnotations.
                    Attribute::RuntimeInvisibleAnnotations {
                        name_index: ria_attr,
                        annotations: vec![Annotation {
                            type_index: jb_nullable,
                            elements: vec![],
                        }],
                    },
                    Attribute::RuntimeInvisibleTypeAnnotations {
                        name_index: rita_attr,
                        type_annotations: vec![TypeAnnotation {
                            target_type: TargetType::Empty { target_type: 0x13 },
                            type_path: vec![TargetPath {
                                type_path_kind: 0,
                                type_argument_index: 0,
                            }],
                            type_index: jb_nullable,
                            elements: vec![],
                        }],
                    },
                ],
            },
            Field {
                access_flags: FieldAccessFlags::PUBLIC,
                name_index: f_map,
                descriptor_index: f_map_desc,
                field_type: FieldType::Object("java/util/Map".into()),
                attributes: vec![
                    Attribute::Signature {
                        name_index: sig_attr,
                        signature_index: f_map_sig,
                    },
                    Attribute::RuntimeVisibleTypeAnnotations {
                        name_index: rvta_attr,
                        type_annotations: vec![
                            TypeAnnotation {
                                target_type: TargetType::Empty { target_type: 0x13 },
                                type_path: vec![TargetPath {
                                    type_path_kind: 3,
                                    type_argument_index: 1,
                                }],
                                type_index: jspec_nullable,
                                elements: vec![],
                            },
                            TypeAnnotation {
                                target_type: TargetType::Empty { target_type: 0x13 },
                                type_path: vec![
                                    TargetPath {
                                        type_path_kind: 3,
                                        type_argument_index: 1,
                                    },
                                    TargetPath {
                                        type_path_kind: 3,
                                        type_argument_index: 0,
                                    },
                                ],
                                type_index: jspec_nonnull,
                                elements: vec![],
                            },
                        ],
                    },
                ],
            },
        ],
        methods: vec![Method {
            access_flags: MethodAccessFlags::PUBLIC,
            name_index: m_name,
            descriptor_index: m_desc,
            attributes: vec![
                Attribute::Signature {
                    name_index: sig_attr,
                    signature_index: m_sig,
                },
                Attribute::RuntimeVisibleAnnotations {
                    name_index: rva_attr,
                    annotations: vec![Annotation {
                        type_index: jspec_unmarked,
                        elements: vec![],
                    }],
                },
                Attribute::RuntimeVisibleParameterAnnotations {
                    name_index: rvpa_attr,
                    parameter_annotations: vec![
                        ristretto_classfile::attributes::ParameterAnnotation {
                            annotations: vec![Annotation {
                                type_index: javax_nullable,
                                elements: vec![],
                            }],
                        },
                        ristretto_classfile::attributes::ParameterAnnotation {
                            annotations: vec![],
                        },
                    ],
                },
                Attribute::RuntimeVisibleTypeAnnotations {
                    name_index: rvta_attr,
                    type_annotations: vec![
                        TypeAnnotation {
                            target_type: TargetType::Empty { target_type: 0x14 },
                            type_path: vec![
                                TargetPath {
                                    type_path_kind: 3,
                                    type_argument_index: 0,
                                },
                                TargetPath {
                                    type_path_kind: 2,
                                    type_argument_index: 0,
                                },
                            ],
                            type_index: jspec_nullable,
                            elements: vec![],
                        },
                        TypeAnnotation {
                            target_type: TargetType::FormalParameter {
                                target_type: 0x16,
                                formal_parameter_index: 1,
                            },
                            type_path: vec![TargetPath {
                                type_path_kind: 0,
                                type_argument_index: 0,
                            }],
                            type_index: jspec_nullable,
                            elements: vec![],
                        },
                    ],
                },
            ],
        }],
        attributes: vec![
            Attribute::Signature {
                name_index: sig_attr,
                signature_index: class_sig,
            },
            Attribute::RuntimeVisibleAnnotations {
                name_index: rva_attr,
                annotations: vec![Annotation {
                    type_index: jspec_marked,
                    elements: vec![],
                }],
            },
            Attribute::RuntimeVisibleTypeAnnotations {
                name_index: rvta_attr,
                type_annotations: vec![
                    TypeAnnotation {
                        target_type: TargetType::TypeParameterBound {
                            target_type: 0x11,
                            type_parameter_index: 0,
                            bound_index: 0,
                        },
                        type_path: vec![],
                        type_index: jspec_nullable,
                        elements: vec![],
                    },
                    TypeAnnotation {
                        target_type: TargetType::SuperType {
                            target_type: 0x10,
                            supertype_index: u16::MAX,
                        },
                        type_path: vec![TargetPath {
                            type_path_kind: 3,
                            type_argument_index: 0,
                        }],
                        type_index: jspec_nullable,
                        elements: vec![],
                    },
                ],
            },
        ],
        constant_pool: cp,
        ..Default::default()
    };

    let Some(ParsedClassfile::Type(ty)) =
        parse_class_bytes(&serialize(&cf), jdk_base_origin()).unwrap()
    else {
        panic!("expected ParsedClassfile::Type");
    };

    assert_eq!(
        ty.nullness_scope,
        NullnessScope::NonNullByDefault(NullabilitySource::JSpecify)
    );
    assert_eq!(
        ty.type_param_bounds,
        BTreeMap::from([(SmolStr::new("T"), vec!["java.lang.Object?".to_string()])])
    );
    assert_eq!(
        ty.super_class_generic.as_deref(),
        Some("java.util.ArrayList<java.lang.String?>")
    );

    assert_eq!(
        ty.members[0].type_ref.as_deref(),
        Some("java.util.List<String?>")
    );
    assert_eq!(ty.members[1].type_ref.as_deref(), Some("String[]?"));
    assert_eq!(ty.members[2].type_ref.as_deref(), Some("String?[]"));
    assert_eq!(ty.members[2].decl_nullability, None);
    assert_eq!(
        ty.members[3].type_ref.as_deref(),
        Some("java.util.Map<String, java.util.List<com.example.User!>?>")
    );

    let m = &ty.members[4];
    assert_eq!(
        m.nullness_scope,
        NullnessScope::Unspecified(NullabilitySource::JSpecify)
    );
    assert_eq!(
        m.type_ref.as_deref(),
        Some("java.util.List<? extends String?>")
    );
    assert_eq!(m.params[0].type_ref, "String?");
    assert_eq!(
        m.params[0].decl_nullability,
        Some((
            Nullability::Nullable,
            NullabilitySource::ThirdPartyAnnotation
        ))
    );
    assert_eq!(m.params[1].type_ref, "String?[]");
    assert_eq!(m.params[1].decl_nullability, None);
}
