(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "A"))
    (CLASS_BODY
      (L_CURLY "{")
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "Runnable")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "r"))
          (EQ "=")
          (NEW_EXPR
            (NEW_KW "new")
            (PATH_TYPE (NAME_REF (IDENT "Runnable")))
            (ARG_LIST
              (L_PAREN "(")
              (R_PAREN ")"))
            (CLASS_BODY
              (L_CURLY "{")
              (METHOD_DECL
                (MODIFIER_LIST (PUBLIC_KW "public"))
                (PRIMITIVE_TYPE (VOID_KW "void"))
                (NAME (IDENT "run"))
                (PARAM_LIST
                  (L_PAREN "(")
                  (R_PAREN ")"))
                (BLOCK
                  (L_CURLY "{")
                  (R_CURLY "}")))
              (R_CURLY "}"))))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "Runnable")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "s"))
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
      (FIELD_DECL
        (PRIMITIVE_TYPE (INT_KW "int"))
        (VARIABLE_DECLARATOR (NAME (IDENT "after")))
        (SEMICOLON ";"))
      (R_CURLY "}"))))
