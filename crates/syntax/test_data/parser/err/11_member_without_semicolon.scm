(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "A"))
    (CLASS_BODY
      (L_CURLY "{")
      (MEMBER_PLACEHOLDER
        (INT_KW "int")
        (IDENT "x")
        (EQ "=")
        (INT_NUMBER "5"))
      (R_CURLY "}")))
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "B"))
    (CLASS_BODY
      (L_CURLY "{")
      (R_CURLY "}"))))
(errors (error "expected ';'" "2:14..2:14"))
