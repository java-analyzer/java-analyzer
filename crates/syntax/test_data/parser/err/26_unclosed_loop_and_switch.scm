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
          (WHILE_STMT
            (WHILE_KW "while")
            (L_PAREN "(")
            (EXPR_PLACEHOLDER (IDENT "x"))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (SWITCH_STMT
                (SWITCH_KW "switch")
                (L_PAREN "(")
                (EXPR_PLACEHOLDER (IDENT "y"))
                (R_PAREN ")")
                (SWITCH_BODY
                  (L_CURLY "{")
                  (SWITCH_CASE
                    (SWITCH_LABEL
                      (CASE_KW "case")
                      (EXPR_PLACEHOLDER (INT_NUMBER "1"))
                      (COLON ":"))
                    (EXPR_STMT
                      (EXPR_PLACEHOLDER
                        (IDENT "foo")
                        (L_PAREN "(")
                        (R_PAREN ")"))
                      (SEMICOLON ";")))))))))
      (FIELD_DECL
        (MODIFIER_LIST (PRIVATE_KW "private"))
        (PRIMITIVE_TYPE (INT_KW "int"))
        (VARIABLE_DECLARATOR (NAME (IDENT "b")))
        (SEMICOLON ";"))
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
  (error "expected '}'" "6:27..6:27")
  (error "expected '}'" "6:27..6:27")
  (error "expected '}'" "6:27..6:27"))
