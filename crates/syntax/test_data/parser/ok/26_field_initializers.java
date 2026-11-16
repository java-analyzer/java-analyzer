class A {
    int a = 1, b = foo(1, 2), c;
    Map<String, Integer> m = new HashMap<String, Integer>(), n = null;
    Triple<A, B, C> t = new Triple<A, B, C>(), u;
    int[] arr = {1, 2}, empty = {};
    Runnable r = () -> { int x = 1; run(); }, s = () -> {};
    Object o = new Object() { int f; void g() { f++; } };
    boolean cmp = 1 < 2, other = 3 > 2;
    String text = """
        multi; line, with = stuff
        """;
    int after;
}
