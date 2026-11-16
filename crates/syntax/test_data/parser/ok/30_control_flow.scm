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
          (IF_STMT
            (IF_KW "if")
            (L_PAREN "(")
            (NAME_REF_EXPR (NAME_REF (IDENT "a")))
            (R_PAREN ")")
            (EXPR_STMT
              (CALL_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "b")))
                (ARG_LIST
                  (L_PAREN "(")
                  (R_PAREN ")")))
              (SEMICOLON ";")))
          (IF_STMT
            (IF_KW "if")
            (L_PAREN "(")
            (NAME_REF_EXPR (NAME_REF (IDENT "a")))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (EXPR_STMT
                (CALL_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "b")))
                  (ARG_LIST
                    (L_PAREN "(")
                    (R_PAREN ")")))
                (SEMICOLON ";"))
              (R_CURLY "}"))
            (ELSE_KW "else")
            (BLOCK
              (L_CURLY "{")
              (EXPR_STMT
                (CALL_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "c")))
                  (ARG_LIST
                    (L_PAREN "(")
                    (R_PAREN ")")))
                (SEMICOLON ";"))
              (R_CURLY "}")))
          (IF_STMT
            (IF_KW "if")
            (L_PAREN "(")
            (NAME_REF_EXPR (NAME_REF (IDENT "a")))
            (R_PAREN ")")
            (EXPR_STMT
              (CALL_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "b")))
                (ARG_LIST
                  (L_PAREN "(")
                  (R_PAREN ")")))
              (SEMICOLON ";"))
            (ELSE_KW "else")
            (IF_STMT
              (IF_KW "if")
              (L_PAREN "(")
              (NAME_REF_EXPR (NAME_REF (IDENT "c")))
              (R_PAREN ")")
              (EXPR_STMT
                (CALL_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "d")))
                  (ARG_LIST
                    (L_PAREN "(")
                    (R_PAREN ")")))
                (SEMICOLON ";"))
              (ELSE_KW "else")
              (EXPR_STMT
                (CALL_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "e")))
                  (ARG_LIST
                    (L_PAREN "(")
                    (R_PAREN ")")))
                (SEMICOLON ";"))))
          (IF_STMT
            (IF_KW "if")
            (L_PAREN "(")
            (BINARY_EXPR
              (BINARY_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "x")))
                (L_ANGLE "<")
                (NAME_REF_EXPR (NAME_REF (IDENT "y"))))
              (AMP2 "&&")
              (BINARY_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "y")))
                (R_ANGLE ">")
                (NAME_REF_EXPR (NAME_REF (IDENT "z")))))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}")))
          (WHILE_STMT
            (WHILE_KW "while")
            (L_PAREN "(")
            (NAME_REF_EXPR (NAME_REF (IDENT "running")))
            (R_PAREN ")")
            (EXPR_STMT
              (CALL_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "step")))
                (ARG_LIST
                  (L_PAREN "(")
                  (R_PAREN ")")))
              (SEMICOLON ";")))
          (WHILE_STMT
            (WHILE_KW "while")
            (L_PAREN "(")
            (BINARY_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "i")))
              (L_ANGLE "<")
              (NAME_REF_EXPR (NAME_REF (IDENT "n"))))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (EXPR_STMT
                (POSTFIX_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "i")))
                  (PLUS2 "++"))
                (SEMICOLON ";"))
              (R_CURLY "}")))
          (DO_STMT
            (DO_KW "do")
            (BLOCK
              (L_CURLY "{")
              (EXPR_STMT
                (POSTFIX_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "i")))
                  (PLUS2 "++"))
                (SEMICOLON ";"))
              (R_CURLY "}"))
            (WHILE_KW "while")
            (L_PAREN "(")
            (BINARY_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "i")))
              (L_ANGLE "<")
              (NAME_REF_EXPR (NAME_REF (IDENT "n"))))
            (R_PAREN ")")
            (SEMICOLON ";"))
          (DO_STMT
            (DO_KW "do")
            (EXPR_STMT
              (POSTFIX_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "i")))
                (PLUS2 "++"))
              (SEMICOLON ";"))
            (WHILE_KW "while")
            (L_PAREN "(")
            (BINARY_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "i")))
              (L_ANGLE "<")
              (NAME_REF_EXPR (NAME_REF (IDENT "n"))))
            (R_PAREN ")")
            (SEMICOLON ";"))
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
              (NAME_REF_EXPR (NAME_REF (IDENT "n"))))
            (SEMICOLON ";")
            (POSTFIX_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "i")))
              (PLUS2 "++"))
            (R_PAREN ")")
            (EXPR_STMT
              (ASSIGN_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "sum")))
                (PLUSEQ "+=")
                (NAME_REF_EXPR (NAME_REF (IDENT "i"))))
              (SEMICOLON ";")))
          (FOR_STMT
            (FOR_KW "for")
            (L_PAREN "(")
            (LOCAL_VAR_DECL
              (PRIMITIVE_TYPE (INT_KW "int"))
              (VARIABLE_DECLARATOR
                (NAME (IDENT "i"))
                (EQ "=")
                (LITERAL (INT_NUMBER "0")))
              (COMMA ",")
              (VARIABLE_DECLARATOR
                (NAME (IDENT "j"))
                (EQ "=")
                (NAME_REF_EXPR (NAME_REF (IDENT "n")))))
            (SEMICOLON ";")
            (BINARY_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "i")))
              (L_ANGLE "<")
              (NAME_REF_EXPR (NAME_REF (IDENT "j"))))
            (SEMICOLON ";")
            (POSTFIX_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "i")))
              (PLUS2 "++"))
            (COMMA ",")
            (POSTFIX_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "j")))
              (MINUS2 "--"))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}")))
          (FOR_STMT
            (FOR_KW "for")
            (L_PAREN "(")
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "i")))
              (EQ "=")
              (LITERAL (INT_NUMBER "0")))
            (SEMICOLON ";")
            (BINARY_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "i")))
              (L_ANGLE "<")
              (NAME_REF_EXPR (NAME_REF (IDENT "n"))))
            (SEMICOLON ";")
            (POSTFIX_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "i")))
              (PLUS2 "++"))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}")))
          (FOR_STMT
            (FOR_KW "for")
            (L_PAREN "(")
            (SEMICOLON ";")
            (SEMICOLON ";")
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (BREAK_STMT
                (BREAK_KW "break")
                (SEMICOLON ";"))
              (R_CURLY "}")))
          (FOREACH_STMT
            (FOR_KW "for")
            (L_PAREN "(")
            (MODIFIER_LIST (FINAL_KW "final"))
            (PATH_TYPE (NAME_REF (IDENT "String")))
            (NAME (IDENT "s"))
            (COLON ":")
            (NAME_REF_EXPR (NAME_REF (IDENT "names")))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (EXPR_STMT
                (CALL_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "use")))
                  (ARG_LIST
                    (L_PAREN "(")
                    (NAME_REF_EXPR (NAME_REF (IDENT "s")))
                    (R_PAREN ")")))
                (SEMICOLON ";"))
              (R_CURLY "}")))
          (FOREACH_STMT
            (FOR_KW "for")
            (L_PAREN "(")
            (PATH_TYPE
              (NAME_REF (IDENT "Map"))
              (DOT ".")
              (NAME_REF (IDENT "Entry"))
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
            (NAME (IDENT "e"))
            (COLON ":")
            (METHOD_CALL_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "map")))
              (DOT ".")
              (NAME_REF (IDENT "entrySet"))
              (ARG_LIST
                (L_PAREN "(")
                (R_PAREN ")")))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}")))
          (FOREACH_STMT
            (FOR_KW "for")
            (L_PAREN "(")
            (PATH_TYPE (NAME_REF (IDENT "var")))
            (NAME (IDENT "x"))
            (COLON ":")
            (NAME_REF_EXPR (NAME_REF (IDENT "xs")))
            (R_PAREN ")")
            (EXPR_STMT
              (CALL_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "use")))
                (ARG_LIST
                  (L_PAREN "(")
                  (NAME_REF_EXPR (NAME_REF (IDENT "x")))
                  (R_PAREN ")")))
              (SEMICOLON ";")))
          (FOREACH_STMT
            (FOR_KW "for")
            (L_PAREN "(")
            (MODIFIER_LIST
              (ANNOTATION
                (AT "@")
                (PATH (NAME_REF (IDENT "Anno")))))
            (PRIMITIVE_TYPE (INT_KW "int"))
            (NAME (IDENT "i"))
            (COLON ":")
            (NAME_REF_EXPR (NAME_REF (IDENT "ints")))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}")))
          (FOREACH_STMT
            (FOR_KW "for")
            (L_PAREN "(")
            (ARRAY_TYPE
              (PRIMITIVE_TYPE (INT_KW "int"))
              (L_BRACK "[")
              (R_BRACK "]"))
            (NAME (IDENT "row"))
            (COLON ":")
            (NAME_REF_EXPR (NAME_REF (IDENT "matrix")))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}")))
          (FOR_STMT
            (FOR_KW "for")
            (L_PAREN "(")
            (SEMICOLON ";")
            (SEMICOLON ";")
            (R_PAREN ")")
            (EMPTY_STMT (SEMICOLON ";")))
          (IF_STMT
            (IF_KW "if")
            (L_PAREN "(")
            (NAME_REF_EXPR (NAME_REF (IDENT "a")))
            (R_PAREN ")")
            (EMPTY_STMT (SEMICOLON ";"))
            (ELSE_KW "else")
            (EMPTY_STMT (SEMICOLON ";")))
          (LABELED_STMT
            (NAME (IDENT "label"))
            (COLON ":")
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
                (LITERAL (INT_NUMBER "3")))
              (SEMICOLON ";")
              (POSTFIX_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "i")))
                (PLUS2 "++"))
              (R_PAREN ")")
              (BLOCK
                (L_CURLY "{")
                (CONTINUE_STMT
                  (CONTINUE_KW "continue")
                  (NAME_REF (IDENT "label"))
                  (SEMICOLON ";"))
                (R_CURLY "}"))))
          (R_CURLY "}")))
      (R_CURLY "}"))))
