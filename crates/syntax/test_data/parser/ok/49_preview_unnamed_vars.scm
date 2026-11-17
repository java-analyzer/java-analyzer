(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "PreviewUnnamed"))
    (CLASS_BODY
      (L_CURLY "{")
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "m"))
        (PARAM_LIST
          (L_PAREN "(")
          (PARAM
            (PATH_TYPE (NAME_REF (IDENT "Object")))
            (NAME (IDENT "o")))
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (LOCAL_VAR_DECL
            (PRIMITIVE_TYPE (INT_KW "int"))
            (VARIABLE_DECLARATOR
              (UNDERSCORE_KW "_")
              (EQ "=")
              (LITERAL (INT_NUMBER "1")))
            (SEMICOLON ";"))
          (IF_STMT
            (IF_KW "if")
            (L_PAREN "(")
            (INSTANCEOF_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "o")))
              (INSTANCEOF_KW "instanceof")
              (TYPE_PATTERN
                (PATH_TYPE (NAME_REF (IDENT "String")))
                (UNDERSCORE_KW "_")))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}")))
          (R_CURLY "}")))
      (R_CURLY "}"))))
