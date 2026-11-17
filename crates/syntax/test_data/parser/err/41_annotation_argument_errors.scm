(SOURCE_FILE
  (CLASS_DECL
    (MODIFIER_LIST
      (ANNOTATION
        (AT "@")
        (PATH (NAME_REF (IDENT "Anno")))
        (ANNOTATION_ARGS
          (L_PAREN "(")
          (ANNOTATION_ARG
            (NAME (IDENT "x"))
            (EQ "="))
          (R_PAREN ")")))
      (ANNOTATION
        (AT "@")
        (PATH (NAME_REF (IDENT "Anno")))
        (ANNOTATION_ARGS
          (L_PAREN "(")
          (ANNOTATION_ARG
            (NAME (IDENT "x"))
            (EQ "=")
            (LITERAL (INT_NUMBER "1")))
          (COMMA ",")
          (R_PAREN ")")))
      (ANNOTATION
        (AT "@")
        (PATH (NAME_REF (IDENT "Anno")))
        (ANNOTATION_ARGS
          (L_PAREN "(")
          (COMMA ",")
          (ANNOTATION_ARG
            (NAME (IDENT "x"))
            (EQ "=")
            (LITERAL (INT_NUMBER "1")))
          (R_PAREN ")")))
      (ANNOTATION
        (AT "@")
        (PATH (NAME_REF (IDENT "Anno")))
        (ANNOTATION_ARGS
          (L_PAREN "(")
          (ANNOTATION_ARG
            (NAME (IDENT "xs"))
            (EQ "=")
            (ARRAY_INIT
              (L_CURLY "{")
              (LITERAL (INT_NUMBER "1"))
              (COMMA ",")
              (LITERAL (INT_NUMBER "2"))))
          (R_PAREN ")")))
      (ANNOTATION
        (AT "@")
        (PATH (NAME_REF (IDENT "Anno")))
        (ANNOTATION_ARGS
          (L_PAREN "(")
          (ERROR (QUESTION "?"))
          (ANNOTATION_ARG (LITERAL (INT_NUMBER "1")))
          (R_PAREN ")"))))
    (CLASS_KW "class")
    (NAME (IDENT "AnnotationArgErrors"))
    (CLASS_BODY
      (L_CURLY "{")
      (R_CURLY "}"))))
(errors
  (error "expected an expression" "1:10..1:10")
  (error "expected an expression" "2:13..2:13")
  (error "expected an expression" "3:7..3:7")
  (error "expected '}'" "4:17..4:17")
  (error "expected an expression" "5:7..5:7"))
