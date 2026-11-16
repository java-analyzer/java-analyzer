(SOURCE_FILE
  (INTERFACE_DECL
    (INTERFACE_KW "interface")
    (NAME (IDENT "I"))
    (CLASS_BODY
      (L_CURLY "{")
      (R_CURLY "}")))
  (ENUM_DECL
    (ENUM_KW "enum")
    (NAME (IDENT "E"))
    (ENUM_BODY
      (L_CURLY "{")
      (R_CURLY "}")))
  (RECORD_DECL
    (IDENT "record")
    (NAME (IDENT "R"))
    (RECORD_COMPONENT_LIST
      (L_PAREN "(")
      (RECORD_COMPONENT
        (PRIMITIVE_TYPE (INT_KW "int"))
        (NAME (IDENT "x")))
      (COMMA ",")
      (RECORD_COMPONENT
        (PATH_TYPE (NAME_REF (IDENT "String")))
        (NAME (IDENT "y")))
      (R_PAREN ")"))
    (CLASS_BODY
      (L_CURLY "{")
      (R_CURLY "}")))
  (ANNOTATION_TYPE_DECL
    (AT "@")
    (INTERFACE_KW "interface")
    (NAME (IDENT "A"))
    (CLASS_BODY
      (L_CURLY "{")
      (R_CURLY "}")))
  (CLASS_DECL
    (MODIFIER_LIST (ABSTRACT_KW "abstract"))
    (CLASS_KW "class")
    (NAME (IDENT "Bar"))
    (CLASS_BODY
      (L_CURLY "{")
      (R_CURLY "}")))
  (CLASS_DECL
    (MODIFIER_LIST
      (PUBLIC_KW "public")
      (STATIC_KW "static")
      (STRICTFP_KW "strictfp"))
    (CLASS_KW "class")
    (NAME (IDENT "Baz"))
    (CLASS_BODY
      (L_CURLY "{")
      (R_CURLY "}"))))
