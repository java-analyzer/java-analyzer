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
          (LITERAL (STRING_LITERAL "\"unterminated;"))))
      (FIELD_DECL
        (PRIMITIVE_TYPE (INT_KW "int"))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "x"))
          (EQ "=")
          (LITERAL (INT_NUMBER "0x")))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "String")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "tb"))
          (EQ "=")
          (LITERAL (TEXT_BLOCK "\"\"\"\n        bad \\q escape\n        \"\"\"")))
        (SEMICOLON ";")))))
(errors
  (error "unterminated string literal" "2:16..2:30")
  (error "expected ';'" "2:30..2:30")
  (error "expected hexadecimal digits" "3:13..3:15")
  (error "invalid escape sequence" "4:17..6:12")
  (error "expected '}'" "6:13..6:13")
  (error "unterminated block comment" "7:5..9:1"))
