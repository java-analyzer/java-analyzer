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
          (SWITCH_STMT
            (SWITCH_KW "switch")
            (L_PAREN "(")
            (EXPR_PLACEHOLDER (IDENT "x"))
            (R_PAREN ")")
            (SWITCH_BODY
              (L_CURLY "{")
              (EXPR_STMT
                (EXPR_PLACEHOLDER
                  (IDENT "foo")
                  (L_PAREN "(")
                  (R_PAREN ")"))
                (SEMICOLON ";"))
              (SWITCH_CASE
                (SWITCH_LABEL
                  (CASE_KW "case")
                  (EXPR_PLACEHOLDER (INT_NUMBER "1"))
                  (COLON ":"))
                (EXPR_STMT
                  (EXPR_PLACEHOLDER
                    (IDENT "bar")
                    (L_PAREN "(")
                    (R_PAREN ")"))
                  (SEMICOLON ";")))
              (R_CURLY "}")))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "b"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (SWITCH_STMT
            (SWITCH_KW "switch")
            (L_PAREN "(")
            (EXPR_PLACEHOLDER (IDENT "x"))
            (R_PAREN ")")
            (SWITCH_BODY
              (L_CURLY "{")
              (SWITCH_CASE
                (SWITCH_LABEL
                  (CASE_KW "case")
                  (EXPR_PLACEHOLDER
                    (INT_NUMBER "1")
                    (IDENT "bar")
                    (L_PAREN "(")
                    (R_PAREN ")")))
                (EMPTY_STMT (SEMICOLON ";")))
              (R_CURLY "}")))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "c"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (SWITCH_STMT
            (SWITCH_KW "switch")
            (L_PAREN "(")
            (EXPR_PLACEHOLDER (IDENT "x"))
            (R_PAREN ")")
            (SWITCH_BODY
              (L_CURLY "{")
              (SWITCH_CASE
                (SWITCH_LABEL
                  (CASE_KW "case")
                  (EXPR_PLACEHOLDER (INT_NUMBER "1"))
                  (ARROW "->")))
              (R_CURLY "}")))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "d"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (SWITCH_STMT
            (SWITCH_KW "switch")
            (L_PAREN "(")
            (EXPR_PLACEHOLDER (IDENT "x"))
            (R_PAREN ")")
            (SWITCH_BODY
              (L_CURLY "{")
              (SWITCH_CASE
                (SWITCH_LABEL
                  (CASE_KW "case")
                  (COLON ":")))
              (R_CURLY "}")))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "e"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (SWITCH_STMT (SWITCH_KW "switch"))
          (EXPR_STMT
            (EXPR_PLACEHOLDER
              (IDENT "x")
              (L_CURLY "{")
              (R_CURLY "}")))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "ok"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (R_CURLY "}")))
      (R_CURLY "}"))))
(errors
  (error "expected 'case' or 'default'" "2:29..2:32")
  (error "expected ':' or '->'" "3:41..3:41")
  (error "expected a statement" "4:38..4:38")
  (error "expected an expression" "5:33..5:33")
  (error "expected '('" "6:22..6:22")
  (error "expected '{'" "6:22..6:22")
  (error "expected ';'" "6:28..6:28"))
