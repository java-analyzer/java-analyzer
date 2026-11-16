(SOURCE_FILE
  (PACKAGE_DECL
    (PACKAGE_KW "package")
    (PATH
      (NAME_REF (IDENT "a"))
      (DOT ".")
      (NAME_REF (IDENT "b"))))
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "A"))
    (CLASS_BODY
      (L_CURLY "{")
      (R_CURLY "}"))))
(errors (error "expected ';'" "1:12..1:12"))
