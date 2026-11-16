(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "A"))
    (CLASS_BODY
      (L_CURLY "{")
      (MEMBER_PLACEHOLDER
        (INT_KW "int")
        (IDENT "x")
        (SEMICOLON ";")))))
(errors (error "expected '}'" "2:11..2:11"))
