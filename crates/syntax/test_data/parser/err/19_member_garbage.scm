(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "A"))
    (CLASS_BODY
      (L_CURLY "{")
      (ERROR
        (EQ "=")
        (INT_NUMBER "5")
        (SEMICOLON ";"))
      (FIELD_DECL
        (PRIMITIVE_TYPE (INT_KW "int"))
        (VARIABLE_DECLARATOR (NAME (IDENT "x")))
        (SEMICOLON ";"))
      (ERROR
        (R_PAREN ")")
        (L_PAREN "(")
        (INT_NUMBER "123"))
      (FIELD_DECL
        (PRIMITIVE_TYPE (INT_KW "int"))
        (VARIABLE_DECLARATOR (NAME (IDENT "y")))
        (SEMICOLON ";"))
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
  (error "expected a member declaration" "2:5..2:6")
  (error "expected a member declaration" "4:5..4:6"))
