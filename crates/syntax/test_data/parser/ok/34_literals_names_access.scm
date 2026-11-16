(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "A"))
    (CLASS_BODY
      (L_CURLY "{")
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "Object")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "a"))
          (EQ "=")
          (LITERAL (INT_NUMBER "1")))
        (COMMA ",")
        (VARIABLE_DECLARATOR
          (NAME (IDENT "b"))
          (EQ "=")
          (LITERAL (LONG_NUMBER "2L")))
        (COMMA ",")
        (VARIABLE_DECLARATOR
          (NAME (IDENT "c"))
          (EQ "=")
          (LITERAL (DOUBLE_NUMBER "1.5")))
        (COMMA ",")
        (VARIABLE_DECLARATOR
          (NAME (IDENT "d"))
          (EQ "=")
          (LITERAL (FLOAT_NUMBER "1.5f")))
        (COMMA ",")
        (VARIABLE_DECLARATOR
          (NAME (IDENT "e"))
          (EQ "=")
          (LITERAL (CHAR_LITERAL "'x'")))
        (COMMA ",")
        (VARIABLE_DECLARATOR
          (NAME (IDENT "f"))
          (EQ "=")
          (LITERAL (STRING_LITERAL "\"s\"")))
        (COMMA ",")
        (VARIABLE_DECLARATOR
          (NAME (IDENT "g"))
          (EQ "=")
          (LITERAL (TRUE_KW "true")))
        (COMMA ",")
        (VARIABLE_DECLARATOR
          (NAME (IDENT "h"))
          (EQ "=")
          (LITERAL (FALSE_KW "false")))
        (COMMA ",")
        (VARIABLE_DECLARATOR
          (NAME (IDENT "i"))
          (EQ "=")
          (LITERAL (NULL_KW "null")))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "Object")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "text"))
          (EQ "=")
          (LITERAL (TEXT_BLOCK "\"\"\"\n        block\n        \"\"\"")))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "Object")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "names"))
          (EQ "=")
          (NAME_REF_EXPR (NAME_REF (IDENT "x"))))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "Object")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "field"))
          (EQ "=")
          (FIELD_ACCESS_EXPR
            (FIELD_ACCESS_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "a")))
              (DOT ".")
              (NAME_REF (IDENT "b")))
            (DOT ".")
            (NAME_REF (IDENT "c"))))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "Object")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "call"))
          (EQ "=")
          (CALL_EXPR
            (NAME_REF_EXPR (NAME_REF (IDENT "foo")))
            (ARG_LIST
              (L_PAREN "(")
              (R_PAREN ")"))))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "Object")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "args"))
          (EQ "=")
          (CALL_EXPR
            (NAME_REF_EXPR (NAME_REF (IDENT "foo")))
            (ARG_LIST
              (L_PAREN "(")
              (LITERAL (INT_NUMBER "1"))
              (COMMA ",")
              (LITERAL (STRING_LITERAL "\"two\""))
              (COMMA ",")
              (NAME_REF_EXPR (NAME_REF (IDENT "three")))
              (COMMA ",")
              (CALL_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "four")))
                (ARG_LIST
                  (L_PAREN "(")
                  (R_PAREN ")")))
              (R_PAREN ")"))))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "Object")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "chain"))
          (EQ "=")
          (FIELD_ACCESS_EXPR
            (METHOD_CALL_EXPR
              (FIELD_ACCESS_EXPR
                (METHOD_CALL_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "a")))
                  (DOT ".")
                  (NAME_REF (IDENT "b"))
                  (ARG_LIST
                    (L_PAREN "(")
                    (R_PAREN ")")))
                (DOT ".")
                (NAME_REF (IDENT "c")))
              (DOT ".")
              (NAME_REF (IDENT "d"))
              (ARG_LIST
                (L_PAREN "(")
                (NAME_REF_EXPR (NAME_REF (IDENT "x")))
                (R_PAREN ")")))
            (DOT ".")
            (NAME_REF (IDENT "e"))))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "Object")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "generic"))
          (EQ "=")
          (METHOD_CALL_EXPR
            (NAME_REF_EXPR (NAME_REF (IDENT "Collections")))
            (DOT ".")
            (TYPE_ARGS
              (L_ANGLE "<")
              (PATH_TYPE (NAME_REF (IDENT "String")))
              (R_ANGLE ">"))
            (NAME_REF (IDENT "emptyList"))
            (ARG_LIST
              (L_PAREN "(")
              (R_PAREN ")"))))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "Object")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "genericThis"))
          (EQ "=")
          (METHOD_CALL_EXPR
            (THIS_EXPR (THIS_KW "this"))
            (DOT ".")
            (TYPE_ARGS
              (L_ANGLE "<")
              (PATH_TYPE (NAME_REF (IDENT "T")))
              (R_ANGLE ">"))
            (NAME_REF (IDENT "run"))
            (ARG_LIST
              (L_PAREN "(")
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (R_PAREN ")"))))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "Object")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "index"))
          (EQ "=")
          (ARRAY_ACCESS_EXPR
            (ARRAY_ACCESS_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "arr")))
              (L_BRACK "[")
              (LITERAL (INT_NUMBER "0"))
              (R_BRACK "]"))
            (L_BRACK "[")
            (BINARY_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "i")))
              (PLUS "+")
              (LITERAL (INT_NUMBER "1")))
            (R_BRACK "]")))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "Object")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "postfix"))
          (EQ "=")
          (POSTFIX_EXPR
            (NAME_REF_EXPR (NAME_REF (IDENT "i")))
            (PLUS2 "++")))
        (COMMA ",")
        (VARIABLE_DECLARATOR
          (NAME (IDENT "other"))
          (EQ "=")
          (POSTFIX_EXPR
            (NAME_REF_EXPR (NAME_REF (IDENT "j")))
            (MINUS2 "--")))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "Object")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "paren"))
          (EQ "=")
          (PAREN_EXPR
            (L_PAREN "(")
            (NAME_REF_EXPR (NAME_REF (IDENT "a")))
            (R_PAREN ")")))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "Object")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "nestedParens"))
          (EQ "=")
          (PAREN_EXPR
            (L_PAREN "(")
            (PAREN_EXPR
              (L_PAREN "(")
              (NAME_REF_EXPR (NAME_REF (IDENT "a")))
              (R_PAREN ")"))
            (R_PAREN ")")))
        (SEMICOLON ";"))
      (R_CURLY "}"))))
