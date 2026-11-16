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
          (TRY_STMT
            (TRY_KW "try")
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}")))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "b"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (TRY_STMT (TRY_KW "try"))
          (EXPR_STMT
            (EXPR_PLACEHOLDER
              (IDENT "x")
              (L_PAREN "(")
              (R_PAREN ")"))
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
          (TRY_STMT
            (TRY_KW "try")
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}"))
            (CATCH_CLAUSE
              (CATCH_KW "catch")
              (BLOCK
                (L_CURLY "{")
                (R_CURLY "}"))))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "d"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (TRY_STMT
            (TRY_KW "try")
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}"))
            (CATCH_CLAUSE
              (CATCH_KW "catch")
              (L_PAREN "(")
              (PATH_TYPE (NAME_REF (IDENT "E")))
              (R_PAREN ")")
              (BLOCK
                (L_CURLY "{")
                (R_CURLY "}"))))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "e"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (TRY_STMT
            (TRY_KW "try")
            (RESOURCE_LIST
              (L_PAREN "(")
              (RESOURCE
                (EXPR_PLACEHOLDER
                  (IDENT "r")
                  (L_CURLY "{")
                  (R_CURLY "}")))))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "f"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (TRY_STMT
            (TRY_KW "try")
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}"))
            (CATCH_CLAUSE
              (CATCH_KW "catch")
              (L_PAREN "(")
              (PATH_TYPE (NAME_REF (IDENT "E")))
              (NAME (IDENT "e"))
              (R_PAREN ")")))
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
  (error "expected 'catch' or 'finally'" "2:23..2:23")
  (error "expected '{'" "3:19..3:19")
  (error "expected 'catch' or 'finally'" "3:19..3:19")
  (error "expected '('" "4:29..4:29")
  (error "expected a name" "5:32..5:32")
  (error "expected ')'" "6:26..6:26")
  (error "expected '{'" "6:26..6:26")
  (error "expected '{'" "7:35..7:35"))
