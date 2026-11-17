(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "TypeAnnotationErrors"))
    (CLASS_BODY
      (L_CURLY "{")
      (FIELD_DECL
        (PATH_TYPE
          (NAME_REF (IDENT "List"))
          (TYPE_ARGS
            (L_ANGLE "<")
            (ERROR
              (ANNOTATION
                (AT "@")
                (PATH (NAME_REF (IDENT "A")))))
            (R_ANGLE ">")))
        (VARIABLE_DECLARATOR (NAME (IDENT "missingTypeAfterAnno")))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PATH_TYPE
          (NAME_REF (IDENT "Outer"))
          (DOT ".")
          (ANNOTATION
            (AT "@")
            (PATH (NAME_REF (IDENT "A")))))
        (VARIABLE_DECLARATOR)
        (SEMICOLON ";"))
      (FIELD_DECL
        (PRIMITIVE_TYPE (INT_KW "int"))
        (VARIABLE_DECLARATOR (NAME (IDENT "x"))))
      (ERROR
        (MODIFIER_LIST
          (ANNOTATION
            (AT "@")
            (PATH (NAME_REF (IDENT "A"))))))
      (SEMICOLON ";")
      (R_CURLY "}"))))
(errors
  (error "expected a type" "2:10..2:10")
  (error "expected a name" "3:11..3:11")
  (error "expected a name" "3:11..3:11")
  (error "expected ';'" "4:8..4:8")
  (error "expected a member declaration" "4:11..4:11"))
