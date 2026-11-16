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
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (INSTANCEOF_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "o")))
                (INSTANCEOF_KW "instanceof")))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "b"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (INSTANCEOF_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "o")))
                (INSTANCEOF_KW "instanceof")
                (RECORD_PATTERN
                  (PATH_TYPE (NAME_REF (IDENT "Foo")))
                  (L_PAREN "(")
                  (TYPE_PATTERN
                    (PRIMITIVE_TYPE (INT_KW "int"))
                    (NAME (IDENT "a")))
                  (COMMA ","))))
            (SEMICOLON ";"))
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
            (NAME_REF_EXPR (NAME_REF (IDENT "o")))
            (R_PAREN ")")
            (SWITCH_BODY
              (L_CURLY "{")
              (SWITCH_CASE
                (SWITCH_LABEL
                  (CASE_KW "case")
                  (RECORD_PATTERN
                    (PATH_TYPE (NAME_REF (IDENT "Foo")))
                    (L_PAREN "(")
                    (TYPE_PATTERN
                      (PRIMITIVE_TYPE (INT_KW "int"))
                      (NAME (IDENT "a")))
                    (COMMA ",")
                    (R_PAREN ")"))
                  (ARROW "->"))
                (EXPR_STMT
                  (CALL_EXPR
                    (NAME_REF_EXPR (NAME_REF (IDENT "a")))
                    (ARG_LIST
                      (L_PAREN "(")
                      (R_PAREN ")")))
                  (SEMICOLON ";")))
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
            (NAME_REF_EXPR (NAME_REF (IDENT "o")))
            (R_PAREN ")")
            (SWITCH_BODY
              (L_CURLY "{")
              (SWITCH_CASE
                (SWITCH_LABEL
                  (CASE_KW "case")
                  (TYPE_PATTERN
                    (PATH_TYPE (NAME_REF (IDENT "Foo")))
                    (NAME (IDENT "f")))
                  (GUARD (IDENT "when"))
                  (ARROW "->"))
                (EXPR_STMT
                  (CALL_EXPR
                    (NAME_REF_EXPR (NAME_REF (IDENT "a")))
                    (ARG_LIST
                      (L_PAREN "(")
                      (R_PAREN ")")))
                  (SEMICOLON ";")))
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
          (SWITCH_STMT
            (SWITCH_KW "switch")
            (L_PAREN "(")
            (NAME_REF_EXPR (NAME_REF (IDENT "o")))
            (R_PAREN ")")
            (SWITCH_BODY
              (L_CURLY "{")
              (SWITCH_CASE
                (SWITCH_LABEL
                  (CASE_KW "case")
                  (ARROW "->"))
                (EXPR_STMT
                  (CALL_EXPR
                    (NAME_REF_EXPR (NAME_REF (IDENT "a")))
                    (ARG_LIST
                      (L_PAREN "(")
                      (R_PAREN ")")))
                  (SEMICOLON ";")))
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
  (error "expected a type" "2:32..2:32")
  (error "expected ')'" "3:43..3:43")
  (error "expected an expression" "5:44..5:44")
  (error "expected an expression" "6:33..6:33"))
