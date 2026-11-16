@Anno(Foo.class)
@Anno(value = {A.class, B.class}, other = @Nested(C.class))
class A {}
