(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "A"))
    (CLASS_BODY
      (L_CURLY "{")
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "f"))
        (PARAM_LIST
          (L_PAREN "(")
          (PARAM
            (PRIMITIVE_TYPE (INT_KW "int"))
            (NAME (IDENT "x")))
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
                  (COLON ":"))
                (EXPR_STMT
                  (CALL_EXPR
                    (NAME_REF_EXPR (NAME_REF (IDENT "a")))
                    (ARG_LIST
                      (L_PAREN "(")
                      (R_PAREN ")")))
                  (SEMICOLON ";"))
                (BREAK_STMT
                  (BREAK_KW "break")
                  (SEMICOLON ";")))
              (SWITCH_CASE
                (SWITCH_LABEL
                  (CASE_KW "case")
                  (LITERAL (INT_NUMBER "2"))
                  (COLON ":")))
              (SWITCH_CASE
                (SWITCH_LABEL
                  (CASE_KW "case")
                  (LITERAL (INT_NUMBER "3"))
                  (COLON ":"))
                (BLOCK
                  (L_CURLY "{")
                  (EXPR_STMT
                    (CALL_EXPR
                      (NAME_REF_EXPR (NAME_REF (IDENT "b")))
                      (ARG_LIST
                        (L_PAREN "(")
                        (R_PAREN ")")))
                    (SEMICOLON ";"))
                  (R_CURLY "}")))
              (SWITCH_CASE
                (SWITCH_LABEL
                  (DEFAULT_KW "default")
                  (COLON ":"))
                (EXPR_STMT
                  (CALL_EXPR
                    (NAME_REF_EXPR (NAME_REF (IDENT "c")))
                    (ARG_LIST
                      (L_PAREN "(")
                      (R_PAREN ")")))
                  (SEMICOLON ";")))
              (R_CURLY "}")))
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
                  (COMMA ",")
                  (LITERAL (INT_NUMBER "2"))
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
                  (CASE_KW "case")
                  (LITERAL (INT_NUMBER "3"))
                  (ARROW "->"))
                (BLOCK
                  (L_CURLY "{")
                  (EXPR_STMT
                    (CALL_EXPR
                      (NAME_REF_EXPR (NAME_REF (IDENT "b")))
                      (ARG_LIST
                        (L_PAREN "(")
                        (R_PAREN ")")))
                    (SEMICOLON ";"))
                  (R_CURLY "}")))
              (SWITCH_CASE
                (SWITCH_LABEL
                  (CASE_KW "case")
                  (LITERAL (INT_NUMBER "4"))
                  (ARROW "->"))
                (THROW_STMT
                  (THROW_KW "throw")
                  (NEW_EXPR
                    (NEW_KW "new")
                    (PATH_TYPE (NAME_REF (IDENT "X")))
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
                    (NAME_REF_EXPR (NAME_REF (IDENT "c")))
                    (ARG_LIST
                      (L_PAREN "(")
                      (R_PAREN ")")))
                  (SEMICOLON ";")))
              (R_CURLY "}")))
          (SWITCH_STMT
            (SWITCH_KW "switch")
            (L_PAREN "(")
            (NAME_REF_EXPR (NAME_REF (IDENT "s")))
            (R_PAREN ")")
            (SWITCH_BODY
              (L_CURLY "{")
              (SWITCH_CASE
                (SWITCH_LABEL
                  (CASE_KW "case")
                  (LITERAL (STRING_LITERAL "\"a\""))
                  (COLON ":"))
                (RETURN_STMT
                  (RETURN_KW "return")
                  (SEMICOLON ";")))
              (SWITCH_CASE
                (SWITCH_LABEL
                  (CASE_KW "case")
                  (LITERAL (NULL_KW "null"))
                  (COMMA ",")
                  (DEFAULT_KW "default")
                  (COLON ":"))
                (BREAK_STMT
                  (BREAK_KW "break")
                  (SEMICOLON ";")))
              (R_CURLY "}")))
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
                  (GUARD
                    (IDENT "when")
                    (BINARY_EXPR
                      (FIELD_ACCESS_EXPR
                        (NAME_REF_EXPR (NAME_REF (IDENT "f")))
                        (DOT ".")
                        (NAME_REF (IDENT "x")))
                      (R_ANGLE ">")
                      (LITERAL (INT_NUMBER "0"))))
                  (ARROW "->"))
                (EXPR_STMT
                  (CALL_EXPR
                    (NAME_REF_EXPR (NAME_REF (IDENT "use")))
                    (ARG_LIST
                      (L_PAREN "(")
                      (NAME_REF_EXPR (NAME_REF (IDENT "f")))
                      (R_PAREN ")")))
                  (SEMICOLON ";")))
              (SWITCH_CASE
                (SWITCH_LABEL
                  (CASE_KW "case")
                  (RECORD_PATTERN
                    (PATH_TYPE (NAME_REF (IDENT "Bar")))
                    (L_PAREN "(")
                    (TYPE_PATTERN
                      (PRIMITIVE_TYPE (INT_KW "int"))
                      (NAME (IDENT "a")))
                    (COMMA ",")
                    (TYPE_PATTERN
                      (PRIMITIVE_TYPE (INT_KW "int"))
                      (NAME (IDENT "b")))
                    (R_PAREN ")"))
                  (ARROW "->"))
                (EXPR_STMT
                  (CALL_EXPR
                    (NAME_REF_EXPR (NAME_REF (IDENT "use")))
                    (ARG_LIST
                      (L_PAREN "(")
                      (NAME_REF_EXPR (NAME_REF (IDENT "a")))
                      (COMMA ",")
                      (NAME_REF_EXPR (NAME_REF (IDENT "b")))
                      (R_PAREN ")")))
                  (SEMICOLON ";")))
              (SWITCH_CASE
                (SWITCH_LABEL
                  (DEFAULT_KW "default")
                  (ARROW "->"))
                (BLOCK
                  (L_CURLY "{")
                  (R_CURLY "}")))
              (R_CURLY "}")))
          (SWITCH_STMT
            (SWITCH_KW "switch")
            (L_PAREN "(")
            (NAME_REF_EXPR (NAME_REF (IDENT "x")))
            (R_PAREN ")")
            (SWITCH_BODY
              (L_CURLY "{")
              (R_CURLY "}")))
          (SWITCH_STMT
            (SWITCH_KW "switch")
            (L_PAREN "(")
            (NAME_REF_EXPR (NAME_REF (IDENT "c")))
            (R_PAREN ")")
            (SWITCH_BODY
              (L_CURLY "{")
              (SWITCH_CASE
                (SWITCH_LABEL
                  (CASE_KW "case")
                  (LITERAL (CHAR_LITERAL "'a'"))
                  (COLON ":"))
                (EXPR_STMT
                  (ASSIGN_EXPR
                    (NAME_REF_EXPR (NAME_REF (IDENT "x")))
                    (EQ "=")
                    (TERNARY_EXPR
                      (NAME_REF_EXPR (NAME_REF (IDENT "flag")))
                      (QUESTION "?")
                      (LITERAL (INT_NUMBER "1"))
                      (COLON ":")
                      (LITERAL (INT_NUMBER "2"))))
                  (SEMICOLON ";"))
                (BREAK_STMT
                  (BREAK_KW "break")
                  (SEMICOLON ";")))
              (SWITCH_CASE
                (SWITCH_LABEL
                  (DEFAULT_KW "default")
                  (COLON ":"))
                (BREAK_STMT
                  (BREAK_KW "break")
                  (SEMICOLON ";")))
              (R_CURLY "}")))
          (R_CURLY "}")))
      (R_CURLY "}"))))
