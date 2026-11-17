(SOURCE_FILE
  (CLASS_DECL
    (MODIFIER_LIST (IDENT "sealed"))
    (CLASS_KW "class")
    (NAME (IDENT "Level14Gates"))
    (PERMITS_CLAUSE
      (IDENT "permits")
      (PATH_TYPE (NAME_REF (IDENT "Sub"))))
    (CLASS_BODY
      (L_CURLY "{")
      (RECORD_DECL
        (IDENT "record")
        (NAME (IDENT "Point"))
        (RECORD_COMPONENT_LIST
          (L_PAREN "(")
          (RECORD_COMPONENT
            (PRIMITIVE_TYPE (INT_KW "int"))
            (NAME (IDENT "x")))
          (COMMA ",")
          (RECORD_COMPONENT
            (PRIMITIVE_TYPE (INT_KW "int"))
            (NAME (IDENT "y")))
          (R_PAREN ")"))
        (CLASS_BODY
          (L_CURLY "{")
          (R_CURLY "}")))
      (FIELD_DECL
        (PATH_TYPE (NAME_REF (IDENT "String")))
        (VARIABLE_DECLARATOR
          (NAME (IDENT "tb"))
          (EQ "=")
          (LITERAL (TEXT_BLOCK "\"\"\"\n        hello\n        \"\"\"")))
        (SEMICOLON ";"))
      (METHOD_DECL
        (PRIMITIVE_TYPE (INT_KW "int"))
        (NAME (IDENT "test"))
        (PARAM_LIST
          (L_PAREN "(")
          (PARAM
            (PATH_TYPE (NAME_REF (IDENT "Object")))
            (NAME (IDENT "o")))
          (R_PAREN ")"))
        (BLOCK
          (L_CURLY "{")
          (LOCAL_VAR_DECL
            (PRIMITIVE_TYPE (INT_KW "int"))
            (VARIABLE_DECLARATOR
              (UNDERSCORE_KW "_")
              (EQ "=")
              (LITERAL (INT_NUMBER "1")))
            (SEMICOLON ";"))
          (IF_STMT
            (IF_KW "if")
            (L_PAREN "(")
            (INSTANCEOF_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "o")))
              (INSTANCEOF_KW "instanceof")
              (TYPE_PATTERN
                (PATH_TYPE (NAME_REF (IDENT "String")))
                (NAME (IDENT "s"))))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}")))
          (IF_STMT
            (IF_KW "if")
            (L_PAREN "(")
            (INSTANCEOF_EXPR
              (NAME_REF_EXPR (NAME_REF (IDENT "o")))
              (INSTANCEOF_KW "instanceof")
              (RECORD_PATTERN
                (PATH_TYPE (NAME_REF (IDENT "Point")))
                (L_PAREN "(")
                (TYPE_PATTERN
                  (PRIMITIVE_TYPE (INT_KW "int"))
                  (NAME (IDENT "x")))
                (COMMA ",")
                (TYPE_PATTERN
                  (PRIMITIVE_TYPE (INT_KW "int"))
                  (NAME (IDENT "y")))
                (R_PAREN ")")))
            (R_PAREN ")")
            (BLOCK
              (L_CURLY "{")
              (R_CURLY "}")))
          (RETURN_STMT
            (RETURN_KW "return")
            (SWITCH_EXPR
              (SWITCH_KW "switch")
              (L_PAREN "(")
              (NAME_REF_EXPR (NAME_REF (IDENT "o")))
              (R_PAREN ")")
              (SWITCH_BODY
                (L_CURLY "{")
                (SWITCH_CASE
                  (SWITCH_LABEL
                    (CASE_KW "case")
                    (TYPE_PATTERN
                      (PATH_TYPE (NAME_REF (IDENT "String")))
                      (NAME (IDENT "s")))
                    (GUARD
                      (IDENT "when")
                      (UNARY_EXPR
                        (BANG "!")
                        (METHOD_CALL_EXPR
                          (NAME_REF_EXPR (NAME_REF (IDENT "s")))
                          (DOT ".")
                          (NAME_REF (IDENT "isEmpty"))
                          (ARG_LIST
                            (L_PAREN "(")
                            (R_PAREN ")")))))
                    (ARROW "->"))
                  (EXPR_STMT
                    (LITERAL (INT_NUMBER "1"))
                    (SEMICOLON ";")))
                (SWITCH_CASE
                  (SWITCH_LABEL
                    (DEFAULT_KW "default")
                    (ARROW "->"))
                  (EXPR_STMT
                    (LITERAL (INT_NUMBER "0"))
                    (SEMICOLON ";")))
                (R_CURLY "}")))
            (SEMICOLON ";"))
          (R_CURLY "}")))
      (R_CURLY "}"))))
(errors
  (error "sealed classes require language level 17" "2:1..2:7")
  (error "sealed classes require language level 17" "2:27..2:38")
  (error "records are a preview feature in java 14; enable preview" "3:5..3:34")
  (error "text blocks are a preview feature in java 14; enable preview" "5:17..7:12")
  (error "unnamed variables and patterns require language level 22" "10:13..10:14")
  (error "instanceof patterns are a preview feature in java 14; enable preview" "11:13..11:34")
  (error "instanceof patterns are a preview feature in java 14; enable preview" "12:13..12:45")
  (error "record patterns require language level 21" "12:26..12:45")
  (error "switch patterns require language level 21" "14:13..14:47"))
