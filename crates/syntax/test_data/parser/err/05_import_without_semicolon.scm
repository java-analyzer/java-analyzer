(SOURCE_FILE
  (IMPORT_DECL
    (IMPORT_KW "import")
    (PATH
      (NAME_REF (IDENT "a"))
      (DOT ".")
      (NAME_REF (IDENT "B"))))
  (IMPORT_DECL
    (IMPORT_KW "import")
    (PATH
      (NAME_REF (IDENT "c"))
      (DOT ".")
      (NAME_REF (IDENT "D")))
    (SEMICOLON ";")))
(errors (error "expected ';'" "1:11..1:11"))
