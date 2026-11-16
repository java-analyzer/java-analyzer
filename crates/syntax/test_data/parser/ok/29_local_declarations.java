class A {
    void f() {
        int a;
        int b = 1, c = 2, d;
        final String s = "x";
        var v = compute();
        List<String> names = new ArrayList<>();
        Map<String, List<Integer>> index = new HashMap<>(), other = null;
        int[] arr = {1, 2, 3};
        int matrix[][] = new int[2][2];
        java.util.List<?> any = null;
        Map.Entry<String, ? extends Number> entry = e;
        Outer.Inner x;
        @SuppressWarnings("unchecked") List raw = (List) o;
        final @Anno int annotated = 0;
        Runnable r = () -> { work(); }, q = () -> {};
        boolean cmp = a < b, other2 = c > d;
        class Local { int x; void m() {} }
        interface LocalI {}
        enum LocalE { A, B }
        record LocalR(int a) {}
        abstract class LocalAbs {}
        final class LocalFinal {}
    }
}
