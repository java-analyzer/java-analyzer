(SOURCE_FILE
  (CLASS_DECL
    (MODIFIER_LIST
      (ANNOTATION
        (AT "@")
        (PATH (NAME_REF (IDENT "Anno")))
        (ANNOTATION_ARGS
          (L_PAREN "(")
          (IDENT "Foo")
          (DOT ".")
          (CLASS_KW "class")
          (R_PAREN ")")))
      (ANNOTATION
        (AT "@")
        (PATH (NAME_REF (IDENT "Anno")))
        (ANNOTATION_ARGS
          (L_PAREN "(")
          (IDENT "value")
          (EQ "=")
          (L_CURLY "{")
          (IDENT "A")
          (DOT ".")
          (CLASS_KW "class")
          (COMMA ",")
          (IDENT "B")
          (DOT ".")
          (CLASS_KW "class")
          (R_CURLY "}")
          (COMMA ",")
          (IDENT "other")
          (EQ "=")
          (AT "@")
          (IDENT "Nested")
          (L_PAREN "(")
          (IDENT "C")
          (DOT ".")
          (CLASS_KW "class")
          (R_PAREN ")")
          (R_PAREN ")"))))
    (CLASS_KW "class")
    (NAME (IDENT "A"))
    (CLASS_BODY
      (L_CURLY "{")
      (R_CURLY "}"))))
