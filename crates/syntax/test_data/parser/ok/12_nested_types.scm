(SOURCE_FILE
  (CLASS_DECL
    (CLASS_KW "class")
    (NAME (IDENT "Outer"))
    (CLASS_BODY
      (L_CURLY "{")
      (CLASS_DECL
        (MODIFIER_LIST (STATIC_KW "static"))
        (CLASS_KW "class")
        (NAME (IDENT "Inner"))
        (CLASS_BODY
          (L_CURLY "{")
          (R_CURLY "}")))
      (INTERFACE_DECL
        (INTERFACE_KW "interface")
        (NAME (IDENT "Callback"))
        (CLASS_BODY
          (L_CURLY "{")
          (R_CURLY "}")))
      (ENUM_DECL
        (ENUM_KW "enum")
        (NAME (IDENT "Kind"))
        (CLASS_BODY
          (L_CURLY "{")
          (MEMBER_PLACEHOLDER
            (IDENT "A")
            (COMMA ",")
            (IDENT "B"))
          (R_CURLY "}")))
      (RECORD_DECL
        (IDENT "record")
        (NAME (IDENT "Pair"))
        (RECORD_COMPONENT_LIST
          (L_PAREN "(")
          (INT_KW "int")
          (IDENT "a")
          (COMMA ",")
          (INT_KW "int")
          (IDENT "b")
          (R_PAREN ")"))
        (CLASS_BODY
          (L_CURLY "{")
          (R_CURLY "}")))
      (ANNOTATION_TYPE_DECL
        (AT "@")
        (INTERFACE_KW "interface")
        (NAME (IDENT "Marker"))
        (CLASS_BODY
          (L_CURLY "{")
          (R_CURLY "}")))
      (CLASS_DECL
        (MODIFIER_LIST
          (PRIVATE_KW "private")
          (FINAL_KW "final"))
        (CLASS_KW "class")
        (NAME (IDENT "Deep"))
        (CLASS_BODY
          (L_CURLY "{")
          (CLASS_DECL
            (CLASS_KW "class")
            (NAME (IDENT "Deeper"))
            (CLASS_BODY
              (L_CURLY "{")
              (R_CURLY "}")))
          (R_CURLY "}")))
      (R_CURLY "}"))))
