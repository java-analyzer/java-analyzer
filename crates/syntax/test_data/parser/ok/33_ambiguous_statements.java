class A {
    void f() {
        List<String> declaration;
        a < b;
        Collections.<String>emptyList();
        arr[i] = 1;
        arr[i][j]++;
        foo.bar.baz = 1;
        Foo.Bar.baz();
        int.class.getName();
        x = flag ? a : b;
        label: x = cond ? 1 : 2;
        yield = 3;
        yield++;
        record = 1;
        var = 2;
        (cast) y;
        String[] parts = line.split(",");
        Foo<Bar>[] generic;
        Object o = cond ? new int[] {1} : null;
        i = a < b ? 1 : 2;
    }
    int yielding(int x) {
        return switch (x) { case 1 -> 10; default -> { yield 20; } };
    }
}
