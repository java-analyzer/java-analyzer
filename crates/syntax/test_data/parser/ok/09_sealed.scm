(SOURCE_FILE
  (INTERFACE_DECL
    (MODIFIER_LIST
      (PUBLIC_KW "public")
      (IDENT "sealed"))
    (INTERFACE_KW "interface")
    (NAME (IDENT "Shape"))
    (PERMITS_CLAUSE
      (IDENT "permits")
      (PATH_TYPE (NAME_REF (IDENT "Circle")))
      (COMMA ",")
      (PATH_TYPE (NAME_REF (IDENT "Square"))))
    (CLASS_BODY
      (L_CURLY "{")
      (R_CURLY "}")))
  (CLASS_DECL
    (MODIFIER_LIST (FINAL_KW "final"))
    (CLASS_KW "class")
    (NAME (IDENT "Circle"))
    (IMPLEMENTS_CLAUSE
      (IMPLEMENTS_KW "implements")
      (PATH_TYPE (NAME_REF (IDENT "Shape"))))
    (CLASS_BODY
      (L_CURLY "{")
      (R_CURLY "}")))
  (CLASS_DECL
    (MODIFIER_LIST (NON_SEALED_KW "non-sealed"))
    (CLASS_KW "class")
    (NAME (IDENT "Square"))
    (IMPLEMENTS_CLAUSE
      (IMPLEMENTS_KW "implements")
      (PATH_TYPE (NAME_REF (IDENT "Shape"))))
    (CLASS_BODY
      (L_CURLY "{")
      (R_CURLY "}")))
  (CLASS_DECL
    (MODIFIER_LIST
      (IDENT "sealed")
      (ABSTRACT_KW "abstract"))
    (CLASS_KW "class")
    (NAME (IDENT "Base"))
    (PERMITS_CLAUSE
      (IDENT "permits")
      (PATH_TYPE (NAME_REF (IDENT "Derived"))))
    (CLASS_BODY
      (L_CURLY "{")
      (R_CURLY "}"))))
