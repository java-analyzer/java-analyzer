class A {
    Object o1 = new Object();
    Object o2 = new Foo(1, 2);
    Object o3 = new ArrayList<>();
    Object o4 = new HashMap<String, List<Integer>>();
    Object o5 = new Outer.Inner();
    Object o6 = outer.new Inner();
    Object o7 = new Runnable() { public void run() { work(); } };
    Object o8 = new Foo<String>(x) { int extra; };
    int[] a1 = new int[3];
    int[][] a2 = new int[3][4];
    int[][] a3 = new int[3][];
    int[] a4 = new int[] {1, 2, 3};
    int[][] a5 = new int[][] {{1}, {2, 3}, {}};
    int[] a6 = {1, 2, 3,};
    int[][] a7 = {{1}, {2}};
    String[] a8 = new String[n + 1];
    Foo[] a9 = {new Foo(), null};
}
