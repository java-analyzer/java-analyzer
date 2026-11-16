(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "A"))
    (CLASS_BODY
      (L_CURLY "{")
      (SEMICOLON ";")
      (SEMICOLON ";")
      (FIELD_DECL
        (PRIMITIVE_TYPE (INT_KW "int"))
        (VARIABLE_DECLARATOR (NAME (IDENT "x")))
        (SEMICOLON ";"))
      (SEMICOLON ";")
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "f"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (BLOCK
            (L_CURLY "{")
            (R_CURLY "}"))
          (BLOCK
            (L_CURLY "{")
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}"))
            (R_CURLY "}"))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "g"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (WHILE_STMT
            (WHILE_KW "while")
            (L_PAREN "(")
            (EXPR_PLACEHOLDER (TRUE_KW "true"))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (IF_STMT
                (IF_KW "if")
                (L_PAREN "(")
                (EXPR_PLACEHOLDER (IDENT "x"))
                (R_PAREN ")")
                (BLOCK
                  (L_CURLY "{")
                  (EXPR_STMT
                    (EXPR_PLACEHOLDER
                      (IDENT "y")
                      (L_PAREN "(")
                      (R_PAREN ")"))
                    (SEMICOLON ";"))
                  (R_CURLY "}")))
              (R_CURLY "}")))
          (R_CURLY "}")))
      (CLASS_DECL
        (CLASS_KW "class")
        (NAME (IDENT "Inner"))
        (CLASS_BODY
          (L_CURLY "{")
          (METHOD_DECL
            (PRIMITIVE_TYPE (VOID_KW "void"))
            (NAME (IDENT "h"))
            (PARAM_LIST
              (L_PAREN "(")
              (R_PAREN ")"))
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}")))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "after"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (R_CURLY "}")))
      (R_CURLY "}"))))
