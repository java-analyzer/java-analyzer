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
            (BINARY_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (L_ANGLE "<")
              (LITERAL (INT_NUMBER "0")))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (THROW_STMT
                (THROW_KW "throw")
                (NEW_EXPR
                  (NEW_KW "new")
                  (PATH_TYPE (NAME_REF (IDENT "IllegalArgumentException")))
                  (ARG_LIST
                    (L_PAREN "(")
                    (R_PAREN ")")))
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
            (CALL_EXPR
              (THIS_EXPR (THIS_KW "this"))
              (ARG_LIST
                (L_PAREN "(")
                (NAME_REF_EXPR (NAME_REF (IDENT "x")))
                (COMMA ",")
                (LITERAL (INT_NUMBER "0"))
                (R_PAREN ")")))
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
            (NEW_EXPR
              (NEW_KW "new")
              (PATH_TYPE (NAME_REF (IDENT "Point")))
              (ARG_LIST
                (L_PAREN "(")
                (LITERAL (INT_NUMBER "0"))
                (COMMA ",")
                (LITERAL (INT_NUMBER "0"))
                (R_PAREN ")")))
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
            (METHOD_CALL_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "Math")))
              (DOT ".")
              (NAME_REF (IDENT "sqrt"))
              (ARG_LIST
                (L_PAREN "(")
                (BINARY_EXPR
                  (BINARY_EXPR
                    (NAME_REF_EXPR (NAME_REF (IDENT "x")))
                    (STAR "*")
                    (NAME_REF_EXPR (NAME_REF (IDENT "x"))))
                  (PLUS "+")
                  (BINARY_EXPR
                    (NAME_REF_EXPR (NAME_REF (IDENT "y")))
                    (STAR "*")
                    (NAME_REF_EXPR (NAME_REF (IDENT "y")))))
                (R_PAREN ")")))
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
