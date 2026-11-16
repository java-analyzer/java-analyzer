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
            (PATH_TYPE (NAME_REF (IDENT "Runnable")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "r1"))
              (EQ "=")
              (LAMBDA_EXPR
                (LAMBDA_PARAMS
                  (L_PAREN "(")
                  (R_PAREN ")"))
                (ARROW "->")
                (CALL_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "work")))
                  (ARG_LIST
                    (L_PAREN "(")
                    (R_PAREN ")")))))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PATH_TYPE (NAME_REF (IDENT "Runnable")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "r2"))
              (EQ "=")
              (LAMBDA_EXPR
                (LAMBDA_PARAMS
                  (L_PAREN "(")
                  (R_PAREN ")"))
                (ARROW "->")
                (BLOCK
                  (L_CURLY "{")
                  (EXPR_STMT
                    (CALL_EXPR
                      (NAME_REF_EXPR (NAME_REF (IDENT "work")))
                      (ARG_LIST
                        (L_PAREN "(")
                        (R_PAREN ")")))
                    (SEMICOLON ";"))
                  (R_CURLY "}"))))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PATH_TYPE
              (NAME_REF (IDENT "Function"))
              (TYPE_ARGS
                (L_ANGLE "<")
                (PATH_TYPE (NAME_REF (IDENT "Integer")))
                (COMMA ",")
                (PATH_TYPE (NAME_REF (IDENT "Integer")))
                (R_ANGLE ">")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "f1"))
              (EQ "=")
              (LAMBDA_EXPR
                (LAMBDA_PARAMS (PARAM (NAME (IDENT "x"))))
                (ARROW "->")
                (BINARY_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "x")))
                  (PLUS "+")
                  (LITERAL (INT_NUMBER "1")))))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PATH_TYPE
              (NAME_REF (IDENT "BiFunction"))
              (TYPE_ARGS
                (L_ANGLE "<")
                (PATH_TYPE (NAME_REF (IDENT "Integer")))
                (COMMA ",")
                (PATH_TYPE (NAME_REF (IDENT "Integer")))
                (COMMA ",")
                (PATH_TYPE (NAME_REF (IDENT "Integer")))
                (R_ANGLE ">")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "f2"))
              (EQ "=")
              (LAMBDA_EXPR
                (LAMBDA_PARAMS
                  (L_PAREN "(")
                  (PARAM (NAME (IDENT "a")))
                  (COMMA ",")
                  (PARAM (NAME (IDENT "b")))
                  (R_PAREN ")"))
                (ARROW "->")
                (BINARY_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "a")))
                  (PLUS "+")
                  (NAME_REF_EXPR (NAME_REF (IDENT "b"))))))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PATH_TYPE
              (NAME_REF (IDENT "BiFunction"))
              (TYPE_ARGS
                (L_ANGLE "<")
                (PATH_TYPE (NAME_REF (IDENT "Integer")))
                (COMMA ",")
                (PATH_TYPE (NAME_REF (IDENT "Integer")))
                (COMMA ",")
                (PATH_TYPE (NAME_REF (IDENT "Integer")))
                (R_ANGLE ">")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "f3"))
              (EQ "=")
              (LAMBDA_EXPR
                (LAMBDA_PARAMS
                  (L_PAREN "(")
                  (PARAM
                    (PRIMITIVE_TYPE (INT_KW "int"))
                    (NAME (IDENT "a")))
                  (COMMA ",")
                  (PARAM
                    (PRIMITIVE_TYPE (INT_KW "int"))
                    (NAME (IDENT "b")))
                  (R_PAREN ")"))
                (ARROW "->")
                (BLOCK
                  (L_CURLY "{")
                  (RETURN_STMT
                    (RETURN_KW "return")
                    (BINARY_EXPR
                      (NAME_REF_EXPR (NAME_REF (IDENT "a")))
                      (PLUS "+")
                      (NAME_REF_EXPR (NAME_REF (IDENT "b"))))
                    (SEMICOLON ";"))
                  (R_CURLY "}"))))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PATH_TYPE
              (NAME_REF (IDENT "Function"))
              (TYPE_ARGS
                (L_ANGLE "<")
                (PATH_TYPE (NAME_REF (IDENT "String")))
                (COMMA ",")
                (PATH_TYPE (NAME_REF (IDENT "Integer")))
                (R_ANGLE ">")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "f4"))
              (EQ "=")
              (LAMBDA_EXPR
                (LAMBDA_PARAMS
                  (L_PAREN "(")
                  (PARAM
                    (MODIFIER_LIST (FINAL_KW "final"))
                    (PATH_TYPE (NAME_REF (IDENT "var")))
                    (NAME (IDENT "s")))
                  (R_PAREN ")"))
                (ARROW "->")
                (METHOD_CALL_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "s")))
                  (DOT ".")
                  (NAME_REF (IDENT "length"))
                  (ARG_LIST
                    (L_PAREN "(")
                    (R_PAREN ")")))))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PATH_TYPE
              (NAME_REF (IDENT "Supplier"))
              (TYPE_ARGS
                (L_ANGLE "<")
                (PATH_TYPE
                  (NAME_REF (IDENT "Supplier"))
                  (TYPE_ARGS
                    (L_ANGLE "<")
                    (PATH_TYPE (NAME_REF (IDENT "Integer")))
                    (R_ANGLE ">")))
                (R_ANGLE ">")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "f5"))
              (EQ "=")
              (LAMBDA_EXPR
                (LAMBDA_PARAMS
                  (L_PAREN "(")
                  (R_PAREN ")"))
                (ARROW "->")
                (LAMBDA_EXPR
                  (LAMBDA_PARAMS
                    (L_PAREN "(")
                    (R_PAREN ")"))
                  (ARROW "->")
                  (LITERAL (INT_NUMBER "1")))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (CALL_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "run")))
              (ARG_LIST
                (L_PAREN "(")
                (LAMBDA_EXPR
                  (LAMBDA_PARAMS (PARAM (NAME (IDENT "x"))))
                  (ARROW "->")
                  (LAMBDA_EXPR
                    (LAMBDA_PARAMS (PARAM (NAME (IDENT "y"))))
                    (ARROW "->")
                    (BINARY_EXPR
                      (NAME_REF_EXPR (NAME_REF (IDENT "x")))
                      (PLUS "+")
                      (NAME_REF_EXPR (NAME_REF (IDENT "y"))))))
                (R_PAREN ")")))
            (SEMICOLON ";"))
          (EXPR_STMT
            (METHOD_CALL_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "list")))
              (DOT ".")
              (NAME_REF (IDENT "forEach"))
              (ARG_LIST
                (L_PAREN "(")
                (LAMBDA_EXPR
                  (LAMBDA_PARAMS (PARAM (NAME (IDENT "item"))))
                  (ARROW "->")
                  (BLOCK
                    (L_CURLY "{")
                    (EXPR_STMT
                      (CALL_EXPR
                        (NAME_REF_EXPR (NAME_REF (IDENT "use")))
                        (ARG_LIST
                          (L_PAREN "(")
                          (NAME_REF_EXPR (NAME_REF (IDENT "item")))
                          (R_PAREN ")")))
                      (SEMICOLON ";"))
                    (R_CURLY "}")))
                (R_PAREN ")")))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PATH_TYPE (NAME_REF (IDENT "Object")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "o"))
              (EQ "=")
              (TERNARY_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "cond")))
                (QUESTION "?")
                (CAST_EXPR
                  (L_PAREN "(")
                  (PATH_TYPE (NAME_REF (IDENT "Runnable")))
                  (R_PAREN ")")
                  (LAMBDA_EXPR
                    (LAMBDA_PARAMS
                      (L_PAREN "(")
                      (R_PAREN ")"))
                    (ARROW "->")
                    (BLOCK
                      (L_CURLY "{")
                      (R_CURLY "}"))))
                (COLON ":")
                (LITERAL (NULL_KW "null"))))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PATH_TYPE
              (NAME_REF (IDENT "Function"))
              (TYPE_ARGS
                (L_ANGLE "<")
                (PATH_TYPE (NAME_REF (IDENT "A")))
                (COMMA ",")
                (PATH_TYPE (NAME_REF (IDENT "B")))
                (R_ANGLE ">")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "m1"))
              (EQ "=")
              (METHOD_REF_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "Foo")))
                (COLON2 "::")
                (NAME_REF (IDENT "bar"))))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PATH_TYPE
              (NAME_REF (IDENT "Supplier"))
              (TYPE_ARGS
                (L_ANGLE "<")
                (PATH_TYPE (NAME_REF (IDENT "Foo")))
                (R_ANGLE ">")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "m2"))
              (EQ "=")
              (METHOD_REF_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "Foo")))
                (COLON2 "::")
                (NEW_KW "new")))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PATH_TYPE
              (NAME_REF (IDENT "Function"))
              (TYPE_ARGS
                (L_ANGLE "<")
                (PATH_TYPE (NAME_REF (IDENT "Integer")))
                (COMMA ",")
                (ARRAY_TYPE
                  (PRIMITIVE_TYPE (INT_KW "int"))
                  (L_BRACK "[")
                  (R_BRACK "]"))
                (R_ANGLE ">")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "m3"))
              (EQ "=")
              (METHOD_REF_EXPR
                (ARRAY_TYPE
                  (PRIMITIVE_TYPE (INT_KW "int"))
                  (L_BRACK "[")
                  (R_BRACK "]"))
                (COLON2 "::")
                (NEW_KW "new")))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PATH_TYPE
              (NAME_REF (IDENT "Function"))
              (TYPE_ARGS
                (L_ANGLE "<")
                (PATH_TYPE (NAME_REF (IDENT "Integer")))
                (COMMA ",")
                (ARRAY_TYPE
                  (PATH_TYPE (NAME_REF (IDENT "String")))
                  (L_BRACK "[")
                  (R_BRACK "]"))
                (R_ANGLE ">")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "m4"))
              (EQ "=")
              (METHOD_REF_EXPR
                (ARRAY_TYPE
                  (PATH_TYPE (NAME_REF (IDENT "String")))
                  (L_BRACK "[")
                  (R_BRACK "]"))
                (COLON2 "::")
                (NEW_KW "new")))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PATH_TYPE
              (NAME_REF (IDENT "Supplier"))
              (TYPE_ARGS
                (L_ANGLE "<")
                (PATH_TYPE
                  (NAME_REF (IDENT "List"))
                  (TYPE_ARGS
                    (L_ANGLE "<")
                    (PATH_TYPE (NAME_REF (IDENT "String")))
                    (R_ANGLE ">")))
                (R_ANGLE ">")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "m5"))
              (EQ "=")
              (METHOD_REF_EXPR
                (PATH_TYPE
                  (NAME_REF (IDENT "ArrayList"))
                  (TYPE_ARGS
                    (L_ANGLE "<")
                    (PATH_TYPE (NAME_REF (IDENT "String")))
                    (R_ANGLE ">")))
                (COLON2 "::")
                (NEW_KW "new")))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PATH_TYPE
              (NAME_REF (IDENT "Function"))
              (TYPE_ARGS
                (L_ANGLE "<")
                (PATH_TYPE (NAME_REF (IDENT "Foo")))
                (COMMA ",")
                (PATH_TYPE (NAME_REF (IDENT "Bar")))
                (R_ANGLE ">")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "m6"))
              (EQ "=")
              (METHOD_REF_EXPR
                (THIS_EXPR (THIS_KW "this"))
                (COLON2 "::")
                (NAME_REF (IDENT "convert"))))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PATH_TYPE
              (NAME_REF (IDENT "Function"))
              (TYPE_ARGS
                (L_ANGLE "<")
                (PATH_TYPE (NAME_REF (IDENT "Foo")))
                (COMMA ",")
                (PATH_TYPE (NAME_REF (IDENT "Bar")))
                (R_ANGLE ">")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "m7"))
              (EQ "=")
              (METHOD_REF_EXPR
                (SUPER_EXPR (SUPER_KW "super"))
                (COLON2 "::")
                (NAME_REF (IDENT "convert"))))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PATH_TYPE
              (NAME_REF (IDENT "Function"))
              (TYPE_ARGS
                (L_ANGLE "<")
                (PATH_TYPE (NAME_REF (IDENT "Foo")))
                (COMMA ",")
                (PATH_TYPE (NAME_REF (IDENT "Bar")))
                (R_ANGLE ">")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "m8"))
              (EQ "=")
              (METHOD_REF_EXPR
                (FIELD_ACCESS_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "Outer")))
                  (DOT ".")
                  (NAME_REF (IDENT "Inner")))
                (COLON2 "::")
                (NAME_REF (IDENT "method"))))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PATH_TYPE
              (NAME_REF (IDENT "Function"))
              (TYPE_ARGS
                (L_ANGLE "<")
                (PATH_TYPE (NAME_REF (IDENT "Foo")))
                (COMMA ",")
                (PATH_TYPE (NAME_REF (IDENT "Bar")))
                (R_ANGLE ">")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "m9"))
              (EQ "=")
              (METHOD_REF_EXPR
                (FIELD_ACCESS_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "System")))
                  (DOT ".")
                  (NAME_REF (IDENT "out")))
                (COLON2 "::")
                (NAME_REF (IDENT "println"))))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PATH_TYPE
              (NAME_REF (IDENT "Function"))
              (TYPE_ARGS
                (L_ANGLE "<")
                (PATH_TYPE (NAME_REF (IDENT "Foo")))
                (COMMA ",")
                (PATH_TYPE (NAME_REF (IDENT "Bar")))
                (R_ANGLE ">")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "m10"))
              (EQ "=")
              (METHOD_REF_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "Foo")))
                (COLON2 "::")
                (TYPE_ARGS
                  (L_ANGLE "<")
                  (PATH_TYPE (NAME_REF (IDENT "String")))
                  (R_ANGLE ">"))
                (NAME_REF (IDENT "generic"))))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PATH_TYPE
              (NAME_REF (IDENT "Class"))
              (TYPE_ARGS
                (L_ANGLE "<")
                (WILDCARD_TYPE (QUESTION "?"))
                (R_ANGLE ">")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "c1"))
              (EQ "=")
              (CLASS_LITERAL
                (NAME_REF_EXPR (NAME_REF (IDENT "String")))
                (DOT ".")
                (CLASS_KW "class")))
            (COMMA ",")
            (VARIABLE_DECLARATOR
              (NAME (IDENT "c2"))
              (EQ "=")
              (CLASS_LITERAL
                (PRIMITIVE_TYPE (INT_KW "int"))
                (DOT ".")
                (CLASS_KW "class")))
            (COMMA ",")
            (VARIABLE_DECLARATOR
              (NAME (IDENT "c3"))
              (EQ "=")
              (CLASS_LITERAL
                (ARRAY_TYPE
                  (PRIMITIVE_TYPE (INT_KW "int"))
                  (L_BRACK "[")
                  (R_BRACK "]"))
                (DOT ".")
                (CLASS_KW "class")))
            (COMMA ",")
            (VARIABLE_DECLARATOR
              (NAME (IDENT "c4"))
              (EQ "=")
              (CLASS_LITERAL
                (ARRAY_TYPE
                  (PATH_TYPE (NAME_REF (IDENT "Foo")))
                  (L_BRACK "[")
                  (R_BRACK "]"))
                (DOT ".")
                (CLASS_KW "class")))
            (COMMA ",")
            (VARIABLE_DECLARATOR
              (NAME (IDENT "c5"))
              (EQ "=")
              (CLASS_LITERAL
                (PRIMITIVE_TYPE (VOID_KW "void"))
                (DOT ".")
                (CLASS_KW "class")))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PATH_TYPE
              (NAME_REF (IDENT "Class"))
              (TYPE_ARGS
                (L_ANGLE "<")
                (WILDCARD_TYPE (QUESTION "?"))
                (R_ANGLE ">")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "c6"))
              (EQ "=")
              (CLASS_LITERAL
                (FIELD_ACCESS_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "Outer")))
                  (DOT ".")
                  (NAME_REF (IDENT "Inner")))
                (DOT ".")
                (CLASS_KW "class")))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (R_CURLY "}"))))
