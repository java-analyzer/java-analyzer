(SOURCE_FILE
  (INTERFACE_DECL
    (INTERFACE_KW "interface")
    (NAME (IDENT "I"))
    (CLASS_BODY
      (L_CURLY "{")
      (FIELD_DECL
        (PRIMITIVE_TYPE (INT_KW "int"))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "CONSTANT"))
          (EQ "=")
          (EXPR_PLACEHOLDER (INT_NUMBER "1")))
        (SEMICOLON ";"))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "m"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (SEMICOLON ";"))
      (METHOD_DECL
        (MODIFIER_LIST (DEFAULT_KW "default"))
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "d"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (R_CURLY "}")))
      (METHOD_DECL
        (MODIFIER_LIST (STATIC_KW "static"))
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "s"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (R_CURLY "}")))
      (R_CURLY "}")))
  (ANNOTATION_TYPE_DECL
    (AT "@")
    (INTERFACE_KW "interface")
    (NAME (IDENT "A"))
    (CLASS_BODY
      (L_CURLY "{")
      (METHOD_DECL
        (PRIMITIVE_TYPE (INT_KW "int"))
        (NAME (IDENT "value"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (DEFAULT_VALUE
          (DEFAULT_KW "default")
          (EXPR_PLACEHOLDER (INT_NUMBER "5")))
        (SEMICOLON ";"))
      (R_CURLY "}"))))
