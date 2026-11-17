(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "SwitchArrowExpr"))
    (CLASS_BODY
      (L_CURLY "{")
      (METHOD_DECL
        (PRIMITIVE_TYPE (INT_KW "int"))
        (NAME (IDENT "f"))
        (PARAM_LIST
          (L_PAREN "(")
          (PARAM
            (PRIMITIVE_TYPE (INT_KW "int"))
            (NAME (IDENT "x")))
          (COMMA ",")
          (PARAM
            (PRIMITIVE_TYPE (INT_KW "int"))
            (NAME (IDENT "y")))
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (RETURN_STMT
            (RETURN_KW "return")
            (SWITCH_EXPR
              (SWITCH_KW "switch")
              (L_PAREN "(")
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (R_PAREN ")")
              (SWITCH_BODY
                (L_CURLY "{")
                (SWITCH_CASE
                  (SWITCH_LABEL
                    (CASE_KW "case")
                    (LITERAL (INT_NUMBER "1"))
                    (ARROW "->"))
                  (EXPR_STMT
                    (SWITCH_EXPR
                      (SWITCH_KW "switch")
                      (L_PAREN "(")
                      (NAME_REF_EXPR (NAME_REF (IDENT "y")))
                      (R_PAREN ")")
                      (SWITCH_BODY
                        (L_CURLY "{")
                        (SWITCH_CASE
                          (SWITCH_LABEL
                            (CASE_KW "case")
                            (LITERAL (INT_NUMBER "0"))
                            (ARROW "->"))
                          (EXPR_STMT
                            (LITERAL (INT_NUMBER "10"))
                            (SEMICOLON ";")))
                        (SWITCH_CASE
                          (SWITCH_LABEL
                            (DEFAULT_KW "default")
                            (ARROW "->"))
                          (EXPR_STMT
                            (LITERAL (INT_NUMBER "20"))
                            (SEMICOLON ";")))
                        (R_CURLY "}")))
                    (SEMICOLON ";")))
                (SWITCH_CASE
                  (SWITCH_LABEL
                    (CASE_KW "case")
                    (LITERAL (INT_NUMBER "2"))
                    (ARROW "->"))
                  (EXPR_STMT
                    (METHOD_CALL_EXPR
                      (SWITCH_EXPR
                        (SWITCH_KW "switch")
                        (L_PAREN "(")
                        (NAME_REF_EXPR (NAME_REF (IDENT "y")))
                        (R_PAREN ")")
                        (SWITCH_BODY
                          (L_CURLY "{")
                          (SWITCH_CASE
                            (SWITCH_LABEL
                              (DEFAULT_KW "default")
                              (ARROW "->"))
                            (EXPR_STMT
                              (LITERAL (STRING_LITERAL "\"ab\""))
                              (SEMICOLON ";")))
                          (R_CURLY "}")))
                      (DOT ".")
                      (NAME_REF (IDENT "length"))
                      (ARG_LIST
                        (L_PAREN "(")
                        (R_PAREN ")")))
                    (SEMICOLON ";")))
                (SWITCH_CASE
                  (SWITCH_LABEL
                    (DEFAULT_KW "default")
                    (ARROW "->"))
                  (EXPR_STMT
                    (LITERAL (INT_NUMBER "0"))
                    (SEMICOLON ";")))
                (R_CURLY "}")))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "s"))
        (PARAM_LIST
          (L_PAREN "(")
          (PARAM
            (PRIMITIVE_TYPE (INT_KW "int"))
            (NAME (IDENT "x")))
          (COMMA ",")
          (PARAM
            (PRIMITIVE_TYPE (INT_KW "int"))
            (NAME (IDENT "y")))
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (SWITCH_STMT
            (SWITCH_KW "switch")
            (L_PAREN "(")
            (NAME_REF_EXPR (NAME_REF (IDENT "x")))
            (R_PAREN ")")
            (SWITCH_BODY
              (L_CURLY "{")
              (SWITCH_CASE
                (SWITCH_LABEL
                  (CASE_KW "case")
                  (LITERAL (INT_NUMBER "1"))
                  (ARROW "->"))
                (EXPR_STMT
                  (SWITCH_EXPR
                    (SWITCH_KW "switch")
                    (L_PAREN "(")
                    (NAME_REF_EXPR (NAME_REF (IDENT "y")))
                    (R_PAREN ")")
                    (SWITCH_BODY
                      (L_CURLY "{")
                      (SWITCH_CASE
                        (SWITCH_LABEL
                          (CASE_KW "case")
                          (LITERAL (INT_NUMBER "0"))
                          (ARROW "->"))
                        (EXPR_STMT
                          (CALL_EXPR
                            (NAME_REF_EXPR (NAME_REF (IDENT "a")))
                            (ARG_LIST
                              (L_PAREN "(")
                              (R_PAREN ")")))
                          (SEMICOLON ";")))
                      (SWITCH_CASE
                        (SWITCH_LABEL
                          (DEFAULT_KW "default")
                          (ARROW "->"))
                        (EXPR_STMT
                          (CALL_EXPR
                            (NAME_REF_EXPR (NAME_REF (IDENT "b")))
                            (ARG_LIST
                              (L_PAREN "(")
                              (R_PAREN ")")))
                          (SEMICOLON ";")))
                      (R_CURLY "}")))
                  (SEMICOLON ";")))
              (SWITCH_CASE
                (SWITCH_LABEL
                  (DEFAULT_KW "default")
                  (ARROW "->"))
                (EXPR_STMT
                  (CALL_EXPR
                    (NAME_REF_EXPR (NAME_REF (IDENT "c")))
                    (ARG_LIST
                      (L_PAREN "(")
                      (R_PAREN ")")))
                  (SEMICOLON ";")))
              (R_CURLY "}")))
          (R_CURLY "}")))
      (R_CURLY "}"))))
