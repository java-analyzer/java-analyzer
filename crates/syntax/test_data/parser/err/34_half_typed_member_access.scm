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
            (FIELD_ACCESS_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "foo")))
              (DOT ".")))
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
            (METHOD_CALL_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "foo")))
              (DOT ".")
              (NAME_REF (IDENT "bar"))
              (ARG_LIST (L_PAREN "("))))
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
            (FIELD_ACCESS_EXPR
              (THIS_EXPR (THIS_KW "this"))
              (DOT ".")))
          (LOCAL_VAR_DECL
            (PRIMITIVE_TYPE (INT_KW "int"))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "x"))
              (EQ "=")
              (LITERAL (INT_NUMBER "1")))
            (SEMICOLON ";"))
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
            (FIELD_ACCESS_EXPR
              (METHOD_CALL_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "foo")))
                (DOT ".")
                (NAME_REF (IDENT "bar"))
                (ARG_LIST
                  (L_PAREN "(")
                  (LITERAL (INT_NUMBER "1"))
                  (COMMA ",")
                  (LITERAL (INT_NUMBER "2"))
                  (R_PAREN ")")))
              (DOT ".")))
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
            (METHOD_CALL_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "Outer")))
              (DOT ".")
              (TYPE_ARGS (L_ANGLE "<"))))
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
  (error "expected a name" "3:13..3:13")
  (error "expected ';'" "3:13..3:13")
  (error "expected ')'" "6:17..6:17")
  (error "expected ';'" "6:17..6:17")
  (error "expected a name" "9:14..9:14")
  (error "expected ';'" "9:14..9:14")
  (error "expected a name" "13:23..13:23")
  (error "expected ';'" "13:23..13:23")
  (error "expected a type" "16:16..16:16")
  (error "expected '>'" "16:16..16:16")
  (error "expected a name" "16:16..16:16")
  (error "expected '('" "16:16..16:16")
  (error "expected ';'" "16:16..16:16"))
