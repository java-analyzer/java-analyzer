(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "Level11Gates"))
    (CLASS_BODY
      (L_CURLY "{")
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "String")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "tb"))
          (EQ "=")
          (LITERAL (TEXT_BLOCK "\"\"\"\n        hello\n        \"\"\"")))
        (SEMICOLON ";"))
      (METHOD_DECL
        (PRIMITIVE_TYPE (INT_KW "int"))
        (NAME (IDENT "f"))
        (PARAM_LIST
          (L_PAREN "(")
          (PARAM
            (PRIMITIVE_TYPE (INT_KW "int"))
            (NAME (IDENT "x")))
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (RETURN_STMT
            (RETURN_KW "return")
            (SWITCH_EXPR
              (SWITCH_KW "switch")
              (L_PAREN "(")
              (NAME_REF_EXPR (NAME_REF (IDENT "x")))
              (R_PAREN ")")
              (SWITCH_BODY
                (L_CURLY "{")
                (SWITCH_CASE
                  (SWITCH_LABEL
                    (CASE_KW "case")
                    (LITERAL (INT_NUMBER "1"))
                    (ARROW "->"))
                  (EXPR_STMT
                    (LITERAL (INT_NUMBER "10"))
                    (SEMICOLON ";")))
                (SWITCH_CASE
                  (SWITCH_LABEL
                    (DEFAULT_KW "default")
                    (ARROW "->"))
                  (BLOCK
                    (L_CURLY "{")
                    (YIELD_STMT
                      (IDENT "yield")
                      (LITERAL (INT_NUMBER "20"))
                      (SEMICOLON ";"))
                    (R_CURLY "}")))
                (R_CURLY "}")))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (R_CURLY "}"))))
(errors
  (error "text blocks require language level 15" "3:17..5:12")
  (error "switch expressions require language level 14" "8:16..13:10")
  (error "switch expressions require language level 14" "11:17..11:26"))
