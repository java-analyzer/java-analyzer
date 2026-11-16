class A {
    void a() { switch (x) { foo(); case 1: bar(); } }
    void b() { switch (x) { case 1 bar(); } }
    void c() { switch (x) { case 1 -> } }
    void d() { switch (x) { case : } }
    void e() { switch x { } }
    void ok() {}
}
