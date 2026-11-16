(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "A"))
    (EXTENDS_CLAUSE (EXTENDS_KW "extends")))
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "B"))
    (TYPE_PARAMS
      (L_ANGLE "<")
      (TYPE_PARAM (NAME (IDENT "T"))))
    (IMPLEMENTS_CLAUSE
      (IMPLEMENTS_KW "implements")
      (PATH_TYPE (NAME_REF (IDENT "C"))))
    (CLASS_BODY
      (L_CURLY "{")
      (R_CURLY "}")))
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "D"))
    (IMPLEMENTS_CLAUSE
      (IMPLEMENTS_KW "implements")
      (PATH_TYPE (NAME_REF (IDENT "E")))
      (COMMA ","))
    (CLASS_BODY
      (L_CURLY "{")
      (R_CURLY "}"))))
(errors
  (error "expected a type" "1:16..1:16")
  (error "expected '{'" "1:16..1:16")
  (error "expected '>'" "2:10..2:10")
  (error "expected a type" "3:22..3:22"))
