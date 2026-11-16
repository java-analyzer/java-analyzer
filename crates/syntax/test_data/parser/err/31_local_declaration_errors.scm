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
          (LOCAL_VAR_DECL
            (MODIFIER_LIST (FINAL_KW "final"))
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
          (LOCAL_VAR_DECL
            (PRIMITIVE_TYPE (INT_KW "int"))
            (VARIABLE_DECLARATOR)
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
          (LOCAL_VAR_DECL
            (PRIMITIVE_TYPE (INT_KW "int"))
            (VARIABLE_DECLARATOR (NAME (IDENT "x")))
            (COMMA ",")
            (VARIABLE_DECLARATOR)
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
          (LOCAL_VAR_DECL
            (PATH_TYPE (NAME_REF (IDENT "Foo")))
            (VARIABLE_DECLARATOR (NAME (IDENT "x"))))
          (EXPR_STMT
            (EXPR_PLACEHOLDER (IDENT "y"))
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
          (CLASS_DECL
            (CLASS_KW "class")
            (CLASS_BODY
              (L_CURLY "{")
              (R_CURLY "}")))
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
  (error "expected a type" "2:21..2:21")
  (error "expected a name" "3:19..3:19")
  (error "expected a name" "4:22..4:22")
  (error "expected ';'" "5:21..5:21")
  (error "expected a name" "6:21..6:21"))
