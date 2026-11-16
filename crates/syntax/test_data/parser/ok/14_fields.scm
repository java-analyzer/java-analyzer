(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "A"))
    (CLASS_BODY
      (L_CURLY "{")
      (FIELD_DECL
        (PRIMITIVE_TYPE (INT_KW "int"))
        (VARIABLE_DECLARATOR (NAME (IDENT "x")))
        (SEMICOLON ";"))
      (FIELD_DECL
        (MODIFIER_LIST
          (PRIVATE_KW "private")
          (FINAL_KW "final"))
        (PATH_TYPE (NAME_REF (IDENT "String")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "name"))
          (EQ "=")
          (EXPR_PLACEHOLDER (STRING_LITERAL "\"a\"")))
        (SEMICOLON ";"))
      (FIELD_DECL
        (MODIFIER_LIST (STATIC_KW "static"))
        (ARRAY_TYPE
          (PRIMITIVE_TYPE (INT_KW "int"))
          (L_BRACK "[")
          (R_BRACK "]"))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "values"))
          (EQ "=")
          (EXPR_PLACEHOLDER
            (L_CURLY "{")
            (INT_NUMBER "1")
            (COMMA ",")
            (INT_NUMBER "2")
            (COMMA ",")
            (INT_NUMBER "3")
            (R_CURLY "}")))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PATH_TYPE
          (NAME_REF (IDENT "List"))
          (TYPE_ARGS
            (L_ANGLE "<")
            (PATH_TYPE (NAME_REF (IDENT "String")))
            (R_ANGLE ">")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "list"))
          (EQ "=")
          (EXPR_PLACEHOLDER
            (NEW_KW "new")
            (IDENT "ArrayList")
            (L_ANGLE "<")
            (R_ANGLE ">")
            (L_PAREN "(")
            (R_PAREN ")")))
        (SEMICOLON ";"))
      (R_CURLY "}"))))
