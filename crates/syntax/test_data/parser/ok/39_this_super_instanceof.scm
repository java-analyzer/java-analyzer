(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "A"))
    (EXTENDS_CLAUSE
      (EXTENDS_KW "extends")
      (PATH_TYPE (NAME_REF (IDENT "B"))))
    (CLASS_BODY
      (L_CURLY "{")
      (CONSTRUCTOR_DECL
        (NAME (IDENT "A"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (EXPR_STMT
            (CALL_EXPR
              (THIS_EXPR (THIS_KW "this"))
              (ARG_LIST
                (L_PAREN "(")
                (LITERAL (INT_NUMBER "1"))
                (R_PAREN ")")))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (CONSTRUCTOR_DECL
        (NAME (IDENT "A"))
        (PARAM_LIST
          (L_PAREN "(")
          (PARAM
            (PRIMITIVE_TYPE (INT_KW "int"))
            (NAME (IDENT "x")))
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (EXPR_STMT
            (CALL_EXPR
              (SUPER_EXPR (SUPER_KW "super"))
              (ARG_LIST
                (L_PAREN "(")
                (NAME_REF_EXPR (NAME_REF (IDENT "x")))
                (R_PAREN ")")))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (CONSTRUCTOR_DECL
        (NAME (IDENT "A"))
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
          (EXPR_STMT
            (ASSIGN_EXPR
              (FIELD_ACCESS_EXPR
                (THIS_EXPR (THIS_KW "this"))
                (DOT ".")
                (NAME_REF (IDENT "x")))
              (EQ "=")
              (NAME_REF_EXPR (NAME_REF (IDENT "x"))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (FIELD_ACCESS_EXPR
                (SUPER_EXPR (SUPER_KW "super"))
                (DOT ".")
                (NAME_REF (IDENT "y")))
              (EQ "=")
              (NAME_REF_EXPR (NAME_REF (IDENT "y"))))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "f"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (EXPR_STMT
            (METHOD_CALL_EXPR
              (THIS_EXPR (THIS_KW "this"))
              (DOT ".")
              (NAME_REF (IDENT "g"))
              (ARG_LIST
                (L_PAREN "(")
                (R_PAREN ")")))
            (SEMICOLON ";"))
          (EXPR_STMT
            (METHOD_CALL_EXPR
              (SUPER_EXPR (SUPER_KW "super"))
              (DOT ".")
              (NAME_REF (IDENT "g"))
              (ARG_LIST
                (L_PAREN "(")
                (R_PAREN ")")))
            (SEMICOLON ";"))
          (EXPR_STMT
            (METHOD_CALL_EXPR
              (THIS_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "Outer")))
                (DOT ".")
                (THIS_KW "this"))
              (DOT ".")
              (NAME_REF (IDENT "g"))
              (ARG_LIST
                (L_PAREN "(")
                (R_PAREN ")")))
            (SEMICOLON ";"))
          (EXPR_STMT
            (METHOD_CALL_EXPR
              (SUPER_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "Outer")))
                (DOT ".")
                (SUPER_KW "super"))
              (DOT ".")
              (NAME_REF (IDENT "g"))
              (ARG_LIST
                (L_PAREN "(")
                (R_PAREN ")")))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PATH_TYPE (NAME_REF (IDENT "Object")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "o"))
              (EQ "=")
              (THIS_EXPR (THIS_KW "this")))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PRIMITIVE_TYPE (BOOLEAN_KW "boolean"))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "b1"))
              (EQ "=")
              (INSTANCEOF_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "x")))
                (INSTANCEOF_KW "instanceof")
                (PATH_TYPE (NAME_REF (IDENT "Foo")))))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PRIMITIVE_TYPE (BOOLEAN_KW "boolean"))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "b2"))
              (EQ "=")
              (INSTANCEOF_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "x")))
                (INSTANCEOF_KW "instanceof")
                (TYPE_PATTERN
                  (PATH_TYPE (NAME_REF (IDENT "Foo")))
                  (NAME (IDENT "f")))))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PRIMITIVE_TYPE (BOOLEAN_KW "boolean"))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "b3"))
              (EQ "=")
              (BINARY_EXPR
                (INSTANCEOF_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "x")))
                  (INSTANCEOF_KW "instanceof")
                  (TYPE_PATTERN
                    (PATH_TYPE (NAME_REF (IDENT "Foo")))
                    (NAME (IDENT "f"))))
                (AMP2 "&&")
                (METHOD_CALL_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "f")))
                  (DOT ".")
                  (NAME_REF (IDENT "ok"))
                  (ARG_LIST
                    (L_PAREN "(")
                    (R_PAREN ")")))))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PRIMITIVE_TYPE (BOOLEAN_KW "boolean"))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "b4"))
              (EQ "=")
              (INSTANCEOF_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "x")))
                (INSTANCEOF_KW "instanceof")
                (TYPE_PATTERN
                  (MODIFIER_LIST (FINAL_KW "final"))
                  (PATH_TYPE (NAME_REF (IDENT "Foo")))
                  (NAME (IDENT "f")))))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PRIMITIVE_TYPE (BOOLEAN_KW "boolean"))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "b5"))
              (EQ "=")
              (INSTANCEOF_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "x")))
                (INSTANCEOF_KW "instanceof")
                (RECORD_PATTERN
                  (PATH_TYPE (NAME_REF (IDENT "Point")))
                  (L_PAREN "(")
                  (TYPE_PATTERN
                    (PRIMITIVE_TYPE (INT_KW "int"))
                    (NAME (IDENT "a")))
                  (COMMA ",")
                  (TYPE_PATTERN
                    (PRIMITIVE_TYPE (INT_KW "int"))
                    (NAME (IDENT "b")))
                  (R_PAREN ")"))))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PRIMITIVE_TYPE (BOOLEAN_KW "boolean"))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "b6"))
              (EQ "=")
              (BINARY_EXPR
                (INSTANCEOF_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "x")))
                  (INSTANCEOF_KW "instanceof")
                  (RECORD_PATTERN
                    (PATH_TYPE (NAME_REF (IDENT "Point")))
                    (L_PAREN "(")
                    (TYPE_PATTERN
                      (PATH_TYPE (NAME_REF (IDENT "var")))
                      (NAME (IDENT "a")))
                    (COMMA ",")
                    (RECORD_PATTERN
                      (PATH_TYPE (NAME_REF (IDENT "Point")))
                      (L_PAREN "(")
                      (TYPE_PATTERN
                        (PRIMITIVE_TYPE (INT_KW "int"))
                        (NAME (IDENT "c")))
                      (COMMA ",")
                      (TYPE_PATTERN
                        (PRIMITIVE_TYPE (INT_KW "int"))
                        (NAME (IDENT "d")))
                      (R_PAREN ")"))
                    (R_PAREN ")")))
                (AMP2 "&&")
                (BINARY_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "a")))
                  (R_ANGLE ">")
                  (NAME_REF_EXPR (NAME_REF (IDENT "c"))))))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PRIMITIVE_TYPE (BOOLEAN_KW "boolean"))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "b7"))
              (EQ "=")
              (UNARY_EXPR
                (BANG "!")
                (PAREN_EXPR
                  (L_PAREN "(")
                  (INSTANCEOF_EXPR
                    (NAME_REF_EXPR (NAME_REF (IDENT "x")))
                    (INSTANCEOF_KW "instanceof")
                    (PATH_TYPE (NAME_REF (IDENT "Foo"))))
                  (R_PAREN ")"))))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PRIMITIVE_TYPE (BOOLEAN_KW "boolean"))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "b8"))
              (EQ "=")
              (INSTANCEOF_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "x")))
                (INSTANCEOF_KW "instanceof")
                (TYPE_PATTERN
                  (PATH_TYPE
                    (NAME_REF (IDENT "List"))
                    (TYPE_ARGS
                      (L_ANGLE "<")
                      (WILDCARD_TYPE (QUESTION "?"))
                      (R_ANGLE ">")))
                  (NAME (IDENT "l")))))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PRIMITIVE_TYPE (BOOLEAN_KW "boolean"))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "b9"))
              (EQ "=")
              (TERNARY_EXPR
                (INSTANCEOF_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "x")))
                  (INSTANCEOF_KW "instanceof")
                  (PATH_TYPE (NAME_REF (IDENT "Foo"))))
                (QUESTION "?")
                (LITERAL (TRUE_KW "true"))
                (COLON ":")
                (LITERAL (FALSE_KW "false"))))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (R_CURLY "}"))))
