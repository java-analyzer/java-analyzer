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
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (LOCAL_VAR_DECL
            (PATH_TYPE
              (NAME_REF (IDENT "List"))
              (TYPE_ARGS
                (L_ANGLE "<")
                (PATH_TYPE (NAME_REF (IDENT "String")))
                (R_ANGLE ">")))
            (VARIABLE_DECLARATOR (NAME (IDENT "declaration")))
            (SEMICOLON ";"))
          (EXPR_STMT
            (BINARY_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "a")))
              (L_ANGLE "<")
              (NAME_REF_EXPR (NAME_REF (IDENT "b"))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (METHOD_CALL_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "Collections")))
              (DOT ".")
              (TYPE_ARGS
                (L_ANGLE "<")
                (PATH_TYPE (NAME_REF (IDENT "String")))
                (R_ANGLE ">"))
              (NAME_REF (IDENT "emptyList"))
              (ARG_LIST
                (L_PAREN "(")
                (R_PAREN ")")))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (ARRAY_ACCESS_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "arr")))
                (L_BRACK "[")
                (NAME_REF_EXPR (NAME_REF (IDENT "i")))
                (R_BRACK "]"))
              (EQ "=")
              (LITERAL (INT_NUMBER "1")))
            (SEMICOLON ";"))
          (EXPR_STMT
            (POSTFIX_EXPR
              (ARRAY_ACCESS_EXPR
                (ARRAY_ACCESS_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "arr")))
                  (L_BRACK "[")
                  (NAME_REF_EXPR (NAME_REF (IDENT "i")))
                  (R_BRACK "]"))
                (L_BRACK "[")
                (NAME_REF_EXPR (NAME_REF (IDENT "j")))
                (R_BRACK "]"))
              (PLUS2 "++"))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (FIELD_ACCESS_EXPR
                (FIELD_ACCESS_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "foo")))
                  (DOT ".")
                  (NAME_REF (IDENT "bar")))
                (DOT ".")
                (NAME_REF (IDENT "baz")))
              (EQ "=")
              (LITERAL (INT_NUMBER "1")))
            (SEMICOLON ";"))
          (EXPR_STMT
            (METHOD_CALL_EXPR
              (FIELD_ACCESS_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "Foo")))
                (DOT ".")
                (NAME_REF (IDENT "Bar")))
              (DOT ".")
              (NAME_REF (IDENT "baz"))
              (ARG_LIST
                (L_PAREN "(")
                (R_PAREN ")")))
            (SEMICOLON ";"))
          (EXPR_STMT
            (METHOD_CALL_EXPR
              (CLASS_LITERAL
                (PRIMITIVE_TYPE (INT_KW "int"))
                (DOT ".")
                (CLASS_KW "class"))
              (DOT ".")
              (NAME_REF (IDENT "getName"))
              (ARG_LIST
                (L_PAREN "(")
                (R_PAREN ")")))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (TERNARY_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "flag")))
                (QUESTION "?")
                (NAME_REF_EXPR (NAME_REF (IDENT "a")))
                (COLON ":")
                (NAME_REF_EXPR (NAME_REF (IDENT "b")))))
            (SEMICOLON ";"))
          (LABELED_STMT
            (NAME (IDENT "label"))
            (COLON ":")
            (EXPR_STMT
              (ASSIGN_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "x")))
                (EQ "=")
                (TERNARY_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "cond")))
                  (QUESTION "?")
                  (LITERAL (INT_NUMBER "1"))
                  (COLON ":")
                  (LITERAL (INT_NUMBER "2"))))
              (SEMICOLON ";")))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "yield")))
              (EQ "=")
              (LITERAL (INT_NUMBER "3")))
            (SEMICOLON ";"))
          (EXPR_STMT
            (POSTFIX_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "yield")))
              (PLUS2 "++"))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "record")))
              (EQ "=")
              (LITERAL (INT_NUMBER "1")))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "var")))
              (EQ "=")
              (LITERAL (INT_NUMBER "2")))
            (SEMICOLON ";"))
          (EXPR_STMT
            (CAST_EXPR
              (L_PAREN "(")
              (PATH_TYPE (NAME_REF (IDENT "cast")))
              (R_PAREN ")")
              (NAME_REF_EXPR (NAME_REF (IDENT "y"))))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (ARRAY_TYPE
              (PATH_TYPE (NAME_REF (IDENT "String")))
              (L_BRACK "[")
              (R_BRACK "]"))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "parts"))
              (EQ "=")
              (METHOD_CALL_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "line")))
                (DOT ".")
                (NAME_REF (IDENT "split"))
                (ARG_LIST
                  (L_PAREN "(")
                  (LITERAL (STRING_LITERAL "\",\""))
                  (R_PAREN ")"))))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (ARRAY_TYPE
              (PATH_TYPE
                (NAME_REF (IDENT "Foo"))
                (TYPE_ARGS
                  (L_ANGLE "<")
                  (PATH_TYPE (NAME_REF (IDENT "Bar")))
                  (R_ANGLE ">")))
              (L_BRACK "[")
              (R_BRACK "]"))
            (VARIABLE_DECLARATOR (NAME (IDENT "generic")))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PATH_TYPE (NAME_REF (IDENT "Object")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "o"))
              (EQ "=")
              (TERNARY_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "cond")))
                (QUESTION "?")
                (NEW_EXPR
                  (NEW_KW "new")
                  (ARRAY_TYPE
                    (PRIMITIVE_TYPE (INT_KW "int"))
                    (L_BRACK "[")
                    (R_BRACK "]"))
                  (ARRAY_INIT
                    (L_CURLY "{")
                    (LITERAL (INT_NUMBER "1"))
                    (R_CURLY "}")))
                (COLON ":")
                (LITERAL (NULL_KW "null"))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "i")))
              (EQ "=")
              (TERNARY_EXPR
                (BINARY_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "a")))
                  (L_ANGLE "<")
                  (NAME_REF_EXPR (NAME_REF (IDENT "b"))))
                (QUESTION "?")
                (LITERAL (INT_NUMBER "1"))
                (COLON ":")
                (LITERAL (INT_NUMBER "2"))))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (INT_KW "int"))
        (NAME (IDENT "yielding"))
        (PARAM_LIST
          (L_PAREN "(")
          (PARAM
            (PRIMITIVE_TYPE (INT_KW "int"))
            (NAME (IDENT "x")))
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
                    (LITERAL (INT_NUMBER "10"))
                    (SEMICOLON ";")))
                (SWITCH_CASE
                  (SWITCH_LABEL
                    (DEFAULT_KW "default")
                    (ARROW "->"))
                  (BLOCK
                    (L_CURLY "{")
                    (YIELD_STMT
                      (IDENT "yield")
                      (LITERAL (INT_NUMBER "20"))
                      (SEMICOLON ";"))
                    (R_CURLY "}")))
                (R_CURLY "}")))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (R_CURLY "}"))))
