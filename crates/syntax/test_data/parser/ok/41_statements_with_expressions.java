class A {
    int f(int n) throws Exception {
        for (int i = 0, j = n; i < j; i++, j--) { sum += i * j; }
        for (i = 0; i < n; i += 2) { }
        for (String s : names) { use(s.trim()); }
        for (var e : map.entrySet()) { use(e.getKey(), e.getValue()); }
        while (it.hasNext()) { it.next(); }
        do { n--; } while (n > 0);
        if (a != null && a.isEmpty() || b == null) { return -1; }
        try (var in = new Scanner(System.in); Reader r = open()) { r.read(); }
        try (this.res) { }
        catchAll: try { throw new Exception("x" + n); } catch (Exception e) { log.error("failed", e); }
        assert n > 0 : "n must be positive, was " + n;
        synchronized (lock) { counter++; }
        label: for (;;) { if (done) break label; else continue label; }
        int[] arr = new int[n];
        arr[0] = arr[1] + arr[2];
        Runnable r = () -> System.out.println("hi");
        r.run();
        return n > 0 ? n : -n;
    }
}
