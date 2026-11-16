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
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (LITERAL (INT_NUMBER "1")))
            (SEMICOLON ";"))
          (EXPR_STMT
            (CALL_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "foo")))
              (ARG_LIST
                (L_PAREN "(")
                (R_PAREN ")")))
            (SEMICOLON ";"))
          (EXPR_STMT
            (METHOD_CALL_EXPR
              (METHOD_CALL_EXPR
                (FIELD_ACCESS_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "a")))
                  (DOT ".")
                  (NAME_REF (IDENT "b")))
                (DOT ".")
                (NAME_REF (IDENT "c"))
                (ARG_LIST
                  (L_PAREN "(")
                  (LITERAL (INT_NUMBER "1"))
                  (COMMA ",")
                  (LITERAL (INT_NUMBER "2"))
                  (R_PAREN ")")))
              (DOT ".")
              (NAME_REF (IDENT "d"))
              (ARG_LIST
                (L_PAREN "(")
                (R_PAREN ")")))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (FIELD_ACCESS_EXPR
                (THIS_EXPR (THIS_KW "this"))
                (DOT ".")
                (NAME_REF (IDENT "x")))
              (EQ "=")
              (NAME_REF_EXPR (NAME_REF (IDENT "y"))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (POSTFIX_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "i")))
              (PLUS2 "++"))
            (SEMICOLON ";"))
          (EXPR_STMT
            (METHOD_CALL_EXPR
              (NEW_EXPR
                (NEW_KW "new")
                (PATH_TYPE (NAME_REF (IDENT "Thing")))
                (ARG_LIST
                  (L_PAREN "(")
                  (R_PAREN ")")))
              (DOT ".")
              (NAME_REF (IDENT "run"))
              (ARG_LIST
                (L_PAREN "(")
                (R_PAREN ")")))
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
            (BINARY_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "a")))
              (PLUS "+")
              (BINARY_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "b")))
                (STAR "*")
                (LITERAL (INT_NUMBER "2"))))
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
            (NEW_EXPR
              (NEW_KW "new")
              (PATH_TYPE (NAME_REF (IDENT "IllegalStateException")))
              (ARG_LIST
                (L_PAREN "(")
                (LITERAL (STRING_LITERAL "\"no\""))
                (R_PAREN ")")))
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
            (BINARY_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (R_ANGLE ">")
              (LITERAL (INT_NUMBER "0")))
            (SEMICOLON ";"))
          (ASSERT_STMT
            (ASSERT_KW "assert")
            (BINARY_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (R_ANGLE ">")
              (LITERAL (INT_NUMBER "0")))
            (COLON ":")
            (BINARY_EXPR
              (LITERAL (STRING_LITERAL "\"message \""))
              (PLUS "+")
              (NAME_REF_EXPR (NAME_REF (IDENT "x"))))
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
            (THIS_EXPR (THIS_KW "this"))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (EXPR_STMT
                (POSTFIX_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "count")))
                  (PLUS2 "++"))
                (SEMICOLON ";"))
              (R_CURLY "}")))
          (R_CURLY "}")))
      (R_CURLY "}"))))
