(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "A"))
    (CLASS_BODY
      (L_CURLY "{")
      (METHOD_DECL
        (PRIMITIVE_TYPE (INT_KW "int"))
        (NAME (IDENT "f"))
        (PARAM_LIST
          (L_PAREN "(")
          (PARAM
            (PATH_TYPE (NAME_REF (IDENT "Object")))
            (NAME (IDENT "o")))
          (COMMA ",")
          (PARAM
            (PRIMITIVE_TYPE (INT_KW "int"))
            (NAME (IDENT "x")))
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (LOCAL_VAR_DECL
            (PRIMITIVE_TYPE (INT_KW "int"))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "a"))
              (EQ "=")
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
                      (LITERAL (INT_NUMBER "10"))
                      (SEMICOLON ";")))
                  (SWITCH_CASE
                    (SWITCH_LABEL
                      (CASE_KW "case")
                      (LITERAL (INT_NUMBER "2"))
                      (COMMA ",")
                      (LITERAL (INT_NUMBER "3"))
                      (ARROW "->"))
                    (EXPR_STMT
                      (LITERAL (INT_NUMBER "20"))
                      (SEMICOLON ";")))
                  (SWITCH_CASE
                    (SWITCH_LABEL
                      (CASE_KW "case")
                      (LITERAL (INT_NUMBER "4"))
                      (ARROW "->"))
                    (BLOCK
                      (L_CURLY "{")
                      (YIELD_STMT
                        (IDENT "yield")
                        (LITERAL (INT_NUMBER "30"))
                        (SEMICOLON ";"))
                      (R_CURLY "}")))
                  (SWITCH_CASE
                    (SWITCH_LABEL
                      (DEFAULT_KW "default")
                      (ARROW "->"))
                    (THROW_STMT
                      (THROW_KW "throw")
                      (NEW_EXPR
                        (NEW_KW "new")
                        (PATH_TYPE (NAME_REF (IDENT "IllegalStateException")))
                        (ARG_LIST
                          (L_PAREN "(")
                          (R_PAREN ")")))
                      (SEMICOLON ";")))
                  (R_CURLY "}"))))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PATH_TYPE (NAME_REF (IDENT "String")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "s"))
              (EQ "=")
              (SWITCH_EXPR
                (SWITCH_KW "switch")
                (L_PAREN "(")
                (NAME_REF_EXPR (NAME_REF (IDENT "o")))
                (R_PAREN ")")
                (SWITCH_BODY
                  (L_CURLY "{")
                  (SWITCH_CASE
                    (SWITCH_LABEL
                      (CASE_KW "case")
                      (LITERAL (NULL_KW "null"))
                      (ARROW "->"))
                    (EXPR_STMT
                      (LITERAL (STRING_LITERAL "\"null\""))
                      (SEMICOLON ";")))
                  (SWITCH_CASE
                    (SWITCH_LABEL
                      (CASE_KW "case")
                      (TYPE_PATTERN
                        (PATH_TYPE (NAME_REF (IDENT "Integer")))
                        (NAME (IDENT "i")))
                      (GUARD
                        (IDENT "when")
                        (BINARY_EXPR
                          (NAME_REF_EXPR (NAME_REF (IDENT "i")))
                          (R_ANGLE ">")
                          (LITERAL (INT_NUMBER "10"))))
                      (ARROW "->"))
                    (EXPR_STMT
                      (LITERAL (STRING_LITERAL "\"big\""))
                      (SEMICOLON ";")))
                  (SWITCH_CASE
                    (SWITCH_LABEL
                      (CASE_KW "case")
                      (TYPE_PATTERN
                        (PATH_TYPE (NAME_REF (IDENT "Integer")))
                        (NAME (IDENT "i")))
                      (ARROW "->"))
                    (EXPR_STMT
                      (LITERAL (STRING_LITERAL "\"int\""))
                      (SEMICOLON ";")))
                  (SWITCH_CASE
                    (SWITCH_LABEL
                      (CASE_KW "case")
                      (TYPE_PATTERN
                        (PATH_TYPE (NAME_REF (IDENT "String")))
                        (NAME (IDENT "str")))
                      (ARROW "->"))
                    (EXPR_STMT
                      (NAME_REF_EXPR (NAME_REF (IDENT "str")))
                      (SEMICOLON ";")))
                  (SWITCH_CASE
                    (SWITCH_LABEL
                      (CASE_KW "case")
                      (RECORD_PATTERN
                        (PATH_TYPE (NAME_REF (IDENT "Point")))
                        (L_PAREN "(")
                        (TYPE_PATTERN
                          (PRIMITIVE_TYPE (INT_KW "int"))
                          (NAME (IDENT "px")))
                        (COMMA ",")
                        (TYPE_PATTERN
                          (PRIMITIVE_TYPE (INT_KW "int"))
                          (NAME (IDENT "py")))
                        (R_PAREN ")"))
                      (ARROW "->"))
                    (EXPR_STMT
                      (BINARY_EXPR
                        (LITERAL (STRING_LITERAL "\"point \""))
                        (PLUS "+")
                        (NAME_REF_EXPR (NAME_REF (IDENT "px"))))
                      (SEMICOLON ";")))
                  (SWITCH_CASE
                    (SWITCH_LABEL
                      (CASE_KW "case")
                      (TYPE_PATTERN
                        (PATH_TYPE (NAME_REF (IDENT "Color")))
                        (NAME (IDENT "c")))
                      (GUARD
                        (IDENT "when")
                        (BINARY_EXPR
                          (BINARY_EXPR
                            (NAME_REF_EXPR (NAME_REF (IDENT "c")))
                            (EQ2 "==")
                            (NAME_REF_EXPR (NAME_REF (IDENT "RED"))))
                          (PIPE2 "||")
                          (BINARY_EXPR
                            (NAME_REF_EXPR (NAME_REF (IDENT "c")))
                            (EQ2 "==")
                            (NAME_REF_EXPR (NAME_REF (IDENT "BLUE"))))))
                      (ARROW "->"))
                    (EXPR_STMT
                      (LITERAL (STRING_LITERAL "\"color\""))
                      (SEMICOLON ";")))
                  (SWITCH_CASE
                    (SWITCH_LABEL
                      (DEFAULT_KW "default")
                      (ARROW "->"))
                    (EXPR_STMT
                      (LITERAL (STRING_LITERAL "\"other\""))
                      (SEMICOLON ";")))
                  (R_CURLY "}"))))
            (SEMICOLON ";"))
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
                  (NAME_REF_EXPR (NAME_REF (IDENT "FOO")))
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
                  (FIELD_ACCESS_EXPR
                    (NAME_REF_EXPR (NAME_REF (IDENT "Outer")))
                    (DOT ".")
                    (NAME_REF (IDENT "BAR")))
                  (ARROW "->"))
                (EXPR_STMT
                  (CALL_EXPR
                    (NAME_REF_EXPR (NAME_REF (IDENT "b")))
                    (ARG_LIST
                      (L_PAREN "(")
                      (R_PAREN ")")))
                  (SEMICOLON ";")))
              (SWITCH_CASE
                (SWITCH_LABEL
                  (CASE_KW "case")
                  (LITERAL (NULL_KW "null"))
                  (COMMA ",")
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
                  (ASSIGN_EXPR
                    (NAME_REF_EXPR (NAME_REF (IDENT "y")))
                    (EQ "=")
                    (TERNARY_EXPR
                      (BINARY_EXPR
                        (NAME_REF_EXPR (NAME_REF (IDENT "x")))
                        (R_ANGLE ">")
                        (LITERAL (INT_NUMBER "0")))
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
                  (CASE_KW "case")
                  (NAME_REF_EXPR (NAME_REF (IDENT "A")))
                  (COMMA ",")
                  (NAME_REF_EXPR (NAME_REF (IDENT "B")))
                  (COLON ":"))
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
          (RETURN_STMT
            (RETURN_KW "return")
            (BINARY_EXPR
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
                      (LITERAL (INT_NUMBER "1"))
                      (SEMICOLON ";")))
                  (SWITCH_CASE
                    (SWITCH_LABEL
                      (DEFAULT_KW "default")
                      (ARROW "->"))
                    (EXPR_STMT
                      (LITERAL (INT_NUMBER "0"))
                      (SEMICOLON ";")))
                  (R_CURLY "}")))
              (PLUS "+")
              (LITERAL (INT_NUMBER "1")))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (R_CURLY "}"))))
