(SOURCE_FILE
  (SHEBANG "#!/usr/bin/env java --source 25")
  (IMPORT_DECL
    (IMPORT_KW "import")
    (IDENT "module")
    (PATH
      (NAME_REF (IDENT "java"))
      (DOT ".")
      (NAME_REF (IDENT "base")))
    (SEMICOLON ";"))
  (IMPORT_DECL
    (IMPORT_KW "import")
    (PATH
      (NAME_REF (IDENT "java"))
      (DOT ".")
      (NAME_REF (IDENT "util"))
      (DOT ".")
      (NAME_REF (IDENT "List")))
    (SEMICOLON ";"))
  (FIELD_DECL
    (PRIMITIVE_TYPE (INT_KW "int"))
    (VARIABLE_DECLARATOR
      (NAME (IDENT "greetingCount"))
      (EQ "=")
      (LITERAL (INT_NUMBER "1")))
    (SEMICOLON ";"))
  (METHOD_DECL
    (PRIMITIVE_TYPE (VOID_KW "void"))
    (NAME (IDENT "main"))
    (PARAM_LIST
      (L_PAREN "(")
      (PARAM
        (ARRAY_TYPE
          (PATH_TYPE (NAME_REF (IDENT "String")))
          (L_BRACK "[")
          (R_BRACK "]"))
        (NAME (IDENT "args")))
      (R_PAREN ")"))
    (BLOCK
      (L_CURLY "{")
      (FOR_STMT
        (FOR_KW "for")
        (L_PAREN "(")
        (LOCAL_VAR_DECL
          (PRIMITIVE_TYPE (INT_KW "int"))
          (VARIABLE_DECLARATOR
            (NAME (IDENT "i"))
            (EQ "=")
            (LITERAL (INT_NUMBER "0"))))
        (SEMICOLON ";")
        (BINARY_EXPR
          (NAME_REF_EXPR (NAME_REF (IDENT "i")))
          (L_ANGLE "<")
          (NAME_REF_EXPR (NAME_REF (IDENT "greetingCount"))))
        (SEMICOLON ";")
        (POSTFIX_EXPR
          (NAME_REF_EXPR (NAME_REF (IDENT "i")))
          (PLUS2 "++"))
        (R_PAREN ")")
        (BLOCK
          (L_CURLY "{")
          (EXPR_STMT
            (CALL_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "println")))
              (ARG_LIST
                (L_PAREN "(")
                (LITERAL (STRING_LITERAL "\"Hello\""))
                (R_PAREN ")")))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (R_CURLY "}"))))
