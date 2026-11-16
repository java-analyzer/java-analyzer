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
            (EXPR_PLACEHOLDER (IDENT "x"))
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
                    (IDENT "a")
                    (L_PAREN "(")
                    (R_PAREN ")"))
                  (SEMICOLON ";"))
                (BREAK_STMT
                  (BREAK_KW "break")
                  (SEMICOLON ";")))
              (SWITCH_CASE
                (SWITCH_LABEL
                  (CASE_KW "case")
                  (EXPR_PLACEHOLDER (INT_NUMBER "2"))
                  (COLON ":")))
              (SWITCH_CASE
                (SWITCH_LABEL
                  (CASE_KW "case")
                  (EXPR_PLACEHOLDER (INT_NUMBER "3"))
                  (COLON ":"))
                (BLOCK
                  (L_CURLY "{")
                  (EXPR_STMT
                    (EXPR_PLACEHOLDER
                      (IDENT "b")
                      (L_PAREN "(")
                      (R_PAREN ")"))
                    (SEMICOLON ";"))
                  (R_CURLY "}")))
              (SWITCH_CASE
                (SWITCH_LABEL
                  (DEFAULT_KW "default")
                  (COLON ":"))
                (EXPR_STMT
                  (EXPR_PLACEHOLDER
                    (IDENT "c")
                    (L_PAREN "(")
                    (R_PAREN ")"))
                  (SEMICOLON ";")))
              (R_CURLY "}")))
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
                    (COMMA ",")
                    (INT_NUMBER "2"))
                  (ARROW "->"))
                (EXPR_STMT
                  (EXPR_PLACEHOLDER
                    (IDENT "a")
                    (L_PAREN "(")
                    (R_PAREN ")"))
                  (SEMICOLON ";")))
              (SWITCH_CASE
                (SWITCH_LABEL
                  (CASE_KW "case")
                  (EXPR_PLACEHOLDER (INT_NUMBER "3"))
                  (ARROW "->"))
                (BLOCK
                  (L_CURLY "{")
                  (EXPR_STMT
                    (EXPR_PLACEHOLDER
                      (IDENT "b")
                      (L_PAREN "(")
                      (R_PAREN ")"))
                    (SEMICOLON ";"))
                  (R_CURLY "}")))
              (SWITCH_CASE
                (SWITCH_LABEL
                  (CASE_KW "case")
                  (EXPR_PLACEHOLDER (INT_NUMBER "4"))
                  (ARROW "->"))
                (THROW_STMT
                  (THROW_KW "throw")
                  (EXPR_PLACEHOLDER
                    (NEW_KW "new")
                    (IDENT "X")
                    (L_PAREN "(")
                    (R_PAREN ")"))
                  (SEMICOLON ";")))
              (SWITCH_CASE
                (SWITCH_LABEL
                  (DEFAULT_KW "default")
                  (ARROW "->"))
                (EXPR_STMT
                  (EXPR_PLACEHOLDER
                    (IDENT "c")
                    (L_PAREN "(")
                    (R_PAREN ")"))
                  (SEMICOLON ";")))
              (R_CURLY "}")))
          (SWITCH_STMT
            (SWITCH_KW "switch")
            (L_PAREN "(")
            (EXPR_PLACEHOLDER (IDENT "s"))
            (R_PAREN ")")
            (SWITCH_BODY
              (L_CURLY "{")
              (SWITCH_CASE
                (SWITCH_LABEL
                  (CASE_KW "case")
                  (EXPR_PLACEHOLDER (STRING_LITERAL "\"a\""))
                  (COLON ":"))
                (RETURN_STMT
                  (RETURN_KW "return")
                  (SEMICOLON ";")))
              (SWITCH_CASE
                (SWITCH_LABEL
                  (CASE_KW "case")
                  (EXPR_PLACEHOLDER
                    (NULL_KW "null")
                    (COMMA ",")
                    (DEFAULT_KW "default"))
                  (COLON ":"))
                (BREAK_STMT
                  (BREAK_KW "break")
                  (SEMICOLON ";")))
              (R_CURLY "}")))
          (SWITCH_STMT
            (SWITCH_KW "switch")
            (L_PAREN "(")
            (EXPR_PLACEHOLDER (IDENT "o"))
            (R_PAREN ")")
            (SWITCH_BODY
              (L_CURLY "{")
              (SWITCH_CASE
                (SWITCH_LABEL
                  (CASE_KW "case")
                  (EXPR_PLACEHOLDER
                    (IDENT "Foo")
                    (IDENT "f")
                    (IDENT "when")
                    (IDENT "f")
                    (DOT ".")
                    (IDENT "x")
                    (R_ANGLE ">")
                    (INT_NUMBER "0"))
                  (ARROW "->"))
                (EXPR_STMT
                  (EXPR_PLACEHOLDER
                    (IDENT "use")
                    (L_PAREN "(")
                    (IDENT "f")
                    (R_PAREN ")"))
                  (SEMICOLON ";")))
              (SWITCH_CASE
                (SWITCH_LABEL
                  (CASE_KW "case")
                  (EXPR_PLACEHOLDER
                    (IDENT "Bar")
                    (L_PAREN "(")
                    (INT_KW "int")
                    (IDENT "a")
                    (COMMA ",")
                    (INT_KW "int")
                    (IDENT "b")
                    (R_PAREN ")"))
                  (ARROW "->"))
                (EXPR_STMT
                  (EXPR_PLACEHOLDER
                    (IDENT "use")
                    (L_PAREN "(")
                    (IDENT "a")
                    (COMMA ",")
                    (IDENT "b")
                    (R_PAREN ")"))
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
            (EXPR_PLACEHOLDER (IDENT "x"))
            (R_PAREN ")")
            (SWITCH_BODY
              (L_CURLY "{")
              (R_CURLY "}")))
          (SWITCH_STMT
            (SWITCH_KW "switch")
            (L_PAREN "(")
            (EXPR_PLACEHOLDER (IDENT "c"))
            (R_PAREN ")")
            (SWITCH_BODY
              (L_CURLY "{")
              (SWITCH_CASE
                (SWITCH_LABEL
                  (CASE_KW "case")
                  (EXPR_PLACEHOLDER (CHAR_LITERAL "'a'"))
                  (COLON ":"))
                (EXPR_STMT
                  (EXPR_PLACEHOLDER
                    (IDENT "x")
                    (EQ "=")
                    (IDENT "flag")
                    (QUESTION "?")
                    (INT_NUMBER "1")
                    (COLON ":")
                    (INT_NUMBER "2"))
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
