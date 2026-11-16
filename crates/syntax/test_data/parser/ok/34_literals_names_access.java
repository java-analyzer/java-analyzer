class A {
    Object a = 1, b = 2L, c = 1.5, d = 1.5f, e = 'x', f = "s", g = true, h = false, i = null;
    Object text = """
        block
        """;
    Object names = x;
    Object field = a.b.c;
    Object call = foo();
    Object args = foo(1, "two", three, four());
    Object chain = a.b().c.d(x).e;
    Object generic = Collections.<String>emptyList();
    Object genericThis = this.<T>run(x);
    Object index = arr[0][i + 1];
    Object postfix = i++, other = j--;
    Object paren = (a);
    Object nestedParens = ((a));
}
