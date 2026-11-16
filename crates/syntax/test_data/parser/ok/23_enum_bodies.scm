(SOURCE_FILE
  (ENUM_DECL
    (ENUM_KW "enum")
    (NAME (IDENT "Empty"))
    (ENUM_BODY
      (L_CURLY "{")
      (R_CURLY "}")))
  (ENUM_DECL
    (ENUM_KW "enum")
    (NAME (IDENT "OnlySemicolon"))
    (ENUM_BODY
      (L_CURLY "{")
      (SEMICOLON ";")
      (R_CURLY "}")))
  (ENUM_DECL
    (ENUM_KW "enum")
    (NAME (IDENT "Trailing"))
    (ENUM_BODY
      (L_CURLY "{")
      (ENUM_CONSTANT (NAME (IDENT "A")))
      (COMMA ",")
      (ENUM_CONSTANT (NAME (IDENT "B")))
      (COMMA ",")
      (R_CURLY "}")))
  (ENUM_DECL
    (ENUM_KW "enum")
    (NAME (IDENT "TrailingSemicolon"))
    (ENUM_BODY
      (L_CURLY "{")
      (ENUM_CONSTANT (NAME (IDENT "A")))
      (COMMA ",")
      (ENUM_CONSTANT (NAME (IDENT "B")))
      (COMMA ",")
      (SEMICOLON ";")
      (FIELD_DECL
        (PRIMITIVE_TYPE (INT_KW "int"))
        (VARIABLE_DECLARATOR (NAME (IDENT "x")))
        (SEMICOLON ";"))
      (R_CURLY "}")))
  (ENUM_DECL
    (ENUM_KW "enum")
    (NAME (IDENT "Annotated"))
    (ENUM_BODY
      (L_CURLY "{")
      (ENUM_CONSTANT
        (ANNOTATION
          (AT "@")
          (PATH (NAME_REF (IDENT "Deprecated"))))
        (NAME (IDENT "OLD")))
      (COMMA ",")
      (ENUM_CONSTANT (NAME (IDENT "NEW")))
      (R_CURLY "}")))
  (ENUM_DECL
    (ENUM_KW "enum")
    (NAME (IDENT "WithBodies"))
    (ENUM_BODY
      (L_CURLY "{")
      (ENUM_CONSTANT
        (NAME (IDENT "ONE"))
        (ARG_LIST
          (L_PAREN "(")
          (INT_NUMBER "1")
          (R_PAREN ")"))
        (CLASS_BODY
          (L_CURLY "{")
          (METHOD_DECL
            (PRIMITIVE_TYPE (INT_KW "int"))
            (NAME (IDENT "value"))
            (PARAM_LIST
              (L_PAREN "(")
              (R_PAREN ")"))
            (BLOCK
              (L_CURLY "{")
              (RETURN_STMT
                (RETURN_KW "return")
                (EXPR_PLACEHOLDER (INT_NUMBER "1"))
                (SEMICOLON ";"))
              (R_CURLY "}")))
          (R_CURLY "}")))
      (COMMA ",")
      (ENUM_CONSTANT
        (NAME (IDENT "TWO"))
        (ARG_LIST
          (L_PAREN "(")
          (INT_NUMBER "2")
          (COMMA ",")
          (STRING_LITERAL "\"two\"")
          (R_PAREN ")"))
        (CLASS_BODY
          (L_CURLY "{")
          (R_CURLY "}")))
      (COMMA ",")
      (ENUM_CONSTANT (NAME (IDENT "THREE")))
      (SEMICOLON ";")
      (FIELD_DECL
        (MODIFIER_LIST
          (PRIVATE_KW "private")
          (FINAL_KW "final"))
        (PRIMITIVE_TYPE (INT_KW "int"))
        (VARIABLE_DECLARATOR (NAME (IDENT "x")))
        (SEMICOLON ";"))
      (CONSTRUCTOR_DECL
        (NAME (IDENT "WithBodies"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (EXPR_STMT
            (EXPR_PLACEHOLDER
              (THIS_KW "this")
              (L_PAREN "(")
              (INT_NUMBER "0")
              (R_PAREN ")"))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (CONSTRUCTOR_DECL
        (NAME (IDENT "WithBodies"))
        (PARAM_LIST
          (L_PAREN "(")
          (PARAM
            (PRIMITIVE_TYPE (INT_KW "int"))
            (NAME (IDENT "x")))
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (EXPR_STMT
            (EXPR_PLACEHOLDER
              (THIS_KW "this")
              (DOT ".")
              (IDENT "x")
              (EQ "=")
              (IDENT "x"))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (CONSTRUCTOR_DECL
        (NAME (IDENT "WithBodies"))
        (PARAM_LIST
          (L_PAREN "(")
          (PARAM
            (PRIMITIVE_TYPE (INT_KW "int"))
            (NAME (IDENT "x")))
          (COMMA ",")
          (PARAM
            (PATH_TYPE (NAME_REF (IDENT "String")))
            (NAME (IDENT "s")))
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (EXPR_STMT
            (EXPR_PLACEHOLDER
              (THIS_KW "this")
              (L_PAREN "(")
              (IDENT "x")
              (R_PAREN ")"))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (INT_KW "int"))
        (NAME (IDENT "value"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (RETURN_STMT
            (RETURN_KW "return")
            (EXPR_PLACEHOLDER (IDENT "x"))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (R_CURLY "}"))))
