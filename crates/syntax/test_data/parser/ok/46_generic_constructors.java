class GenericConstructors {
    Object a = new <String>Foo();
    Object b = new <String, Integer>Foo<Long>(1, 2);
    Object c = new <@A String> @B Foo<@C Bar>();
    Object d = outer.new <String>Inner(x);
    Object e = new <String>Outer<Integer>.Inner<Boolean>() {};
}
