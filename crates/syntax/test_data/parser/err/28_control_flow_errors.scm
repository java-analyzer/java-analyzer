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
          (IF_STMT
            (IF_KW "if")
            (EXPR_STMT
              (BINARY_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "x")))
                (R_ANGLE ">")
                (LITERAL (INT_NUMBER "1")))))
          (ERROR (R_PAREN ")"))
          (BLOCK
            (L_CURLY "{")
            (R_CURLY "}"))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "b"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (IF_STMT
            (IF_KW "if")
            (L_PAREN "(")
            (BINARY_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (R_ANGLE ">")
              (LITERAL (INT_NUMBER "1")))
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}")))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "c"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (IF_STMT
            (IF_KW "if")
            (L_PAREN "(")
            (NAME_REF_EXPR (NAME_REF (IDENT "x")))
            (R_PAREN ")"))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "d"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (WHILE_STMT
            (WHILE_KW "while")
            (L_PAREN "(")
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}")))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "e"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (DO_STMT
            (DO_KW "do")
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}"))
            (WHILE_KW "while")
            (L_PAREN "(")
            (NAME_REF_EXPR (NAME_REF (IDENT "x")))
            (R_PAREN ")"))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "f"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (DO_STMT
            (DO_KW "do")
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}")))
          (EXPR_STMT
            (PAREN_EXPR
              (L_PAREN "(")
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (R_PAREN ")"))
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
          (FOR_STMT
            (FOR_KW "for")
            (L_PAREN "(")
            (LOCAL_VAR_DECL
              (PRIMITIVE_TYPE (INT_KW "int"))
              (VARIABLE_DECLARATOR
                (NAME (IDENT "i"))
                (EQ "=")
                (LITERAL (INT_NUMBER "0"))))
            (SEMICOLON ";")
            (BINARY_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "i")))
              (L_ANGLE "<")
              (LITERAL (INT_NUMBER "3")))
            (POSTFIX_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "i")))
              (PLUS2 "++"))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}")))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "h"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (FOREACH_STMT
            (FOR_KW "for")
            (L_PAREN "(")
            (PRIMITIVE_TYPE (INT_KW "int"))
            (NAME (IDENT "i"))
            (COLON ":")
            (R_PAREN ")")
            (BLOCK
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
  (error "expected '('" "2:18..2:18")
  (error "expected ';'" "2:24..2:24")
  (error "expected an expression" "2:24..2:24")
  (error "expected ')'" "3:25..3:25")
  (error "expected a statement" "4:22..4:22")
  (error "expected an expression" "5:23..5:23")
  (error "expected ';'" "6:32..6:32")
  (error "expected 'while'" "7:22..7:22")
  (error "expected ';'" "7:22..7:22")
  (error "expected ';'" "8:37..8:37")
  (error "expected an expression" "9:28..9:28"))
