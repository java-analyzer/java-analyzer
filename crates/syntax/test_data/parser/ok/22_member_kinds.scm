(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "A"))
    (CLASS_BODY
      (L_CURLY "{")
      (FIELD_DECL
        (PRIMITIVE_TYPE (INT_KW "int"))
        (VARIABLE_DECLARATOR (NAME (IDENT "a")))
        (COMMA ",")
        (VARIABLE_DECLARATOR
          (NAME (IDENT "b"))
          (EQ "=")
          (LITERAL (INT_NUMBER "2")))
        (COMMA ",")
        (VARIABLE_DECLARATOR
          (NAME (IDENT "c"))
          (L_BRACK "[")
          (R_BRACK "]"))
        (COMMA ",")
        (VARIABLE_DECLARATOR
          (NAME (IDENT "d"))
          (L_BRACK "[")
          (R_BRACK "]")
          (EQ "=")
          (ARRAY_INIT
            (L_CURLY "{")
            (LITERAL (INT_NUMBER "1"))
            (R_CURLY "}")))
        (SEMICOLON ";"))
      (FIELD_DECL
        (MODIFIER_LIST
          (PRIVATE_KW "private")
          (STATIC_KW "static")
          (FINAL_KW "final"))
        (PATH_TYPE
          (NAME_REF (IDENT "Map"))
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
        (VARIABLE_DECLARATOR
          (NAME (IDENT "index"))
          (EQ "=")
          (NEW_EXPR
            (NEW_KW "new")
            (PATH_TYPE
              (NAME_REF (IDENT "HashMap"))
              (TYPE_ARGS
                (L_ANGLE "<")
                (R_ANGLE ">")))
            (ARG_LIST
              (L_PAREN "(")
              (R_PAREN ")"))))
        (SEMICOLON ";"))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "run"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (R_CURLY "}")))
      (METHOD_DECL
        (MODIFIER_LIST (ABSTRACT_KW "abstract"))
        (PRIMITIVE_TYPE (INT_KW "int"))
        (NAME (IDENT "size"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (THROWS_CLAUSE
          (THROWS_KW "throws")
          (PATH_TYPE (NAME_REF (IDENT "IOException")))
          (COMMA ",")
          (PATH_TYPE (NAME_REF (IDENT "SQLException"))))
        (SEMICOLON ";"))
      (METHOD_DECL
        (TYPE_PARAMS
          (L_ANGLE "<")
          (TYPE_PARAM
            (NAME (IDENT "T"))
            (EXTENDS_KW "extends")
            (PATH_TYPE
              (NAME_REF (IDENT "Comparable"))
              (TYPE_ARGS
                (L_ANGLE "<")
                (PATH_TYPE (NAME_REF (IDENT "T")))
                (R_ANGLE ">"))))
          (R_ANGLE ">"))
        (PATH_TYPE (NAME_REF (IDENT "T")))
        (NAME (IDENT "max"))
        (PARAM_LIST
          (L_PAREN "(")
          (PARAM
            (PATH_TYPE (NAME_REF (IDENT "T")))
            (NAME (IDENT "x")))
          (COMMA ",")
          (PARAM
            (PATH_TYPE (NAME_REF (IDENT "T")))
            (NAME (IDENT "y")))
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (RETURN_STMT
            (RETURN_KW "return")
            (NAME_REF_EXPR (NAME_REF (IDENT "x")))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (INT_KW "int"))
        (NAME (IDENT "legacy"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (L_BRACK "[")
        (R_BRACK "]")
        (BLOCK
          (L_CURLY "{")
          (RETURN_STMT
            (RETURN_KW "return")
            (LITERAL (NULL_KW "null"))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (METHOD_DECL
        (PATH_TYPE (NAME_REF (IDENT "String")))
        (NAME (IDENT "join"))
        (PARAM_LIST
          (L_PAREN "(")
          (PARAM
            (PATH_TYPE (NAME_REF (IDENT "String")))
            (NAME (IDENT "sep")))
          (COMMA ",")
          (PARAM
            (PATH_TYPE (NAME_REF (IDENT "String")))
            (DOT3 "...")
            (NAME (IDENT "parts")))
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (RETURN_STMT
            (RETURN_KW "return")
            (LITERAL (STRING_LITERAL "\"\""))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "modifiers"))
        (PARAM_LIST
          (L_PAREN "(")
          (PARAM
            (MODIFIER_LIST (FINAL_KW "final"))
            (PRIMITIVE_TYPE (INT_KW "int"))
            (NAME (IDENT "a")))
          (COMMA ",")
          (PARAM
            (MODIFIER_LIST
              (ANNOTATION
                (AT "@")
                (PATH (NAME_REF (IDENT "Nullable")))))
            (PATH_TYPE (NAME_REF (IDENT "String")))
            (NAME (IDENT "b")))
          (COMMA ",")
          (PARAM
            (MODIFIER_LIST
              (FINAL_KW "final")
              (ANNOTATION
                (AT "@")
                (PATH (NAME_REF (IDENT "Anno")))))
            (PRIMITIVE_TYPE (LONG_KW "long"))
            (NAME (IDENT "c")))
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (R_CURLY "}")))
      (CONSTRUCTOR_DECL
        (TYPE_PARAMS
          (L_ANGLE "<")
          (TYPE_PARAM (NAME (IDENT "T")))
          (R_ANGLE ">"))
        (NAME (IDENT "A"))
        (PARAM_LIST
          (L_PAREN "(")
          (PARAM
            (PATH_TYPE (NAME_REF (IDENT "T")))
            (NAME (IDENT "value")))
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (R_CURLY "}")))
      (CONSTRUCTOR_DECL
        (MODIFIER_LIST (PROTECTED_KW "protected"))
        (NAME (IDENT "A"))
        (PARAM_LIST
          (L_PAREN "(")
          (PARAM
            (PRIMITIVE_TYPE (INT_KW "int"))
            (NAME (IDENT "x")))
          (COMMA ",")
          (PARAM
            (PRIMITIVE_TYPE (INT_KW "int"))
            (NAME (IDENT "y")))
          (R_PAREN ")"))
        (THROWS_CLAUSE
          (THROWS_KW "throws")
          (PATH_TYPE (NAME_REF (IDENT "Exception"))))
        (BLOCK
          (L_CURLY "{")
          (EXPR_STMT
            (CALL_EXPR
              (THIS_EXPR (THIS_KW "this"))
              (ARG_LIST
                (L_PAREN "(")
                (R_PAREN ")")))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (METHOD_DECL
        (MODIFIER_LIST (NATIVE_KW "native"))
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "nativeCall"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (SEMICOLON ";"))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "trailing"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (R_CURLY "}")))
      (R_CURLY "}"))))
