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
          (EMPTY_STMT (SEMICOLON ";"))
          (EXPR_STMT
            (EXPR_PLACEHOLDER
              (IDENT "x")
              (EQ "=")
              (INT_NUMBER "1"))
            (SEMICOLON ";"))
          (EXPR_STMT
            (EXPR_PLACEHOLDER
              (IDENT "foo")
              (L_PAREN "(")
              (R_PAREN ")"))
            (SEMICOLON ";"))
          (EXPR_STMT
            (EXPR_PLACEHOLDER
              (IDENT "a")
              (DOT ".")
              (IDENT "b")
              (DOT ".")
              (IDENT "c")
              (L_PAREN "(")
              (INT_NUMBER "1")
              (COMMA ",")
              (INT_NUMBER "2")
              (R_PAREN ")")
              (DOT ".")
              (IDENT "d")
              (L_PAREN "(")
              (R_PAREN ")"))
            (SEMICOLON ";"))
          (EXPR_STMT
            (EXPR_PLACEHOLDER
              (THIS_KW "this")
              (DOT ".")
              (IDENT "x")
              (EQ "=")
              (IDENT "y"))
            (SEMICOLON ";"))
          (EXPR_STMT
            (EXPR_PLACEHOLDER
              (IDENT "i")
              (PLUS2 "++"))
            (SEMICOLON ";"))
          (EXPR_STMT
            (EXPR_PLACEHOLDER
              (NEW_KW "new")
              (IDENT "Thing")
              (L_PAREN "(")
              (R_PAREN ")")
              (DOT ".")
              (IDENT "run")
              (L_PAREN "(")
              (R_PAREN ")"))
            (SEMICOLON ";"))
          (RETURN_STMT
            (RETURN_KW "return")
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (INT_KW "int"))
        (NAME (IDENT "g"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (RETURN_STMT
            (RETURN_KW "return")
            (EXPR_PLACEHOLDER
              (IDENT "a")
              (PLUS "+")
              (IDENT "b")
              (STAR "*")
              (INT_NUMBER "2"))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "h"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (THROW_STMT
            (THROW_KW "throw")
            (EXPR_PLACEHOLDER
              (NEW_KW "new")
              (IDENT "IllegalStateException")
              (L_PAREN "(")
              (STRING_LITERAL "\"no\"")
              (R_PAREN ")"))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (METHOD_DECL
        (PRIMITIVE_TYPE (VOID_KW "void"))
        (NAME (IDENT "i"))
        (PARAM_LIST
          (L_PAREN "(")
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (LABELED_STMT
            (NAME (IDENT "outer"))
            (COLON ":")
            (LABELED_STMT
              (NAME (IDENT "loop"))
              (COLON ":")
              (BREAK_STMT
                (BREAK_KW "break")
                (NAME_REF (IDENT "outer"))
                (SEMICOLON ";"))))
          (CONTINUE_STMT
            (CONTINUE_KW "continue")
            (NAME_REF (IDENT "loop"))
            (SEMICOLON ";"))
          (BREAK_STMT
            (BREAK_KW "break")
            (SEMICOLON ";"))
          (CONTINUE_STMT
            (CONTINUE_KW "continue")
            (SEMICOLON ";"))
          (ASSERT_STMT
            (ASSERT_KW "assert")
            (EXPR_PLACEHOLDER
              (IDENT "x")
              (R_ANGLE ">")
              (INT_NUMBER "0"))
            (SEMICOLON ";"))
          (ASSERT_STMT
            (ASSERT_KW "assert")
            (EXPR_PLACEHOLDER
              (IDENT "x")
              (R_ANGLE ">")
              (INT_NUMBER "0"))
            (COLON ":")
            (EXPR_PLACEHOLDER
              (STRING_LITERAL "\"message \"")
              (PLUS "+")
              (IDENT "x"))
            (SEMICOLON ";"))
          (BLOCK
            (L_CURLY "{")
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}"))
            (R_CURLY "}"))
          (SYNCHRONIZED_STMT
            (SYNCHRONIZED_KW "synchronized")
            (L_PAREN "(")
            (EXPR_PLACEHOLDER (THIS_KW "this"))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (EXPR_STMT
                (EXPR_PLACEHOLDER
                  (IDENT "count")
                  (PLUS2 "++"))
                (SEMICOLON ";"))
              (R_CURLY "}")))
          (R_CURLY "}")))
      (R_CURLY "}"))))
