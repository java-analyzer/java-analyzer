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
          (EXPR_STMT
            (EXPR_PLACEHOLDER
              (IDENT "foo")
              (L_PAREN "(")
              (PUBLIC_KW "public")
              (VOID_KW "void")
              (IDENT "b")
              (L_PAREN "(")
              (R_PAREN ")")
              (L_CURLY "{")
              (R_CURLY "}")
              (PRIVATE_KW "private")
              (INT_KW "int")
              (IDENT "c"))
            (SEMICOLON ";"))
          (R_CURLY "}"))))))
(errors (error "expected '}'" "6:2..6:2"))
