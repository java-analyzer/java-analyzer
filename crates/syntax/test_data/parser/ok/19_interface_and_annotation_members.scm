(SOURCE_FILE
  (INTERFACE_DECL
    (INTERFACE_KW "interface")
    (NAME (IDENT "I"))
    (CLASS_BODY
      (L_CURLY "{")
      (MEMBER_PLACEHOLDER
        (INT_KW "int")
        (IDENT "CONSTANT")
        (EQ "=")
        (INT_NUMBER "1")
        (SEMICOLON ";"))
      (MEMBER_PLACEHOLDER
        (VOID_KW "void")
        (IDENT "m")
        (L_PAREN "(")
        (R_PAREN ")")
        (SEMICOLON ";"))
      (MEMBER_PLACEHOLDER
        (MODIFIER_LIST (DEFAULT_KW "default"))
        (VOID_KW "void")
        (IDENT "d")
        (L_PAREN "(")
        (R_PAREN ")")
        (L_CURLY "{")
        (R_CURLY "}"))
      (MEMBER_PLACEHOLDER
        (MODIFIER_LIST (STATIC_KW "static"))
        (VOID_KW "void")
        (IDENT "s")
        (L_PAREN "(")
        (R_PAREN ")")
        (L_CURLY "{")
        (R_CURLY "}"))
      (R_CURLY "}")))
  (ANNOTATION_TYPE_DECL
    (AT "@")
    (INTERFACE_KW "interface")
    (NAME (IDENT "A"))
    (CLASS_BODY
      (L_CURLY "{")
      (MEMBER_PLACEHOLDER
        (INT_KW "int")
        (IDENT "value")
        (L_PAREN "(")
        (R_PAREN ")")
        (DEFAULT_KW "default")
        (INT_NUMBER "5")
        (SEMICOLON ";"))
      (R_CURLY "}"))))
