(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "A"))
    (CLASS_BODY
      (L_CURLY "{")
      (INITIALIZER
        (MODIFIER_LIST (STATIC_KW "static"))
        (BLOCK
          (L_CURLY "{")
          (EXPR_STMT
            (CALL_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "init")))
              (ARG_LIST
                (L_PAREN "(")
                (R_PAREN ")")))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (INITIALIZER
        (BLOCK
          (L_CURLY "{")
          (EXPR_STMT
            (CALL_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "instance")))
              (ARG_LIST
                (L_PAREN "(")
                (R_PAREN ")")))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (R_CURLY "}"))))
