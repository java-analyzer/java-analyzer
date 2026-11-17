(SOURCE_FILE
  (IMPORT_DECL
    (IMPORT_KW "import")
    (PATH
      (NAME_REF (IDENT "com"))
      (DOT ".")
      (NAME_REF (IDENT "example"))
      (DOT ".")
      (NAME_REF (IDENT "Service")))
    (SEMICOLON ";"))
  (IMPORT_DECL
    (IMPORT_KW "import")
    (PATH
      (NAME_REF (IDENT "com"))
      (DOT ".")
      (NAME_REF (IDENT "example"))
      (DOT ".")
      (NAME_REF (IDENT "impl")))
    (DOT ".")
    (STAR "*")
    (SEMICOLON ";"))
  (MODULE_DECL
    (MODIFIER_LIST
      (ANNOTATION
        (AT "@")
        (PATH (NAME_REF (IDENT "Deprecated")))
        (ANNOTATION_ARGS
          (L_PAREN "(")
          (ANNOTATION_ARG
            (NAME (IDENT "since"))
            (EQ "=")
            (LITERAL (STRING_LITERAL "\"21\"")))
          (R_PAREN ")"))))
    (IDENT "open")
    (IDENT "module")
    (PATH
      (NAME_REF (IDENT "com"))
      (DOT ".")
      (NAME_REF (IDENT "example"))
      (DOT ".")
      (NAME_REF (IDENT "app")))
    (MODULE_BODY
      (L_CURLY "{")
      (MODULE_DIRECTIVE
        (IDENT "requires")
        (PATH
          (NAME_REF (IDENT "java"))
          (DOT ".")
          (NAME_REF (IDENT "base")))
        (SEMICOLON ";"))
      (MODULE_DIRECTIVE
        (IDENT "requires")
        (STATIC_KW "static")
        (PATH
          (NAME_REF (IDENT "java"))
          (DOT ".")
          (NAME_REF (IDENT "compiler")))
        (SEMICOLON ";"))
      (MODULE_DIRECTIVE
        (IDENT "requires")
        (IDENT "transitive")
        (PATH
          (NAME_REF (IDENT "java"))
          (DOT ".")
          (NAME_REF (IDENT "logging")))
        (SEMICOLON ";"))
      (MODULE_DIRECTIVE
        (IDENT "requires")
        (STATIC_KW "static")
        (IDENT "transitive")
        (PATH
          (NAME_REF (IDENT "java"))
          (DOT ".")
          (NAME_REF (IDENT "sql")))
        (SEMICOLON ";"))
      (MODULE_DIRECTIVE
        (IDENT "requires")
        (PATH (NAME_REF (IDENT "transitive")))
        (SEMICOLON ";"))
      (MODULE_DIRECTIVE
        (IDENT "requires")
        (PATH
          (NAME_REF (IDENT "transitive"))
          (DOT ".")
          (NAME_REF (IDENT "helper")))
        (SEMICOLON ";"))
      (MODULE_DIRECTIVE
        (IDENT "exports")
        (PATH
          (NAME_REF (IDENT "com"))
          (DOT ".")
          (NAME_REF (IDENT "example"))
          (DOT ".")
          (NAME_REF (IDENT "api")))
        (SEMICOLON ";"))
      (MODULE_DIRECTIVE
        (IDENT "exports")
        (PATH
          (NAME_REF (IDENT "com"))
          (DOT ".")
          (NAME_REF (IDENT "example"))
          (DOT ".")
          (NAME_REF (IDENT "internal")))
        (IDENT "to")
        (PATH
          (NAME_REF (IDENT "com"))
          (DOT ".")
          (NAME_REF (IDENT "example"))
          (DOT ".")
          (NAME_REF (IDENT "test")))
        (COMMA ",")
        (PATH
          (NAME_REF (IDENT "com"))
          (DOT ".")
          (NAME_REF (IDENT "example"))
          (DOT ".")
          (NAME_REF (IDENT "tools")))
        (SEMICOLON ";"))
      (MODULE_DIRECTIVE
        (IDENT "opens")
        (PATH
          (NAME_REF (IDENT "com"))
          (DOT ".")
          (NAME_REF (IDENT "example"))
          (DOT ".")
          (NAME_REF (IDENT "model")))
        (SEMICOLON ";"))
      (MODULE_DIRECTIVE
        (IDENT "opens")
        (PATH
          (NAME_REF (IDENT "com"))
          (DOT ".")
          (NAME_REF (IDENT "example"))
          (DOT ".")
          (NAME_REF (IDENT "secret")))
        (IDENT "to")
        (PATH
          (NAME_REF (IDENT "com"))
          (DOT ".")
          (NAME_REF (IDENT "example"))
          (DOT ".")
          (NAME_REF (IDENT "framework")))
        (SEMICOLON ";"))
      (MODULE_DIRECTIVE
        (IDENT "uses")
        (PATH (NAME_REF (IDENT "Service")))
        (SEMICOLON ";"))
      (MODULE_DIRECTIVE
        (IDENT "uses")
        (PATH
          (NAME_REF (IDENT "com"))
          (DOT ".")
          (NAME_REF (IDENT "example"))
          (DOT ".")
          (NAME_REF (IDENT "spi"))
          (DOT ".")
          (NAME_REF (IDENT "Plugin")))
        (SEMICOLON ";"))
      (MODULE_DIRECTIVE
        (IDENT "provides")
        (PATH (NAME_REF (IDENT "Service")))
        (IDENT "with")
        (PATH (NAME_REF (IDENT "DefaultService")))
        (COMMA ",")
        (PATH (NAME_REF (IDENT "ExtraService")))
        (SEMICOLON ";"))
      (R_CURLY "}"))))
