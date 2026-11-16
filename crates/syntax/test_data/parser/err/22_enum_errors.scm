(SOURCE_FILE
  (ENUM_DECL
    (ENUM_KW "enum")
    (NAME (IDENT "A"))
    (ENUM_BODY
      (L_CURLY "{")
      (ENUM_CONSTANT (NAME (IDENT "X")))
      (ENUM_CONSTANT (NAME (IDENT "Y")))
      (COMMA ",")
      (ENUM_CONSTANT (NAME (IDENT "Z")))
      (SEMICOLON ";")
      (FIELD_DECL
        (PRIMITIVE_TYPE (INT_KW "int"))
        (VARIABLE_DECLARATOR (NAME (IDENT "f")))
        (SEMICOLON ";"))
      (R_CURLY "}")))
  (ENUM_DECL
    (ENUM_KW "enum")
    (NAME (IDENT "B"))
    (ENUM_BODY
      (L_CURLY "{")
      (ENUM_CONSTANT (NAME (IDENT "P")))
      (COMMA ",")
      (ENUM_CONSTANT (NAME (IDENT "Q")))
      (FIELD_DECL
        (PRIMITIVE_TYPE (INT_KW "int"))
        (VARIABLE_DECLARATOR (NAME (IDENT "g")))
        (SEMICOLON ";"))
      (R_CURLY "}")))
  (ENUM_DECL
    (ENUM_KW "enum")
    (NAME (IDENT "C"))
    (ENUM_BODY
      (L_CURLY "{")
      (ENUM_CONSTANT
        (NAME (IDENT "M"))
        (ARG_LIST
          (L_PAREN "(")
          (LITERAL (INT_NUMBER "1"))))
      (ENUM_DECL
        (ENUM_KW "enum")
        (NAME (IDENT "D"))
        (ENUM_BODY
          (L_CURLY "{")
          (ENUM_CONSTANT (NAME (IDENT "N")))
          (R_CURLY "}"))))))
(errors
  (error "expected ','" "1:11..1:11")
  (error "expected ';'" "2:14..2:14")
  (error "expected ')'" "3:13..3:13")
  (error "expected ';'" "3:13..3:13")
  (error "expected '}'" "4:13..4:13"))
