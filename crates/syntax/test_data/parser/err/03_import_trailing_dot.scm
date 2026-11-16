(SOURCE_FILE
  (IMPORT_DECL
    (IMPORT_KW "import")
    (PATH
      (NAME_REF (IDENT "a"))
      (DOT ".")
      (NAME_REF (IDENT "b"))
      (DOT "."))
    (SEMICOLON ";"))
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "A"))
    (CLASS_BODY
      (L_CURLY "{")
      (R_CURLY "}"))))
(errors (error "expected a name" "1:12..1:12"))
