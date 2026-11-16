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
          (PATH_TYPE (NAME_REF (IDENT "String")))
          (R_ANGLE ">"))))
    (IMPLEMENTS_CLAUSE
      (IMPLEMENTS_KW "implements")
      (PATH_TYPE (NAME_REF (IDENT "C")))
      (COMMA ",")
      (PATH_TYPE
        (NAME_REF (IDENT "D"))
        (DOT ".")
        (NAME_REF (IDENT "E"))
        (TYPE_ARGS
          (L_ANGLE "<")
          (PATH_TYPE (NAME_REF (IDENT "F")))
          (R_ANGLE ">"))))
    (CLASS_BODY
      (L_CURLY "{")
      (R_CURLY "}")))
  (INTERFACE_DECL
    (INTERFACE_KW "interface")
    (NAME (IDENT "J"))
    (EXTENDS_CLAUSE
      (EXTENDS_KW "extends")
      (PATH_TYPE (NAME_REF (IDENT "K")))
      (COMMA ",")
      (PATH_TYPE (NAME_REF (IDENT "L"))))
    (CLASS_BODY
      (L_CURLY "{")
      (R_CURLY "}")))
  (ENUM_DECL
    (ENUM_KW "enum")
    (NAME (IDENT "E"))
    (IMPLEMENTS_CLAUSE
      (IMPLEMENTS_KW "implements")
      (PATH_TYPE (NAME_REF (IDENT "I"))))
    (ENUM_BODY
      (L_CURLY "{")
      (R_CURLY "}")))
  (RECORD_DECL
    (IDENT "record")
    (NAME (IDENT "R"))
    (TYPE_PARAMS
      (L_ANGLE "<")
      (TYPE_PARAM (NAME (IDENT "T")))
      (R_ANGLE ">"))
    (RECORD_COMPONENT_LIST
      (L_PAREN "(")
      (RECORD_COMPONENT
        (PATH_TYPE (NAME_REF (IDENT "T")))
        (NAME (IDENT "value")))
      (R_PAREN ")"))
    (IMPLEMENTS_CLAUSE
      (IMPLEMENTS_KW "implements")
      (PATH_TYPE
        (NAME_REF (IDENT "Comparable"))
        (TYPE_ARGS
          (L_ANGLE "<")
          (PATH_TYPE
            (NAME_REF (IDENT "R"))
            (TYPE_ARGS
              (L_ANGLE "<")
              (PATH_TYPE (NAME_REF (IDENT "T")))
              (R_ANGLE ">")))
          (R_ANGLE ">"))))
    (CLASS_BODY
      (L_CURLY "{")
      (R_CURLY "}"))))
