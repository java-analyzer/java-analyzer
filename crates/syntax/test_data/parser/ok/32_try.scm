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
        (THROWS_CLAUSE
          (THROWS_KW "throws")
          (PATH_TYPE (NAME_REF (IDENT "Exception"))))
        (BLOCK
          (L_CURLY "{")
          (TRY_STMT
            (TRY_KW "try")
            (BLOCK
              (L_CURLY "{")
              (EXPR_STMT
                (EXPR_PLACEHOLDER
                  (IDENT "a")
                  (L_PAREN "(")
                  (R_PAREN ")"))
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
                  (EXPR_PLACEHOLDER
                    (IDENT "b")
                    (L_PAREN "(")
                    (R_PAREN ")"))
                  (SEMICOLON ";"))
                (R_CURLY "}"))))
          (TRY_STMT
            (TRY_KW "try")
            (BLOCK
              (L_CURLY "{")
              (EXPR_STMT
                (EXPR_PLACEHOLDER
                  (IDENT "a")
                  (L_PAREN "(")
                  (R_PAREN ")"))
                (SEMICOLON ";"))
              (R_CURLY "}"))
            (FINALLY_CLAUSE
              (FINALLY_KW "finally")
              (BLOCK
                (L_CURLY "{")
                (EXPR_STMT
                  (EXPR_PLACEHOLDER
                    (IDENT "c")
                    (L_PAREN "(")
                    (R_PAREN ")"))
                  (SEMICOLON ";"))
                (R_CURLY "}"))))
          (TRY_STMT
            (TRY_KW "try")
            (BLOCK
              (L_CURLY "{")
              (EXPR_STMT
                (EXPR_PLACEHOLDER
                  (IDENT "a")
                  (L_PAREN "(")
                  (R_PAREN ")"))
                (SEMICOLON ";"))
              (R_CURLY "}"))
            (CATCH_CLAUSE
              (CATCH_KW "catch")
              (L_PAREN "(")
              (PATH_TYPE (NAME_REF (IDENT "IOException")))
              (PIPE "|")
              (PATH_TYPE (NAME_REF (IDENT "RuntimeException")))
              (NAME (IDENT "e"))
              (R_PAREN ")")
              (BLOCK
                (L_CURLY "{")
                (R_CURLY "}")))
            (CATCH_CLAUSE
              (CATCH_KW "catch")
              (L_PAREN "(")
              (MODIFIER_LIST (FINAL_KW "final"))
              (PATH_TYPE (NAME_REF (IDENT "Error")))
              (NAME (IDENT "e"))
              (R_PAREN ")")
              (BLOCK
                (L_CURLY "{")
                (R_CURLY "}")))
            (FINALLY_CLAUSE
              (FINALLY_KW "finally")
              (BLOCK
                (L_CURLY "{")
                (R_CURLY "}"))))
          (TRY_STMT
            (TRY_KW "try")
            (RESOURCE_LIST
              (L_PAREN "(")
              (RESOURCE
                (PATH_TYPE (NAME_REF (IDENT "Reader")))
                (NAME (IDENT "r"))
                (EQ "=")
                (EXPR_PLACEHOLDER
                  (IDENT "open")
                  (L_PAREN "(")
                  (R_PAREN ")")))
              (R_PAREN ")"))
            (BLOCK
              (L_CURLY "{")
              (EXPR_STMT
                (EXPR_PLACEHOLDER
                  (IDENT "r")
                  (DOT ".")
                  (IDENT "read")
                  (L_PAREN "(")
                  (R_PAREN ")"))
                (SEMICOLON ";"))
              (R_CURLY "}")))
          (TRY_STMT
            (TRY_KW "try")
            (RESOURCE_LIST
              (L_PAREN "(")
              (RESOURCE
                (MODIFIER_LIST (FINAL_KW "final"))
                (PATH_TYPE (NAME_REF (IDENT "Reader")))
                (NAME (IDENT "r"))
                (EQ "=")
                (EXPR_PLACEHOLDER
                  (IDENT "open")
                  (L_PAREN "(")
                  (R_PAREN ")")))
              (SEMICOLON ";")
              (RESOURCE
                (PATH_TYPE (NAME_REF (IDENT "var")))
                (NAME (IDENT "w"))
                (EQ "=")
                (EXPR_PLACEHOLDER
                  (IDENT "openWriter")
                  (L_PAREN "(")
                  (R_PAREN ")")))
              (SEMICOLON ";")
              (R_PAREN ")"))
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}")))
          (TRY_STMT
            (TRY_KW "try")
            (RESOURCE_LIST
              (L_PAREN "(")
              (RESOURCE (EXPR_PLACEHOLDER (IDENT "r")))
              (R_PAREN ")"))
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}")))
          (TRY_STMT
            (TRY_KW "try")
            (RESOURCE_LIST
              (L_PAREN "(")
              (RESOURCE (EXPR_PLACEHOLDER (IDENT "r")))
              (SEMICOLON ";")
              (RESOURCE (EXPR_PLACEHOLDER (IDENT "w")))
              (R_PAREN ")"))
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}"))
            (CATCH_CLAUSE
              (CATCH_KW "catch")
              (L_PAREN "(")
              (PATH_TYPE (NAME_REF (IDENT "Exception")))
              (NAME (IDENT "e"))
              (R_PAREN ")")
              (BLOCK
                (L_CURLY "{")
                (R_CURLY "}"))))
          (TRY_STMT
            (TRY_KW "try")
            (RESOURCE_LIST
              (L_PAREN "(")
              (RESOURCE
                (PATH_TYPE (NAME_REF (IDENT "var")))
                (NAME (IDENT "in"))
                (EQ "=")
                (EXPR_PLACEHOLDER
                  (NEW_KW "new")
                  (IDENT "Scanner")
                  (L_PAREN "(")
                  (IDENT "System")
                  (DOT ".")
                  (IDENT "in")
                  (R_PAREN ")")))
              (R_PAREN ")"))
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}"))
            (CATCH_CLAUSE
              (CATCH_KW "catch")
              (L_PAREN "(")
              (MODIFIER_LIST
                (ANNOTATION
                  (AT "@")
                  (PATH (NAME_REF (IDENT "Anno")))))
              (PATH_TYPE (NAME_REF (IDENT "Exception")))
              (NAME (IDENT "e"))
              (R_PAREN ")")
              (BLOCK
                (L_CURLY "{")
                (R_CURLY "}"))))
          (R_CURLY "}")))
      (R_CURLY "}"))))
