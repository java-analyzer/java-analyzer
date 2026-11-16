(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "A"))
    (CLASS_BODY
      (L_CURLY "{")
      (METHOD_DECL
        (PRIMITIVE_TYPE (INT_KW "int"))
        (NAME (IDENT "f"))
        (PARAM_LIST
          (L_PAREN "(")
          (PARAM
            (PRIMITIVE_TYPE (INT_KW "int"))
            (NAME (IDENT "n")))
          (R_PAREN ")"))
        (THROWS_CLAUSE
          (THROWS_KW "throws")
          (PATH_TYPE (NAME_REF (IDENT "Exception"))))
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
              (EXPR_STMT
                (ASSIGN_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "sum")))
                  (PLUSEQ "+=")
                  (BINARY_EXPR
                    (NAME_REF_EXPR (NAME_REF (IDENT "i")))
                    (STAR "*")
                    (NAME_REF_EXPR (NAME_REF (IDENT "j")))))
                (SEMICOLON ";"))
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
            (ASSIGN_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "i")))
              (PLUSEQ "+=")
              (LITERAL (INT_NUMBER "2")))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}")))
          (FOREACH_STMT
            (FOR_KW "for")
            (L_PAREN "(")
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
                    (METHOD_CALL_EXPR
                      (NAME_REF_EXPR (NAME_REF (IDENT "s")))
                      (DOT ".")
                      (NAME_REF (IDENT "trim"))
                      (ARG_LIST
                        (L_PAREN "(")
                        (R_PAREN ")")))
                    (R_PAREN ")")))
                (SEMICOLON ";"))
              (R_CURLY "}")))
          (FOREACH_STMT
            (FOR_KW "for")
            (L_PAREN "(")
            (PATH_TYPE (NAME_REF (IDENT "var")))
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
              (EXPR_STMT
                (CALL_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "use")))
                  (ARG_LIST
                    (L_PAREN "(")
                    (METHOD_CALL_EXPR
                      (NAME_REF_EXPR (NAME_REF (IDENT "e")))
                      (DOT ".")
                      (NAME_REF (IDENT "getKey"))
                      (ARG_LIST
                        (L_PAREN "(")
                        (R_PAREN ")")))
                    (COMMA ",")
                    (METHOD_CALL_EXPR
                      (NAME_REF_EXPR (NAME_REF (IDENT "e")))
                      (DOT ".")
                      (NAME_REF (IDENT "getValue"))
                      (ARG_LIST
                        (L_PAREN "(")
                        (R_PAREN ")")))
                    (R_PAREN ")")))
                (SEMICOLON ";"))
              (R_CURLY "}")))
          (WHILE_STMT
            (WHILE_KW "while")
            (L_PAREN "(")
            (METHOD_CALL_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "it")))
              (DOT ".")
              (NAME_REF (IDENT "hasNext"))
              (ARG_LIST
                (L_PAREN "(")
                (R_PAREN ")")))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (EXPR_STMT
                (METHOD_CALL_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "it")))
                  (DOT ".")
                  (NAME_REF (IDENT "next"))
                  (ARG_LIST
                    (L_PAREN "(")
                    (R_PAREN ")")))
                (SEMICOLON ";"))
              (R_CURLY "}")))
          (DO_STMT
            (DO_KW "do")
            (BLOCK
              (L_CURLY "{")
              (EXPR_STMT
                (POSTFIX_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "n")))
                  (MINUS2 "--"))
                (SEMICOLON ";"))
              (R_CURLY "}"))
            (WHILE_KW "while")
            (L_PAREN "(")
            (BINARY_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "n")))
              (R_ANGLE ">")
              (LITERAL (INT_NUMBER "0")))
            (R_PAREN ")")
            (SEMICOLON ";"))
          (IF_STMT
            (IF_KW "if")
            (L_PAREN "(")
            (BINARY_EXPR
              (BINARY_EXPR
                (BINARY_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "a")))
                  (NEQ "!=")
                  (LITERAL (NULL_KW "null")))
                (AMP2 "&&")
                (METHOD_CALL_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "a")))
                  (DOT ".")
                  (NAME_REF (IDENT "isEmpty"))
                  (ARG_LIST
                    (L_PAREN "(")
                    (R_PAREN ")"))))
              (PIPE2 "||")
              (BINARY_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "b")))
                (EQ2 "==")
                (LITERAL (NULL_KW "null"))))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (RETURN_STMT
                (RETURN_KW "return")
                (UNARY_EXPR
                  (MINUS "-")
                  (LITERAL (INT_NUMBER "1")))
                (SEMICOLON ";"))
              (R_CURLY "}")))
          (TRY_STMT
            (TRY_KW "try")
            (RESOURCE_LIST
              (L_PAREN "(")
              (RESOURCE
                (PATH_TYPE (NAME_REF (IDENT "var")))
                (NAME (IDENT "in"))
                (EQ "=")
                (NEW_EXPR
                  (NEW_KW "new")
                  (PATH_TYPE (NAME_REF (IDENT "Scanner")))
                  (ARG_LIST
                    (L_PAREN "(")
                    (FIELD_ACCESS_EXPR
                      (NAME_REF_EXPR (NAME_REF (IDENT "System")))
                      (DOT ".")
                      (NAME_REF (IDENT "in")))
                    (R_PAREN ")"))))
              (SEMICOLON ";")
              (RESOURCE
                (PATH_TYPE (NAME_REF (IDENT "Reader")))
                (NAME (IDENT "r"))
                (EQ "=")
                (CALL_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "open")))
                  (ARG_LIST
                    (L_PAREN "(")
                    (R_PAREN ")"))))
              (R_PAREN ")"))
            (BLOCK
              (L_CURLY "{")
              (EXPR_STMT
                (METHOD_CALL_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "r")))
                  (DOT ".")
                  (NAME_REF (IDENT "read"))
                  (ARG_LIST
                    (L_PAREN "(")
                    (R_PAREN ")")))
                (SEMICOLON ";"))
              (R_CURLY "}")))
          (TRY_STMT
            (TRY_KW "try")
            (RESOURCE_LIST
              (L_PAREN "(")
              (RESOURCE
                (FIELD_ACCESS_EXPR
                  (THIS_EXPR (THIS_KW "this"))
                  (DOT ".")
                  (NAME_REF (IDENT "res"))))
              (R_PAREN ")"))
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}")))
          (LABELED_STMT
            (NAME (IDENT "catchAll"))
            (COLON ":")
            (TRY_STMT
              (TRY_KW "try")
              (BLOCK
                (L_CURLY "{")
                (THROW_STMT
                  (THROW_KW "throw")
                  (NEW_EXPR
                    (NEW_KW "new")
                    (PATH_TYPE (NAME_REF (IDENT "Exception")))
                    (ARG_LIST
                      (L_PAREN "(")
                      (BINARY_EXPR
                        (LITERAL (STRING_LITERAL "\"x\""))
                        (PLUS "+")
                        (NAME_REF_EXPR (NAME_REF (IDENT "n"))))
                      (R_PAREN ")")))
                  (SEMICOLON ";"))
                (R_CURLY "}"))
              (CATCH_CLAUSE
                (CATCH_KW "catch")
                (L_PAREN "(")
                (PATH_TYPE (NAME_REF (IDENT "Exception")))
                (NAME (IDENT "e"))
                (R_PAREN ")")
                (BLOCK
                  (L_CURLY "{")
                  (EXPR_STMT
                    (METHOD_CALL_EXPR
                      (NAME_REF_EXPR (NAME_REF (IDENT "log")))
                      (DOT ".")
                      (NAME_REF (IDENT "error"))
                      (ARG_LIST
                        (L_PAREN "(")
                        (LITERAL (STRING_LITERAL "\"failed\""))
                        (COMMA ",")
                        (NAME_REF_EXPR (NAME_REF (IDENT "e")))
                        (R_PAREN ")")))
                    (SEMICOLON ";"))
                  (R_CURLY "}")))))
          (ASSERT_STMT
            (ASSERT_KW "assert")
            (BINARY_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "n")))
              (R_ANGLE ">")
              (LITERAL (INT_NUMBER "0")))
            (COLON ":")
            (BINARY_EXPR
              (LITERAL (STRING_LITERAL "\"n must be positive, was \""))
              (PLUS "+")
              (NAME_REF_EXPR (NAME_REF (IDENT "n"))))
            (SEMICOLON ";"))
          (SYNCHRONIZED_STMT
            (SYNCHRONIZED_KW "synchronized")
            (L_PAREN "(")
            (NAME_REF_EXPR (NAME_REF (IDENT "lock")))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (EXPR_STMT
                (POSTFIX_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "counter")))
                  (PLUS2 "++"))
                (SEMICOLON ";"))
              (R_CURLY "}")))
          (LABELED_STMT
            (NAME (IDENT "label"))
            (COLON ":")
            (FOR_STMT
              (FOR_KW "for")
              (L_PAREN "(")
              (SEMICOLON ";")
              (SEMICOLON ";")
              (R_PAREN ")")
              (BLOCK
                (L_CURLY "{")
                (IF_STMT
                  (IF_KW "if")
                  (L_PAREN "(")
                  (NAME_REF_EXPR (NAME_REF (IDENT "done")))
                  (R_PAREN ")")
                  (BREAK_STMT
                    (BREAK_KW "break")
                    (NAME_REF (IDENT "label"))
                    (SEMICOLON ";"))
                  (ELSE_KW "else")
                  (CONTINUE_STMT
                    (CONTINUE_KW "continue")
                    (NAME_REF (IDENT "label"))
                    (SEMICOLON ";")))
                (R_CURLY "}"))))
          (LOCAL_VAR_DECL
            (ARRAY_TYPE
              (PRIMITIVE_TYPE (INT_KW "int"))
              (L_BRACK "[")
              (R_BRACK "]"))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "arr"))
              (EQ "=")
              (NEW_EXPR
                (NEW_KW "new")
                (PRIMITIVE_TYPE (INT_KW "int"))
                (L_BRACK "[")
                (NAME_REF_EXPR (NAME_REF (IDENT "n")))
                (R_BRACK "]")))
            (SEMICOLON ";"))
          (EXPR_STMT
            (ASSIGN_EXPR
              (ARRAY_ACCESS_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "arr")))
                (L_BRACK "[")
                (LITERAL (INT_NUMBER "0"))
                (R_BRACK "]"))
              (EQ "=")
              (BINARY_EXPR
                (ARRAY_ACCESS_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "arr")))
                  (L_BRACK "[")
                  (LITERAL (INT_NUMBER "1"))
                  (R_BRACK "]"))
                (PLUS "+")
                (ARRAY_ACCESS_EXPR
                  (NAME_REF_EXPR (NAME_REF (IDENT "arr")))
                  (L_BRACK "[")
                  (LITERAL (INT_NUMBER "2"))
                  (R_BRACK "]"))))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PATH_TYPE (NAME_REF (IDENT "Runnable")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "r"))
              (EQ "=")
              (LAMBDA_EXPR
                (LAMBDA_PARAMS
                  (L_PAREN "(")
                  (R_PAREN ")"))
                (ARROW "->")
                (METHOD_CALL_EXPR
                  (FIELD_ACCESS_EXPR
                    (NAME_REF_EXPR (NAME_REF (IDENT "System")))
                    (DOT ".")
                    (NAME_REF (IDENT "out")))
                  (DOT ".")
                  (NAME_REF (IDENT "println"))
                  (ARG_LIST
                    (L_PAREN "(")
                    (LITERAL (STRING_LITERAL "\"hi\""))
                    (R_PAREN ")")))))
            (SEMICOLON ";"))
          (EXPR_STMT
            (METHOD_CALL_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "r")))
              (DOT ".")
              (NAME_REF (IDENT "run"))
              (ARG_LIST
                (L_PAREN "(")
                (R_PAREN ")")))
            (SEMICOLON ";"))
          (RETURN_STMT
            (RETURN_KW "return")
            (TERNARY_EXPR
              (BINARY_EXPR
                (NAME_REF_EXPR (NAME_REF (IDENT "n")))
                (R_ANGLE ">")
                (LITERAL (INT_NUMBER "0")))
              (QUESTION "?")
              (NAME_REF_EXPR (NAME_REF (IDENT "n")))
              (COLON ":")
              (UNARY_EXPR
                (MINUS "-")
                (NAME_REF_EXPR (NAME_REF (IDENT "n")))))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (R_CURLY "}"))))
