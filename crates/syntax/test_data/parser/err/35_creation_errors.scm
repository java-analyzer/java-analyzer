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
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (NEW_EXPR (NEW_KW "new")))
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
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (NEW_EXPR
                (NEW_KW "new")
                (PATH_TYPE (NAME_REF (IDENT "Foo")))))
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
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (NEW_EXPR
                (NEW_KW "new")
                (ARRAY_TYPE
                  (PRIMITIVE_TYPE (INT_KW "int"))
                  (L_BRACK "[")
                  (R_BRACK "]"))))
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
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (NEW_EXPR
                (NEW_KW "new")
                (PRIMITIVE_TYPE (INT_KW "int"))
                (L_BRACK "[")
                (LITERAL (INT_NUMBER "3"))))
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
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (NEW_EXPR
                (NEW_KW "new")
                (PATH_TYPE (NAME_REF (IDENT "Foo")))
                (ARG_LIST (L_PAREN "("))))
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
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (NEW_EXPR
                (NEW_KW "new")
                (ARRAY_TYPE
                  (PRIMITIVE_TYPE (INT_KW "int"))
                  (L_BRACK "[")
                  (R_BRACK "]"))
                (ARRAY_INIT
                  (L_CURLY "{")
                  (LITERAL (INT_NUMBER "1"))
                  (COMMA ",")
                  (LITERAL (INT_NUMBER "2")))))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "g"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")))
          (BLOCK
            (L_CURLY "{")
            (EXPR_STMT (LITERAL (INT_NUMBER "1")))
            (ERROR
              (COMMA ",")
              (INT_NUMBER "2"))
            (R_CURLY "}"))
          (EMPTY_STMT (SEMICOLON ";"))
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
  (error "expected a type" "2:23..2:23")
  (error "expected '(' or '['" "3:27..3:27")
  (error "expected '{'" "4:29..4:29")
  (error "expected ']'" "5:29..5:29")
  (error "expected ')'" "6:28..6:28")
  (error "expected '}'" "7:35..7:35")
  (error "expected an expression" "8:19..8:19")
  (error "expected ';'" "8:19..8:19")
  (error "expected ';'" "8:22..8:22")
  (error "expected an expression" "8:22..8:22"))
