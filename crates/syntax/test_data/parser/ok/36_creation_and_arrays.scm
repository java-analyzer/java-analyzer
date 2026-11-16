(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "A"))
    (CLASS_BODY
      (L_CURLY "{")
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "Object")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "o1"))
          (EQ "=")
          (NEW_EXPR
            (NEW_KW "new")
            (PATH_TYPE (NAME_REF (IDENT "Object")))
            (ARG_LIST
              (L_PAREN "(")
              (R_PAREN ")"))))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "Object")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "o2"))
          (EQ "=")
          (NEW_EXPR
            (NEW_KW "new")
            (PATH_TYPE (NAME_REF (IDENT "Foo")))
            (ARG_LIST
              (L_PAREN "(")
              (LITERAL (INT_NUMBER "1"))
              (COMMA ",")
              (LITERAL (INT_NUMBER "2"))
              (R_PAREN ")"))))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "Object")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "o3"))
          (EQ "=")
          (NEW_EXPR
            (NEW_KW "new")
            (PATH_TYPE
              (NAME_REF (IDENT "ArrayList"))
              (TYPE_ARGS
                (L_ANGLE "<")
                (R_ANGLE ">")))
            (ARG_LIST
              (L_PAREN "(")
              (R_PAREN ")"))))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "Object")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "o4"))
          (EQ "=")
          (NEW_EXPR
            (NEW_KW "new")
            (PATH_TYPE
              (NAME_REF (IDENT "HashMap"))
              (TYPE_ARGS
                (L_ANGLE "<")
                (PATH_TYPE (NAME_REF (IDENT "String")))
                (COMMA ",")
                (PATH_TYPE
                  (NAME_REF (IDENT "List"))
                  (TYPE_ARGS
                    (L_ANGLE "<")
                    (PATH_TYPE (NAME_REF (IDENT "Integer")))
                    (R_ANGLE ">")))
                (R_ANGLE ">")))
            (ARG_LIST
              (L_PAREN "(")
              (R_PAREN ")"))))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "Object")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "o5"))
          (EQ "=")
          (NEW_EXPR
            (NEW_KW "new")
            (PATH_TYPE
              (NAME_REF (IDENT "Outer"))
              (DOT ".")
              (NAME_REF (IDENT "Inner")))
            (ARG_LIST
              (L_PAREN "(")
              (R_PAREN ")"))))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "Object")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "o6"))
          (EQ "=")
          (NEW_EXPR
            (NAME_REF_EXPR (NAME_REF (IDENT "outer")))
            (DOT ".")
            (NEW_KW "new")
            (PATH_TYPE (NAME_REF (IDENT "Inner")))
            (ARG_LIST
              (L_PAREN "(")
              (R_PAREN ")"))))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "Object")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "o7"))
          (EQ "=")
          (NEW_EXPR
            (NEW_KW "new")
            (PATH_TYPE (NAME_REF (IDENT "Runnable")))
            (ARG_LIST
              (L_PAREN "(")
              (R_PAREN ")"))
            (CLASS_BODY
              (L_CURLY "{")
              (METHOD_DECL
                (MODIFIER_LIST (PUBLIC_KW "public"))
                (PRIMITIVE_TYPE (VOID_KW "void"))
                (NAME (IDENT "run"))
                (PARAM_LIST
                  (L_PAREN "(")
                  (R_PAREN ")"))
                (BLOCK
                  (L_CURLY "{")
                  (EXPR_STMT
                    (CALL_EXPR
                      (NAME_REF_EXPR (NAME_REF (IDENT "work")))
                      (ARG_LIST
                        (L_PAREN "(")
                        (R_PAREN ")")))
                    (SEMICOLON ";"))
                  (R_CURLY "}")))
              (R_CURLY "}"))))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "Object")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "o8"))
          (EQ "=")
          (NEW_EXPR
            (NEW_KW "new")
            (PATH_TYPE
              (NAME_REF (IDENT "Foo"))
              (TYPE_ARGS
                (L_ANGLE "<")
                (PATH_TYPE (NAME_REF (IDENT "String")))
                (R_ANGLE ">")))
            (ARG_LIST
              (L_PAREN "(")
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (R_PAREN ")"))
            (CLASS_BODY
              (L_CURLY "{")
              (FIELD_DECL
                (PRIMITIVE_TYPE (INT_KW "int"))
                (VARIABLE_DECLARATOR (NAME (IDENT "extra")))
                (SEMICOLON ";"))
              (R_CURLY "}"))))
        (SEMICOLON ";"))
      (FIELD_DECL
        (ARRAY_TYPE
          (PRIMITIVE_TYPE (INT_KW "int"))
          (L_BRACK "[")
          (R_BRACK "]"))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "a1"))
          (EQ "=")
          (NEW_EXPR
            (NEW_KW "new")
            (PRIMITIVE_TYPE (INT_KW "int"))
            (L_BRACK "[")
            (LITERAL (INT_NUMBER "3"))
            (R_BRACK "]")))
        (SEMICOLON ";"))
      (FIELD_DECL
        (ARRAY_TYPE
          (ARRAY_TYPE
            (PRIMITIVE_TYPE (INT_KW "int"))
            (L_BRACK "[")
            (R_BRACK "]"))
          (L_BRACK "[")
          (R_BRACK "]"))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "a2"))
          (EQ "=")
          (NEW_EXPR
            (NEW_KW "new")
            (PRIMITIVE_TYPE (INT_KW "int"))
            (L_BRACK "[")
            (LITERAL (INT_NUMBER "3"))
            (R_BRACK "]")
            (L_BRACK "[")
            (LITERAL (INT_NUMBER "4"))
            (R_BRACK "]")))
        (SEMICOLON ";"))
      (FIELD_DECL
        (ARRAY_TYPE
          (ARRAY_TYPE
            (PRIMITIVE_TYPE (INT_KW "int"))
            (L_BRACK "[")
            (R_BRACK "]"))
          (L_BRACK "[")
          (R_BRACK "]"))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "a3"))
          (EQ "=")
          (NEW_EXPR
            (NEW_KW "new")
            (PRIMITIVE_TYPE (INT_KW "int"))
            (L_BRACK "[")
            (LITERAL (INT_NUMBER "3"))
            (R_BRACK "]")
            (L_BRACK "[")
            (R_BRACK "]")))
        (SEMICOLON ";"))
      (FIELD_DECL
        (ARRAY_TYPE
          (PRIMITIVE_TYPE (INT_KW "int"))
          (L_BRACK "[")
          (R_BRACK "]"))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "a4"))
          (EQ "=")
          (NEW_EXPR
            (NEW_KW "new")
            (ARRAY_TYPE
              (PRIMITIVE_TYPE (INT_KW "int"))
              (L_BRACK "[")
              (R_BRACK "]"))
            (ARRAY_INIT
              (L_CURLY "{")
              (LITERAL (INT_NUMBER "1"))
              (COMMA ",")
              (LITERAL (INT_NUMBER "2"))
              (COMMA ",")
              (LITERAL (INT_NUMBER "3"))
              (R_CURLY "}"))))
        (SEMICOLON ";"))
      (FIELD_DECL
        (ARRAY_TYPE
          (ARRAY_TYPE
            (PRIMITIVE_TYPE (INT_KW "int"))
            (L_BRACK "[")
            (R_BRACK "]"))
          (L_BRACK "[")
          (R_BRACK "]"))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "a5"))
          (EQ "=")
          (NEW_EXPR
            (NEW_KW "new")
            (ARRAY_TYPE
              (ARRAY_TYPE
                (PRIMITIVE_TYPE (INT_KW "int"))
                (L_BRACK "[")
                (R_BRACK "]"))
              (L_BRACK "[")
              (R_BRACK "]"))
            (ARRAY_INIT
              (L_CURLY "{")
              (ARRAY_INIT
                (L_CURLY "{")
                (LITERAL (INT_NUMBER "1"))
                (R_CURLY "}"))
              (COMMA ",")
              (ARRAY_INIT
                (L_CURLY "{")
                (LITERAL (INT_NUMBER "2"))
                (COMMA ",")
                (LITERAL (INT_NUMBER "3"))
                (R_CURLY "}"))
              (COMMA ",")
              (ARRAY_INIT
                (L_CURLY "{")
                (R_CURLY "}"))
              (R_CURLY "}"))))
        (SEMICOLON ";"))
      (FIELD_DECL
        (ARRAY_TYPE
          (PRIMITIVE_TYPE (INT_KW "int"))
          (L_BRACK "[")
          (R_BRACK "]"))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "a6"))
          (EQ "=")
          (ARRAY_INIT
            (L_CURLY "{")
            (LITERAL (INT_NUMBER "1"))
            (COMMA ",")
            (LITERAL (INT_NUMBER "2"))
            (COMMA ",")
            (LITERAL (INT_NUMBER "3"))
            (COMMA ",")
            (R_CURLY "}")))
        (SEMICOLON ";"))
      (FIELD_DECL
        (ARRAY_TYPE
          (ARRAY_TYPE
            (PRIMITIVE_TYPE (INT_KW "int"))
            (L_BRACK "[")
            (R_BRACK "]"))
          (L_BRACK "[")
          (R_BRACK "]"))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "a7"))
          (EQ "=")
          (ARRAY_INIT
            (L_CURLY "{")
            (ARRAY_INIT
              (L_CURLY "{")
              (LITERAL (INT_NUMBER "1"))
              (R_CURLY "}"))
            (COMMA ",")
            (ARRAY_INIT
              (L_CURLY "{")
              (LITERAL (INT_NUMBER "2"))
              (R_CURLY "}"))
            (R_CURLY "}")))
        (SEMICOLON ";"))
      (FIELD_DECL
        (ARRAY_TYPE
          (PATH_TYPE (NAME_REF (IDENT "String")))
          (L_BRACK "[")
          (R_BRACK "]"))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "a8"))
          (EQ "=")
          (NEW_EXPR
            (NEW_KW "new")
            (PATH_TYPE (NAME_REF (IDENT "String")))
            (L_BRACK "[")
            (BINARY_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "n")))
              (PLUS "+")
              (LITERAL (INT_NUMBER "1")))
            (R_BRACK "]")))
        (SEMICOLON ";"))
      (FIELD_DECL
        (ARRAY_TYPE
          (PATH_TYPE (NAME_REF (IDENT "Foo")))
          (L_BRACK "[")
          (R_BRACK "]"))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "a9"))
          (EQ "=")
          (ARRAY_INIT
            (L_CURLY "{")
            (NEW_EXPR
              (NEW_KW "new")
              (PATH_TYPE (NAME_REF (IDENT "Foo")))
              (ARG_LIST
                (L_PAREN "(")
                (R_PAREN ")")))
            (COMMA ",")
            (LITERAL (NULL_KW "null"))
            (R_CURLY "}")))
        (SEMICOLON ";"))
      (R_CURLY "}"))))
