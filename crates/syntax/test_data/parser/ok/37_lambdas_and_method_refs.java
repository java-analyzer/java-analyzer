class A {
    void f() {
        Runnable r1 = () -> work();
        Runnable r2 = () -> { work(); };
        Function<Integer, Integer> f1 = x -> x + 1;
        BiFunction<Integer, Integer, Integer> f2 = (a, b) -> a + b;
        BiFunction<Integer, Integer, Integer> f3 = (int a, int b) -> { return a + b; };
        Function<String, Integer> f4 = (final var s) -> s.length();
        Supplier<Supplier<Integer>> f5 = () -> () -> 1;
        run(x -> y -> x + y);
        list.forEach(item -> { use(item); });
        Object o = cond ? (Runnable) () -> {} : null;
        Function<A, B> m1 = Foo::bar;
        Supplier<Foo> m2 = Foo::new;
        Function<Integer, int[]> m3 = int[]::new;
        Function<Integer, String[]> m4 = String[]::new;
        Supplier<List<String>> m5 = ArrayList<String>::new;
        Function<Foo, Bar> m6 = this::convert;
        Function<Foo, Bar> m7 = super::convert;
        Function<Foo, Bar> m8 = Outer.Inner::method;
        Function<Foo, Bar> m9 = System.out::println;
        Function<Foo, Bar> m10 = Foo::<String>generic;
        Class<?> c1 = String.class, c2 = int.class, c3 = int[].class, c4 = Foo[].class, c5 = void.class;
        Class<?> c6 = Outer.Inner.class;
    }
}
