(SOURCE_FILE
  (ANNOTATION_TYPE_DECL
    (AT "@")
    (INTERFACE_KW "interface")
    (NAME (IDENT "Config"))
    (CLASS_BODY
      (L_CURLY "{")
      (METHOD_DECL
        (PATH_TYPE (NAME_REF (IDENT "String")))
        (NAME (IDENT "name"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (DEFAULT_VALUE
          (DEFAULT_KW "default")
          (EXPR_PLACEHOLDER (STRING_LITERAL "\"x\"")))
        (SEMICOLON ";"))
      (METHOD_DECL
        (ARRAY_TYPE
          (PRIMITIVE_TYPE (INT_KW "int"))
          (L_BRACK "[")
          (R_BRACK "]"))
        (NAME (IDENT "values"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (DEFAULT_VALUE
          (DEFAULT_KW "default")
          (EXPR_PLACEHOLDER
            (L_CURLY "{")
            (INT_NUMBER "1")
            (COMMA ",")
            (INT_NUMBER "2")
            (COMMA ",")
            (INT_NUMBER "3")
            (R_CURLY "}")))
        (SEMICOLON ";"))
      (METHOD_DECL
        (PATH_TYPE
          (NAME_REF (IDENT "Class"))
          (TYPE_ARGS
            (L_ANGLE "<")
            (WILDCARD_TYPE (QUESTION "?"))
            (R_ANGLE ">")))
        (NAME (IDENT "type"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (DEFAULT_VALUE
          (DEFAULT_KW "default")
          (EXPR_PLACEHOLDER
            (IDENT "Object")
            (DOT ".")
            (CLASS_KW "class")))
        (SEMICOLON ";"))
      (METHOD_DECL
        (PATH_TYPE (NAME_REF (IDENT "Level")))
        (NAME (IDENT "level"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (DEFAULT_VALUE
          (DEFAULT_KW "default")
          (EXPR_PLACEHOLDER
            (IDENT "Level")
            (DOT ".")
            (IDENT "INFO")))
        (SEMICOLON ";"))
      (METHOD_DECL
        (ARRAY_TYPE
          (PATH_TYPE (NAME_REF (IDENT "String")))
          (L_BRACK "[")
          (R_BRACK "]"))
        (NAME (IDENT "tags"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (SEMICOLON ";"))
      (R_CURLY "}"))))
