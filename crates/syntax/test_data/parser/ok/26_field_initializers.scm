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
          (LITERAL (INT_NUMBER "1")))
        (COMMA ",")
        (VARIABLE_DECLARATOR
          (NAME (IDENT "b"))
          (EQ "=")
          (CALL_EXPR
            (NAME_REF_EXPR (NAME_REF (IDENT "foo")))
            (ARG_LIST
              (L_PAREN "(")
              (LITERAL (INT_NUMBER "1"))
              (COMMA ",")
              (LITERAL (INT_NUMBER "2"))
              (R_PAREN ")"))))
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
          (NEW_EXPR
            (NEW_KW "new")
            (PATH_TYPE
              (NAME_REF (IDENT "HashMap"))
              (TYPE_ARGS
                (L_ANGLE "<")
                (PATH_TYPE (NAME_REF (IDENT "String")))
                (COMMA ",")
                (PATH_TYPE (NAME_REF (IDENT "Integer")))
                (R_ANGLE ">")))
            (ARG_LIST
              (L_PAREN "(")
              (R_PAREN ")"))))
        (COMMA ",")
        (VARIABLE_DECLARATOR
          (NAME (IDENT "n"))
          (EQ "=")
          (LITERAL (NULL_KW "null")))
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
          (NEW_EXPR
            (NEW_KW "new")
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
            (ARG_LIST
              (L_PAREN "(")
              (R_PAREN ")"))))
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
          (ARRAY_INIT
            (L_CURLY "{")
            (LITERAL (INT_NUMBER "1"))
            (COMMA ",")
            (LITERAL (INT_NUMBER "2"))
            (R_CURLY "}")))
        (COMMA ",")
        (VARIABLE_DECLARATOR
          (NAME (IDENT "empty"))
          (EQ "=")
          (ARRAY_INIT
            (L_CURLY "{")
            (R_CURLY "}")))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "Runnable")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "r"))
          (EQ "=")
          (LAMBDA_EXPR
            (LAMBDA_PARAMS
              (L_PAREN "(")
              (R_PAREN ")"))
            (ARROW "->")
            (BLOCK
              (L_CURLY "{")
              (LOCAL_VAR_DECL
                (PRIMITIVE_TYPE (INT_KW "int"))
                (VARIABLE_DECLARATOR
                  (NAME (IDENT "x"))
                  (EQ "=")
                  (LITERAL (INT_NUMBER "1")))
                (SEMICOLON ";"))
              (EXPR_STMT
                (CALL_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "run")))
                  (ARG_LIST
                    (L_PAREN "(")
                    (R_PAREN ")")))
                (SEMICOLON ";"))
              (R_CURLY "}"))))
        (COMMA ",")
        (VARIABLE_DECLARATOR
          (NAME (IDENT "s"))
          (EQ "=")
          (LAMBDA_EXPR
            (LAMBDA_PARAMS
              (L_PAREN "(")
              (R_PAREN ")"))
            (ARROW "->")
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}"))))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "Object")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "o"))
          (EQ "=")
          (NEW_EXPR
            (NEW_KW "new")
            (PATH_TYPE (NAME_REF (IDENT "Object")))
            (ARG_LIST
              (L_PAREN "(")
              (R_PAREN ")"))
            (CLASS_BODY
              (L_CURLY "{")
              (FIELD_DECL
                (PRIMITIVE_TYPE (INT_KW "int"))
                (VARIABLE_DECLARATOR (NAME (IDENT "f")))
                (SEMICOLON ";"))
              (METHOD_DECL
                (PRIMITIVE_TYPE (VOID_KW "void"))
                (NAME (IDENT "g"))
                (PARAM_LIST
                  (L_PAREN "(")
                  (R_PAREN ")"))
                (BLOCK
                  (L_CURLY "{")
                  (EXPR_STMT
                    (POSTFIX_EXPR
                      (NAME_REF_EXPR (NAME_REF (IDENT "f")))
                      (PLUS2 "++"))
                    (SEMICOLON ";"))
                  (R_CURLY "}")))
              (R_CURLY "}"))))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PRIMITIVE_TYPE (BOOLEAN_KW "boolean"))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "cmp"))
          (EQ "=")
          (BINARY_EXPR
            (LITERAL (INT_NUMBER "1"))
            (L_ANGLE "<")
            (LITERAL (INT_NUMBER "2"))))
        (COMMA ",")
        (VARIABLE_DECLARATOR
          (NAME (IDENT "other"))
          (EQ "=")
          (BINARY_EXPR
            (LITERAL (INT_NUMBER "3"))
            (R_ANGLE ">")
            (LITERAL (INT_NUMBER "2"))))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "String")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "text"))
          (EQ "=")
          (LITERAL (TEXT_BLOCK "\"\"\"\n        multi; line, with = stuff\n        \"\"\"")))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PRIMITIVE_TYPE (INT_KW "int"))
        (VARIABLE_DECLARATOR (NAME (IDENT "after")))
        (SEMICOLON ";"))
      (R_CURLY "}"))))
