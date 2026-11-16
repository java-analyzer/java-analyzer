(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "A"))
    (CLASS_BODY
      (L_CURLY "{")
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "a"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (IDENT "foo")
          (L_PAREN "(")))
      (METHOD_DECL
        (MODIFIER_LIST (PUBLIC_KW "public"))
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "b"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (R_CURLY "}")))
      (FIELD_DECL
        (MODIFIER_LIST (PRIVATE_KW "private"))
        (PRIMITIVE_TYPE (INT_KW "int"))
        (VARIABLE_DECLARATOR (NAME (IDENT "c")))
        (SEMICOLON ";"))
      (R_CURLY "}"))))
(errors (error "expected '}'" "3:13..3:13"))
