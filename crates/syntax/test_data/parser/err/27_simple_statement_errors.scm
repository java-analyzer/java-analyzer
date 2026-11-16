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
            (PRIMITIVE_TYPE (INT_KW "int"))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "x"))
              (EQ "=")
              (EXPR_PLACEHOLDER (INT_NUMBER "1"))))
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
            (EXPR_PLACEHOLDER
              (IDENT "foo")
              (L_PAREN "(")
              (R_PAREN ")")))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "c"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (RETURN_STMT (RETURN_KW "return"))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "d"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (BREAK_STMT (BREAK_KW "break"))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "e"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (ERROR (ELSE_KW "else"))
          (BLOCK
            (L_CURLY "{")
            (R_CURLY "}"))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "f"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (ERROR (FINALLY_KW "finally"))
          (BLOCK
            (L_CURLY "{")
            (R_CURLY "}"))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "g"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (THROW_STMT
            (THROW_KW "throw")
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "h"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (EXPR_STMT (EXPR_PLACEHOLDER (IDENT "yield")))
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
  (error "expected ';'" "2:25..2:25")
  (error "expected ';'" "3:21..3:21")
  (error "expected ';'" "4:22..4:22")
  (error "expected ';'" "5:21..5:21")
  (error "unexpected 'else'" "6:16..6:20")
  (error "unexpected 'finally'" "7:16..7:23")
  (error "expected an expression" "8:21..8:21")
  (error "expected ';'" "9:21..9:21"))
