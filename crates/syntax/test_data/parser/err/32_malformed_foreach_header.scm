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
          (FOREACH_STMT
            (FOR_KW "for")
            (L_PAREN "(")
            (PATH_TYPE
              (NAME_REF (IDENT "Map"))
              (DOT ".")
              (NAME_REF (IDENT "Entry"))
              (TYPE_ARGS
                (L_ANGLE "<")
                (PATH_TYPE (NAME_REF (IDENT "String")))))
            (NAME (IDENT "List"))
            (EXPR_PLACEHOLDER
              (L_ANGLE "<")
              (IDENT "Integer")
              (R_ANGLE ">")
              (R_ANGLE ">")
              (IDENT "e")
              (COLON ":")
              (IDENT "map")
              (DOT ".")
              (IDENT "entrySet")
              (L_PAREN "(")
              (R_PAREN ")"))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}")))
          (FOR_STMT
            (FOR_KW "for")
            (L_PAREN "(")
            (LOCAL_VAR_DECL
              (PRIMITIVE_TYPE (INT_KW "int"))
              (VARIABLE_DECLARATOR (NAME (IDENT "x"))))
            (EXPR_PLACEHOLDER
              (IDENT "y")
              (COLON ":")
              (IDENT "xs"))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}")))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "ok"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (R_CURLY "}")))
      (R_CURLY "}"))))
(errors
  (error "expected '>'" "3:30..3:30")
  (error "expected ':'" "3:35..3:35")
  (error "expected ';'" "4:19..4:19")
  (error "expected ';'" "4:26..4:26"))
