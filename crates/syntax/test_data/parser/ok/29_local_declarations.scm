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
          (LOCAL_VAR_DECL
            (PRIMITIVE_TYPE (INT_KW "int"))
            (VARIABLE_DECLARATOR (NAME (IDENT "a")))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PRIMITIVE_TYPE (INT_KW "int"))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "b"))
              (EQ "=")
              (EXPR_PLACEHOLDER (INT_NUMBER "1")))
            (COMMA ",")
            (VARIABLE_DECLARATOR
              (NAME (IDENT "c"))
              (EQ "=")
              (EXPR_PLACEHOLDER (INT_NUMBER "2")))
            (COMMA ",")
            (VARIABLE_DECLARATOR (NAME (IDENT "d")))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (MODIFIER_LIST (FINAL_KW "final"))
            (PATH_TYPE (NAME_REF (IDENT "String")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "s"))
              (EQ "=")
              (EXPR_PLACEHOLDER (STRING_LITERAL "\"x\"")))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PATH_TYPE (NAME_REF (IDENT "var")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "v"))
              (EQ "=")
              (EXPR_PLACEHOLDER
                (IDENT "compute")
                (L_PAREN "(")
                (R_PAREN ")")))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PATH_TYPE
              (NAME_REF (IDENT "List"))
              (TYPE_ARGS
                (L_ANGLE "<")
                (PATH_TYPE (NAME_REF (IDENT "String")))
                (R_ANGLE ">")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "names"))
              (EQ "=")
              (EXPR_PLACEHOLDER
                (NEW_KW "new")
                (IDENT "ArrayList")
                (L_ANGLE "<")
                (R_ANGLE ">")
                (L_PAREN "(")
                (R_PAREN ")")))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PATH_TYPE
              (NAME_REF (IDENT "Map"))
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
            (VARIABLE_DECLARATOR
              (NAME (IDENT "index"))
              (EQ "=")
              (EXPR_PLACEHOLDER
                (NEW_KW "new")
                (IDENT "HashMap")
                (L_ANGLE "<")
                (R_ANGLE ">")
                (L_PAREN "(")
                (R_PAREN ")")))
            (COMMA ",")
            (VARIABLE_DECLARATOR
              (NAME (IDENT "other"))
              (EQ "=")
              (EXPR_PLACEHOLDER (NULL_KW "null")))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (ARRAY_TYPE
              (PRIMITIVE_TYPE (INT_KW "int"))
              (L_BRACK "[")
              (R_BRACK "]"))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "arr"))
              (EQ "=")
              (EXPR_PLACEHOLDER
                (L_CURLY "{")
                (INT_NUMBER "1")
                (COMMA ",")
                (INT_NUMBER "2")
                (COMMA ",")
                (INT_NUMBER "3")
                (R_CURLY "}")))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PRIMITIVE_TYPE (INT_KW "int"))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "matrix"))
              (L_BRACK "[")
              (R_BRACK "]")
              (L_BRACK "[")
              (R_BRACK "]")
              (EQ "=")
              (EXPR_PLACEHOLDER
                (NEW_KW "new")
                (INT_KW "int")
                (L_BRACK "[")
                (INT_NUMBER "2")
                (R_BRACK "]")
                (L_BRACK "[")
                (INT_NUMBER "2")
                (R_BRACK "]")))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PATH_TYPE
              (NAME_REF (IDENT "java"))
              (DOT ".")
              (NAME_REF (IDENT "util"))
              (DOT ".")
              (NAME_REF (IDENT "List"))
              (TYPE_ARGS
                (L_ANGLE "<")
                (WILDCARD_TYPE (QUESTION "?"))
                (R_ANGLE ">")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "any"))
              (EQ "=")
              (EXPR_PLACEHOLDER (NULL_KW "null")))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PATH_TYPE
              (NAME_REF (IDENT "Map"))
              (DOT ".")
              (NAME_REF (IDENT "Entry"))
              (TYPE_ARGS
                (L_ANGLE "<")
                (PATH_TYPE (NAME_REF (IDENT "String")))
                (COMMA ",")
                (WILDCARD_TYPE
                  (QUESTION "?")
                  (EXTENDS_KW "extends")
                  (PATH_TYPE (NAME_REF (IDENT "Number"))))
                (R_ANGLE ">")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "entry"))
              (EQ "=")
              (EXPR_PLACEHOLDER (IDENT "e")))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PATH_TYPE
              (NAME_REF (IDENT "Outer"))
              (DOT ".")
              (NAME_REF (IDENT "Inner")))
            (VARIABLE_DECLARATOR (NAME (IDENT "x")))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (MODIFIER_LIST
              (ANNOTATION
                (AT "@")
                (PATH (NAME_REF (IDENT "SuppressWarnings")))
                (ANNOTATION_ARGS
                  (L_PAREN "(")
                  (STRING_LITERAL "\"unchecked\"")
                  (R_PAREN ")"))))
            (PATH_TYPE (NAME_REF (IDENT "List")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "raw"))
              (EQ "=")
              (EXPR_PLACEHOLDER
                (L_PAREN "(")
                (IDENT "List")
                (R_PAREN ")")
                (IDENT "o")))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (MODIFIER_LIST
              (FINAL_KW "final")
              (ANNOTATION
                (AT "@")
                (PATH (NAME_REF (IDENT "Anno")))))
            (PRIMITIVE_TYPE (INT_KW "int"))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "annotated"))
              (EQ "=")
              (EXPR_PLACEHOLDER (INT_NUMBER "0")))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PATH_TYPE (NAME_REF (IDENT "Runnable")))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "r"))
              (EQ "=")
              (EXPR_PLACEHOLDER
                (L_PAREN "(")
                (R_PAREN ")")
                (ARROW "->")
                (L_CURLY "{")
                (IDENT "work")
                (L_PAREN "(")
                (R_PAREN ")")
                (SEMICOLON ";")
                (R_CURLY "}")))
            (COMMA ",")
            (VARIABLE_DECLARATOR
              (NAME (IDENT "q"))
              (EQ "=")
              (EXPR_PLACEHOLDER
                (L_PAREN "(")
                (R_PAREN ")")
                (ARROW "->")
                (L_CURLY "{")
                (R_CURLY "}")))
            (SEMICOLON ";"))
          (LOCAL_VAR_DECL
            (PRIMITIVE_TYPE (BOOLEAN_KW "boolean"))
            (VARIABLE_DECLARATOR
              (NAME (IDENT "cmp"))
              (EQ "=")
              (EXPR_PLACEHOLDER
                (IDENT "a")
                (L_ANGLE "<")
                (IDENT "b")))
            (COMMA ",")
            (VARIABLE_DECLARATOR
              (NAME (IDENT "other2"))
              (EQ "=")
              (EXPR_PLACEHOLDER
                (IDENT "c")
                (R_ANGLE ">")
                (IDENT "d")))
            (SEMICOLON ";"))
          (CLASS_DECL
            (CLASS_KW "class")
            (NAME (IDENT "Local"))
            (CLASS_BODY
              (L_CURLY "{")
              (FIELD_DECL
                (PRIMITIVE_TYPE (INT_KW "int"))
                (VARIABLE_DECLARATOR (NAME (IDENT "x")))
                (SEMICOLON ";"))
              (METHOD_DECL
                (PRIMITIVE_TYPE (VOID_KW "void"))
                (NAME (IDENT "m"))
                (PARAM_LIST
                  (L_PAREN "(")
                  (R_PAREN ")"))
                (BLOCK
                  (L_CURLY "{")
                  (R_CURLY "}")))
              (R_CURLY "}")))
          (INTERFACE_DECL
            (INTERFACE_KW "interface")
            (NAME (IDENT "LocalI"))
            (CLASS_BODY
              (L_CURLY "{")
              (R_CURLY "}")))
          (ENUM_DECL
            (ENUM_KW "enum")
            (NAME (IDENT "LocalE"))
            (ENUM_BODY
              (L_CURLY "{")
              (ENUM_CONSTANT (NAME (IDENT "A")))
              (COMMA ",")
              (ENUM_CONSTANT (NAME (IDENT "B")))
              (R_CURLY "}")))
          (RECORD_DECL
            (IDENT "record")
            (NAME (IDENT "LocalR"))
            (RECORD_COMPONENT_LIST
              (L_PAREN "(")
              (RECORD_COMPONENT
                (PRIMITIVE_TYPE (INT_KW "int"))
                (NAME (IDENT "a")))
              (R_PAREN ")"))
            (CLASS_BODY
              (L_CURLY "{")
              (R_CURLY "}")))
          (CLASS_DECL
            (MODIFIER_LIST (ABSTRACT_KW "abstract"))
            (CLASS_KW "class")
            (NAME (IDENT "LocalAbs"))
            (CLASS_BODY
              (L_CURLY "{")
              (R_CURLY "}")))
          (CLASS_DECL
            (MODIFIER_LIST (FINAL_KW "final"))
            (CLASS_KW "class")
            (NAME (IDENT "LocalFinal"))
            (CLASS_BODY
              (L_CURLY "{")
              (R_CURLY "}")))
          (R_CURLY "}")))
      (R_CURLY "}"))))
