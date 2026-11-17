(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "Unnamed"))
    (CLASS_BODY
      (L_CURLY "{")
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "m"))
        (PARAM_LIST
          (L_PAREN "(")
          (PARAM
            (PATH_TYPE (NAME_REF (IDENT "Object")))
            (NAME (IDENT "o")))
          (COMMA ",")
          (PARAM
            (PATH_TYPE
              (NAME_REF (IDENT "List"))
              (TYPE_ARGS
                (L_ANGLE "<")
                (PATH_TYPE (NAME_REF (IDENT "String")))
                (R_ANGLE ">")))
            (NAME (IDENT "xs")))
          (R_PAREN ")"))
        (THROWS_CLAUSE
          (THROWS_KW "throws")
          (PATH_TYPE (NAME_REF (IDENT "Exception"))))
        (BLOCK
          (L_CURLY "{")
          (LOCAL_VAR_DECL
            (PATH_TYPE (NAME_REF (IDENT "var")))
            (VARIABLE_DECLARATOR
              (UNDERSCORE_KW "_")
              (EQ "=")
              (METHOD_CALL_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "o")))
                (DOT ".")
                (NAME_REF (IDENT "toString"))
                (ARG_LIST
                  (L_PAREN "(")
                  (R_PAREN ")"))))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PRIMITIVE_TYPE (INT_KW "int"))
            (VARIABLE_DECLARATOR
              (UNDERSCORE_KW "_")
              (EQ "=")
              (LITERAL (INT_NUMBER "1")))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (MODIFIER_LIST (FINAL_KW "final"))
            (PATH_TYPE (NAME_REF (IDENT "String")))
            (VARIABLE_DECLARATOR
              (UNDERSCORE_KW "_")
              (EQ "=")
              (LITERAL (STRING_LITERAL "\"x\"")))
            (COMMA ",")
            (VARIABLE_DECLARATOR
              (NAME (IDENT "kept"))
              (EQ "=")
              (LITERAL (STRING_LITERAL "\"y\"")))
            (SEMICOLON ";"))
          (FOR_STMT
            (FOR_KW "for")
            (L_PAREN "(")
            (LOCAL_VAR_DECL
              (PRIMITIVE_TYPE (INT_KW "int"))
              (VARIABLE_DECLARATOR
                (UNDERSCORE_KW "_")
                (EQ "=")
                (LITERAL (INT_NUMBER "0")))
              (COMMA ",")
              (VARIABLE_DECLARATOR
                (NAME (IDENT "i"))
                (EQ "=")
                (LITERAL (INT_NUMBER "0"))))
            (SEMICOLON ";")
            (BINARY_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "i")))
              (L_ANGLE "<")
              (LITERAL (INT_NUMBER "1")))
            (SEMICOLON ";")
            (POSTFIX_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "i")))
              (PLUS2 "++"))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}")))
          (FOREACH_STMT
            (FOR_KW "for")
            (L_PAREN "(")
            (PATH_TYPE (NAME_REF (IDENT "var")))
            (UNDERSCORE_KW "_")
            (COLON ":")
            (NAME_REF_EXPR (NAME_REF (IDENT "xs")))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}")))
          (FOREACH_STMT
            (FOR_KW "for")
            (L_PAREN "(")
            (MODIFIER_LIST
              (FINAL_KW "final")
              (ANNOTATION
                (AT "@")
                (PATH (NAME_REF (IDENT "A")))))
            (PATH_TYPE (NAME_REF (IDENT "String")))
            (UNDERSCORE_KW "_")
            (COLON ":")
            (NAME_REF_EXPR (NAME_REF (IDENT "xs")))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}")))
          (TRY_STMT
            (TRY_KW "try")
            (RESOURCE_LIST
              (L_PAREN "(")
              (RESOURCE
                (PATH_TYPE (NAME_REF (IDENT "var")))
                (UNDERSCORE_KW "_")
                (EQ "=")
                (CALL_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "open")))
                  (ARG_LIST
                    (L_PAREN "(")
                    (R_PAREN ")"))))
              (SEMICOLON ";")
              (RESOURCE
                (PATH_TYPE (NAME_REF (IDENT "AutoCloseable")))
                (UNDERSCORE_KW "_")
                (EQ "=")
                (CALL_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "open")))
                  (ARG_LIST
                    (L_PAREN "(")
                    (R_PAREN ")"))))
              (R_PAREN ")"))
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}"))
            (CATCH_CLAUSE
              (CATCH_KW "catch")
              (L_PAREN "(")
              (PATH_TYPE (NAME_REF (IDENT "IllegalStateException")))
              (PIPE "|")
              (PATH_TYPE (NAME_REF (IDENT "IllegalArgumentException")))
              (UNDERSCORE_KW "_")
              (R_PAREN ")")
              (BLOCK
                (L_CURLY "{")
                (R_CURLY "}"))))
          (LOCAL_VAR_DECL
            (PATH_TYPE (NAME_REF (IDENT "Fn1")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "a"))
              (EQ "=")
              (LAMBDA_EXPR
                (LAMBDA_PARAMS (PARAM (UNDERSCORE_KW "_")))
                (ARROW "->")
                (LITERAL (INT_NUMBER "1"))))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PATH_TYPE (NAME_REF (IDENT "Fn2")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "b"))
              (EQ "=")
              (LAMBDA_EXPR
                (LAMBDA_PARAMS
                  (L_PAREN "(")
                  (PARAM (UNDERSCORE_KW "_"))
                  (COMMA ",")
                  (PARAM (NAME (IDENT "y")))
                  (R_PAREN ")"))
                (ARROW "->")
                (NAME_REF_EXPR (NAME_REF (IDENT "y")))))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PATH_TYPE (NAME_REF (IDENT "Fn2")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "c"))
              (EQ "=")
              (LAMBDA_EXPR
                (LAMBDA_PARAMS
                  (L_PAREN "(")
                  (PARAM
                    (PRIMITIVE_TYPE (INT_KW "int"))
                    (UNDERSCORE_KW "_"))
                  (COMMA ",")
                  (PARAM
                    (PATH_TYPE (NAME_REF (IDENT "var")))
                    (NAME (IDENT "y")))
                  (R_PAREN ")"))
                (ARROW "->")
                (NAME_REF_EXPR (NAME_REF (IDENT "y")))))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PATH_TYPE (NAME_REF (IDENT "Fn1")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "d"))
              (EQ "=")
              (LAMBDA_EXPR
                (LAMBDA_PARAMS
                  (L_PAREN "(")
                  (PARAM
                    (MODIFIER_LIST
                      (FINAL_KW "final")
                      (ANNOTATION
                        (AT "@")
                        (PATH (NAME_REF (IDENT "A")))))
                    (PATH_TYPE (NAME_REF (IDENT "String")))
                    (UNDERSCORE_KW "_"))
                  (R_PAREN ")"))
                (ARROW "->")
                (LITERAL (INT_NUMBER "1"))))
            (SEMICOLON ";"))
          (IF_STMT
            (IF_KW "if")
            (L_PAREN "(")
            (INSTANCEOF_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "o")))
              (INSTANCEOF_KW "instanceof")
              (TYPE_PATTERN
                (PATH_TYPE (NAME_REF (IDENT "String")))
                (UNDERSCORE_KW "_")))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}")))
          (IF_STMT
            (IF_KW "if")
            (L_PAREN "(")
            (INSTANCEOF_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "o")))
              (INSTANCEOF_KW "instanceof")
              (RECORD_PATTERN
                (PATH_TYPE (NAME_REF (IDENT "Point")))
                (L_PAREN "(")
                (UNNAMED_PATTERN (UNDERSCORE_KW "_"))
                (COMMA ",")
                (TYPE_PATTERN
                  (PRIMITIVE_TYPE (INT_KW "int"))
                  (NAME (IDENT "y")))
                (R_PAREN ")")))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}")))
          (IF_STMT
            (IF_KW "if")
            (L_PAREN "(")
            (INSTANCEOF_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "o")))
              (INSTANCEOF_KW "instanceof")
              (RECORD_PATTERN
                (PATH_TYPE (NAME_REF (IDENT "Outer")))
                (L_PAREN "(")
                (RECORD_PATTERN
                  (PATH_TYPE (NAME_REF (IDENT "Inner")))
                  (L_PAREN "(")
                  (UNNAMED_PATTERN (UNDERSCORE_KW "_"))
                  (R_PAREN ")"))
                (COMMA ",")
                (UNNAMED_PATTERN (UNDERSCORE_KW "_"))
                (R_PAREN ")")))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
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
                    (PATH_TYPE
                      (NAME_REF (IDENT "Outer"))
                      (DOT ".")
                      (NAME_REF (IDENT "Inner")))
                    (UNDERSCORE_KW "_"))
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
                  (TYPE_PATTERN
                    (ARRAY_TYPE
                      (PRIMITIVE_TYPE (BYTE_KW "byte"))
                      (L_BRACK "[")
                      (R_BRACK "]"))
                    (UNDERSCORE_KW "_"))
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
                  (RECORD_PATTERN
                    (PATH_TYPE (NAME_REF (IDENT "Point")))
                    (L_PAREN "(")
                    (TYPE_PATTERN
                      (PATH_TYPE (NAME_REF (IDENT "Foo")))
                      (UNDERSCORE_KW "_"))
                    (COMMA ",")
                    (UNNAMED_PATTERN (UNDERSCORE_KW "_"))
                    (R_PAREN ")"))
                  (ARROW "->"))
                (EXPR_STMT
                  (CALL_EXPR
                    (NAME_REF_EXPR (NAME_REF (IDENT "c")))
                    (ARG_LIST
                      (L_PAREN "(")
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
          (R_CURLY "}")))
      (R_CURLY "}"))))
