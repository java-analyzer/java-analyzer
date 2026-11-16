(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "A"))
    (CLASS_BODY
      (L_CURLY "{")
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "Runnable")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "r"))
          (EQ "=")
          (EXPR_PLACEHOLDER
            (NEW_KW "new")
            (IDENT "Runnable")
            (L_PAREN "(")
            (R_PAREN ")")
            (L_CURLY "{")
            (PUBLIC_KW "public")
            (VOID_KW "void")
            (IDENT "run")
            (L_PAREN "(")
            (R_PAREN ")")
            (L_CURLY "{")
            (R_CURLY "}")
            (R_CURLY "}")))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "Runnable")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "s"))
          (EQ "=")
          (EXPR_PLACEHOLDER
            (L_PAREN "(")
            (R_PAREN ")")
            (ARROW "->")
            (L_CURLY "{")
            (IDENT "work")
            (L_PAREN "(")
            (R_PAREN ")")
            (SEMICOLON ";")
            (R_CURLY "}")))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PRIMITIVE_TYPE (INT_KW "int"))
        (VARIABLE_DECLARATOR (NAME (IDENT "after")))
        (SEMICOLON ";"))
      (R_CURLY "}"))))
