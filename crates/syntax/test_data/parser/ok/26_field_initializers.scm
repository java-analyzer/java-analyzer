(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "A"))
    (CLASS_BODY
      (L_CURLY "{")
      (FIELD_DECL
        (PRIMITIVE_TYPE (INT_KW "int"))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "a"))
          (EQ "=")
          (EXPR_PLACEHOLDER (INT_NUMBER "1")))
        (COMMA ",")
        (VARIABLE_DECLARATOR
          (NAME (IDENT "b"))
          (EQ "=")
          (EXPR_PLACEHOLDER
            (IDENT "foo")
            (L_PAREN "(")
            (INT_NUMBER "1")
            (COMMA ",")
            (INT_NUMBER "2")
            (R_PAREN ")")))
        (COMMA ",")
        (VARIABLE_DECLARATOR (NAME (IDENT "c")))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PATH_TYPE
          (NAME_REF (IDENT "Map"))
          (TYPE_ARGS
            (L_ANGLE "<")
            (PATH_TYPE (NAME_REF (IDENT "String")))
            (COMMA ",")
            (PATH_TYPE (NAME_REF (IDENT "Integer")))
            (R_ANGLE ">")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "m"))
          (EQ "=")
          (EXPR_PLACEHOLDER
            (NEW_KW "new")
            (IDENT "HashMap")
            (L_ANGLE "<")
            (IDENT "String")
            (COMMA ",")
            (IDENT "Integer")
            (R_ANGLE ">")
            (L_PAREN "(")
            (R_PAREN ")")))
        (COMMA ",")
        (VARIABLE_DECLARATOR
          (NAME (IDENT "n"))
          (EQ "=")
          (EXPR_PLACEHOLDER (NULL_KW "null")))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PATH_TYPE
          (NAME_REF (IDENT "Triple"))
          (TYPE_ARGS
            (L_ANGLE "<")
            (PATH_TYPE (NAME_REF (IDENT "A")))
            (COMMA ",")
            (PATH_TYPE (NAME_REF (IDENT "B")))
            (COMMA ",")
            (PATH_TYPE (NAME_REF (IDENT "C")))
            (R_ANGLE ">")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "t"))
          (EQ "=")
          (EXPR_PLACEHOLDER
            (NEW_KW "new")
            (IDENT "Triple")
            (L_ANGLE "<")
            (IDENT "A")
            (COMMA ",")
            (IDENT "B")
            (COMMA ",")
            (IDENT "C")
            (R_ANGLE ">")
            (L_PAREN "(")
            (R_PAREN ")")))
        (COMMA ",")
        (VARIABLE_DECLARATOR (NAME (IDENT "u")))
        (SEMICOLON ";"))
      (FIELD_DECL
        (ARRAY_TYPE
          (PRIMITIVE_TYPE (INT_KW "int"))
          (L_BRACK "[")
          (R_BRACK "]"))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "arr"))
          (EQ "=")
          (EXPR_PLACEHOLDER
            (L_CURLY "{")
            (INT_NUMBER "1")
            (COMMA ",")
            (INT_NUMBER "2")
            (R_CURLY "}")))
        (COMMA ",")
        (VARIABLE_DECLARATOR
          (NAME (IDENT "empty"))
          (EQ "=")
          (EXPR_PLACEHOLDER
            (L_CURLY "{")
            (R_CURLY "}")))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "Runnable")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "r"))
          (EQ "=")
          (EXPR_PLACEHOLDER
            (L_PAREN "(")
            (R_PAREN ")")
            (ARROW "->")
            (L_CURLY "{")
            (INT_KW "int")
            (IDENT "x")
            (EQ "=")
            (INT_NUMBER "1")
            (SEMICOLON ";")
            (IDENT "run")
            (L_PAREN "(")
            (R_PAREN ")")
            (SEMICOLON ";")
            (R_CURLY "}")))
        (COMMA ",")
        (VARIABLE_DECLARATOR
          (NAME (IDENT "s"))
          (EQ "=")
          (EXPR_PLACEHOLDER
            (L_PAREN "(")
            (R_PAREN ")")
            (ARROW "->")
            (L_CURLY "{")
            (R_CURLY "}")))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "Object")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "o"))
          (EQ "=")
          (EXPR_PLACEHOLDER
            (NEW_KW "new")
            (IDENT "Object")
            (L_PAREN "(")
            (R_PAREN ")")
            (L_CURLY "{")
            (INT_KW "int")
            (IDENT "f")
            (SEMICOLON ";")
            (VOID_KW "void")
            (IDENT "g")
            (L_PAREN "(")
            (R_PAREN ")")
            (L_CURLY "{")
            (IDENT "f")
            (PLUS2 "++")
            (SEMICOLON ";")
            (R_CURLY "}")
            (R_CURLY "}")))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PRIMITIVE_TYPE (BOOLEAN_KW "boolean"))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "cmp"))
          (EQ "=")
          (EXPR_PLACEHOLDER
            (INT_NUMBER "1")
            (L_ANGLE "<")
            (INT_NUMBER "2")))
        (COMMA ",")
        (VARIABLE_DECLARATOR
          (NAME (IDENT "other"))
          (EQ "=")
          (EXPR_PLACEHOLDER
            (INT_NUMBER "3")
            (R_ANGLE ">")
            (INT_NUMBER "2")))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "String")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "text"))
          (EQ "=")
          (EXPR_PLACEHOLDER (TEXT_BLOCK "\"\"\"\n        multi; line, with = stuff\n        \"\"\"")))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PRIMITIVE_TYPE (INT_KW "int"))
        (VARIABLE_DECLARATOR (NAME (IDENT "after")))
        (SEMICOLON ";"))
      (R_CURLY "}"))))
