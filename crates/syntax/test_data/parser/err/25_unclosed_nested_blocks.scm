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
          (IF_STMT
            (IF_KW "if")
            (L_PAREN "(")
            (EXPR_PLACEHOLDER (IDENT "x"))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (EXPR_STMT
                (EXPR_PLACEHOLDER
                  (IDENT "foo")
                  (L_PAREN "(")
                  (R_PAREN ")"))
                (SEMICOLON ";"))))))
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
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "c"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (R_CURLY "}")))
      (R_CURLY "}"))))
(errors
  (error "expected '}'" "4:19..4:19")
  (error "expected '}'" "4:19..4:19"))
