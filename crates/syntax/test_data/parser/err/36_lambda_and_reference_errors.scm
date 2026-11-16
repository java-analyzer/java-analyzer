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
              (NAME_REF_EXPR (NAME_REF (IDENT "f")))
              (EQ "=")
              (LAMBDA_EXPR
                (LAMBDA_PARAMS (PARAM (NAME (IDENT "x"))))
                (ARROW "->")))
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
              (NAME_REF_EXPR (NAME_REF (IDENT "f")))
              (EQ "=")
              (LAMBDA_EXPR
                (LAMBDA_PARAMS
                  (L_PAREN "(")
                  (PARAM (NAME (IDENT "a")))
                  (COMMA ",")
                  (PARAM (NAME (IDENT "b")))
                  (R_PAREN ")"))
                (ARROW "->")))
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
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "f")))
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
                  (R_CURLY "}")))))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "d"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "f")))
              (EQ "=")
              (METHOD_REF_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "Foo")))
                (COLON2 "::")))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "e"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "f")))
              (EQ "=")
              (CLASS_LITERAL
                (ARRAY_TYPE
                  (PRIMITIVE_TYPE (INT_KW "int"))
                  (L_BRACK "[")
                  (R_BRACK "]"))))
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
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "f")))
              (EQ "=")
              (LAMBDA_EXPR
                (LAMBDA_PARAMS
                  (L_PAREN "(")
                  (PARAM (NAME (IDENT "a")))
                  (COMMA ",")
                  (R_PAREN ")"))
                (ARROW "->")
                (NAME_REF_EXPR (NAME_REF (IDENT "a")))))
            (SEMICOLON ";"))
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
  (error "expected an expression" "2:24..2:24")
  (error "expected an expression" "3:29..3:29")
  (error "expected ';'" "4:37..4:37")
  (error "expected a name" "5:25..5:25")
  (error "expected '.class' or '::'" "6:25..6:25")
  (error "expected a parameter" "7:23..7:23"))
