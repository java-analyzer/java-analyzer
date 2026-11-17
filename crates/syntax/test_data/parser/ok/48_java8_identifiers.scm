(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "Java8Identifiers"))
    (CLASS_BODY
      (L_CURLY "{")
      (FIELD_DECL
        (PRIMITIVE_TYPE (INT_KW "int"))
        (VARIABLE_DECLARATOR
          (NAME (UNDERSCORE_KW "_"))
          (EQ "=")
          (LITERAL (INT_NUMBER "1")))
        (SEMICOLON ";"))
      (METHOD_DECL
        (PRIMITIVE_TYPE (INT_KW "int"))
        (NAME (IDENT "f"))
        (PARAM_LIST
          (L_PAREN "(")
          (PARAM
            (PRIMITIVE_TYPE (INT_KW "int"))
            (NAME (UNDERSCORE_KW "_")))
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (LOCAL_VAR_DECL
            (PRIMITIVE_TYPE (INT_KW "int"))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "local"))
              (EQ "=")
              (NAME_REF_EXPR (NAME_REF (UNDERSCORE_KW "_"))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (CALL_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "yield")))
              (ARG_LIST
                (L_PAREN "(")
                (NAME_REF_EXPR (NAME_REF (IDENT "local")))
                (R_PAREN ")")))
            (SEMICOLON ";"))
          (RETURN_STMT
            (RETURN_KW "return")
            (NAME_REF_EXPR (NAME_REF (UNDERSCORE_KW "_")))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (R_CURLY "}"))))
