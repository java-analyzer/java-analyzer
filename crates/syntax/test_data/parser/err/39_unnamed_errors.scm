(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "UnnamedErrors"))
    (CLASS_BODY
      (L_CURLY "{")
      (FIELD_DECL
        (PRIMITIVE_TYPE (INT_KW "int"))
        (VARIABLE_DECLARATOR))
      (ERROR
        (UNDERSCORE_KW "_")
        (EQ "=")
        (INT_NUMBER "1")
        (SEMICOLON ";"))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "m"))
        (PARAM_LIST
          (L_PAREN "(")
          (PARAM (PRIMITIVE_TYPE (INT_KW "int")))))
      (ERROR
        (UNDERSCORE_KW "_")
        (R_PAREN ")")
        (SEMICOLON ";"))
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
              (EQ "=")))
          (ERROR
            (UNDERSCORE_KW "_")
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "b"))
        (PARAM_LIST
          (L_PAREN "(")
          (PARAM
            (PATH_TYPE (NAME_REF (IDENT "Object")))
            (NAME (IDENT "o")))
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (IF_STMT
            (IF_KW "if")
            (L_PAREN "(")
            (INSTANCEOF_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "o")))
              (INSTANCEOF_KW "instanceof"))
            (ERROR
              (UNDERSCORE_KW "_")
              (R_PAREN ")")))
          (BLOCK
            (L_CURLY "{")
            (R_CURLY "}"))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "c"))
        (PARAM_LIST
          (L_PAREN "(")
          (PARAM
            (PATH_TYPE (NAME_REF (IDENT "Object")))
            (NAME (IDENT "o")))
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (SWITCH_STMT
            (SWITCH_KW "switch")
            (L_PAREN "(")
            (NAME_REF_EXPR (NAME_REF (IDENT "o")))
            (R_PAREN ")")
            (SWITCH_BODY
              (L_CURLY "{")
              (SWITCH_CASE
                (SWITCH_LABEL (CASE_KW "case"))
                (EXPR_STMT
                  (LAMBDA_EXPR
                    (LAMBDA_PARAMS (PARAM (UNDERSCORE_KW "_")))
                    (ARROW "->")
                    (CALL_EXPR
                      (NAME_REF_EXPR (NAME_REF (IDENT "a")))
                      (ARG_LIST
                        (L_PAREN "(")
                        (R_PAREN ")"))))
                  (SEMICOLON ";")))
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
  (error "expected a name" "2:8..2:8")
  (error "expected a member declaration" "2:9..2:10")
  (error "expected a name" "3:15..3:15")
  (error "expected ')'" "3:15..3:15")
  (error "expected '{' or ';'" "3:15..3:15")
  (error "expected a member declaration" "3:16..3:17")
  (error "expected an expression" "4:19..4:19")
  (error "expected ';'" "4:19..4:19")
  (error "expected an expression" "4:19..4:19")
  (error "expected a type" "5:40..5:40")
  (error "expected ')'" "5:40..5:40")
  (error "expected an expression" "5:40..5:40")
  (error "expected an expression" "6:41..6:41")
  (error "expected ':' or '->'" "6:41..6:41"))
