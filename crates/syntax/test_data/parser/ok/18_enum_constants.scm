(SOURCE_FILE
  (ENUM_DECL
    (ENUM_KW "enum")
    (NAME (IDENT "Color"))
    (ENUM_BODY
      (L_CURLY "{")
      (ENUM_CONSTANT (NAME (IDENT "RED")))
      (COMMA ",")
      (ENUM_CONSTANT (NAME (IDENT "GREEN")))
      (COMMA ",")
      (ENUM_CONSTANT (NAME (IDENT "BLUE")))
      (R_CURLY "}")))
  (ENUM_DECL
    (ENUM_KW "enum")
    (NAME (IDENT "Planet"))
    (ENUM_BODY
      (L_CURLY "{")
      (ENUM_CONSTANT
        (NAME (IDENT "MERCURY"))
        (ARG_LIST
          (L_PAREN "(")
          (DOUBLE_NUMBER "1.0")
          (R_PAREN ")")))
      (COMMA ",")
      (ENUM_CONSTANT
        (NAME (IDENT "VENUS"))
        (ARG_LIST
          (L_PAREN "(")
          (DOUBLE_NUMBER "2.0")
          (R_PAREN ")"))
        (CLASS_BODY
          (L_CURLY "{")
          (METHOD_DECL
            (PRIMITIVE_TYPE (VOID_KW "void"))
            (NAME (IDENT "f"))
            (PARAM_LIST
              (L_PAREN "(")
              (R_PAREN ")"))
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}")))
          (R_CURLY "}")))
      (SEMICOLON ";")
      (FIELD_DECL
        (MODIFIER_LIST
          (PRIVATE_KW "private")
          (FINAL_KW "final"))
        (PRIMITIVE_TYPE (DOUBLE_KW "double"))
        (VARIABLE_DECLARATOR (NAME (IDENT "mass")))
        (SEMICOLON ";"))
      (CONSTRUCTOR_DECL
        (NAME (IDENT "Planet"))
        (PARAM_LIST
          (L_PAREN "(")
          (PARAM
            (PRIMITIVE_TYPE (DOUBLE_KW "double"))
            (NAME (IDENT "mass")))
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (THIS_KW "this")
          (DOT ".")
          (IDENT "mass")
          (EQ "=")
          (IDENT "mass")
          (SEMICOLON ";")
          (R_CURLY "}")))
      (R_CURLY "}"))))
