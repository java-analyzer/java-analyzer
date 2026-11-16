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
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (CAST_EXPR
                (L_PAREN "(")
                (PRIMITIVE_TYPE (INT_KW "int"))
                (R_PAREN ")")
                (NAME_REF_EXPR (NAME_REF (IDENT "y")))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (CAST_EXPR
                (L_PAREN "(")
                (PRIMITIVE_TYPE (LONG_KW "long"))
                (R_PAREN ")")
                (UNARY_EXPR
                  (MINUS "-")
                  (NAME_REF_EXPR (NAME_REF (IDENT "y"))))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (CAST_EXPR
                (L_PAREN "(")
                (PATH_TYPE (NAME_REF (IDENT "String")))
                (R_PAREN ")")
                (NAME_REF_EXPR (NAME_REF (IDENT "obj")))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (CAST_EXPR
                (L_PAREN "(")
                (PATH_TYPE
                  (NAME_REF (IDENT "List"))
                  (TYPE_ARGS
                    (L_ANGLE "<")
                    (PATH_TYPE (NAME_REF (IDENT "String")))
                    (R_ANGLE ">")))
                (R_PAREN ")")
                (NAME_REF_EXPR (NAME_REF (IDENT "obj")))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (CAST_EXPR
                (L_PAREN "(")
                (ARRAY_TYPE
                  (PRIMITIVE_TYPE (INT_KW "int"))
                  (L_BRACK "[")
                  (R_BRACK "]"))
                (R_PAREN ")")
                (NAME_REF_EXPR (NAME_REF (IDENT "obj")))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (CAST_EXPR
                (L_PAREN "(")
                (PATH_TYPE
                  (NAME_REF (IDENT "Foo"))
                  (DOT ".")
                  (NAME_REF (IDENT "Bar")))
                (R_PAREN ")")
                (NAME_REF_EXPR (NAME_REF (IDENT "obj")))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (CAST_EXPR
                (L_PAREN "(")
                (PATH_TYPE (NAME_REF (IDENT "Runnable")))
                (AMP "&")
                (PATH_TYPE (NAME_REF (IDENT "Serializable")))
                (R_PAREN ")")
                (LAMBDA_EXPR
                  (LAMBDA_PARAMS
                    (L_PAREN "(")
                    (R_PAREN ")"))
                  (ARROW "->")
                  (BLOCK
                    (L_CURLY "{")
                    (R_CURLY "}")))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (BINARY_EXPR
                (PAREN_EXPR
                  (L_PAREN "(")
                  (NAME_REF_EXPR (NAME_REF (IDENT "a")))
                  (R_PAREN ")"))
                (PLUS "+")
                (NAME_REF_EXPR (NAME_REF (IDENT "b")))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (BINARY_EXPR
                (PAREN_EXPR
                  (L_PAREN "(")
                  (NAME_REF_EXPR (NAME_REF (IDENT "a")))
                  (R_PAREN ")"))
                (MINUS "-")
                (NAME_REF_EXPR (NAME_REF (IDENT "b")))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (BINARY_EXPR
                (PAREN_EXPR
                  (L_PAREN "(")
                  (NAME_REF_EXPR (NAME_REF (IDENT "a")))
                  (R_PAREN ")"))
                (STAR "*")
                (NAME_REF_EXPR (NAME_REF (IDENT "b")))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (PAREN_EXPR
                (L_PAREN "(")
                (BINARY_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "a")))
                  (L_ANGLE "<")
                  (NAME_REF_EXPR (NAME_REF (IDENT "b"))))
                (R_PAREN ")")))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (TERNARY_EXPR
                (PAREN_EXPR
                  (L_PAREN "(")
                  (NAME_REF_EXPR (NAME_REF (IDENT "a")))
                  (R_PAREN ")"))
                (QUESTION "?")
                (NAME_REF_EXPR (NAME_REF (IDENT "b")))
                (COLON ":")
                (NAME_REF_EXPR (NAME_REF (IDENT "c")))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (BINARY_EXPR
                (PAREN_EXPR
                  (L_PAREN "(")
                  (NAME_REF_EXPR (NAME_REF (IDENT "a")))
                  (R_PAREN ")"))
                (EQ2 "==")
                (NAME_REF_EXPR (NAME_REF (IDENT "b")))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (CAST_EXPR
                (L_PAREN "(")
                (PATH_TYPE (NAME_REF (IDENT "T")))
                (R_PAREN ")")
                (CAST_EXPR
                  (L_PAREN "(")
                  (PATH_TYPE (NAME_REF (IDENT "U")))
                  (R_PAREN ")")
                  (NAME_REF_EXPR (NAME_REF (IDENT "obj"))))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (CAST_EXPR
                (L_PAREN "(")
                (PATH_TYPE (NAME_REF (IDENT "T")))
                (R_PAREN ")")
                (UNARY_EXPR
                  (BANG "!")
                  (NAME_REF_EXPR (NAME_REF (IDENT "flag"))))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (CAST_EXPR
                (L_PAREN "(")
                (PATH_TYPE (NAME_REF (IDENT "T")))
                (R_PAREN ")")
                (NEW_EXPR
                  (NEW_KW "new")
                  (PATH_TYPE (NAME_REF (IDENT "Foo")))
                  (ARG_LIST
                    (L_PAREN "(")
                    (R_PAREN ")")))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (CAST_EXPR
                (L_PAREN "(")
                (PATH_TYPE (NAME_REF (IDENT "T")))
                (R_PAREN ")")
                (THIS_EXPR (THIS_KW "this"))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (METHOD_CALL_EXPR
                (PAREN_EXPR
                  (L_PAREN "(")
                  (CAST_EXPR
                    (L_PAREN "(")
                    (PATH_TYPE (NAME_REF (IDENT "Foo")))
                    (R_PAREN ")")
                    (NAME_REF_EXPR (NAME_REF (IDENT "obj"))))
                  (R_PAREN ")"))
                (DOT ".")
                (NAME_REF (IDENT "bar"))
                (ARG_LIST
                  (L_PAREN "(")
                  (R_PAREN ")"))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (CAST_EXPR
                (L_PAREN "(")
                (PATH_TYPE (NAME_REF (IDENT "Foo")))
                (R_PAREN ")")
                (METHOD_CALL_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "obj")))
                  (DOT ".")
                  (NAME_REF (IDENT "bar"))
                  (ARG_LIST
                    (L_PAREN "(")
                    (R_PAREN ")")))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (BINARY_EXPR
                (CAST_EXPR
                  (L_PAREN "(")
                  (PRIMITIVE_TYPE (BYTE_KW "byte"))
                  (R_PAREN ")")
                  (LITERAL (INT_NUMBER "1")))
                (PLUS "+")
                (LITERAL (INT_NUMBER "2"))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (BINARY_EXPR
                (PAREN_EXPR
                  (L_PAREN "(")
                  (BINARY_EXPR
                    (NAME_REF_EXPR (NAME_REF (IDENT "a")))
                    (PLUS "+")
                    (NAME_REF_EXPR (NAME_REF (IDENT "b"))))
                  (R_PAREN ")"))
                (STAR "*")
                (PAREN_EXPR
                  (L_PAREN "(")
                  (BINARY_EXPR
                    (NAME_REF_EXPR (NAME_REF (IDENT "c")))
                    (PLUS "+")
                    (NAME_REF_EXPR (NAME_REF (IDENT "d"))))
                  (R_PAREN ")"))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (PAREN_EXPR
                (L_PAREN "(")
                (NAME_REF_EXPR (NAME_REF (IDENT "a")))
                (R_PAREN ")")))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (R_CURLY "}"))))
