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
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (BINARY_EXPR
                (BINARY_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "a")))
                  (PLUS "+")
                  (BINARY_EXPR
                    (NAME_REF_EXPR (NAME_REF (IDENT "b")))
                    (STAR "*")
                    (NAME_REF_EXPR (NAME_REF (IDENT "c")))))
                (MINUS "-")
                (BINARY_EXPR
                  (BINARY_EXPR
                    (NAME_REF_EXPR (NAME_REF (IDENT "d")))
                    (SLASH "/")
                    (NAME_REF_EXPR (NAME_REF (IDENT "e"))))
                  (PERCENT "%")
                  (NAME_REF_EXPR (NAME_REF (IDENT "f"))))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (BINARY_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "a")))
                (PIPE2 "||")
                (BINARY_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "b")))
                  (AMP2 "&&")
                  (BINARY_EXPR
                    (NAME_REF_EXPR (NAME_REF (IDENT "c")))
                    (PIPE "|")
                    (BINARY_EXPR
                      (NAME_REF_EXPR (NAME_REF (IDENT "d")))
                      (CARET "^")
                      (BINARY_EXPR
                        (NAME_REF_EXPR (NAME_REF (IDENT "e")))
                        (AMP "&")
                        (NAME_REF_EXPR (NAME_REF (IDENT "f")))))))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (BINARY_EXPR
                (BINARY_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "a")))
                  (EQ2 "==")
                  (NAME_REF_EXPR (NAME_REF (IDENT "b"))))
                (NEQ "!=")
                (NAME_REF_EXPR (NAME_REF (IDENT "c")))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (BINARY_EXPR
                (BINARY_EXPR
                  (BINARY_EXPR
                    (BINARY_EXPR
                      (NAME_REF_EXPR (NAME_REF (IDENT "a")))
                      (L_ANGLE "<")
                      (NAME_REF_EXPR (NAME_REF (IDENT "b"))))
                    (PIPE "|")
                    (BINARY_EXPR
                      (NAME_REF_EXPR (NAME_REF (IDENT "c")))
                      (R_ANGLE ">")
                      (NAME_REF_EXPR (NAME_REF (IDENT "d")))))
                  (PIPE "|")
                  (BINARY_EXPR
                    (NAME_REF_EXPR (NAME_REF (IDENT "e")))
                    (LTEQ "<=")
                    (NAME_REF_EXPR (NAME_REF (IDENT "f")))))
                (PIPE "|")
                (BINARY_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "g")))
                  (GTEQ ">=")
                  (NAME_REF_EXPR (NAME_REF (IDENT "h"))))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (BINARY_EXPR
                (BINARY_EXPR
                  (BINARY_EXPR
                    (NAME_REF_EXPR (NAME_REF (IDENT "a")))
                    (SHL "<<")
                    (LITERAL (INT_NUMBER "2")))
                  (SHR ">>")
                  (LITERAL (INT_NUMBER "3")))
                (USHR ">>>")
                (LITERAL (INT_NUMBER "4"))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (BINARY_EXPR
                (BINARY_EXPR
                  (UNARY_EXPR
                    (MINUS "-")
                    (NAME_REF_EXPR (NAME_REF (IDENT "a"))))
                  (PLUS "+")
                  (UNARY_EXPR
                    (PLUS "+")
                    (NAME_REF_EXPR (NAME_REF (IDENT "b")))))
                (MINUS "-")
                (UNARY_EXPR
                  (MINUS "-")
                  (NAME_REF_EXPR (NAME_REF (IDENT "c"))))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (BINARY_EXPR
                (UNARY_EXPR
                  (BANG "!")
                  (NAME_REF_EXPR (NAME_REF (IDENT "a"))))
                (AMP2 "&&")
                (UNARY_EXPR
                  (TILDE "~")
                  (NAME_REF_EXPR (NAME_REF (IDENT "b"))))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (BINARY_EXPR
                (UNARY_EXPR
                  (PLUS2 "++")
                  (NAME_REF_EXPR (NAME_REF (IDENT "a"))))
                (PLUS "+")
                (UNARY_EXPR
                  (MINUS2 "--")
                  (NAME_REF_EXPR (NAME_REF (IDENT "b"))))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (BINARY_EXPR
                (POSTFIX_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "a")))
                  (PLUS2 "++"))
                (PLUS "+")
                (POSTFIX_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "b")))
                  (MINUS2 "--"))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (TERNARY_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "a")))
                (QUESTION "?")
                (NAME_REF_EXPR (NAME_REF (IDENT "b")))
                (COLON ":")
                (TERNARY_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "c")))
                  (QUESTION "?")
                  (NAME_REF_EXPR (NAME_REF (IDENT "d")))
                  (COLON ":")
                  (NAME_REF_EXPR (NAME_REF (IDENT "e"))))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (TERNARY_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "a")))
                (QUESTION "?")
                (ASSIGN_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "b")))
                  (EQ "=")
                  (LITERAL (INT_NUMBER "1")))
                (COLON ":")
                (NAME_REF_EXPR (NAME_REF (IDENT "c")))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (TERNARY_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "cond")))
                (QUESTION "?")
                (LITERAL (INT_NUMBER "1"))
                (COLON ":")
                (LITERAL (INT_NUMBER "2"))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (BINARY_EXPR
                (PAREN_EXPR
                  (L_PAREN "(")
                  (BINARY_EXPR
                    (NAME_REF_EXPR (NAME_REF (IDENT "a")))
                    (PLUS "+")
                    (NAME_REF_EXPR (NAME_REF (IDENT "b"))))
                  (R_PAREN ")"))
                (STAR "*")
                (NAME_REF_EXPR (NAME_REF (IDENT "c")))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (PLUSEQ "+=")
              (LITERAL (INT_NUMBER "1")))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (MINUSEQ "-=")
              (LITERAL (INT_NUMBER "1")))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (STAREQ "*=")
              (LITERAL (INT_NUMBER "2")))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (SLASHEQ "/=")
              (LITERAL (INT_NUMBER "2")))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (PERCENTEQ "%=")
              (LITERAL (INT_NUMBER "3")))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (AMPEQ "&=")
              (LITERAL (INT_NUMBER "1")))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (PIPEEQ "|=")
              (LITERAL (INT_NUMBER "2")))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (CARETEQ "^=")
              (LITERAL (INT_NUMBER "3")))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (SHLEQ "<<=")
              (LITERAL (INT_NUMBER "1")))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (SHREQ ">>=")
              (LITERAL (INT_NUMBER "1")))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (USHREQ ">>>=")
              (LITERAL (INT_NUMBER "1")))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (EQ "=")
              (ASSIGN_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "y")))
                (EQ "=")
                (NAME_REF_EXPR (NAME_REF (IDENT "z")))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "a")))
              (EQ "=")
              (BINARY_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "b")))
                (GTEQ ">=")
                (NAME_REF_EXPR (NAME_REF (IDENT "c")))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "a")))
              (EQ "=")
              (BINARY_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "b")))
                (R_ANGLE ">")
                (NAME_REF_EXPR (NAME_REF (IDENT "c")))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "i")))
              (EQ "=")
              (TERNARY_EXPR
                (BINARY_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "a")))
                  (R_ANGLE ">")
                  (NAME_REF_EXPR (NAME_REF (IDENT "b"))))
                (QUESTION "?")
                (NAME_REF_EXPR (NAME_REF (IDENT "a")))
                (COLON ":")
                (NAME_REF_EXPR (NAME_REF (IDENT "b")))))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (R_CURLY "}"))))
