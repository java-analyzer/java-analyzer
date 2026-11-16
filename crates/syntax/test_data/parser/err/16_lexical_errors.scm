(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "A"))
    (CLASS_BODY
      (L_CURLY "{")
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "String")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "s"))
          (EQ "=")
          (EXPR_PLACEHOLDER
            (STRING_LITERAL "\"unterminated;")
            (INT_KW "int")
            (IDENT "x")
            (EQ "=")
            (INT_NUMBER "0x")))
        (SEMICOLON ";")))))
(errors
  (error "unterminated string literal" "2:16..2:30")
  (error "expected hexadecimal digits" "3:13..3:15")
  (error "expected '}'" "3:16..3:16")
  (error "unterminated block comment" "4:5..6:1"))
