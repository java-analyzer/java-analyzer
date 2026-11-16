(SOURCE_FILE
  (PACKAGE_DECL
    (PACKAGE_KW "package")
    (PATH (NAME_REF (IDENT "a")))
    (SEMICOLON ";"))
  (IMPORT_DECL
    (IMPORT_KW "import")
    (PATH
      (NAME_REF (IDENT "java"))
      (DOT ".")
      (NAME_REF (IDENT "util"))
      (DOT ".")
      (NAME_REF (IDENT "List")))
    (SEMICOLON ";"))
  (IMPORT_DECL
    (IMPORT_KW "import")
    (STATIC_KW "static")
    (PATH
      (NAME_REF (IDENT "java"))
      (DOT ".")
      (NAME_REF (IDENT "lang"))
      (DOT ".")
      (NAME_REF (IDENT "Math"))
      (DOT ".")
      (NAME_REF (IDENT "max")))
    (SEMICOLON ";"))
  (IMPORT_DECL
    (IMPORT_KW "import")
    (PATH
      (NAME_REF (IDENT "java"))
      (DOT ".")
      (NAME_REF (IDENT "io")))
    (DOT ".")
    (STAR "*")
    (SEMICOLON ";"))
  (IMPORT_DECL
    (IMPORT_KW "import")
    (STATIC_KW "static")
    (PATH
      (NAME_REF (IDENT "org"))
      (DOT ".")
      (NAME_REF (IDENT "junit"))
      (DOT ".")
      (NAME_REF (IDENT "Assert")))
    (DOT ".")
    (STAR "*")
    (SEMICOLON ";")))
