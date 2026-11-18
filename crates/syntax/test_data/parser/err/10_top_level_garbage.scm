(SOURCE_FILE
  (ERROR
    (IDENT "foo")
    (IDENT "bar")
    (IDENT "baz"))
  (SEMICOLON ";")
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "A"))
    (CLASS_BODY
      (L_CURLY "{")
      (R_CURLY "}")))
  (FIELD_DECL
    (PRIMITIVE_TYPE (INT_KW "int"))
    (VARIABLE_DECLARATOR
      (NAME (IDENT "x"))
      (EQ "=")
      (LITERAL (INT_NUMBER "5")))
    (SEMICOLON ";"))
  (CLASS_DECL
    (MODIFIER_LIST (PUBLIC_KW "public"))
    (CLASS_KW "class")
    (NAME (IDENT "B"))
    (CLASS_BODY
      (L_CURLY "{")
      (R_CURLY "}"))))
(errors (error "expected a class, interface, enum or record declaration" "1:1..1:4"))
