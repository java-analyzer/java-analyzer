(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "A"))
    (CLASS_BODY
      (L_CURLY "{")
      (CONSTRUCTOR_DECL
        (NAME (IDENT "A"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (R_CURLY "}")))
      (METHOD_DECL
        (MODIFIER_LIST (PUBLIC_KW "public"))
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "run"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
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
      (METHOD_DECL
        (MODIFIER_LIST (ABSTRACT_KW "abstract"))
        (PRIMITIVE_TYPE (INT_KW "int"))
        (NAME (IDENT "size"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (SEMICOLON ";"))
      (METHOD_DECL
        (TYPE_PARAMS
          (L_ANGLE "<")
          (TYPE_PARAM (NAME (IDENT "T")))
          (R_ANGLE ">"))
        (PATH_TYPE (NAME_REF (IDENT "T")))
        (NAME (IDENT "pick"))
        (PARAM_LIST
          (L_PAREN "(")
          (PARAM
            (PATH_TYPE (NAME_REF (IDENT "T")))
            (NAME (IDENT "a")))
          (COMMA ",")
          (PARAM
            (PATH_TYPE (NAME_REF (IDENT "T")))
            (NAME (IDENT "b")))
          (R_PAREN ")"))
        (THROWS_CLAUSE
          (THROWS_KW "throws")
          (PATH_TYPE (NAME_REF (IDENT "Exception"))))
        (BLOCK
          (L_CURLY "{")
          (RETURN_STMT
            (RETURN_KW "return")
            (EXPR_PLACEHOLDER (IDENT "a"))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (R_CURLY "}"))))
