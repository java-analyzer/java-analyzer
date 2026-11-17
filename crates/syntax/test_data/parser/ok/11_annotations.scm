(SOURCE_FILE
  (CLASS_DECL
    (MODIFIER_LIST
      (ANNOTATION
        (AT "@")
        (PATH (NAME_REF (IDENT "Deprecated"))))
      (ANNOTATION
        (AT "@")
        (PATH (NAME_REF (IDENT "SuppressWarnings")))
        (ANNOTATION_ARGS
          (L_PAREN "(")
          (ANNOTATION_ARG
            (ARRAY_INIT
              (L_CURLY "{")
              (LITERAL (STRING_LITERAL "\"a\""))
              (COMMA ",")
              (LITERAL (STRING_LITERAL "\"b\""))
              (R_CURLY "}")))
          (R_PAREN ")")))
      (ANNOTATION
        (AT "@")
        (PATH
          (NAME_REF (IDENT "com"))
          (DOT ".")
          (NAME_REF (IDENT "example"))
          (DOT ".")
          (NAME_REF (IDENT "Marker")))
        (ANNOTATION_ARGS
          (L_PAREN "(")
          (ANNOTATION_ARG
            (NAME (IDENT "name"))
            (EQ "=")
            (LITERAL (STRING_LITERAL "\"x\"")))
          (COMMA ",")
          (ANNOTATION_ARG
            (NAME (IDENT "values"))
            (EQ "=")
            (ARRAY_INIT
              (L_CURLY "{")
              (LITERAL (INT_NUMBER "1"))
              (COMMA ",")
              (LITERAL (INT_NUMBER "2"))
              (R_CURLY "}")))
          (R_PAREN ")")))
      (PUBLIC_KW "public"))
    (CLASS_KW "class")
    (NAME (IDENT "Foo"))
    (CLASS_BODY
      (L_CURLY "{")
      (R_CURLY "}"))))
