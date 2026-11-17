@Marker()
@Single(1 + 2 * 3)
@Type(int[][].class)
@Outer(@Inner(value = 1))
@Container({@Item("a"), @Item("b"),})
@Full(
    a = 1,
    b = "x",
    c = Foo.BAR,
    d = {1, 2},
    e = @Nested,
    f = @Nested(k = String.class)
)
@interface AnnoDefaults {
    Nested single() default @Nested(k = Object.class);
    Nested[] many() default {@Nested, @Nested(k = void.class)};
}
