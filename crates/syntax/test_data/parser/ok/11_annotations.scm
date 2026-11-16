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
          (L_CURLY "{")
          (STRING_LITERAL "\"a\"")
          (COMMA ",")
          (STRING_LITERAL "\"b\"")
          (R_CURLY "}")
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
          (IDENT "name")
          (EQ "=")
          (STRING_LITERAL "\"x\"")
          (COMMA ",")
          (IDENT "values")
          (EQ "=")
          (L_CURLY "{")
          (INT_NUMBER "1")
          (COMMA ",")
          (INT_NUMBER "2")
          (R_CURLY "}")
          (R_PAREN ")")))
      (PUBLIC_KW "public"))
    (CLASS_KW "class")
    (NAME (IDENT "Foo"))
    (CLASS_BODY
      (L_CURLY "{")
      (R_CURLY "}"))))
