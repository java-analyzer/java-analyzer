(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "A"))
    (CLASS_BODY
      (L_CURLY "{")
      (MEMBER_PLACEHOLDER
        (IDENT "String")
        (IDENT "s")
        (EQ "=")
        (STRING_LITERAL "\"unterminated;")
        (INT_KW "int")
        (IDENT "x")
        (EQ "=")
        (INT_NUMBER "0x")
        (SEMICOLON ";")))))
(errors
  (error "unterminated string literal" "2:16..2:30")
  (error "expected hexadecimal digits" "3:13..3:15")
  (error "expected '}'" "3:16..3:16")
  (error "unterminated block comment" "4:5..6:1"))
