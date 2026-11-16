(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "A"))
    (EXTENDS_CLAUSE
      (EXTENDS_KW "extends")
      (PATH_TYPE
        (NAME_REF (IDENT "B"))
        (TYPE_ARGS
          (L_ANGLE "<")
          (WILDCARD_TYPE
            (QUESTION "?")
            (EXTENDS_KW "extends")
            (PATH_TYPE (NAME_REF (IDENT "C"))))
          (COMMA ",")
          (WILDCARD_TYPE
            (QUESTION "?")
            (SUPER_KW "super")
            (PATH_TYPE (NAME_REF (IDENT "D"))))
          (COMMA ",")
          (WILDCARD_TYPE (QUESTION "?"))
          (R_ANGLE ">"))))
    (IMPLEMENTS_CLAUSE
      (IMPLEMENTS_KW "implements")
      (PATH_TYPE
        (NAME_REF (IDENT "E"))
        (TYPE_ARGS
          (L_ANGLE "<")
          (ARRAY_TYPE
            (PRIMITIVE_TYPE (INT_KW "int"))
            (L_BRACK "[")
            (R_BRACK "]"))
          (COMMA ",")
          (ARRAY_TYPE
            (PATH_TYPE
              (NAME_REF (IDENT "F"))
              (TYPE_ARGS
                (L_ANGLE "<")
                (PATH_TYPE
                  (NAME_REF (IDENT "G"))
                  (TYPE_ARGS
                    (L_ANGLE "<")
                    (PATH_TYPE (NAME_REF (IDENT "String")))
                    (R_ANGLE ">")))
                (R_ANGLE ">")))
            (L_BRACK "[")
            (R_BRACK "]"))
          (R_ANGLE ">"))))
    (CLASS_BODY
      (L_CURLY "{")
      (R_CURLY "}"))))
