(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "A"))
    (CLASS_BODY
      (L_CURLY "{")
      (FIELD_DECL
        (PRIMITIVE_TYPE (INT_KW "int"))
        (VARIABLE_DECLARATOR)
        (SEMICOLON ";"))
      (FIELD_DECL
        (PRIMITIVE_TYPE (INT_KW "int"))
        (VARIABLE_DECLARATOR (NAME (IDENT "y"))))
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "String")))
        (VARIABLE_DECLARATOR (NAME (IDENT "z")))
        (COMMA ",")
        (VARIABLE_DECLARATOR)
        (SEMICOLON ";"))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "ok"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (R_CURLY "}")))
      (R_CURLY "}"))))
(errors
  (error "expected a name" "2:8..2:8")
  (error "expected ';'" "3:10..3:10")
  (error "expected a name" "4:14..4:14"))
