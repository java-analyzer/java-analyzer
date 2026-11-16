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
  (ERROR
    (INT_KW "int")
    (IDENT "x")
    (EQ "=")
    (INT_NUMBER "5"))
  (SEMICOLON ";")
  (CLASS_DECL
    (MODIFIER_LIST (PUBLIC_KW "public"))
    (CLASS_KW "class")
    (NAME (IDENT "B"))
    (CLASS_BODY
      (L_CURLY "{")
      (R_CURLY "}"))))
(errors
  (error "expected a class, interface, enum or record declaration" "1:1..1:4")
  (error "expected a class, interface, enum or record declaration" "3:1..3:4"))
