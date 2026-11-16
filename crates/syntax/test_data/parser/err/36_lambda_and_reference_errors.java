class A {
    void a() { f = x -> ; }
    void b() { f = (a, b) -> ; }
    void c() { f = () -> { work(); } }
    void d() { f = Foo::; }
    void e() { f = int[]; }
    void f() { f = (a, ) -> a; }
    void ok() {}
}
