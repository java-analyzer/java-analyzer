class A {
    void f(int x) {
        switch (x) {
            case 1:
                a();
                break;
            case 2:
            case 3: {
                b();
            }
            default:
                c();
        }
        switch (x) {
            case 1, 2 -> a();
            case 3 -> { b(); }
            case 4 -> throw new X();
            default -> c();
        }
        switch (s) {
            case "a": return;
            case null, default: break;
        }
        switch (o) {
            case Foo f when f.x > 0 -> use(f);
            case Bar(int a, int b) -> use(a, b);
            default -> {}
        }
        switch (x) { }
        switch (c) {
            case 'a': x = flag ? 1 : 2; break;
            default: break;
        }
    }
}
