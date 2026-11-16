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
            (EXPR_PLACEHOLDER (IDENT "a"))
            (R_PAREN ")")
            (EXPR_STMT
              (EXPR_PLACEHOLDER
                (IDENT "b")
                (L_PAREN "(")
                (R_PAREN ")"))
              (SEMICOLON ";")))
          (IF_STMT
            (IF_KW "if")
            (L_PAREN "(")
            (EXPR_PLACEHOLDER (IDENT "a"))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (EXPR_STMT
                (EXPR_PLACEHOLDER
                  (IDENT "b")
                  (L_PAREN "(")
                  (R_PAREN ")"))
                (SEMICOLON ";"))
              (R_CURLY "}"))
            (ELSE_KW "else")
            (BLOCK
              (L_CURLY "{")
              (EXPR_STMT
                (EXPR_PLACEHOLDER
                  (IDENT "c")
                  (L_PAREN "(")
                  (R_PAREN ")"))
                (SEMICOLON ";"))
              (R_CURLY "}")))
          (IF_STMT
            (IF_KW "if")
            (L_PAREN "(")
            (EXPR_PLACEHOLDER (IDENT "a"))
            (R_PAREN ")")
            (EXPR_STMT
              (EXPR_PLACEHOLDER
                (IDENT "b")
                (L_PAREN "(")
                (R_PAREN ")"))
              (SEMICOLON ";"))
            (ELSE_KW "else")
            (IF_STMT
              (IF_KW "if")
              (L_PAREN "(")
              (EXPR_PLACEHOLDER (IDENT "c"))
              (R_PAREN ")")
              (EXPR_STMT
                (EXPR_PLACEHOLDER
                  (IDENT "d")
                  (L_PAREN "(")
                  (R_PAREN ")"))
                (SEMICOLON ";"))
              (ELSE_KW "else")
              (EXPR_STMT
                (EXPR_PLACEHOLDER
                  (IDENT "e")
                  (L_PAREN "(")
                  (R_PAREN ")"))
                (SEMICOLON ";"))))
          (IF_STMT
            (IF_KW "if")
            (L_PAREN "(")
            (EXPR_PLACEHOLDER
              (IDENT "x")
              (L_ANGLE "<")
              (IDENT "y")
              (AMP2 "&&")
              (IDENT "y")
              (R_ANGLE ">")
              (IDENT "z"))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}")))
          (WHILE_STMT
            (WHILE_KW "while")
            (L_PAREN "(")
            (EXPR_PLACEHOLDER (IDENT "running"))
            (R_PAREN ")")
            (EXPR_STMT
              (EXPR_PLACEHOLDER
                (IDENT "step")
                (L_PAREN "(")
                (R_PAREN ")"))
              (SEMICOLON ";")))
          (WHILE_STMT
            (WHILE_KW "while")
            (L_PAREN "(")
            (EXPR_PLACEHOLDER
              (IDENT "i")
              (L_ANGLE "<")
              (IDENT "n"))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (EXPR_STMT
                (EXPR_PLACEHOLDER
                  (IDENT "i")
                  (PLUS2 "++"))
                (SEMICOLON ";"))
              (R_CURLY "}")))
          (DO_STMT
            (DO_KW "do")
            (BLOCK
              (L_CURLY "{")
              (EXPR_STMT
                (EXPR_PLACEHOLDER
                  (IDENT "i")
                  (PLUS2 "++"))
                (SEMICOLON ";"))
              (R_CURLY "}"))
            (WHILE_KW "while")
            (L_PAREN "(")
            (EXPR_PLACEHOLDER
              (IDENT "i")
              (L_ANGLE "<")
              (IDENT "n"))
            (R_PAREN ")")
            (SEMICOLON ";"))
          (DO_STMT
            (DO_KW "do")
            (EXPR_STMT
              (EXPR_PLACEHOLDER
                (IDENT "i")
                (PLUS2 "++"))
              (SEMICOLON ";"))
            (WHILE_KW "while")
            (L_PAREN "(")
            (EXPR_PLACEHOLDER
              (IDENT "i")
              (L_ANGLE "<")
              (IDENT "n"))
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
                (EXPR_PLACEHOLDER (INT_NUMBER "0"))))
            (SEMICOLON ";")
            (EXPR_PLACEHOLDER
              (IDENT "i")
              (L_ANGLE "<")
              (IDENT "n"))
            (SEMICOLON ";")
            (EXPR_PLACEHOLDER
              (IDENT "i")
              (PLUS2 "++"))
            (R_PAREN ")")
            (EXPR_STMT
              (EXPR_PLACEHOLDER
                (IDENT "sum")
                (PLUSEQ "+=")
                (IDENT "i"))
              (SEMICOLON ";")))
          (FOR_STMT
            (FOR_KW "for")
            (L_PAREN "(")
            (LOCAL_VAR_DECL
              (PRIMITIVE_TYPE (INT_KW "int"))
              (VARIABLE_DECLARATOR
                (NAME (IDENT "i"))
                (EQ "=")
                (EXPR_PLACEHOLDER (INT_NUMBER "0")))
              (COMMA ",")
              (VARIABLE_DECLARATOR
                (NAME (IDENT "j"))
                (EQ "=")
                (EXPR_PLACEHOLDER (IDENT "n"))))
            (SEMICOLON ";")
            (EXPR_PLACEHOLDER
              (IDENT "i")
              (L_ANGLE "<")
              (IDENT "j"))
            (SEMICOLON ";")
            (EXPR_PLACEHOLDER
              (IDENT "i")
              (PLUS2 "++")
              (COMMA ",")
              (IDENT "j")
              (MINUS2 "--"))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}")))
          (FOR_STMT
            (FOR_KW "for")
            (L_PAREN "(")
            (EXPR_PLACEHOLDER
              (IDENT "i")
              (EQ "=")
              (INT_NUMBER "0"))
            (SEMICOLON ";")
            (EXPR_PLACEHOLDER
              (IDENT "i")
              (L_ANGLE "<")
              (IDENT "n"))
            (SEMICOLON ";")
            (EXPR_PLACEHOLDER
              (IDENT "i")
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
            (EXPR_PLACEHOLDER (IDENT "names"))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (EXPR_STMT
                (EXPR_PLACEHOLDER
                  (IDENT "use")
                  (L_PAREN "(")
                  (IDENT "s")
                  (R_PAREN ")"))
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
            (EXPR_PLACEHOLDER
              (IDENT "map")
              (DOT ".")
              (IDENT "entrySet")
              (L_PAREN "(")
              (R_PAREN ")"))
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
            (EXPR_PLACEHOLDER (IDENT "xs"))
            (R_PAREN ")")
            (EXPR_STMT
              (EXPR_PLACEHOLDER
                (IDENT "use")
                (L_PAREN "(")
                (IDENT "x")
                (R_PAREN ")"))
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
            (EXPR_PLACEHOLDER (IDENT "ints"))
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
            (EXPR_PLACEHOLDER (IDENT "matrix"))
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
            (EXPR_PLACEHOLDER (IDENT "a"))
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
                  (EXPR_PLACEHOLDER (INT_NUMBER "0"))))
              (SEMICOLON ";")
              (EXPR_PLACEHOLDER
                (IDENT "i")
                (L_ANGLE "<")
                (INT_NUMBER "3"))
              (SEMICOLON ";")
              (EXPR_PLACEHOLDER
                (IDENT "i")
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
