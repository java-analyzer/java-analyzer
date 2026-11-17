(SOURCE_FILE
  (CLASS_DECL
    (MODIFIER_LIST
      (ANNOTATION
        (AT "@")
        (PATH (NAME_REF (IDENT "Anno")))
        (ANNOTATION_ARGS
          (L_PAREN "(")
          (ANNOTATION_ARG
            (NAME (IDENT "x"))
            (EQ "=")
            (LITERAL (STRING_LITERAL "\"a\"")))))
      (PUBLIC_KW "public"))
    (CLASS_KW "class")
    (NAME (IDENT "A"))
    (CLASS_BODY
      (L_CURLY "{")
      (R_CURLY "}"))))
(errors (error "expected ')'" "1:14..1:14"))
