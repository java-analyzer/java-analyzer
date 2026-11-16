(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "A"))
    (CLASS_BODY
      (L_CURLY "{")
      (R_CURLY "}")))
  (ERROR (R_CURLY "}"))
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "B"))
    (CLASS_BODY
      (L_CURLY "{")
      (R_CURLY "}"))))
(errors (error "unmatched '}'" "2:1..2:2"))
