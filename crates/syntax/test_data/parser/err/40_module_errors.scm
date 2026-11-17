(SOURCE_FILE
  (MODULE_DECL
    (IDENT "module")
    (PATH
      (NAME_REF (IDENT "com"))
      (DOT ".")
      (NAME_REF (IDENT "example"))
      (DOT ".")
      (NAME_REF (IDENT "broken")))
    (MODULE_BODY
      (L_CURLY "{")
      (MODULE_DIRECTIVE
        (IDENT "requires")
        (PATH)
        (SEMICOLON ";"))
      (MODULE_DIRECTIVE
        (IDENT "exports")
        (PATH
          (NAME_REF (IDENT "com"))
          (DOT ".")
          (NAME_REF (IDENT "example"))
          (DOT ".")
          (NAME_REF (IDENT "api")))
        (IDENT "to")
        (PATH)
        (SEMICOLON ";"))
      (MODULE_DIRECTIVE
        (IDENT "provides")
        (PATH (NAME_REF (IDENT "Svc")))
        (PATH (NAME_REF (IDENT "impl1")))
        (SEMICOLON ";"))
      (ERROR
        (IDENT "bogus")
        (IDENT "directive")
        (SEMICOLON ";"))
      (MODULE_DIRECTIVE
        (IDENT "uses")
        (PATH (NAME_REF (IDENT "Svc"))))
      (R_CURLY "}"))))
(errors
  (error "expected a name" "2:13..2:13")
  (error "expected a name" "3:31..3:31")
  (error "expected 'with'" "4:17..4:17")
  (error "expected a module directive" "5:5..5:10")
  (error "expected ';'" "6:13..6:13"))
