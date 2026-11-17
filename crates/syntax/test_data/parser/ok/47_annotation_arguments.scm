(SOURCE_FILE
  (ANNOTATION_TYPE_DECL
    (MODIFIER_LIST
      (ANNOTATION
        (AT "@")
        (PATH (NAME_REF (IDENT "Marker")))
        (ANNOTATION_ARGS
          (L_PAREN "(")
          (R_PAREN ")")))
      (ANNOTATION
        (AT "@")
        (PATH (NAME_REF (IDENT "Single")))
        (ANNOTATION_ARGS
          (L_PAREN "(")
          (ANNOTATION_ARG
            (BINARY_EXPR
              (LITERAL (INT_NUMBER "1"))
              (PLUS "+")
              (BINARY_EXPR
                (LITERAL (INT_NUMBER "2"))
                (STAR "*")
                (LITERAL (INT_NUMBER "3")))))
          (R_PAREN ")")))
      (ANNOTATION
        (AT "@")
        (PATH (NAME_REF (IDENT "Type")))
        (ANNOTATION_ARGS
          (L_PAREN "(")
          (ANNOTATION_ARG
            (CLASS_LITERAL
              (ARRAY_TYPE
                (ARRAY_TYPE
                  (PRIMITIVE_TYPE (INT_KW "int"))
                  (L_BRACK "[")
                  (R_BRACK "]"))
                (L_BRACK "[")
                (R_BRACK "]"))
              (DOT ".")
              (CLASS_KW "class")))
          (R_PAREN ")")))
      (ANNOTATION
        (AT "@")
        (PATH (NAME_REF (IDENT "Outer")))
        (ANNOTATION_ARGS
          (L_PAREN "(")
          (ANNOTATION_ARG
            (ANNOTATION
              (AT "@")
              (PATH (NAME_REF (IDENT "Inner")))
              (ANNOTATION_ARGS
                (L_PAREN "(")
                (ANNOTATION_ARG
                  (NAME (IDENT "value"))
                  (EQ "=")
                  (LITERAL (INT_NUMBER "1")))
                (R_PAREN ")"))))
          (R_PAREN ")")))
      (ANNOTATION
        (AT "@")
        (PATH (NAME_REF (IDENT "Container")))
        (ANNOTATION_ARGS
          (L_PAREN "(")
          (ANNOTATION_ARG
            (ARRAY_INIT
              (L_CURLY "{")
              (ANNOTATION
                (AT "@")
                (PATH (NAME_REF (IDENT "Item")))
                (ANNOTATION_ARGS
                  (L_PAREN "(")
                  (ANNOTATION_ARG (LITERAL (STRING_LITERAL "\"a\"")))
                  (R_PAREN ")")))
              (COMMA ",")
              (ANNOTATION
                (AT "@")
                (PATH (NAME_REF (IDENT "Item")))
                (ANNOTATION_ARGS
                  (L_PAREN "(")
                  (ANNOTATION_ARG (LITERAL (STRING_LITERAL "\"b\"")))
                  (R_PAREN ")")))
              (COMMA ",")
              (R_CURLY "}")))
          (R_PAREN ")")))
      (ANNOTATION
        (AT "@")
        (PATH (NAME_REF (IDENT "Full")))
        (ANNOTATION_ARGS
          (L_PAREN "(")
          (ANNOTATION_ARG
            (NAME (IDENT "a"))
            (EQ "=")
            (LITERAL (INT_NUMBER "1")))
          (COMMA ",")
          (ANNOTATION_ARG
            (NAME (IDENT "b"))
            (EQ "=")
            (LITERAL (STRING_LITERAL "\"x\"")))
          (COMMA ",")
          (ANNOTATION_ARG
            (NAME (IDENT "c"))
            (EQ "=")
            (FIELD_ACCESS_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "Foo")))
              (DOT ".")
              (NAME_REF (IDENT "BAR"))))
          (COMMA ",")
          (ANNOTATION_ARG
            (NAME (IDENT "d"))
            (EQ "=")
            (ARRAY_INIT
              (L_CURLY "{")
              (LITERAL (INT_NUMBER "1"))
              (COMMA ",")
              (LITERAL (INT_NUMBER "2"))
              (R_CURLY "}")))
          (COMMA ",")
          (ANNOTATION_ARG
            (NAME (IDENT "e"))
            (EQ "=")
            (ANNOTATION
              (AT "@")
              (PATH (NAME_REF (IDENT "Nested")))))
          (COMMA ",")
          (ANNOTATION_ARG
            (NAME (IDENT "f"))
            (EQ "=")
            (ANNOTATION
              (AT "@")
              (PATH (NAME_REF (IDENT "Nested")))
              (ANNOTATION_ARGS
                (L_PAREN "(")
                (ANNOTATION_ARG
                  (NAME (IDENT "k"))
                  (EQ "=")
                  (CLASS_LITERAL
                    (NAME_REF_EXPR (NAME_REF (IDENT "String")))
                    (DOT ".")
                    (CLASS_KW "class")))
                (R_PAREN ")"))))
          (R_PAREN ")"))))
    (AT "@")
    (INTERFACE_KW "interface")
    (NAME (IDENT "AnnoDefaults"))
    (CLASS_BODY
      (L_CURLY "{")
      (METHOD_DECL
        (PATH_TYPE (NAME_REF (IDENT "Nested")))
        (NAME (IDENT "single"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (DEFAULT_VALUE
          (DEFAULT_KW "default")
          (ANNOTATION
            (AT "@")
            (PATH (NAME_REF (IDENT "Nested")))
            (ANNOTATION_ARGS
              (L_PAREN "(")
              (ANNOTATION_ARG
                (NAME (IDENT "k"))
                (EQ "=")
                (CLASS_LITERAL
                  (NAME_REF_EXPR (NAME_REF (IDENT "Object")))
                  (DOT ".")
                  (CLASS_KW "class")))
              (R_PAREN ")"))))
        (SEMICOLON ";"))
      (METHOD_DECL
        (ARRAY_TYPE
          (PATH_TYPE (NAME_REF (IDENT "Nested")))
          (L_BRACK "[")
          (R_BRACK "]"))
        (NAME (IDENT "many"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (DEFAULT_VALUE
          (DEFAULT_KW "default")
          (ARRAY_INIT
            (L_CURLY "{")
            (ANNOTATION
              (AT "@")
              (PATH (NAME_REF (IDENT "Nested"))))
            (COMMA ",")
            (ANNOTATION
              (AT "@")
              (PATH (NAME_REF (IDENT "Nested")))
              (ANNOTATION_ARGS
                (L_PAREN "(")
                (ANNOTATION_ARG
                  (NAME (IDENT "k"))
                  (EQ "=")
                  (CLASS_LITERAL
                    (PRIMITIVE_TYPE (VOID_KW "void"))
                    (DOT ".")
                    (CLASS_KW "class")))
                (R_PAREN ")")))
            (R_CURLY "}")))
        (SEMICOLON ";"))
      (R_CURLY "}"))))
