(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "A"))
    (CLASS_BODY
      (L_CURLY "{")
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "f"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (LOCAL_VAR_DECL
            (PATH_TYPE
              (NAME_REF (IDENT "List"))
              (TYPE_ARGS
                (L_ANGLE "<")
                (PATH_TYPE (NAME_REF (IDENT "String")))
                (R_ANGLE ">")))
            (VARIABLE_DECLARATOR (NAME (IDENT "declaration")))
            (SEMICOLON ";"))
          (EXPR_STMT
            (EXPR_PLACEHOLDER
              (IDENT "a")
              (L_ANGLE "<")
              (IDENT "b"))
            (SEMICOLON ";"))
          (EXPR_STMT
            (EXPR_PLACEHOLDER
              (IDENT "Collections")
              (DOT ".")
              (L_ANGLE "<")
              (IDENT "String")
              (R_ANGLE ">")
              (IDENT "emptyList")
              (L_PAREN "(")
              (R_PAREN ")"))
            (SEMICOLON ";"))
          (EXPR_STMT
            (EXPR_PLACEHOLDER
              (IDENT "arr")
              (L_BRACK "[")
              (IDENT "i")
              (R_BRACK "]")
              (EQ "=")
              (INT_NUMBER "1"))
            (SEMICOLON ";"))
          (EXPR_STMT
            (EXPR_PLACEHOLDER
              (IDENT "arr")
              (L_BRACK "[")
              (IDENT "i")
              (R_BRACK "]")
              (L_BRACK "[")
              (IDENT "j")
              (R_BRACK "]")
              (PLUS2 "++"))
            (SEMICOLON ";"))
          (EXPR_STMT
            (EXPR_PLACEHOLDER
              (IDENT "foo")
              (DOT ".")
              (IDENT "bar")
              (DOT ".")
              (IDENT "baz")
              (EQ "=")
              (INT_NUMBER "1"))
            (SEMICOLON ";"))
          (EXPR_STMT
            (EXPR_PLACEHOLDER
              (IDENT "Foo")
              (DOT ".")
              (IDENT "Bar")
              (DOT ".")
              (IDENT "baz")
              (L_PAREN "(")
              (R_PAREN ")"))
            (SEMICOLON ";"))
          (EXPR_STMT
            (EXPR_PLACEHOLDER
              (INT_KW "int")
              (DOT ".")
              (CLASS_KW "class")
              (DOT ".")
              (IDENT "getName")
              (L_PAREN "(")
              (R_PAREN ")"))
            (SEMICOLON ";"))
          (EXPR_STMT
            (EXPR_PLACEHOLDER
              (IDENT "x")
              (EQ "=")
              (IDENT "flag")
              (QUESTION "?")
              (IDENT "a")
              (COLON ":")
              (IDENT "b"))
            (SEMICOLON ";"))
          (LABELED_STMT
            (NAME (IDENT "label"))
            (COLON ":")
            (EXPR_STMT
              (EXPR_PLACEHOLDER
                (IDENT "x")
                (EQ "=")
                (IDENT "cond")
                (QUESTION "?")
                (INT_NUMBER "1")
                (COLON ":")
                (INT_NUMBER "2"))
              (SEMICOLON ";")))
          (EXPR_STMT
            (EXPR_PLACEHOLDER
              (IDENT "yield")
              (EQ "=")
              (INT_NUMBER "3"))
            (SEMICOLON ";"))
          (EXPR_STMT
            (EXPR_PLACEHOLDER
              (IDENT "yield")
              (PLUS2 "++"))
            (SEMICOLON ";"))
          (EXPR_STMT
            (EXPR_PLACEHOLDER
              (IDENT "record")
              (EQ "=")
              (INT_NUMBER "1"))
            (SEMICOLON ";"))
          (EXPR_STMT
            (EXPR_PLACEHOLDER
              (IDENT "var")
              (EQ "=")
              (INT_NUMBER "2"))
            (SEMICOLON ";"))
          (EXPR_STMT
            (EXPR_PLACEHOLDER
              (L_PAREN "(")
              (IDENT "cast")
              (R_PAREN ")")
              (IDENT "y"))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (ARRAY_TYPE
              (PATH_TYPE (NAME_REF (IDENT "String")))
              (L_BRACK "[")
              (R_BRACK "]"))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "parts"))
              (EQ "=")
              (EXPR_PLACEHOLDER
                (IDENT "line")
                (DOT ".")
                (IDENT "split")
                (L_PAREN "(")
                (STRING_LITERAL "\",\"")
                (R_PAREN ")")))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (ARRAY_TYPE
              (PATH_TYPE
                (NAME_REF (IDENT "Foo"))
                (TYPE_ARGS
                  (L_ANGLE "<")
                  (PATH_TYPE (NAME_REF (IDENT "Bar")))
                  (R_ANGLE ">")))
              (L_BRACK "[")
              (R_BRACK "]"))
            (VARIABLE_DECLARATOR (NAME (IDENT "generic")))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PATH_TYPE (NAME_REF (IDENT "Object")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "o"))
              (EQ "=")
              (EXPR_PLACEHOLDER
                (IDENT "cond")
                (QUESTION "?")
                (NEW_KW "new")
                (INT_KW "int")
                (L_BRACK "[")
                (R_BRACK "]")
                (L_CURLY "{")
                (INT_NUMBER "1")
                (R_CURLY "}")
                (COLON ":")
                (NULL_KW "null")))
            (SEMICOLON ";"))
          (EXPR_STMT
            (EXPR_PLACEHOLDER
              (IDENT "i")
              (EQ "=")
              (IDENT "a")
              (L_ANGLE "<")
              (IDENT "b")
              (QUESTION "?")
              (INT_NUMBER "1")
              (COLON ":")
              (INT_NUMBER "2"))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (INT_KW "int"))
        (NAME (IDENT "yielding"))
        (PARAM_LIST
          (L_PAREN "(")
          (PARAM
            (PRIMITIVE_TYPE (INT_KW "int"))
            (NAME (IDENT "x")))
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (RETURN_STMT
            (RETURN_KW "return")
            (EXPR_PLACEHOLDER
              (SWITCH_KW "switch")
              (L_PAREN "(")
              (IDENT "x")
              (R_PAREN ")")
              (L_CURLY "{")
              (CASE_KW "case")
              (INT_NUMBER "1")
              (ARROW "->")
              (INT_NUMBER "10")
              (SEMICOLON ";")
              (DEFAULT_KW "default")
              (ARROW "->")
              (L_CURLY "{")
              (IDENT "yield")
              (INT_NUMBER "20")
              (SEMICOLON ";")
              (R_CURLY "}")
              (R_CURLY "}"))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (R_CURLY "}"))))
