(SOURCE_FILE
  (CLASS_DECL
    (MODIFIER_LIST
      (ANNOTATION
        (AT "@")
        (PATH (NAME_REF (IDENT "Anno")))
        (ANNOTATION_ARGS
          (L_PAREN "(")
          (ANNOTATION_ARG
            (CLASS_LITERAL
              (NAME_REF_EXPR (NAME_REF (IDENT "Foo")))
              (DOT ".")
              (CLASS_KW "class")))
          (R_PAREN ")")))
      (ANNOTATION
        (AT "@")
        (PATH (NAME_REF (IDENT "Anno")))
        (ANNOTATION_ARGS
          (L_PAREN "(")
          (ANNOTATION_ARG
            (NAME (IDENT "value"))
            (EQ "=")
            (ARRAY_INIT
              (L_CURLY "{")
              (CLASS_LITERAL
                (NAME_REF_EXPR (NAME_REF (IDENT "A")))
                (DOT ".")
                (CLASS_KW "class"))
              (COMMA ",")
              (CLASS_LITERAL
                (NAME_REF_EXPR (NAME_REF (IDENT "B")))
                (DOT ".")
                (CLASS_KW "class"))
              (R_CURLY "}")))
          (COMMA ",")
          (ANNOTATION_ARG
            (NAME (IDENT "other"))
            (EQ "=")
            (ANNOTATION
              (AT "@")
              (PATH (NAME_REF (IDENT "Nested")))
              (ANNOTATION_ARGS
                (L_PAREN "(")
                (ANNOTATION_ARG
                  (CLASS_LITERAL
                    (NAME_REF_EXPR (NAME_REF (IDENT "C")))
                    (DOT ".")
                    (CLASS_KW "class")))
                (R_PAREN ")"))))
          (R_PAREN ")"))))
    (CLASS_KW "class")
    (NAME (IDENT "A"))
    (CLASS_BODY
      (L_CURLY "{")
      (R_CURLY "}"))))
