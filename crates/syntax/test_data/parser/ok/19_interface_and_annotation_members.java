interface I {
    int CONSTANT = 1;
    void m();
    default void d() {}
    static void s() {}
}
@interface A {
    int value() default 5;
}
