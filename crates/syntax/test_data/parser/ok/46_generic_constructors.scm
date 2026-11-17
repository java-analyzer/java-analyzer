(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "GenericConstructors"))
    (CLASS_BODY
      (L_CURLY "{")
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "Object")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "a"))
          (EQ "=")
          (NEW_EXPR
            (NEW_KW "new")
            (TYPE_ARGS
              (L_ANGLE "<")
              (PATH_TYPE (NAME_REF (IDENT "String")))
              (R_ANGLE ">"))
            (PATH_TYPE (NAME_REF (IDENT "Foo")))
            (ARG_LIST
              (L_PAREN "(")
              (R_PAREN ")"))))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "Object")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "b"))
          (EQ "=")
          (NEW_EXPR
            (NEW_KW "new")
            (TYPE_ARGS
              (L_ANGLE "<")
              (PATH_TYPE (NAME_REF (IDENT "String")))
              (COMMA ",")
              (PATH_TYPE (NAME_REF (IDENT "Integer")))
              (R_ANGLE ">"))
            (PATH_TYPE
              (NAME_REF (IDENT "Foo"))
              (TYPE_ARGS
                (L_ANGLE "<")
                (PATH_TYPE (NAME_REF (IDENT "Long")))
                (R_ANGLE ">")))
            (ARG_LIST
              (L_PAREN "(")
              (LITERAL (INT_NUMBER "1"))
              (COMMA ",")
              (LITERAL (INT_NUMBER "2"))
              (R_PAREN ")"))))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "Object")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "c"))
          (EQ "=")
          (NEW_EXPR
            (NEW_KW "new")
            (TYPE_ARGS
              (L_ANGLE "<")
              (PATH_TYPE
                (ANNOTATION
                  (AT "@")
                  (PATH (NAME_REF (IDENT "A"))))
                (NAME_REF (IDENT "String")))
              (R_ANGLE ">"))
            (PATH_TYPE
              (ANNOTATION
                (AT "@")
                (PATH (NAME_REF (IDENT "B"))))
              (NAME_REF (IDENT "Foo"))
              (TYPE_ARGS
                (L_ANGLE "<")
                (PATH_TYPE
                  (ANNOTATION
                    (AT "@")
                    (PATH (NAME_REF (IDENT "C"))))
                  (NAME_REF (IDENT "Bar")))
                (R_ANGLE ">")))
            (ARG_LIST
              (L_PAREN "(")
              (R_PAREN ")"))))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "Object")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "d"))
          (EQ "=")
          (NEW_EXPR
            (NAME_REF_EXPR (NAME_REF (IDENT "outer")))
            (DOT ".")
            (NEW_KW "new")
            (TYPE_ARGS
              (L_ANGLE "<")
              (PATH_TYPE (NAME_REF (IDENT "String")))
              (R_ANGLE ">"))
            (PATH_TYPE (NAME_REF (IDENT "Inner")))
            (ARG_LIST
              (L_PAREN "(")
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (R_PAREN ")"))))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "Object")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "e"))
          (EQ "=")
          (NEW_EXPR
            (NEW_KW "new")
            (TYPE_ARGS
              (L_ANGLE "<")
              (PATH_TYPE (NAME_REF (IDENT "String")))
              (R_ANGLE ">"))
            (PATH_TYPE
              (NAME_REF (IDENT "Outer"))
              (TYPE_ARGS
                (L_ANGLE "<")
                (PATH_TYPE (NAME_REF (IDENT "Integer")))
                (R_ANGLE ">"))
              (DOT ".")
              (NAME_REF (IDENT "Inner"))
              (TYPE_ARGS
                (L_ANGLE "<")
                (PATH_TYPE (NAME_REF (IDENT "Boolean")))
                (R_ANGLE ">")))
            (ARG_LIST
              (L_PAREN "(")
              (R_PAREN ")"))
            (CLASS_BODY
              (L_CURLY "{")
              (R_CURLY "}"))))
        (SEMICOLON ";"))
      (R_CURLY "}"))))
