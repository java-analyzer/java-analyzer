(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "Box"))
    (TYPE_PARAMS
      (L_ANGLE "<")
      (TYPE_PARAM (NAME (IDENT "T")))
      (COMMA ",")
      (TYPE_PARAM
        (NAME (IDENT "U"))
        (EXTENDS_KW "extends")
        (PATH_TYPE
          (NAME_REF (IDENT "Comparable"))
          (TYPE_ARGS
            (L_ANGLE "<")
            (PATH_TYPE (NAME_REF (IDENT "U")))
            (R_ANGLE ">")))
        (AMP "&")
        (PATH_TYPE (NAME_REF (IDENT "Serializable"))))
      (R_ANGLE ">"))
    (CLASS_BODY
      (L_CURLY "{")
      (R_CURLY "}"))))
