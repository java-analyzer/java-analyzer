enum Color {
    RED, GREEN, BLUE
}
enum Planet {
    MERCURY(1.0), VENUS(2.0) { void f() {} };
    private final double mass;
    Planet(double mass) { this.mass = mass; }
}
