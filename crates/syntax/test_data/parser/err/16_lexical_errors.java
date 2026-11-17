class A {
    String s = "unterminated;
    int x = 0x;
    String tb = """
        bad \q escape
        """;
    /* unclosed comment
}
