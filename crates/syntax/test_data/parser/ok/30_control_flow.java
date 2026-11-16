class A {
    void f() {
        if (a) b();
        if (a) { b(); } else { c(); }
        if (a) b(); else if (c) d(); else e();
        if (x < y && y > z) { }
        while (running) step();
        while (i < n) { i++; }
        do { i++; } while (i < n);
        do i++; while (i < n);
        for (int i = 0; i < n; i++) sum += i;
        for (int i = 0, j = n; i < j; i++, j--) { }
        for (i = 0; i < n; i++) { }
        for (;;) { break; }
        for (final String s : names) { use(s); }
        for (Map.Entry<String, List<Integer>> e : map.entrySet()) { }
        for (var x : xs) use(x);
        for (@Anno int i : ints) { }
        for (int[] row : matrix) { }
        for (;;);
        if (a) ; else ;
        label: for (int i = 0; i < 3; i++) { continue label; }
    }
}
