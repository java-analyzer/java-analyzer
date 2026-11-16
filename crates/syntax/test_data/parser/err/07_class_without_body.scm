(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "A")))
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "B"))
    (CLASS_BODY
      (L_CURLY "{")
      (R_CURLY "}"))))
(errors (error "expected '{'" "1:8..1:8"))
