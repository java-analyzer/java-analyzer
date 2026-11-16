(SOURCE_FILE
  (CLASS_DECL
    (MODIFIER_LIST
      (ANNOTATION
        (AT "@")
        (PATH (NAME_REF (IDENT "Anno")))
        (ANNOTATION_ARGS
          (L_PAREN "(")
          (IDENT "x")
          (EQ "=")
          (STRING_LITERAL "\"a\""))))
    (CLASS_KW "class")
    (NAME (IDENT "A"))
    (CLASS_BODY
      (L_CURLY "{")
      (R_CURLY "}"))))
(errors (error "expected ')'" "1:14..1:14"))
