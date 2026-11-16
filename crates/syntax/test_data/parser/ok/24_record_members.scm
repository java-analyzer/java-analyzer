(SOURCE_FILE
  (RECORD_DECL
    (IDENT "record")
    (NAME (IDENT "Point"))
    (RECORD_COMPONENT_LIST
      (L_PAREN "(")
      (RECORD_COMPONENT
        (PRIMITIVE_TYPE (INT_KW "int"))
        (NAME (IDENT "x")))
      (COMMA ",")
      (RECORD_COMPONENT
        (PRIMITIVE_TYPE (INT_KW "int"))
        (NAME (IDENT "y")))
      (R_PAREN ")"))
    (CLASS_BODY
      (L_CURLY "{")
      (CONSTRUCTOR_DECL
        (NAME (IDENT "Point"))
        (BLOCK
          (L_CURLY "{")
          (IF_STMT
            (IF_KW "if")
            (L_PAREN "(")
            (EXPR_PLACEHOLDER
              (IDENT "x")
              (L_ANGLE "<")
              (INT_NUMBER "0"))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (THROW_STMT
                (THROW_KW "throw")
                (EXPR_PLACEHOLDER
                  (NEW_KW "new")
                  (IDENT "IllegalArgumentException")
                  (L_PAREN "(")
                  (R_PAREN ")"))
                (SEMICOLON ";"))
              (R_CURLY "}")))
          (R_CURLY "}")))
      (CONSTRUCTOR_DECL
        (NAME (IDENT "Point"))
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
              (L_PAREN "(")
              (IDENT "x")
              (COMMA ",")
              (INT_NUMBER "0")
              (R_PAREN ")"))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (METHOD_DECL
        (MODIFIER_LIST (STATIC_KW "static"))
        (PATH_TYPE (NAME_REF (IDENT "Point")))
        (NAME (IDENT "origin"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (RETURN_STMT
            (RETURN_KW "return")
            (EXPR_PLACEHOLDER
              (NEW_KW "new")
              (IDENT "Point")
              (L_PAREN "(")
              (INT_NUMBER "0")
              (COMMA ",")
              (INT_NUMBER "0")
              (R_PAREN ")"))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (DOUBLE_KW "double"))
        (NAME (IDENT "dist"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (RETURN_STMT
            (RETURN_KW "return")
            (EXPR_PLACEHOLDER
              (IDENT "Math")
              (DOT ".")
              (IDENT "sqrt")
              (L_PAREN "(")
              (IDENT "x")
              (STAR "*")
              (IDENT "x")
              (PLUS "+")
              (IDENT "y")
              (STAR "*")
              (IDENT "y")
              (R_PAREN ")"))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (R_CURLY "}")))
  (RECORD_DECL
    (IDENT "record")
    (NAME (IDENT "Tagged"))
    (RECORD_COMPONENT_LIST
      (L_PAREN "(")
      (RECORD_COMPONENT
        (MODIFIER_LIST
          (ANNOTATION
            (AT "@")
            (PATH (NAME_REF (IDENT "Nullable")))))
        (PATH_TYPE (NAME_REF (IDENT "String")))
        (NAME (IDENT "tag")))
      (COMMA ",")
      (RECORD_COMPONENT
        (PRIMITIVE_TYPE (INT_KW "int"))
        (DOT3 "...")
        (NAME (IDENT "values")))
      (R_PAREN ")"))
    (CLASS_BODY
      (L_CURLY "{")
      (R_CURLY "}")))
  (RECORD_DECL
    (IDENT "record")
    (NAME (IDENT "Empty"))
    (RECORD_COMPONENT_LIST
      (L_PAREN "(")
      (R_PAREN ")"))
    (CLASS_BODY
      (L_CURLY "{")
      (R_CURLY "}"))))
