(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "A"))
    (CLASS_BODY
      (L_CURLY "{")
      (FIELD_DECL
        (PRIMITIVE_TYPE (INT_KW "int"))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "x"))
          (EQ "="))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PRIMITIVE_TYPE (INT_KW "int"))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "y"))
          (EQ "=")
          (EXPR_PLACEHOLDER (INT_NUMBER "1")))
        (COMMA ",")
        (VARIABLE_DECLARATOR
          (NAME (IDENT "z"))
          (EQ "="))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PRIMITIVE_TYPE (INT_KW "int"))
        (VARIABLE_DECLARATOR (NAME (IDENT "w")))
        (SEMICOLON ";"))
      (R_CURLY "}"))))
(errors
  (error "expected an expression" "2:12..2:12")
  (error "expected an expression" "3:19..3:19"))
