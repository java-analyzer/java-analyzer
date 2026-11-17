(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "TypeAnnotations"))
    (TYPE_PARAMS
      (L_ANGLE "<")
      (TYPE_PARAM
        (ANNOTATION
          (AT "@")
          (PATH (NAME_REF (IDENT "A"))))
        (NAME (IDENT "T"))
        (EXTENDS_KW "extends")
        (PATH_TYPE
          (ANNOTATION
            (AT "@")
            (PATH (NAME_REF (IDENT "B"))))
          (NAME_REF (IDENT "Object"))))
      (R_ANGLE ">"))
    (EXTENDS_CLAUSE
      (EXTENDS_KW "extends")
      (PATH_TYPE
        (ANNOTATION
          (AT "@")
          (PATH (NAME_REF (IDENT "A"))))
        (NAME_REF (IDENT "Base"))
        (TYPE_ARGS
          (L_ANGLE "<")
          (PATH_TYPE
            (ANNOTATION
              (AT "@")
              (PATH (NAME_REF (IDENT "B"))))
            (NAME_REF (IDENT "String")))
          (R_ANGLE ">"))))
    (IMPLEMENTS_CLAUSE
      (IMPLEMENTS_KW "implements")
      (PATH_TYPE
        (ANNOTATION
          (AT "@")
          (PATH (NAME_REF (IDENT "A"))))
        (NAME_REF (IDENT "List"))
        (TYPE_ARGS
          (L_ANGLE "<")
          (WILDCARD_TYPE
            (ANNOTATION
              (AT "@")
              (PATH (NAME_REF (IDENT "B"))))
            (QUESTION "?")
            (EXTENDS_KW "extends")
            (PATH_TYPE
              (ANNOTATION
                (AT "@")
                (PATH (NAME_REF (IDENT "C"))))
              (NAME_REF (IDENT "Number"))))
          (R_ANGLE ">")))
      (COMMA ",")
      (PATH_TYPE
        (ANNOTATION
          (AT "@")
          (PATH (NAME_REF (IDENT "D"))))
        (NAME_REF (IDENT "List"))
        (TYPE_ARGS
          (L_ANGLE "<")
          (WILDCARD_TYPE
            (ANNOTATION
              (AT "@")
              (PATH (NAME_REF (IDENT "E"))))
            (QUESTION "?"))
          (R_ANGLE ">"))))
    (CLASS_BODY
      (L_CURLY "{")
      (FIELD_DECL
        (PATH_TYPE
          (NAME_REF (IDENT "Outer"))
          (DOT ".")
          (ANNOTATION
            (AT "@")
            (PATH (NAME_REF (IDENT "A"))))
          (NAME_REF (IDENT "Inner"))
          (TYPE_ARGS
            (L_ANGLE "<")
            (PATH_TYPE
              (ANNOTATION
                (AT "@")
                (PATH (NAME_REF (IDENT "B"))))
              (NAME_REF (IDENT "String")))
            (R_ANGLE ">")))
        (VARIABLE_DECLARATOR (NAME (IDENT "nested")))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PATH_TYPE
          (NAME_REF (IDENT "ThreadLocal"))
          (TYPE_ARGS
            (L_ANGLE "<")
            (ARRAY_TYPE
              (PRIMITIVE_TYPE (INT_KW "int"))
              (ANNOTATION
                (AT "@")
                (PATH (NAME_REF (IDENT "A"))))
              (L_BRACK "[")
              (R_BRACK "]"))
            (R_ANGLE ">")))
        (VARIABLE_DECLARATOR (NAME (IDENT "primArray")))
        (SEMICOLON ";"))
      (FIELD_DECL
        (ARRAY_TYPE
          (ARRAY_TYPE
            (PRIMITIVE_TYPE (CHAR_KW "char"))
            (L_BRACK "[")
            (R_BRACK "]"))
          (ANNOTATION
            (AT "@")
            (PATH (NAME_REF (IDENT "A"))))
          (L_BRACK "[")
          (R_BRACK "]"))
        (VARIABLE_DECLARATOR (NAME (IDENT "multiArray1")))
        (SEMICOLON ";"))
      (FIELD_DECL
        (ARRAY_TYPE
          (ARRAY_TYPE
            (PATH_TYPE (NAME_REF (IDENT "String")))
            (ANNOTATION
              (AT "@")
              (PATH (NAME_REF (IDENT "A"))))
            (L_BRACK "[")
            (R_BRACK "]"))
          (ANNOTATION
            (AT "@")
            (PATH (NAME_REF (IDENT "B"))))
          (L_BRACK "[")
          (R_BRACK "]"))
        (VARIABLE_DECLARATOR (NAME (IDENT "multiArray2")))
        (SEMICOLON ";"))
      (FIELD_DECL
        (PRIMITIVE_TYPE (INT_KW "int"))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "legacyDims"))
          (ANNOTATION
            (AT "@")
            (PATH (NAME_REF (IDENT "A"))))
          (L_BRACK "[")
          (R_BRACK "]"))
        (SEMICOLON ";"))
      (METHOD_DECL
        (TYPE_PARAMS
          (L_ANGLE "<")
          (TYPE_PARAM
            (ANNOTATION
              (AT "@")
              (PATH (NAME_REF (IDENT "A"))))
            (NAME (IDENT "U"))
            (EXTENDS_KW "extends")
            (PATH_TYPE
              (ANNOTATION
                (AT "@")
                (PATH (NAME_REF (IDENT "B"))))
              (NAME_REF (IDENT "Foo")))
            (AMP "&")
            (PATH_TYPE
              (ANNOTATION
                (AT "@")
                (PATH (NAME_REF (IDENT "C"))))
              (NAME_REF (IDENT "Bar"))))
          (R_ANGLE ">"))
        (PATH_TYPE
          (ANNOTATION
            (AT "@")
            (PATH (NAME_REF (IDENT "D"))))
          (NAME_REF (IDENT "U")))
        (NAME (IDENT "genericMethod"))
        (PARAM_LIST
          (L_PAREN "(")
          (PARAM
            (MODIFIER_LIST
              (ANNOTATION
                (AT "@")
                (PATH (NAME_REF (IDENT "E")))))
            (PATH_TYPE (NAME_REF (IDENT "U")))
            (ANNOTATION
              (AT "@")
              (PATH (NAME_REF (IDENT "F"))))
            (DOT3 "...")
            (NAME (IDENT "args")))
          (R_PAREN ")"))
        (THROWS_CLAUSE
          (THROWS_KW "throws")
          (PATH_TYPE
            (ANNOTATION
              (AT "@")
              (PATH (NAME_REF (IDENT "G"))))
            (NAME_REF (IDENT "Exception"))))
        (BLOCK
          (L_CURLY "{")
          (LOCAL_VAR_DECL
            (PATH_TYPE
              (NAME_REF (IDENT "List"))
              (TYPE_ARGS
                (L_ANGLE "<")
                (PATH_TYPE
                  (ANNOTATION
                    (AT "@")
                    (PATH (NAME_REF (IDENT "Anno")))
                    (ANNOTATION_ARGS
                      (L_PAREN "(")
                      (ANNOTATION_ARG (LITERAL (STRING_LITERAL "\"x\"")))
                      (R_PAREN ")")))
                  (NAME_REF (IDENT "String")))
                (R_ANGLE ">")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "withArgs"))
              (EQ "=")
              (LITERAL (NULL_KW "null")))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (MODIFIER_LIST
              (ANNOTATION
                (AT "@")
                (PATH (NAME_REF (IDENT "A")))))
            (PATH_TYPE (NAME_REF (IDENT "String")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "local"))
              (EQ "=")
              (CAST_EXPR
                (L_PAREN "(")
                (ARRAY_TYPE
                  (ARRAY_TYPE
                    (PATH_TYPE
                      (ANNOTATION
                        (AT "@")
                        (PATH (NAME_REF (IDENT "B"))))
                      (NAME_REF (IDENT "String")))
                    (L_BRACK "[")
                    (R_BRACK "]"))
                  (L_BRACK "[")
                  (R_BRACK "]"))
                (R_PAREN ")")
                (NAME_REF_EXPR (NAME_REF (IDENT "args")))))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PRIMITIVE_TYPE (INT_KW "int"))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "n"))
              (EQ "=")
              (CAST_EXPR
                (L_PAREN "(")
                (PRIMITIVE_TYPE
                  (ANNOTATION
                    (AT "@")
                    (PATH (NAME_REF (IDENT "A"))))
                  (INT_KW "int"))
                (R_PAREN ")")
                (LITERAL (LONG_NUMBER "1L"))))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PATH_TYPE (NAME_REF (IDENT "Object")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "intersection"))
              (EQ "=")
              (CAST_EXPR
                (L_PAREN "(")
                (PATH_TYPE
                  (ANNOTATION
                    (AT "@")
                    (PATH (NAME_REF (IDENT "A"))))
                  (NAME_REF (IDENT "Foo")))
                (AMP "&")
                (PATH_TYPE
                  (ANNOTATION
                    (AT "@")
                    (PATH (NAME_REF (IDENT "B"))))
                  (NAME_REF (IDENT "Bar")))
                (R_PAREN ")")
                (NAME_REF_EXPR (NAME_REF (IDENT "args")))))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PATH_TYPE (NAME_REF (IDENT "Object")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "o"))
              (EQ "=")
              (NEW_EXPR
                (NEW_KW "new")
                (PATH_TYPE
                  (ANNOTATION
                    (AT "@")
                    (PATH (NAME_REF (IDENT "A"))))
                  (NAME_REF (IDENT "Object")))
                (ARG_LIST
                  (L_PAREN "(")
                  (R_PAREN ")"))))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (ARRAY_TYPE
              (ARRAY_TYPE
                (PRIMITIVE_TYPE (INT_KW "int"))
                (L_BRACK "[")
                (R_BRACK "]"))
              (L_BRACK "[")
              (R_BRACK "]"))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "a"))
              (EQ "=")
              (NEW_EXPR
                (NEW_KW "new")
                (PRIMITIVE_TYPE (INT_KW "int"))
                (ANNOTATION
                  (AT "@")
                  (PATH (NAME_REF (IDENT "A"))))
                (L_BRACK "[")
                (LITERAL (INT_NUMBER "1"))
                (R_BRACK "]")
                (ANNOTATION
                  (AT "@")
                  (PATH (NAME_REF (IDENT "B"))))
                (L_BRACK "[")
                (R_BRACK "]")))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (ARRAY_TYPE
              (PRIMITIVE_TYPE (INT_KW "int"))
              (L_BRACK "[")
              (R_BRACK "]"))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "b"))
              (EQ "=")
              (NEW_EXPR
                (NEW_KW "new")
                (ARRAY_TYPE
                  (PRIMITIVE_TYPE (INT_KW "int"))
                  (ANNOTATION
                    (AT "@")
                    (PATH (NAME_REF (IDENT "A"))))
                  (L_BRACK "[")
                  (R_BRACK "]"))
                (ARRAY_INIT
                  (L_CURLY "{")
                  (LITERAL (INT_NUMBER "1"))
                  (COMMA ",")
                  (LITERAL (INT_NUMBER "2"))
                  (R_CURLY "}"))))
            (SEMICOLON ";"))
          (IF_STMT
            (IF_KW "if")
            (L_PAREN "(")
            (INSTANCEOF_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "o")))
              (INSTANCEOF_KW "instanceof")
              (PATH_TYPE
                (ANNOTATION
                  (AT "@")
                  (PATH (NAME_REF (IDENT "A"))))
                (NAME_REF (IDENT "String"))))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}")))
          (IF_STMT
            (IF_KW "if")
            (L_PAREN "(")
            (INSTANCEOF_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "o")))
              (INSTANCEOF_KW "instanceof")
              (TYPE_PATTERN
                (MODIFIER_LIST
                  (ANNOTATION
                    (AT "@")
                    (PATH (NAME_REF (IDENT "A")))))
                (PATH_TYPE (NAME_REF (IDENT "String")))
                (NAME (IDENT "s"))))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}")))
          (FOREACH_STMT
            (FOR_KW "for")
            (L_PAREN "(")
            (MODIFIER_LIST
              (ANNOTATION
                (AT "@")
                (PATH (NAME_REF (IDENT "A")))))
            (PATH_TYPE (NAME_REF (IDENT "String")))
            (NAME (IDENT "item"))
            (COLON ":")
            (NAME_REF_EXPR (NAME_REF (IDENT "withArgs")))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}")))
          (TRY_STMT
            (TRY_KW "try")
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}"))
            (CATCH_CLAUSE
              (CATCH_KW "catch")
              (L_PAREN "(")
              (MODIFIER_LIST
                (ANNOTATION
                  (AT "@")
                  (PATH (NAME_REF (IDENT "A")))))
              (PATH_TYPE (NAME_REF (IDENT "RuntimeException")))
              (PIPE "|")
              (PATH_TYPE
                (ANNOTATION
                  (AT "@")
                  (PATH (NAME_REF (IDENT "B"))))
                (NAME_REF (IDENT "Error")))
              (NAME (IDENT "e"))
              (R_PAREN ")")
              (BLOCK
                (L_CURLY "{")
                (R_CURLY "}"))))
          (RETURN_STMT
            (RETURN_KW "return")
            (CAST_EXPR
              (L_PAREN "(")
              (PATH_TYPE
                (ANNOTATION
                  (AT "@")
                  (PATH (NAME_REF (IDENT "A"))))
                (NAME_REF (IDENT "U")))
              (R_PAREN ")")
              (NAME_REF_EXPR (NAME_REF (IDENT "local"))))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (R_CURLY "}"))))
